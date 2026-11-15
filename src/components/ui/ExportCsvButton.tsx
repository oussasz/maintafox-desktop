/**
 * "Exporter (Excel/CSV)" button shared by the main lists.
 *
 * It writes the rows the screen currently shows (the caller builds them with readable labels) to
 * a semicolon CSV that a French Excel opens directly. Hidden unless the user holds `permission`.
 */

import { Download } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import { type CsvCell, saveCsvWithDialog } from "@/export/csv";
import { isExportCancelled } from "@/export/types";
import { appErrorReason } from "@/lib/app-error";
import { pushAppToast } from "@/store/app-toast-store";
import type { PermissionName } from "@shared/rbac/permissions.generated";

export interface ExportCsvButtonProps {
  /** Suggested file name, with the .csv extension. */
  fileName: string;
  /** Column titles, already translated. */
  header: string[];
  /** The rows as shown now (read when the button is pressed). */
  getRows: () => CsvCell[][];
  /** Right that already opens the list. */
  permission: PermissionName;
  /** Rows the list holds in all, when the screen loaded only part of them. */
  total?: number;
  /** True when more rows exist than the screen loaded (no total known). */
  moreAvailable?: boolean;
}

export function ExportCsvButton({
  fileName,
  header,
  getRows,
  permission,
  total,
  moreAvailable = false,
}: ExportCsvButtonProps) {
  const { t } = useTranslation("common");
  const [busy, setBusy] = useState(false);

  const handleExport = useCallback(async () => {
    const rows = getRows();
    if (rows.length === 0) {
      pushAppToast({ title: t("listExport.empty"), variant: "destructive" });
      return;
    }
    setBusy(true);
    try {
      const path = await saveCsvWithDialog(fileName, header, rows, t("listExport.title"));
      const partialOfTotal = total != null && total > rows.length;
      const partialText = partialOfTotal
        ? t("listExport.truncated", { shown: rows.length, total })
        : moreAvailable
          ? t("listExport.truncatedUnknown", { shown: rows.length })
          : null;
      pushAppToast({
        title: t("listExport.saved"),
        description: partialText ? `${path}. ${partialText}` : path,
        variant: "success",
      });
    } catch (err) {
      if (isExportCancelled(err)) return;
      pushAppToast({
        title: t("listExport.failed"),
        description: appErrorReason(err),
        variant: "destructive",
      });
    } finally {
      setBusy(false);
    }
  }, [fileName, getRows, header, moreAvailable, t, total]);

  return (
    <PermissionGate permission={permission}>
      <Button
        variant="outline"
        className="h-11 gap-1.5"
        disabled={busy}
        onClick={() => void handleExport()}
      >
        <Download className="h-4 w-4" aria-hidden />
        {t("listExport.button")}
      </Button>
    </PermissionGate>
  );
}
