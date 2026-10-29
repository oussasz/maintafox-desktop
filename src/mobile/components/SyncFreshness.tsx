import type { SyncWorkerStatus } from "@shared/ipc-types";

import { useNow } from "../hooks/use-sync-status";
import { useMobileT } from "../i18n";
import { formatRelativeTime } from "../lib/format";

/** Sync states where the last success is worth qualifying (the data may be getting older). */
const QUALIFIED_STATES: ReadonlySet<SyncWorkerStatus["state"]> = new Set([
  "offline",
  "retrying",
  "paused",
  "blocked",
  "error",
]);

/**
 * "Synchronisé il y a 5 min", small and muted, under a screen title. It always shows the last
 * successful sync (a failed attempt does not reset it); when the worker is not healthy the state
 * is added. Nothing is shown until the first status answer. Re-rendered every minute.
 */
export function SyncFreshness({ status }: { status: SyncWorkerStatus | null }) {
  const { t, language } = useMobileT();
  const now = useNow();
  if (status === null) return null;
  const when = formatRelativeTime(status.last_success_at, now, language, t);
  const fresh = when ? t("sync.freshness", { when }) : t("sync.never");
  const text = QUALIFIED_STATES.has(status.state)
    ? t("sync.freshnessWithState", { fresh, state: t(`sync.state.${status.state}`) })
    : fresh;
  return <span className="mf-sync-fresh">{text}</span>;
}
