import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { SyncConflictRecord } from "@shared/ipc-types";

interface SyncCenterModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SyncCenterModal({ open, onOpenChange }: SyncCenterModalProps) {
  const { t } = useTranslation("shell");
  const conflictInbox = useSyncOrchestratorStore((s) => s.conflictInbox);
  const rejectedOutboxCount = useSyncOrchestratorStore((s) => s.rejectedOutboxCount);
  const resolveConflictAction = useSyncOrchestratorStore((s) => s.resolveConflictAction);
  const refreshInbox = useSyncOrchestratorStore((s) => s.refreshInbox);
  const [busyId, setBusyId] = useState<number | null>(null);

  useEffect(() => {
    if (open) {
      void refreshInbox();
    }
  }, [open, refreshInbox]);

  const resolve = async (c: SyncConflictRecord, action: "accept_local" | "accept_remote") => {
    setBusyId(c.id);
    try {
      await resolveConflictAction({
        conflict_id: c.id,
        expected_row_version: c.row_version,
        action,
        resolution_note: null,
      });
    } finally {
      setBusyId(null);
    }
  };

  function conflictLabel(c: SyncConflictRecord): string {
    return `${c.entity_type} · ${c.entity_sync_id}`;
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

        {rejectedOutboxCount > 0 ? (
          <p className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-xs text-text-primary">
            {t("sync.syncCenterRejected")}: <strong>{rejectedOutboxCount}</strong>
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
              <p className="font-medium">{conflictLabel(c)}</p>
              <p className="mt-1 text-text-muted">
                {c.status} · {c.recommended_action}
              </p>
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
                  disabled={busyId === c.id}
                  onClick={() => void resolve(c, "accept_remote")}
                >
                  {t("sync.resolveRemote")}
                </Button>
              </div>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
