import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  CheckCircle2,
  CloudUpload,
  GitMerge,
  PauseCircle,
  RefreshCw,
  WifiOff,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  SYNC_WORKER_STATUS_EVENT,
  SyncWorkerStatusSchema,
  getSyncWorkerStatus,
  runSyncNow,
} from "@/services/sync-service";
import type { SyncWorkerStatus } from "@shared/ipc-types";

import { useMobileT } from "../i18n";
import { formatDateTime } from "../lib/format";
import { type UserError, toUserError } from "../lib/user-error";
import { ErrorBanner } from "../ui/feedback";
import { Actions, Banner, Button, EmptyState, Progress, Row, Screen, Section } from "../ui/kit";

/** Blockers the engine reports (`SyncWorkerStatus.blocker`); others get the generic sentence. */
const KNOWN_BLOCKERS = [
  "not_licensed",
  "not_activated",
  "activation_refused",
  "tenant_config_mismatch",
  "not_configured",
];

function StateHeader({ status }: { status: SyncWorkerStatus }) {
  const { t, language } = useMobileT();
  const last = formatDateTime(status.last_success_at, language);
  const subtitle = last ? t("sync.lastSuccessAt", { date: last }) : t("sync.never");
  const title = t(`sync.state.${status.state}`);
  switch (status.state) {
    case "running":
    case "starting":
      return (
        <EmptyState icon={<RefreshCw size={36} aria-hidden />} title={title} text={subtitle} />
      );
    case "offline":
    case "retrying":
      return (
        <EmptyState
          icon={<WifiOff size={36} aria-hidden />}
          tone="orange"
          title={title}
          text={subtitle}
        />
      );
    case "paused":
      return (
        <EmptyState
          icon={<PauseCircle size={36} aria-hidden />}
          tone="orange"
          title={title}
          text={subtitle}
        />
      );
    case "blocked":
    case "error":
      return (
        <EmptyState
          icon={<XCircle size={36} aria-hidden />}
          tone="red"
          title={title}
          text={subtitle}
        />
      );
    default:
      return (
        <EmptyState
          icon={<CheckCircle2 size={36} aria-hidden />}
          tone="green"
          title={title}
          text={subtitle}
        />
      );
  }
}

/** Sync state, last success, what waits to be sent, conflicts, with a "synchronise now" button. */
export function SyncScreen() {
  const { t } = useMobileT();
  const [status, setStatus] = useState<SyncWorkerStatus | null>(null);
  const [statusError, setStatusError] = useState<UserError | null>(null);
  const [runError, setRunError] = useState<UserError | null>(null);
  const [starting, setStarting] = useState(false);

  const read = useCallback(async () => {
    try {
      setStatus(await getSyncWorkerStatus());
      setStatusError(null);
    } catch (err) {
      setStatusError(toUserError(err, "sync.status"));
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void read();
    void listen<unknown>(SYNC_WORKER_STATUS_EVENT, (event) => {
      const parsed = SyncWorkerStatusSchema.safeParse(event.payload);
      if (parsed.success) {
        setStatus(parsed.data as SyncWorkerStatus);
        setStatusError(null);
      } else {
        setStatusError(toUserError(parsed.error, "sync.status_event"));
      }
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((err: unknown) => setStatusError(toUserError(err, "sync.listen")));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [read]);

  async function onSyncNow() {
    setStarting(true);
    setRunError(null);
    try {
      await runSyncNow();
    } catch (err) {
      setRunError(toUserError(err, "sync.run"));
    } finally {
      setStarting(false);
    }
  }

  const running = starting || status?.state === "running";
  const lastRaw = status?.last_error ?? null;
  const lastError = useMemo(
    () => (lastRaw ? toUserError(lastRaw, "sync.last_error") : null),
    [lastRaw],
  );

  return (
    <Screen title={t("sync.title")} withTabs>
      {statusError ? <ErrorBanner error={statusError} onRetry={() => void read()} /> : null}
      {status === null && !statusError ? <Progress text={t("common.loading")} /> : null}
      {status ? <StateHeader status={status} /> : null}
      {status?.blocker ? (
        <Banner
          tone="warning"
          icon={<AlertTriangle size={22} aria-hidden />}
          title={t("sync.blockedTitle")}
        >
          {KNOWN_BLOCKERS.includes(status.blocker)
            ? t(`sync.blockers.${status.blocker}`)
            : t("sync.blockers.other")}
        </Banner>
      ) : null}
      {lastError && status?.state !== "idle" ? <ErrorBanner error={lastError} /> : null}
      {status ? (
        <Section header={t("sync.details")}>
          <Row
            icon={<CloudUpload aria-hidden />}
            title={t("sync.pending")}
            detail={status.pending_outbox_count}
          />
          <Row
            icon={<GitMerge aria-hidden />}
            iconColor="var(--mf-orange)"
            title={t("sync.conflicts")}
            detail={status.unresolved_conflict_count}
          />
          <Row
            icon={<XCircle aria-hidden />}
            iconColor="var(--mf-red)"
            title={t("sync.deadLetter")}
            detail={status.dead_letter_outbox_count}
          />
        </Section>
      ) : null}
      {runError ? <ErrorBanner error={runError} /> : null}
      <Actions>
        <Button disabled={running} onClick={() => void onSyncNow()}>
          <RefreshCw size={20} aria-hidden />
          {running ? t("sync.syncing") : t("sync.now")}
        </Button>
      </Actions>
    </Screen>
  );
}
