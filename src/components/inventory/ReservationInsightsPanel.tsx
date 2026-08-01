/**
 * Groups article reservations by work order with LinkedEntityBadge.
 */

import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { Badge } from "@/components/ui/badge";
import { formatOrDash, formatSiteLabel } from "@/lib/display";
import type { StockReservation } from "@shared/ipc-types";

export interface ReservationInsightsPanelProps {
  reservations: StockReservation[];
}

function formatQty(n: number): string {
  return Number.isFinite(n) ? String(n) : "—";
}

export function ReservationInsightsPanel({ reservations }: ReservationInsightsPanelProps) {
  const { t } = useTranslation("inventory");

  const groups = useMemo(() => {
    const map = new Map<
      string,
      { workOrderCode: string | null; sourceId: number | null; rows: StockReservation[] }
    >();
    for (const row of reservations) {
      const code = (row.work_order_code ?? row.source_ref ?? "").trim();
      const key =
        code ||
        (row.source_type ? `${row.source_type}:${row.source_id ?? "none"}` : `res-${row.id}`);
      const existing = map.get(key);
      if (existing) {
        existing.rows.push(row);
      } else {
        map.set(key, {
          workOrderCode: row.work_order_code ?? (row.source_ref?.startsWith("OT-") || row.source_ref?.startsWith("WO-")
            ? row.source_ref
            : null),
          sourceId:
            row.source_type === "WORK_ORDER" || row.source_type === "WORK_ORDER_PART"
              ? row.source_id
              : null,
          rows: [row],
        });
      }
    }
    return Array.from(map.entries()).map(([key, value]) => ({ key, ...value }));
  }, [reservations]);

  if (reservations.length === 0) {
    return <p className="text-sm text-text-muted">{t("detail.reservations.empty")}</p>;
  }

  return (
    <div className="space-y-3">
      {groups.map((group) => (
        <div key={group.key} className="rounded border border-surface-border p-2.5">
          <div className="mb-2 flex flex-wrap items-center gap-2">
            {group.workOrderCode ? (
              <LinkedEntityBadge
                entity="work_order"
                code={group.workOrderCode}
                entityId={group.sourceId}
              />
            ) : (
              <span className="text-xs font-medium text-text-muted">
                {t("article.reservations.ungrouped", { defaultValue: "Other reservations" })}
              </span>
            )}
            <span className="text-xs text-text-muted">
              {t("article.reservations.lineCount", {
                defaultValue: "{{count}} line(s)",
                count: group.rows.length,
              })}
            </span>
          </div>
          <div className="space-y-2">
            {group.rows.map((r) => (
              <div key={r.id} className="rounded bg-surface-muted/40 p-2 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge variant="secondary" className="h-5 text-[10px]">
                    {r.status}
                  </Badge>
                  <span className="font-medium tabular-nums">{formatQty(r.quantity_reserved)}</span>
                  <span className="text-xs text-text-muted">{formatOrDash(r.source_type)}</span>
                </div>
                <div className="mt-1 text-xs text-text-muted">
                  {formatSiteLabel(r.warehouse_code, r.warehouse_name)} /{" "}
                  {formatSiteLabel(r.location_code, r.location_name)}
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
