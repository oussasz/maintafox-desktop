import { Pause, Play, RefreshCw } from "lucide-react";
import { useContext, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { PermissionContext } from "@/contexts/PermissionContext";
import { syncBlockerText, useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { SyncWorkerState } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Pause durations offered to the operator (minutes; the worker accepts 1..1440). */
const PAUSE_OPTIONS = [
  { minutes: 15, key: "m15" },
  { minutes: 60, key: "h1" },
  { minutes: 240, key: "h4" },
  { minutes: 1440, key: "h24" },
] as const;

function toneFor(state: SyncWorkerState | undefined): string {
  switch (state) {
    case "idle":
      return "text-status-success";
    case "running":
    case "starting":
      return "text-primary";
    case "error":
      return "text-status-danger";
    default:
      return "text-status-warning";
  }
}

/** Status of the Rust sync worker with sync now / pause / resume (Sync Center, Settings). */
export function SyncWorkerStatusCard() {
  const { t, i18n } = useTranslation("shell");
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

  const formatTime = (iso: string | null) =>
    iso ? new Date(iso).toLocaleString(i18n.language) : t("sync.worker.never");

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
          <p className="text-2xs text-text-muted">{t("sync.worker.title")}</p>
          <p className={`text-sm font-semibold ${toneFor(state)}`}>
            {t(`sync.worker.state.${state ?? "starting"}`)}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {canView ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || paused}
              onClick={() => void run(runSyncNow)}
            >
              <RefreshCw className="mr-1 h-3.5 w-3.5" />
              {t("sync.worker.syncNow")}
            </Button>
          ) : null}
          {canManage && paused ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => void run(resumeNow)}
            >
              <Play className="mr-1 h-3.5 w-3.5" />
              {t("sync.worker.resume")}
            </Button>
          ) : null}
          {canManage && !paused ? (
            <>
              <label className="sr-only" htmlFor="sync-pause-duration">
                {t("sync.worker.pauseDuration")}
              </label>
              <select
                id="sync-pause-duration"
                className="rounded border border-surface-border bg-surface-2 px-2 py-1 text-xs"
                value={pauseMinutes}
                onChange={(e) => setPauseMinutes(Number(e.currentTarget.value))}
              >
                {PAUSE_OPTIONS.map((option) => (
                  <option key={option.key} value={option.minutes}>
                    {t(`sync.worker.pauseOption.${option.key}`)}
                  </option>
                ))}
              </select>
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() => void run(() => pauseForMinutes(pauseMinutes))}
              >
                <Pause className="mr-1 h-3.5 w-3.5" />
                {t("sync.worker.pause")}
              </Button>
            </>
          ) : null}
        </div>
      </div>

      {blocker ? <p className="text-status-warning">{blocker}</p> : null}
      {paused && worker?.paused_until ? (
        <p className="text-status-warning">
          {t("sync.worker.pausedUntil", { time: formatTime(worker.paused_until) })}
        </p>
      ) : null}

      <dl className="grid grid-cols-2 gap-x-4 gap-y-1">
        <dt className="text-text-muted">{t("sync.worker.lastSuccess")}</dt>
        <dd className="text-text-primary">{formatTime(worker?.last_success_at ?? null)}</dd>
        <dt className="text-text-muted">{t("sync.worker.pending")}</dt>
        <dd className="text-text-primary">{worker?.pending_outbox_count ?? 0}</dd>
        <dt className="text-text-muted">{t("sync.worker.conflicts")}</dt>
        <dd className="text-text-primary">{worker?.unresolved_conflict_count ?? 0}</dd>
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
        <p className="text-status-danger">
          {t("sync.worker.deadLetter", { count: worker.dead_letter_outbox_count })}
        </p>
      ) : null}
      {worker?.last_error && state !== "idle" ? (
        <p className="text-status-danger break-words">
          {t("sync.worker.lastError", { message: worker.last_error.message })}
        </p>
      ) : null}
      {actionError ? (
        <p className="text-status-danger break-words">
          {t("sync.worker.actionFailed", { message: actionError })}
        </p>
      ) : null}
      <p className="text-2xs text-text-muted">{t("sync.worker.runsInBackground")}</p>
    </section>
  );
}
