/**
 * WoDiManagementPanel.tsx
 *
 * Collapsible banner showing DI-sourced work orders that are not yet scheduled.
 * Only visible when there are WOs with source_di_id and status in (draft, planned).
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S4.
 */

import { ChevronDown, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useWoStore } from "@/stores/wo-store";

// ── Component ─────────────────────────────────────────────────────────────────

export function WoDiManagementPanel() {
  const { t } = useTranslation("ot");
  const items = useWoStore((s) => s.items);
  const [open, setOpen] = useState(true);

  // Filter to DI-sourced WOs in draft or planned status
  const diWos = useMemo(
    () =>
      items.filter(
        (wo) =>
          wo.source_di_id !== null && (wo.status_code === "draft" || wo.status_code === "planned"),
      ),
    [items],
  );

  if (diWos.length === 0) return null;

  return (
    <div className="border-b border-surface-border bg-amber-50/50">
      <button
        type="button"
        className="flex items-center gap-2 w-full px-6 py-2 hover:bg-amber-50 transition-colors text-left"
        onClick={() => setOpen(!open)}
      >
        {open ? (
          <ChevronDown className="h-4 w-4 text-amber-600" />
        ) : (
          <ChevronRight className="h-4 w-4 text-amber-600" />
        )}
        <span className="text-sm font-medium text-amber-800">{t("diPanel.title")}</span>
        <Badge className="bg-amber-100 text-amber-800 border-0 text-[10px]">{diWos.length}</Badge>
      </button>

      {open && (
        <div className="px-6 pb-3">
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-text-muted border-b border-surface-border">
                <th className="pb-1.5 font-medium">{t("list.columns.number")}</th>
                <th className="pb-1.5 font-medium">DI</th>
                <th className="pb-1.5 font-medium">{t("list.columns.equipment")}</th>
                <th className="pb-1.5 font-medium">{t("list.columns.priority")}</th>
                <th className="pb-1.5 font-medium">{t("list.columns.status")}</th>
                <th className="pb-1.5 font-medium text-right" />
              </tr>
            </thead>
            <tbody>
              {diWos.map((wo) => (
                <tr key={wo.id} className="border-b border-surface-border last:border-0">
                  <td className="py-1.5 font-mono">{wo.code}</td>
                  <td className="py-1.5 font-mono text-text-muted">
                    {wo.source_di_id ? `DI-${wo.source_di_id}` : "—"}
                  </td>
                  <td className="py-1.5 truncate max-w-[150px]">{wo.asset_label ?? "—"}</td>
                  <td className="py-1.5">
                    {wo.urgency_label && (
                      <Badge
                        variant="outline"
                        className="text-[10px] border-0 bg-amber-100 text-amber-800"
                      >
                        {wo.urgency_label}
                      </Badge>
                    )}
                  </td>
                  <td className="py-1.5">
                    <Badge
                      variant="outline"
                      className="text-[10px] border-0 bg-gray-100 text-gray-600"
                    >
                      {t(`status.${wo.status_code ?? "draft"}`)}
                    </Badge>
                  </td>
                  <td className="py-1.5 text-right">
                    <Button variant="outline" size="sm" className="h-6 px-2 text-[10px]">
                      {t("diPanel.schedule")}
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
