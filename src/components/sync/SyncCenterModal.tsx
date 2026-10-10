import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { SyncWorkerStatusCard } from "@/components/sync/SyncWorkerStatusCard";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { ResolveSyncConflictInput, SyncConflictRecord } from "@shared/ipc-types";

interface SyncCenterModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** Conflict kinds with a dedicated explanation in `shell:sync.conflictKind`. */
const KNOWN_CONFLICT_KINDS = new Set([
  "CONCURRENT_EDIT",
  "SERVER_CONFLICT",
  "STALE_INBOUND",
  "EQUAL_VERSION_DIVERGENCE",
  "MATERIALIZE_ERROR",
  "INVALID_INBOUND_PAYLOAD",
  "OUTBOX_DEAD_LETTER",
  "AUTHORITY_MISMATCH",
]);

export function SyncCenterModal({ open, onOpenChange }: SyncCenterModalProps) {
  const { t } = useTranslation("shell");
  const conflictInbox = useSyncOrchestratorStore((s) => s.conflictInbox);
  const rejectedOutboxCount = useSyncOrchestratorStore((s) => s.rejectedOutboxCount);
  const deferredInboxCount = useSyncOrchestratorStore((s) => s.deferredInboxCount);
  const resolveConflictAction = useSyncOrchestratorStore((s) => s.resolveConflictAction);
  const refreshInbox = useSyncOrchestratorStore((s) => s.refreshInbox);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [errorById, setErrorById] = useState<Record<number, string>>({});

  useEffect(() => {
    if (open) {
      void refreshInbox();
    }
  }, [open, refreshInbox]);

  const resolve = async (c: SyncConflictRecord, action: ResolveSyncConflictInput["action"]) => {
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
        resolution_note: null,
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setErrorById((prev) => ({ ...prev, [c.id]: message }));
    } finally {
      setBusyId(null);
    }
  };

  function conflictLabel(c: SyncConflictRecord): string {
    return `${c.entity_type} · ${c.entity_sync_id}`;
  }

  function conflictKind(c: SyncConflictRecord): string {
    const key = KNOWN_CONFLICT_KINDS.has(c.conflict_type) ? c.conflict_type : "other";
    return t(`sync.conflictKind.${key}`);
  }

  function versionLabel(version: number | null): string {
    return version === null ? t("sync.versionUnknown") : `v${version}`;
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("sync.syncCenterTitle")}</DialogTitle>
          <DialogDescription className="text-xs text-text-muted">
            {t("sync.syncCenterConflicts")} · {t("sync.syncCenterRejected")}
          </DialogDescription>
        </DialogHeader>

        <SyncWorkerStatusCard />

        {rejectedOutboxCount > 0 ? (
          <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-text-primary">
            {t("sync.syncCenterRejected")}: <strong>{rejectedOutboxCount}</strong>
          </p>
        ) : null}

        {deferredInboxCount > 0 ? (
          <p className="rounded-md border border-surface-border bg-surface-2/60 px-3 py-2 text-xs text-text-muted">
            {t("sync.deferredInbox", { count: deferredInboxCount })}
          </p>
        ) : null}

        {conflictInbox.length === 0 && rejectedOutboxCount === 0 ? (
          <p className="text-sm text-text-muted">{t("sync.noConflicts")}</p>
        ) : null}

        <div className="space-y-3">
          {conflictInbox.map((c) => (
            <div
              key={c.id}
              className="rounded-md border border-surface-border bg-surface-2/60 p-3 text-xs text-text-primary"
            >
              <p className="font-medium">{conflictKind(c)}</p>
              <p className="mt-1 text-text-muted">{conflictLabel(c)}</p>
              <p className="mt-1 text-text-muted">
                {t("sync.conflictVersions", {
                  local: versionLabel(c.local_row_version),
                  remote: versionLabel(c.inbound_row_version),
                })}
              </p>
              {errorById[c.id] ? (
                <p className="mt-2 text-status-danger">
                  {t("sync.resolveFailed", { message: errorById[c.id] })}
                </p>
              ) : null}
              <div className="mt-3 flex flex-wrap gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busyId === c.id}
                  onClick={() => void resolve(c, "accept_local")}
                >
                  {t("sync.resolveLocal")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busyId === c.id || c.inbound_payload_json === null}
                  onClick={() => void resolve(c, "accept_remote")}
                >
                  {t("sync.resolveRemote")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  disabled={busyId === c.id}
                  onClick={() => void resolve(c, "dismiss")}
                >
                  {t("sync.dismissConflict")}
                </Button>
              </div>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
