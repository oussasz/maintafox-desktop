/**
 * OrgExportMenu.tsx
 *
 * Export of the organization structure through the shared export transport (src/export):
 *  - Print: a dedicated printable document (indented structure table), not the app window.
 *  - PNG: image of the tree panel, written through the native Save dialog.
 *  - CSV: one line per unit with translated headers, written through the native Save dialog.
 * Success, cancellation and failure are reported with toasts.
 */

import { Download, FileImage, FileSpreadsheet, Printer } from "lucide-react";
import { useTranslation } from "react-i18next";

import { orgNodeStatusLabel } from "@/components/org/org-display";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { exportImage, isExportCancelled, printHtml, saveBlobWithDialog } from "@/export";
import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
import { pushAppToast } from "@/store/app-toast-store";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgDesignerNodeRow } from "@shared/ipc-types";

/** UTF-8 byte-order mark so spreadsheet tools read accents correctly. */
const CSV_BOM = "﻿";
/** Indent per level in the printed structure, in px. */
const PRINT_INDENT_PX = 16;

interface OrgExportMenuProps {
  treeContainerId: string;
}

function csvCell(value: string | number): string {
  const text = String(value);
  return /[",;\n\r]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function fileStamp(): string {
  return new Date().toISOString().slice(0, 10);
}

export function OrgExportMenu({ treeContainerId }: OrgExportMenuProps) {
  const { t } = useTranslation("org");
  const { formatDateTime, formatNumber } = useFormatters();
  const snapshot = useOrgDesignerStore((s) => s.snapshot);
  const nodes = snapshot?.nodes ?? [];
  const hasNodes = nodes.length > 0;

  const reportFailure = (err: unknown) => {
    if (isExportCancelled(err)) return;
    const { title, description } = describeAppError(err);
    pushAppToast({
      title: t("export.failed"),
      description: description ? `${title} — ${description}` : title,
      variant: "destructive",
    });
  };

  const reportSaved = (path: string) => {
    pushAppToast({ title: t("export.saved"), description: path, variant: "success" });
  };

  const parentCode = (node: OrgDesignerNodeRow) =>
    node.parent_id != null ? (nodes.find((n) => n.node_id === node.parent_id)?.code ?? "") : "";

  const handlePrint = async () => {
    if (!hasNodes) return;
    const rows = nodes
      .map(
        (n) => `<tr>
  <td style="padding-left:${n.depth * PRINT_INDENT_PX + 4}px">${escapeHtml(n.name)}</td>
  <td class="mono">${escapeHtml(n.code)}</td>
  <td>${escapeHtml(n.node_type_label)}</td>
  <td>${escapeHtml(orgNodeStatusLabel(t, n.status))}</td>
  <td class="num">${escapeHtml(formatNumber(n.child_count))}</td>
  <td class="num">${escapeHtml(formatNumber(n.active_responsibility_count))}</td>
</tr>`,
      )
      .join("\n");
    const title = t("export.printTitle");
    const html = `<!doctype html>
<html><head><meta charset="utf-8"><title>${escapeHtml(title)}</title>
<style>
  body { font-family: system-ui, sans-serif; font-size: 11pt; color: #111; margin: 16mm; }
  h1 { font-size: 15pt; margin: 0 0 4px; }
  p { margin: 0 0 12px; color: #444; }
  table { border-collapse: collapse; width: 100%; }
  th, td { border-bottom: 1px solid #ccc; padding: 4px; text-align: left; vertical-align: top; }
  th { font-size: 9pt; text-transform: uppercase; color: #444; }
  .mono { font-family: ui-monospace, monospace; font-size: 9pt; }
  .num { text-align: right; }
</style></head>
<body>
<h1>${escapeHtml(title)}</h1>
<p>${escapeHtml(t("export.printedAt", { date: formatDateTime(new Date()) }))}</p>
<table>
<thead><tr>
  <th>${escapeHtml(t("export.columns.name"))}</th>
  <th>${escapeHtml(t("export.columns.code"))}</th>
  <th>${escapeHtml(t("export.columns.level"))}</th>
  <th>${escapeHtml(t("export.columns.status"))}</th>
  <th class="num">${escapeHtml(t("export.columns.subUnits"))}</th>
  <th class="num">${escapeHtml(t("export.columns.responsibilities"))}</th>
</tr></thead>
<tbody>
${rows}
</tbody></table>
</body></html>`;
    try {
      await printHtml(html, { title });
    } catch (err) {
      reportFailure(err);
    }
  };

  const handleExportPng = async () => {
    const el = document.getElementById(treeContainerId);
    if (!el) return;
    try {
      const path = await exportImage({
        filename: `${t("export.fileBase")}-${fileStamp()}.png`,
        mimeType: "image/png",
        saveDialogTitle: t("export.exportPng"),
        render: async () => {
          const { toBlob } = await import("html-to-image");
          const background = getComputedStyle(document.body).backgroundColor;
          const blob = await toBlob(el, { backgroundColor: background });
          if (!blob) throw new Error(t("export.imageFailed"));
          return blob;
        },
      });
      reportSaved(path);
    } catch (err) {
      reportFailure(err);
    }
  };

  const handleExportCsv = async () => {
    if (!hasNodes) return;
    const headers = [
      t("export.columns.code"),
      t("export.columns.name"),
      t("export.columns.level"),
      t("export.columns.parentCode"),
      t("export.columns.depth"),
      t("export.columns.status"),
      t("export.columns.subUnits"),
      t("export.columns.responsibilities"),
      t("export.columns.externalLinks"),
    ];
    const rows = nodes.map((n) =>
      [
        n.code,
        n.name,
        n.node_type_label,
        parentCode(n),
        n.depth + 1,
        orgNodeStatusLabel(t, n.status),
        n.child_count,
        n.active_responsibility_count,
        n.active_binding_count,
      ]
        .map(csvCell)
        .join(","),
    );
    const csv = CSV_BOM + [headers.map(csvCell).join(","), ...rows].join("\r\n");
    try {
      const path = await saveBlobWithDialog(
        new Blob([csv], { type: "text/csv;charset=utf-8" }),
        `${t("export.fileBase")}-${fileStamp()}.csv`,
        { title: t("export.exportCsv"), filters: [{ name: "CSV", extensions: ["csv"] }] },
      );
      reportSaved(path);
    } catch (err) {
      reportFailure(err);
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" className="h-11 gap-1.5">
          <Download className="h-4 w-4" aria-hidden />
          {t("export.title")}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem
          className="min-h-11"
          disabled={!hasNodes}
          onClick={() => void handlePrint()}
        >
          <Printer className="mr-2 h-4 w-4" aria-hidden />
          {t("export.print")}
        </DropdownMenuItem>
        <DropdownMenuItem
          className="min-h-11"
          disabled={!hasNodes}
          onClick={() => void handleExportPng()}
        >
          <FileImage className="mr-2 h-4 w-4" aria-hidden />
          {t("export.exportPng")}
        </DropdownMenuItem>
        <DropdownMenuItem
          className="min-h-11"
          disabled={!hasNodes}
          onClick={() => void handleExportCsv()}
        >
          <FileSpreadsheet className="mr-2 h-4 w-4" aria-hidden />
          {t("export.exportCsv")}
        </DropdownMenuItem>
        {!hasNodes && (
          <p className="px-2 py-1.5 text-xs text-text-muted">{t("export.nothingToExport")}</p>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
