/**
 * Sync centre (top bar): worker status, rejected count, the received changes that wait with what
 * each waits for, stock left below zero by received movements (to reconcile in Inventory) and the
 * conflicts to review.
 */
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { NegativeStockSyncNotice } from "@/components/sync/NegativeStockSyncNotice";
import { SyncConflictList } from "@/components/sync/SyncConflictList";
import { SyncWaitingList } from "@/components/sync/SyncWaitingList";
import { SyncWorkerStatusCard } from "@/components/sync/SyncWorkerStatusCard";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { mfAlert } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import { P } from "@shared/rbac/permissions.generated";

interface SyncCenterModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SyncCenterModal({ open, onOpenChange }: SyncCenterModalProps) {
  const { t } = useTranslation("shell");
  const { formatNumber } = useFormatters();
  const rejectedOutboxCount = useSyncOrchestratorStore((s) => s.rejectedOutboxCount);
  const deferredInboxCount = useSyncOrchestratorStore((s) => s.deferredInboxCount);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("sync.syncCenterTitle")}</DialogTitle>
          <DialogDescription className="text-xs text-text-muted">
            {t("sync.syncCenterDescription")}
          </DialogDescription>
        </DialogHeader>

        <SyncWorkerStatusCard />

        {rejectedOutboxCount > 0 ? (
          <p className={mfAlert.danger}>
            {t("sync.rejectedCount", {
              count: rejectedOutboxCount,
              formatted: formatNumber(rejectedOutboxCount),
            })}
          </p>
        ) : null}

        {open ? <SyncWaitingList count={deferredInboxCount} /> : null}

        {open ? (
          <PermissionGate permission={P.INV_VIEW}>
            <NegativeStockSyncNotice onNavigate={() => onOpenChange(false)} />
          </PermissionGate>
        ) : null}

        <SyncConflictList />
      </DialogContent>
    </Dialog>
  );
}
