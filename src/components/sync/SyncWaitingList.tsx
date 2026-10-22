/**
 * Sync centre: the received changes that wait, never as a bare count (0.9.0 test finding 4).
 *
 * Every waiting row is counted under what it waits for (`list_waiting_inbox_rows`,
 * `sync::waiting`): an open conflict named by its number ("Waiting for conflict #12 to be
 * resolved"), a record not received yet, one refused here, an app update… The oldest rows are
 * listed with their record and reason; the recorded text stays in the technical details.
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { listWaitingInboxRows } from "@/services/sync-service";
import type { SyncWaitTarget, SyncWaitingInboxReport } from "@shared/ipc-types";

import {
  KNOWN_SYNC_ENTITY_TYPES,
  KNOWN_WAIT_REASONS,
  payloadIdentity,
} from "./sync-conflict-display";

/** Rows listed under the groups (the groups count them all). */
const LISTED_ROWS = 200;

export interface SyncWaitingListProps {
  /** Waiting rows the summary counts: the list reloads when it changes. */
  count: number;
}

export function SyncWaitingList({ count }: SyncWaitingListProps) {
  const { t } = useTranslation("shell");
  const [report, setReport] = useState<SyncWaitingInboxReport | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      setReport(await listWaitingInboxRows(LISTED_ROWS));
    } catch (err) {
      setLoadError(err);
    }
  }, []);

  useEffect(() => {
    if (count > 0) void load();
  }, [count, load]);

  const entityLabel = (entityType: string | null) =>
    t(
      `sync.entity.${entityType && KNOWN_SYNC_ENTITY_TYPES.has(entityType) ? entityType : "other"}`,
    );

  const reasonText = (target: SyncWaitTarget) => {
    switch (target.state) {
      case "conflict":
        return t("sync.waiting.state.conflict", {
          ref: `${t("sync.conflictRef", { id: target.conflict_id ?? "?" })} (${entityLabel(target.entity_type)})`,
        });
      case "refused":
      case "not_received":
        return t(`sync.waiting.state.${target.state}`, {
          record: entityLabel(target.entity_type),
        });
      case "ready":
        return t("sync.waiting.state.ready");
      default:
        return t(
          `sync.waiting.own.${
            target.reason_code && KNOWN_WAIT_REASONS.has(target.reason_code)
              ? target.reason_code
              : "other"
          }`,
        );
    }
  };

  if (count <= 0) return null;
  if (loadError != null) {
    return (
      <ErrorState
        error={loadError}
        title={t("sync.waiting.loadError")}
        onRetry={() => void load()}
      />
    );
  }
  if (!report) return <LoadingState label={t("sync.waiting.loading")} />;
  if (report.total === 0) return null;

  const unlisted = report.total - report.rows.length;
  return (
    <section className="rounded-md border border-surface-border bg-surface-2/60 px-3 py-2 text-xs text-text-secondary">
      <p className="text-text-primary">{t("sync.waiting.title", { count: report.total })}</p>
      <ul className="mt-2 space-y-1">
        {report.groups.map((g) => (
          <li
            key={`${g.target.state}:${g.target.conflict_id ?? ""}:${g.target.entity_type ?? ""}:${g.target.entity_sync_id ?? ""}:${g.target.reason_code ?? ""}`}
            className="flex items-start justify-between gap-2"
          >
            <span>{reasonText(g.target)}</span>
            <span className="shrink-0 font-mono text-text-muted">{g.count}</span>
          </li>
        ))}
      </ul>
      <details className="mt-2">
        <summary className="cursor-pointer select-none text-text-muted">
          {t("sync.waiting.showRows")}
        </summary>
        <ul className="mt-1 space-y-1">
          {report.rows.map((r) => {
            const id = payloadIdentity(r.payload_json);
            return (
              <li key={r.inbox_id} className="border-t border-surface-border pt-1">
                <p className="text-text-primary">
                  <span>{entityLabel(r.entity_type)}</span>
                  {id.code ? <span className="ms-1 font-mono">{id.code}</span> : null}
                  {id.name ? <span className="ms-1">— {id.name}</span> : null}
                </p>
                <p>{reasonText(r.target)}</p>
                <details className="text-text-muted">
                  <summary className="cursor-pointer select-none">
                    {t("sync.technicalDetails")}
                  </summary>
                  <p className="mt-1 break-all font-mono">
                    {r.entity_type} · {r.entity_sync_id} · {r.reason_code ?? ""}
                  </p>
                  {r.detail_message ? (
                    <p className="mt-1 break-all font-mono">{r.detail_message}</p>
                  ) : null}
                </details>
              </li>
            );
          })}
        </ul>
        {unlisted > 0 ? (
          <p className="mt-1 text-text-muted">{t("sync.waiting.moreRows", { count: unlisted })}</p>
        ) : null}
      </details>
    </section>
  );
}
