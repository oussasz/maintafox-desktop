/**
 * OrgExportMenu.tsx
 *
 * GAP ORG-03 — Dropdown menu for print / PNG / CSV export of the org chart.
 */

import { Download, FileImage, FileSpreadsheet, Printer } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useOrgDesignerStore } from "@/stores/org-designer-store";

interface OrgExportMenuProps {
  treeContainerId: string;
}

export function OrgExportMenu({ treeContainerId }: OrgExportMenuProps) {
  const { t } = useTranslation("org");
  const snapshot = useOrgDesignerStore((s) => s.snapshot);

  const handlePrint = () => {
    window.print();
  };

  const handleExportPng = async () => {
    const el = document.getElementById(treeContainerId);
    if (!el) return;
    try {
      const { toPng } = await import("html-to-image");
      const dataUrl = await toPng(el, { backgroundColor: "#ffffff" });
      const link = document.createElement("a");
      link.download = "org-chart.png";
      link.href = dataUrl;
      link.click();
    } catch {
      console.error("PNG export failed");
    }
  };

  const handleExportCsv = () => {
    if (!snapshot?.nodes.length) return;
    const headers = ["node_id", "code", "name", "type", "parent_id", "depth", "status"];
    const rows = snapshot.nodes.map((n) =>
      [
        n.node_id,
        n.code,
        `"${n.name.replace(/"/g, '""')}"`,
        n.node_type_code,
        n.parent_id ?? "",
        n.depth,
        n.status,
      ].join(","),
    );
    const csv = [headers.join(","), ...rows].join("\n");
    const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.download = "org-nodes.csv";
    link.href = url;
    link.click();
    URL.revokeObjectURL(url);
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="gap-1.5">
          <Download className="h-3.5 w-3.5" />
          {t("export.title")}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onClick={handlePrint}>
          <Printer className="mr-2 h-4 w-4" />
          {t("export.print")}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => void handleExportPng()}>
          <FileImage className="mr-2 h-4 w-4" />
          {t("export.exportPng")}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={handleExportCsv}>
          <FileSpreadsheet className="mr-2 h-4 w-4" />
          {t("export.exportCsv")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
