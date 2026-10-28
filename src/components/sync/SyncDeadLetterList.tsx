/**
 * Settings → sync diagnostics: the changes of this device given up after their delivery attempts
 * (outbox `dead_letter`), each named by its record, with "send again" on the item itself
 * (`SyncReplayButton`, sync.replay). Listing them is `list_outbox_items` (sync.view). Each one also
 * has its `OUTBOX_DEAD_LETTER` conflict in the conflict list: a replay from either place sends the
 * change and reopens that conflict (`replay_sync_failures`).
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { usePermissions } from "@/hooks/use-permissions";
import { listOutboxItems } from "@/services/sync-service";
import type { SyncOutboxItem } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { SyncReplayButton } from "./SyncReplayButton";
import { KNOWN_SYNC_ENTITY_TYPES, payloadIdentity } from "./sync-conflict-display";

/** Items listed (the worker card counts them all). */
const LISTED_ITEMS = 50;

export interface SyncDeadLetterListProps {
  /** Given-up changes the worker counts: the list reloads when it changes. */
  count: number;
}

export function SyncDeadLetterList({ count }: SyncDeadLetterListProps) {
  const { t } = useTranslation("shell");
  const canReplay = usePermissions().can(P.SYNC_REPLAY);
  const [items, setItems] = useState<SyncOutboxItem[] | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      setItems(await listOutboxItems({ status: "dead_letter", limit: LISTED_ITEMS }));
    } catch (err) {
      setLoadError(err);
    }
  }, []);

  useEffect(() => {
    if (count > 0) void load();
  }, [count, load]);

  if (count <= 0) return null;
  if (loadError != null) {
    return (
      <ErrorState
        error={loadError}
        title={t("sync.replay.deadLetterLoadError")}
        onRetry={() => void load()}
      />
    );
  }
  if (!items) return <LoadingState label={t("sync.replay.deadLetterLoading")} />;
  if (items.length === 0) return null;

  return (
    <section className="rounded-md border border-surface-border bg-surface-0 p-3">
      <h3 className="mb-2 text-sm font-medium text-text-primary">
        {t("sync.replay.deadLetterTitle", { count })}
      </h3>
      <ul className="space-y-3">
        {items.map((item) => {
          const id = payloadIdentity(item.payload_json);
          const entity = t(
            `sync.entity.${KNOWN_SYNC_ENTITY_TYPES.has(item.entity_type) ? item.entity_type : "other"}`,
          );
          return (
            <li
              key={item.id}
              className="rounded-md border border-surface-border bg-surface-1 p-3 text-sm text-text-primary"
            >
              <p>
                <span>{entity}</span>
                {id.code ? <span className="ms-1 font-mono text-xs">{id.code}</span> : null}
                {id.name ? <span className="ms-1">— {id.name}</span> : null}
              </p>
              <p className="mt-1 text-xs text-text-muted">
                {t("sync.replay.attempts", { count: item.attempt_count })}
              </p>
              <details className="mt-1 text-xs text-text-muted">
                <summary className="cursor-pointer select-none">
                  {t("sync.technicalDetails")}
                </summary>
                <p className="mt-1 break-all font-mono">
                  {item.entity_type} · {item.entity_sync_id} · {item.operation}
                </p>
                {/* Server-controlled text (the delivery failure): shown only here, for support. */}
                {item.last_error ? (
                  <p className="mt-1 break-all font-mono">{item.last_error}</p>
                ) : null}
              </details>
              {canReplay ? (
                <div className="mt-3 flex flex-wrap gap-2">
                  <SyncReplayButton
                    target={{ kind: "outbox", outboxId: item.id }}
                    record={id.code ?? id.name ?? entity}
                  />
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
