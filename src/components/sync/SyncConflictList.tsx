/**
 * The one conflict review list of the app (sync centre dialog and Settings → sync diagnostics).
 *
 * Each conflict reads as: what happened (translated kind), which record (type + number + name
 * from the payload — never the uuid), the two versions, and the choices. Choices that overwrite
 * or drop a version ask for confirmation first; "retry later" and "escalate" (diagnostics only)
 * are reversible and run directly. A failed inbox load is an error with retry, never
 * "nothing to review".
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { ResolveSyncConflictInput, SyncConflictRecord } from "@shared/ipc-types";

import {
  KNOWN_CONFLICT_KINDS,
  KNOWN_SYNC_ENTITY_TYPES,
  clashingNumber,
  conflictRecordIdentity,
} from "./sync-conflict-display";

type ConflictAction = ResolveSyncConflictInput["action"];

/** Actions that replace or drop a version: confirmed before they run. */
const CONFIRMED_ACTIONS = new Set<ConflictAction>([
  "accept_local",
  "accept_remote",
  "renumber_local",
  "dismiss",
]);

export interface SyncConflictListProps {
  /** Also offer "retry later" and "escalate" (Settings → sync diagnostics). */
  showTriageActions?: boolean;
  /** Note stored with the resolution; none when omitted. */
  withResolutionNote?: boolean;
  /** Load the inbox when the list mounts (default true). */
  loadOnMount?: boolean;
}

