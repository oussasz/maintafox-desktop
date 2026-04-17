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
import { exportWorkforceReportCsv } from "@/services/personnel-service";

function downloadCsv(fileName: string, data: string) {
  const blob = new Blob([data], { type: "text/csv;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}

export function PersonnelExportMenu() {
  const { t } = useTranslation("personnel");

  const handleExport = useCallback(async (kind: "summary" | "skills_gap" | "kpi") => {
    const csv = await exportWorkforceReportCsv(kind);
    downloadCsv(`workforce-${kind}.csv`, csv);
  }, []);

  return (
    <PermissionGate permission="per.report">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="outline" size="sm" className="gap-1.5">
            <Download className="h-3.5 w-3.5" />
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
