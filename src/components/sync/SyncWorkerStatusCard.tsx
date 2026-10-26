import { Pause, Play, RefreshCw } from "lucide-react";
import { useContext, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { PermissionContext } from "@/contexts/PermissionContext";
import type { Tone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import {
  syncBlockerText,
  syncWorkerErrorText,
  useSyncOrchestratorStore,
} from "@/stores/sync-orchestrator-store";
import type { SyncWorkerState } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Pause durations offered to the operator (minutes; the worker accepts 1..1440). */
const PAUSE_OPTIONS = [
  { minutes: 15, key: "m15" },
  { minutes: 60, key: "h1" },
  { minutes: 240, key: "h4" },
  { minutes: 1440, key: "h24" },
] as const;

function toneFor(state: SyncWorkerState | undefined): Tone {
  switch (state) {
    case "idle":
      return "success";
    case "running":
      return "progress";
    case "starting":
    case undefined:
      return "info";
    case "error":
    case "blocked":
      return "danger";
    case "paused":
      return "attention";
    default:
      return "warning";
  }
}

/** Status of the Rust sync worker with sync now / pause / resume (Sync Center, Settings). */
export function SyncWorkerStatusCard() {
  const { t } = useTranslation("shell");
  const { formatDateTime, formatNumber } = useFormatters();
  const worker = useSyncOrchestratorStore((s) => s.worker);
  const actionError = useSyncOrchestratorStore((s) => s.actionError);
  const runSyncNow = useSyncOrchestratorStore((s) => s.runSyncNow);
  const pauseForMinutes = useSyncOrchestratorStore((s) => s.pauseForMinutes);
  const resumeNow = useSyncOrchestratorStore((s) => s.resumeNow);
  // The Sync Center can render outside the permission provider (top bar): the commands check the
  // permission anyway; the UI only hides what the user cannot do.
  const permissions = useContext(PermissionContext);
  const canView = permissions?.can(P.SYNC_VIEW) ?? false;
  const canManage = permissions?.can(P.SYNC_MANAGE) ?? false;
  const [pauseMinutes, setPauseMinutes] = useState<number>(PAUSE_OPTIONS[0].minutes);
  const [busy, setBusy] = useState(false);

  const formatTime = (iso: string | null) => (iso ? formatDateTime(iso) : t("sync.worker.never"));
  /** A count the worker has not reported yet is "unknown", never 0. */
  const count = (n: number | undefined) =>
    n === undefined ? t("sync.worker.notReported") : formatNumber(n);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } finally {
      setBusy(false);
    }
  };

  const state = worker?.state;
  const blocker = syncBlockerText(worker?.blocker ?? null);
  const paused = state === "paused";

  return (
    <section className="rounded-md border border-surface-border bg-surface-0 p-3 space-y-2 text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="mb-1 text-2xs text-text-muted">{t("sync.worker.title")}</p>
          <StatusChip
            size="md"
            dot
            tone={toneFor(state)}
            label={t(`sync.worker.state.${state ?? "starting"}`)}
          />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {canView ? (
            <Button
              type="button"
              variant="outline"
              className="h-10"
              disabled={busy || paused}
              onClick={() => void run(runSyncNow)}
            >
              <RefreshCw className="me-1 h-3.5 w-3.5" aria-hidden />
              {t("sync.worker.syncNow")}
            </Button>
          ) : null}
          {canManage && paused ? (
            <Button
              type="button"
              variant="outline"
              className="h-10"
              disabled={busy}
              onClick={() => void run(resumeNow)}
            >
              <Play className="me-1 h-3.5 w-3.5" aria-hidden />
              {t("sync.worker.resume")}
            </Button>
          ) : null}
          {canManage && !paused ? (
            <>
              <Select
                value={String(pauseMinutes)}
                onValueChange={(v) => setPauseMinutes(Number(v))}
              >
                <SelectTrigger
                  className="h-10 w-32 text-xs"
                  aria-label={t("sync.worker.pauseDuration")}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PAUSE_OPTIONS.map((option) => (
                    <SelectItem key={option.key} value={String(option.minutes)}>
                      {t(`sync.worker.pauseOption.${option.key}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                type="button"
                variant="outline"
                className="h-10"
                disabled={busy}
                onClick={() => void run(() => pauseForMinutes(pauseMinutes))}
              >
                <Pause className="me-1 h-3.5 w-3.5" aria-hidden />
                {t("sync.worker.pause")}
              </Button>
            </>
          ) : null}
        </div>
      </div>

      {blocker ? <p className="text-tone-warning-fg">{blocker}</p> : null}
      {paused && worker?.paused_until ? (
        <p className="text-tone-attention-fg">
          {t("sync.worker.pausedUntil", { time: formatTime(worker.paused_until) })}
        </p>
      ) : null}

      <dl className="grid grid-cols-2 gap-x-4 gap-y-1">
        <dt className="text-text-muted">{t("sync.worker.lastSuccess")}</dt>
        <dd className="text-text-primary">{formatTime(worker?.last_success_at ?? null)}</dd>
        <dt className="text-text-muted">{t("sync.worker.pending")}</dt>
        <dd className="text-text-primary">{count(worker?.pending_outbox_count)}</dd>
        <dt className="text-text-muted">{t("sync.worker.conflicts")}</dt>
        <dd className="text-text-primary">{count(worker?.unresolved_conflict_count)}</dd>
      </dl>

      {worker?.next_attempt_at && state !== "idle" && !paused ? (
        <p className="text-text-muted">
          {t("sync.worker.nextAttempt", { time: formatTime(worker.next_attempt_at) })}
        </p>
      ) : null}
      {worker && (worker.media_pending_count > 0 || worker.media_failed_count > 0) ? (
        <p className="text-text-muted">
          {t("sync.worker.media", {
            pending: worker.media_pending_count,
            failed: worker.media_failed_count,
          })}
        </p>
      ) : null}
      {worker &&
      (worker.media_download_pending_count > 0 || worker.media_download_failed_count > 0) ? (
        <p className="text-text-muted">
          {t("sync.worker.mediaDownloads", {
            pending: worker.media_download_pending_count,
            failed: worker.media_download_failed_count,
          })}
        </p>
      ) : null}
      {worker && worker.dead_letter_outbox_count > 0 ? (
        <p className="text-tone-danger-fg">
          {t("sync.worker.deadLetter", { count: worker.dead_letter_outbox_count })}
        </p>
      ) : null}
      {worker?.last_error && state !== "idle" ? (
        <div className="text-tone-danger-fg">
          <p className="break-words">
            {t("sync.worker.lastError", { message: syncWorkerErrorText(worker.last_error) })}
          </p>
          {/* The raw text (HTTP status, network library) is for support, never the message. */}
          <details className="mt-1 text-text-muted">
            <summary className="cursor-pointer select-none">{t("sync.technicalDetails")}</summary>
            <p className="mt-1 break-all font-mono">
              {worker.last_error.code} · {worker.last_error.message}
            </p>
          </details>
        </div>
      ) : null}
      {actionError ? (
        <p className="break-words text-tone-danger-fg">
          {t("sync.worker.actionFailed", { message: actionError })}
        </p>
      ) : null}
      <p className="text-2xs text-text-muted">{t("sync.worker.runsInBackground")}</p>
    </section>
  );
}
