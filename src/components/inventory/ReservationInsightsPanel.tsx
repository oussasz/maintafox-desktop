/**
 * Groups article reservations by work order with LinkedEntityBadge.
 */

import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { StatusChip } from "@/components/ui/status-chip";
import { RESERVATION_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { formatSiteLabel } from "@/lib/display";
import type { StockReservation } from "@shared/ipc-types";

export interface ReservationInsightsPanelProps {
  reservations: StockReservation[];
}

export function ReservationInsightsPanel({ reservations }: ReservationInsightsPanelProps) {
  const { t } = useTranslation("inventory");
  const { formatNumber, formatDate } = useFormatters();
  const formatQty = (n: number) => formatNumber(n, { maximumFractionDigits: 3 });

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
          workOrderCode:
            row.work_order_code ??
            (row.source_ref?.startsWith("OT-") || row.source_ref?.startsWith("WO-")
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
                {t("article.reservations.ungrouped")}
              </span>
            )}
            <span className="text-xs text-text-muted">
              {t("article.reservations.lineCount", { count: group.rows.length })}
            </span>
          </div>
          <div className="space-y-2">
            {group.rows.map((r) => (
              <div key={r.id} className="rounded bg-surface-muted/40 p-2 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <StatusChip
                    label={t(`detail.reservationStatuses.${r.status}`, { defaultValue: r.status })}
                    tone={toneOf(RESERVATION_STATUS_TONE, r.status)}
                    dot
                  />
                  <span className="font-medium tabular-nums">
                    {t("detail.reservations.reservedQty", {
                      reserved: formatQty(r.quantity_reserved),
                      issued: formatQty(r.quantity_issued),
                    })}
                  </span>
                  <span className="text-xs text-text-muted">
                    {t(`detail.reservations.sources.${r.source_type}`, {
                      defaultValue: t("detail.reservations.sources.OTHER"),
                    })}
                  </span>
                </div>
                <div className="mt-1 text-xs text-text-muted">
                  {formatSiteLabel(r.warehouse_code, r.warehouse_name)} /{" "}
                  {formatSiteLabel(r.location_code, r.location_name)} · {formatDate(r.created_at)}
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
