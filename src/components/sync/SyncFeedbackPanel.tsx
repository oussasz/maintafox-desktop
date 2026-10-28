import { Download, LifeBuoy, ShieldAlert } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { SupportBundleDialog } from "@/components/SupportBundleDialog";
import { SyncConflictList } from "@/components/sync/SyncConflictList";
import { SyncDeadLetterList } from "@/components/sync/SyncDeadLetterList";
import { SyncWorkerStatusCard } from "@/components/sync/SyncWorkerStatusCard";
import { Button } from "@/components/ui/button";
import { saveBlobWithDialog } from "@/export/download";
import { isExportCancelled } from "@/export/types";
import { describeAppError } from "@/lib/app-error";
import { pushAppToast } from "@/store/app-toast-store";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import { P } from "@shared/rbac/permissions.generated";

/**
 * Settings → sync diagnostics: worker status, the changes given up after their delivery attempts
 * and the conflict list. "Send again" sits on each refused item, never as one global button.
 */
export function SyncFeedbackPanel() {
  const { t } = useTranslation("shell");
  const exportDiagnostics = useSyncOrchestratorStore((s) => s.exportDiagnostics);
  const deadLetterCount = useSyncOrchestratorStore((s) => s.worker?.dead_letter_outbox_count ?? 0);

  const [supportOpen, setSupportOpen] = useState(false);
  const [exporting, setExporting] = useState(false);

  const exportFile = async () => {
    setExporting(true);
    try {
      const blob = new Blob([JSON.stringify(exportDiagnostics(), null, 2)], {
        type: "application/json",
      });
      const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
      const path = await saveBlobWithDialog(blob, `sync-diagnostics-${stamp}.json`, {
        title: t("sync.panel.export"),
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      pushAppToast({ title: t("sync.panel.exported"), description: path, variant: "success" });
    } catch (err) {
      if (!isExportCancelled(err)) {
        pushAppToast({
          title: t("sync.panel.exportFailed"),
          description: describeAppError(err).title,
          variant: "destructive",
        });
      }
    } finally {
      setExporting(false);
    }
  };

  return (
    <section className="rounded-xl border border-surface-border bg-surface-1 p-4 space-y-4">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-text-primary">{t("sync.panel.title")}</h2>
          <p className="text-xs text-text-muted">{t("sync.panel.description")}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            className="h-10 gap-2"
            disabled={exporting}
            onClick={() => void exportFile()}
          >
            <Download className="h-4 w-4" aria-hidden />
            {t("sync.panel.export")}
          </Button>
          <Button
            type="button"
            variant="outline"
            className="h-10 gap-2"
            onClick={() => setSupportOpen(true)}
          >
            <LifeBuoy className="h-4 w-4" aria-hidden />
            {t("sync.panel.support")}
          </Button>
        </div>
      </header>

      <SyncWorkerStatusCard />

      {/* The conflict and given-up lists are `list_sync_conflicts` / `list_outbox_items` (sync.view). */}
      <PermissionGate permission={P.SYNC_VIEW}>
        <SyncDeadLetterList count={deadLetterCount} />
      </PermissionGate>
      <PermissionGate permission={P.SYNC_VIEW}>
        <div className="rounded-md border border-surface-border bg-surface-0 p-3">
          <h3 className="mb-2 flex items-center gap-2 text-sm font-medium text-text-primary">
            <ShieldAlert className="h-4 w-4 text-tone-warning-fg" aria-hidden />
            {t("sync.panel.conflictsTitle")}
          </h3>
          <div className="max-h-[28rem] overflow-auto">
            <SyncConflictList showTriageActions />
          </div>
        </div>
      </PermissionGate>

      <SupportBundleDialog open={supportOpen} onClose={() => setSupportOpen(false)} />
    </section>
  );
}