export function SyncConflictList({
  showTriageActions = false,
  withResolutionNote = false,
  loadOnMount = true,
}: SyncConflictListProps) {
  const { t } = useTranslation("shell");
  const { formatDateTime } = useFormatters();
  const conflictInbox = useSyncOrchestratorStore((s) => s.conflictInbox);
  const refreshInbox = useSyncOrchestratorStore((s) => s.refreshInbox);
  const resolveConflictAction = useSyncOrchestratorStore((s) => s.resolveConflictAction);

  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [errorById, setErrorById] = useState<Record<number, string>>({});
  const [pending, setPending] = useState<{
    conflict: SyncConflictRecord;
    action: ConflictAction;
  } | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      await refreshInbox();
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, [refreshInbox]);

  useEffect(() => {
    if (loadOnMount) void load();
  }, [loadOnMount, load]);

  const actionLabel = (action: string) =>
    t(
      `sync.panel.action.${
        [
          "accept_local",
          "accept_remote",
          "renumber_local",
          "retry_later",
          "escalate",
          "dismiss",
        ].includes(action)
          ? action
          : "review"
      }`,
    );

  const run = async (c: SyncConflictRecord, action: ConflictAction) => {
    setBusyId(c.id);
    setErrorById((prev) => {
      const next = { ...prev };
      delete next[c.id];
      return next;
    });
    try {
      await resolveConflictAction({
        conflict_id: c.id,
        expected_row_version: c.row_version,
        action,
        resolution_note: withResolutionNote
          ? t("sync.panel.resolutionNote", { action: actionLabel(action) })
          : null,
      });
    } catch (err) {
      setErrorById((prev) => ({ ...prev, [c.id]: describeAppError(err).title }));
      throw err;
    } finally {
      setBusyId(null);
    }
  };

  const choose = (c: SyncConflictRecord, action: ConflictAction) => {
    if (CONFIRMED_ACTIONS.has(action)) {
      setPending({ conflict: c, action });
    } else {
      void run(c, action).catch(() => {});
    }
  };

  const kindOf = (c: SyncConflictRecord) =>
    t(`sync.conflictKind.${KNOWN_CONFLICT_KINDS.has(c.conflict_type) ? c.conflict_type : "other"}`);

  const entityOf = (c: SyncConflictRecord) =>
    t(`sync.entity.${KNOWN_SYNC_ENTITY_TYPES.has(c.entity_type) ? c.entity_type : "other"}`);

  const versionLabel = (v: number | null) =>
    v === null ? t("sync.versionUnknown") : t("sync.versionNumber", { version: v });

  if (loadError != null && !loading) {
    return (
      <ErrorState error={loadError} title={t("sync.inboxLoadError")} onRetry={() => void load()} />
    );
  }
  if (loading && conflictInbox.length === 0) {
    return <LoadingState label={t("sync.inboxLoading")} />;
  }
  if (conflictInbox.length === 0) {
    return <p className="text-sm text-text-muted">{t("sync.noConflicts")}</p>;
  }

  const confirmText = (() => {
    if (!pending) return "";
    const id = conflictRecordIdentity(pending.conflict);
    const record = id.code ?? id.name ?? entityOf(pending.conflict);
    return t(`sync.confirm.${pending.action}`, { record });
  })();

  return (
    <>
      <ul className="space-y-3" aria-label={t("sync.syncCenterConflicts")}>
        {conflictInbox.map((c) => {
          const id = conflictRecordIdentity(c);
          const recommended = c.recommended_action;
          const primary: ConflictAction[] =
            c.conflict_type === "CODE_CLASH"
              ? ["renumber_local"]
              : ["accept_local", "accept_remote"];
          const actions: ConflictAction[] = [
            ...primary,
            ...(showTriageActions ? (["retry_later", "escalate"] as ConflictAction[]) : []),
            "dismiss",
          ];
          return (
            <li
              key={c.id}
              className="rounded-md border border-surface-border bg-surface-1 p-3 text-sm text-text-primary"
            >
              <div className="flex flex-wrap items-start justify-between gap-2">
                <p className="font-medium">{kindOf(c)}</p>
                {c.status === "escalated" ? (
                  <StatusChip label={t("sync.status.escalated")} tone="attention" />
                ) : null}
              </div>
              <p className="mt-1 text-text-secondary">
                <span>{entityOf(c)}</span>
                {id.code ? <span className="ms-1 font-mono text-xs">{id.code}</span> : null}
                {id.name ? <span className="ms-1">— {id.name}</span> : null}
                {!id.code && !id.name ? (
                  <span className="ms-1 text-text-muted">{t("sync.recordUnidentified")}</span>
                ) : null}
              </p>
              <p className="mt-1 text-xs text-text-muted">
                {t("sync.conflictVersions", {
                  local: versionLabel(c.local_row_version),
                  remote: versionLabel(c.inbound_row_version),
                })}
                {" · "}
                {t("sync.detectedAt", { date: formatDateTime(c.created_at) })}
              </p>
              {c.conflict_type === "CODE_CLASH" ? (
                <p className="mt-1 text-xs text-text-primary">
                  {t("sync.codeClash", { number: clashingNumber(c) ?? t("sync.versionUnknown") })}
                </p>
              ) : null}
              <details className="mt-1 text-xs text-text-muted">
                <summary className="cursor-pointer select-none">
                  {t("sync.technicalDetails")}
                </summary>
                <p className="mt-1 break-all font-mono">
                  {c.entity_type} · {c.entity_sync_id} · {c.conflict_type}
                </p>
              </details>
              {errorById[c.id] ? (
                <p className="mt-2 text-xs text-tone-danger-fg" role="alert">
                  {t("sync.resolveFailed", { message: errorById[c.id] })}
                </p>
              ) : null}
              <div className="mt-3 flex flex-wrap gap-2">
                {actions.map((action) => {
                  const isRecommended = action === recommended;
                  const disabled =
                    busyId === c.id ||
                    (action === "accept_remote" && c.inbound_payload_json === null);
                  return (
                    <Button
                      key={action}
                      type="button"
                      variant={
                        isRecommended ? "default" : action === "dismiss" ? "ghost" : "outline"
                      }
                      className="h-10"
                      disabled={disabled}
                      title={
                        action === "accept_remote" && c.inbound_payload_json === null
                          ? t("sync.noRemoteVersion")
                          : undefined
                      }
                      onClick={() => choose(c, action)}
                    >
                      {actionLabel(action)}
                      {isRecommended ? (
                        <span className="sr-only"> — {t("sync.recommendedShort")}</span>
                      ) : null}
                    </Button>
                  );
                })}
              </div>
              {CONFIRMED_ACTIONS.has(recommended as ConflictAction) ||
              ["retry_later", "escalate"].includes(recommended) ? (
                <p className="mt-1 text-xs text-text-muted">
                  {t("sync.panel.recommended", { action: actionLabel(recommended) })}
                </p>
              ) : null}
            </li>
          );
        })}
      </ul>
      <ConfirmDialog
        open={pending != null}
        onOpenChange={(o) => !o && setPending(null)}
        title={t("sync.confirm.title")}
        description={confirmText}
        confirmLabel={pending ? actionLabel(pending.action) : ""}
        destructive={pending?.action === "dismiss" || pending?.action === "accept_remote"}
        onConfirm={async () => {
          if (pending) await run(pending.conflict, pending.action);
        }}
      />
    </>
  );
}
