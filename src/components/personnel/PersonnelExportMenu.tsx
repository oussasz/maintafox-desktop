import { Download } from "lucide-react";
import { useCallback } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { saveBlobWithDialog } from "@/export/download";
import { isExportCancelled } from "@/export/types";
import { appErrorReason } from "@/lib/app-error";
import { exportWorkforceReportCsv } from "@/services/personnel-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

export function PersonnelExportMenu() {
  const { t } = useTranslation("personnel");

  const handleExport = useCallback(
    async (kind: "summary" | "skills_gap" | "kpi") => {
      let csv: string;
      try {
        csv = await exportWorkforceReportCsv(kind);
      } catch {
        return; // The IPC layer already reported the failure.
      }
      try {
        const path = await saveBlobWithDialog(
          new Blob([csv], { type: "text/csv;charset=utf-8;" }),
          `workforce-${kind}.csv`,
          {
            title: t("reports.action.export"),
            filters: [{ name: "CSV", extensions: ["csv"] }],
          },
        );
        pushAppToast({ title: t("reports.export.saved"), description: path, variant: "success" });
      } catch (err) {
        if (isExportCancelled(err)) return;
        pushAppToast({
          title: t("reports.export.failed"),
          description: appErrorReason(err),
          variant: "destructive",
        });
      }
    },
    [t],
  );

  return (
    <PermissionGate permission={P.PER_REPORT}>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="outline" className="h-11 gap-1.5">
            <Download className="h-4 w-4" aria-hidden />
            {t("reports.action.export")}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onClick={() => void handleExport("summary")}>
            {t("reports.export.summary")}
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => void handleExport("skills_gap")}>
            {t("reports.export.skillsGap")}
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => void handleExport("kpi")}>
            {t("reports.export.kpi")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </PermissionGate>
  );
}
