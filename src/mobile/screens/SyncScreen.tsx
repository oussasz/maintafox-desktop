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
import { useMemo, useState } from "react";

import { runSyncNow } from "@/services/sync-service";
import type { SyncWorkerStatus } from "@shared/ipc-types";

import { useNow, useSyncStatus } from "../hooks/use-sync-status";
import { useMobileT } from "../i18n";
import { formatRelativeTime } from "../lib/format";
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
  "phone_removed",
];

function StateHeader({ status }: { status: SyncWorkerStatus }) {
  const { t, language } = useMobileT();
  const now = useNow();
  const last = formatRelativeTime(status.last_success_at, now, language, t);
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
  const { status, error: statusError, refresh } = useSyncStatus();
  const [runError, setRunError] = useState<UserError | null>(null);
  const [starting, setStarting] = useState(false);

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
      {statusError ? <ErrorBanner error={statusError} onRetry={() => void refresh()} /> : null}
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
