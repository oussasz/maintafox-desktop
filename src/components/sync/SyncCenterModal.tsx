/**
 * Sync centre (top bar): worker status, rejected / deferred counts and the conflicts to review.
 */
import { useTranslation } from "react-i18next";

import { SyncConflictList } from "@/components/sync/SyncConflictList";
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

        {deferredInboxCount > 0 ? (
          <p className="rounded-md border border-surface-border bg-surface-2/60 px-3 py-2 text-xs text-text-muted">
            {t("sync.deferredInbox", { count: deferredInboxCount })}
          </p>
        ) : null}

        <SyncConflictList />
      </DialogContent>
    </Dialog>
  );
}
