import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { CompactChartPanel } from "@/components/metrics/CompactMetricGrid";
import { mfTable } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { formatAssetLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { persistRamsEquipmentSelection } from "@/pages/reliability/rams-equipment-context";
import type { AnalyticsBadActorAsset } from "@shared/ipc-types";
import { ANALYTICS_RAMS_BASE_PATH } from "@shared/kpi-definitions";

export function AnalyticsBadActorTable(props: { rows: AnalyticsBadActorAsset[] }) {
  const { t } = useTranslation("reports");
  const { formatCurrency } = useFormatters();
  const navigate = useNavigate();
  const { rows } = props;

  const openAsset = (equipmentId: number) => {
    persistRamsEquipmentSelection(equipmentId);
    navigate(`${ANALYTICS_RAMS_BASE_PATH}/dashboard`);
  };

  return (
    <CompactChartPanel title={t("hub.dashboards.badActors.title")}>
      {rows.length === 0 ? (
        <p className="text-xs text-text-muted">{t("hub.dashboards.badActors.empty")}</p>
      ) : (
        <table className="w-full text-start text-[11px]">
          <thead>
            <tr className={mfTable.header}>
              <th className="px-2 py-1 font-medium">{t("hub.dashboards.badActors.asset")}</th>
              <th className="px-2 py-1 text-end font-medium">
                {t("hub.dashboards.badActors.events")}
              </th>
              <th className="px-2 py-1 text-end font-medium">{t("hub.dashboards.kpi.mtbf")}</th>
              <th className="px-2 py-1 text-end font-medium">
                {t("hub.dashboards.kpi.availability")}
              </th>
              <th className="px-2 py-1 text-end font-medium">
                {t("hub.dashboards.kpi.maintenanceCost")}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr
                key={row.equipment_id}
                className={cn(mfTable.rowHover, "cursor-pointer")}
                onClick={() => openAsset(row.equipment_id)}
              >
                <td className="max-w-[12rem] truncate px-2 py-1 text-text-primary">
                  {formatAssetLabel(row.asset_code, row.asset_label)}
                </td>
                <td className="px-2 py-1 text-end font-mono tabular-nums text-text-primary">
                  {row.event_count.toLocaleString()}
                </td>
                <td className="px-2 py-1 text-end font-mono tabular-nums text-text-primary">
                  {row.mtbf_hours != null && Number.isFinite(row.mtbf_hours)
                    ? row.mtbf_hours.toFixed(1)
                    : "—"}
                </td>
                <td className="px-2 py-1 text-end font-mono tabular-nums text-text-primary">
                  {row.availability != null && Number.isFinite(row.availability)
                    ? `${(row.availability * 100).toFixed(1)}%`
                    : "—"}
                </td>
                <td className="px-2 py-1 text-end font-mono tabular-nums text-text-primary">
                  {row.cost_available ? formatCurrency(row.maintenance_cost) : "—"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </CompactChartPanel>
  );
}
