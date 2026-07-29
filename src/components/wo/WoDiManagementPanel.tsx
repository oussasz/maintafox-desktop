/**
 * WoDiManagementPanel.tsx
 *
 * Compact dashboard-style panel showing DI-sourced work orders
 * still waiting for planning/scheduling.
 */

import { ChevronDown, ChevronRight, Eye, ListTree } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useWoStore } from "@/stores/wo-store";

// ── Component ─────────────────────────────────────────────────────────────────

const PREVIEW_LIMIT = 3;

export function WoDiManagementPanel() {
  const { t } = useTranslation("ot");
  const items = useWoStore((s) => s.items);
  const openWo = useWoStore((s) => s.openWo);
  const [collapsed, setCollapsed] = useState(false);

  // Filter to DI-sourced WOs in draft or planned status
  const diWos = useMemo(
    () =>
      items.filter(
        (wo) =>
          wo.source_di_id !== null &&
          (wo.status_code === "draft" || wo.status_code === "planning"),
      ),
    [items],
  );

  const previewItems = useMemo(() => diWos.slice(0, PREVIEW_LIMIT), [diWos]);
  const hiddenInPreview = Math.max(0, diWos.length - previewItems.length);

  if (diWos.length === 0) {
    return null;
  }

  return (
    <Card className="mx-4 mt-4 border-amber-200 bg-amber-50/30">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-4 py-3 text-left text-sm font-medium"
        onClick={() => setCollapsed((v) => !v)}
      >
        {collapsed ? (
          <ChevronRight className="h-4 w-4 shrink-0" />
        ) : (
          <ChevronDown className="h-4 w-4 shrink-0" />
        )}
        <span>{t("diPanel.title")}</span>
        <Badge variant="secondary" className="text-[10px] h-5 min-w-[20px] justify-center">
          {diWos.length}
        </Badge>
      </button>

      {!collapsed && (
        <CardContent className="p-0">
          <div className="divide-y">
            {previewItems.map((wo) => (
              <div key={wo.id} className="flex items-center gap-3 px-4 py-2.5 text-xs hover:bg-muted/50">
                <span className="font-mono text-muted-foreground shrink-0 w-[90px]">{wo.code}</span>
                <span className="truncate min-w-0 flex-1 font-medium">{wo.title}</span>
                <span className="shrink-0 max-w-[120px] flex justify-end">
                  <LinkedEntityBadge
                    entity="di"
                    code={wo.source_di_code}
                    entityId={wo.source_di_id}
                    title={wo.source_di_title}
                    className="text-[10px] px-1.5 py-0"
                  />
                </span>
                <span className="truncate text-muted-foreground shrink-0 w-[150px] text-right">
                  {wo.asset_label ?? "—"}
                </span>
                <Badge variant="outline" className="text-[10px] border-0 bg-gray-100 text-gray-600 shrink-0">
                  {t(`status.${wo.status_code ?? "draft"}`)}
                </Badge>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0 shrink-0"
                  onClick={() => void openWo(wo.id)}
                  title={t("diPanel.schedule")}
                >
                  <Eye className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
          {hiddenInPreview > 0 && (
            <div className="border-t border-amber-200/60 bg-amber-50/40 px-4 py-2.5 text-center">
              <Link
                to="/work-orders"
                className="inline-flex items-center justify-center gap-1.5 text-xs font-medium text-amber-900/90 hover:text-amber-950 hover:underline"
              >
                <ListTree className="h-3.5 w-3.5 shrink-0 opacity-80" />
                {t("kanban.loadMore", { count: hiddenInPreview })}
                <ChevronRight className="h-3.5 w-3.5 shrink-0 opacity-70" />
              </Link>
            </div>
          )}
        </CardContent>
      )}
    </Card>
  );
}
