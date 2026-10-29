import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";

import {
  SYNC_WORKER_STATUS_EVENT,
  SyncWorkerStatusSchema,
  getSyncWorkerStatus,
  runSyncNow,
} from "@/services/sync-service";
import { toErrorMessage } from "@/utils/errors";
import type { SyncWorkerStatus } from "@shared/ipc-types";

import { useMobileT } from "../i18n";

import { ErrorBox } from "./Shell";

/** Sync state, last success, pending items and conflicts, with a "synchronise now" button. */
export function SyncCard() {
  const { t, language } = useMobileT();
  const [status, setStatus] = useState<SyncWorkerStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [runError, setRunError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);

  const read = useCallback(async () => {
    try {
      setStatus(await getSyncWorkerStatus());
      setStatusError(null);
    } catch (err) {
      setStatusError(toErrorMessage(err));
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
        setStatusError(parsed.error.message);
      }
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((err: unknown) => setStatusError(toErrorMessage(err)));
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
      setRunError(toErrorMessage(err));
    } finally {
      setStarting(false);
    }
  }

  const running = starting || status?.state === "running";
  const lastSuccess = status?.last_success_at
    ? new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(
        new Date(status.last_success_at),
      )
    : t("sync.never");

  return (
    <section className="mf-card" aria-labelledby="mf-sync-title">
      <h2 id="mf-sync-title" className="mf-card__title">
        {t("sync.title")}
      </h2>
      {statusError ? <ErrorBox title={t("sync.statusError")} message={statusError} /> : null}
      {status ? (
        <dl className="mf-facts">
          <dt>{t("sync.state.label")}</dt>
          <dd>{t(`sync.state.${status.state}`)}</dd>
          <dt>{t("sync.lastSuccess")}</dt>
          <dd>{lastSuccess}</dd>
          <dt>{t("sync.pending")}</dt>
          <dd>{status.pending_outbox_count}</dd>
          <dt>{t("sync.conflicts")}</dt>
          <dd>{status.unresolved_conflict_count}</dd>
          <dt>{t("sync.deadLetter")}</dt>
          <dd>{status.dead_letter_outbox_count}</dd>
        </dl>
      ) : null}
      {status?.blocker ? (
        <p className="mf-warning">{t("sync.blocker", { blocker: status.blocker })}</p>
      ) : null}
      {status?.last_error ? (
        <ErrorBox title={t("sync.lastError")} message={status.last_error.message} />
      ) : null}
      {runError ? <ErrorBox title={t("sync.runError")} message={runError} /> : null}
      <button
        type="button"
        className="mf-btn mf-btn--primary"
        disabled={running}
        onClick={() => void onSyncNow()}
      >
        {running ? t("sync.syncing") : t("sync.now")}
      </button>
    </section>
  );
}
