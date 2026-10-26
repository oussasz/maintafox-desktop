/**
 * "Équipement déclassé — à revoir" (AU-73b): work orders and requests open on retired equipment,
 * crossed in sync with the retirement (backend `wo::retired_equipment_review`).
 *
 * Read only. Each item is settled with the existing actions: cancel the work order or reject the
 * request with a reason (opened from here), or return the equipment to service. A closed item
 * keeps who, when and how; the closed ones are listed under the history toggle. Shown only when
 * there is something to show.
 */

import type { TFunction } from "i18next";
import { AlertTriangle, ChevronDown, ChevronRight, Eye } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type RetiredEquipmentReview,
  listRetiredEquipmentReviews,
} from "@/services/retired-equipment-review-service";
import { useWoStore } from "@/stores/wo-store";

function closureText(t: TFunction<"ot">, item: RetiredEquipmentReview): string {
  return t(`retiredEquipment.closure.${item.closure_kind ?? "other"}`, {
    code: item.record_code ?? "—",
    wo: item.converted_to_wo_code ?? "—",
    equipment: item.equipment_code ?? "—",
  });
}

export function WoRetiredEquipmentReviewPanel() {
  const { t } = useTranslation("ot");
  const { formatDateTime } = useFormatters();
  const openWo = useWoStore((s) => s.openWo);
  const [items, setItems] = useState<RetiredEquipmentReview[]>([]);
  const [failed, setFailed] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [showHistory, setShowHistory] = useState(false);

  const load = useCallback(async () => {
    try {
      const list = await listRetiredEquipmentReviews(true);
      setItems(list.items);
      setFailed(false);
    } catch {
      setFailed(true);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const open = useMemo(() => items.filter((i) => i.status === "open"), [items]);
  const closed = useMemo(() => items.filter((i) => i.status === "closed"), [items]);

  if (failed) {
    return (
      <p
        role="alert"
        className="mx-4 mt-4 rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
      >
        {t("retiredEquipment.loadFailed")}
      </p>
    );
  }
  if (items.length === 0) return null;

  return (
    <Card
      className="mx-4 mt-4 border-tone-warning-border bg-tone-warning-bg"
      data-testid="retired-equipment-review"
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 px-4 py-3 text-left text-sm font-medium"
        onClick={() => setCollapsed((v) => !v)}
        aria-expanded={!collapsed}
      >
        {collapsed ? (
          <ChevronRight className="h-4 w-4 shrink-0" aria-hidden />
        ) : (
          <ChevronDown className="h-4 w-4 shrink-0" aria-hidden />
        )}
        <AlertTriangle className="h-4 w-4 shrink-0 text-tone-warning-fg" aria-hidden />
        <span>{t("retiredEquipment.title")}</span>
        <Badge variant="secondary" className="h-5 min-w-[20px] justify-center text-[10px]">
          {open.length}
        </Badge>
      </button>

      {!collapsed && (
        <CardContent className="space-y-2 px-4 pb-3 pt-0 text-xs">
          {open.length > 0 ? (
            <p className="text-muted-foreground">{t("retiredEquipment.intro")}</p>
          ) : (
            <p className="text-muted-foreground">{t("retiredEquipment.noneOpen")}</p>
          )}
          <ul className="divide-y rounded-md border bg-background">
            {open.map((item) => (
              <li key={item.id} className="flex items-center gap-3 px-3 py-2">
                <Badge variant="outline" className="shrink-0 text-[10px]">
                  {t(`retiredEquipment.kind.${item.kind}`)}
                </Badge>
                <span className="w-[90px] shrink-0 font-mono text-muted-foreground">
                  {item.record_code ?? "—"}
                </span>
                <span className="min-w-0 flex-1 truncate font-medium">
                  {item.record_title ?? ""}
                </span>
                <span className="shrink-0 truncate text-muted-foreground">
                  {t("retiredEquipment.equipment", {
                    code: item.equipment_code ?? "—",
                    name: item.equipment_name ?? "",
                  })}
                </span>
                {item.record_id != null ? (
                  item.kind === "work_order" ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-6 w-6 shrink-0 p-0"
                      onClick={() => void openWo(item.record_id as number)}
                      title={t("retiredEquipment.open")}
                      aria-label={t("retiredEquipment.open")}
                    >
                      <Eye className="h-3.5 w-3.5" aria-hidden />
                    </Button>
                  ) : (
                    <Link
                      to={`/requests?openDi=${item.record_id}`}
                      className="shrink-0 text-tone-warning-fg hover:underline"
                    >
                      {t("retiredEquipment.open")}
                    </Link>
                  )
                ) : null}
              </li>
            ))}
          </ul>
          {closed.length > 0 ? (
            <div>
              <button
                type="button"
                className="text-xs font-medium text-muted-foreground hover:underline"
                onClick={() => setShowHistory((v) => !v)}
                aria-expanded={showHistory}
              >
                {t("retiredEquipment.history", { count: closed.length })}
              </button>
              {showHistory ? (
                <ul className="mt-1 divide-y rounded-md border bg-background">
                  {closed.map((item) => (
                    <li key={item.id} className="px-3 py-2">
                      <div className="font-medium">{closureText(t, item)}</div>
                      <div className="text-muted-foreground">
                        {item.closed_by
                          ? t("retiredEquipment.closedBy", {
                              who: item.closed_by,
                              when: formatDateTime(item.closed_at),
                            })
                          : t("retiredEquipment.closedAt", {
                              when: formatDateTime(item.closed_at),
                            })}
                        {item.closure_note
                          ? ` · ${t("retiredEquipment.note", { note: item.closure_note })}`
                          : ""}
                      </div>
                    </li>
                  ))}
                </ul>
              ) : null}
            </div>
          ) : null}
        </CardContent>
      )}
    </Card>
  );
}
