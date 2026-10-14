import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import {
  VisualMetricCard,
  type VisualMetricTrendSense,
} from "@/components/metrics/VisualMetricCard";
import { seriesUsableForViz } from "@/components/metrics/visual-metric-viz";
import { Button } from "@/components/ui/button";
import { useFormatters } from "@/hooks/use-formatters";
import type { OverviewKpiCell, OverviewKpisPayload } from "@shared/ipc-types";
import {
  OVERVIEW_KPI_REGISTRY_BY_KEY,
  isOverviewKpiAccent,
  isOverviewKpiSemanticKey,
  type OverviewKpiLayoutEntry,
} from "@shared/overview-kpi-registry";

const COLLAPSED_VISIBLE = 5;

function trendPercent(value: number, previous: number): number | null {
  if (!Number.isFinite(value) || !Number.isFinite(previous)) return null;
  if (previous === 0) return value === 0 ? 0 : null;
  return ((value - previous) / Math.abs(previous)) * 100;
}

function senseForDelta(
  delta: number,
  direction: "up-good" | "up-bad" | "neutral",
): VisualMetricTrendSense {
  if (delta === 0 || direction === "neutral") return "neutral";
  const up = delta > 0;
  if (direction === "up-good") return up ? "up-good" : "up-bad";
  return up ? "up-bad" : "up-good";
}

function formatCellValue(
  cell: OverviewKpiCell,
  formatCurrency: (amount: number) => string,
): string {
  switch (cell.value_kind) {
    case "currency":
      return formatCurrency(cell.value);
    case "percent":
      return `${cell.value.toFixed(1)}`;
    case "hours":
      return cell.value >= 100 ? Math.round(cell.value).toLocaleString() : cell.value.toFixed(1);
    default:
      return Math.round(cell.value).toLocaleString();
  }
}

export function OverviewMetricCards(props: {
  overviewKpis: OverviewKpisPayload | null;
  layout: OverviewKpiLayoutEntry[];
}) {
  const { t } = useTranslation("dashboard");
  const navigate = useNavigate();
  const { formatCurrency } = useFormatters();
  const [expanded, setExpanded] = useState(false);

  const cellByKey = useMemo(() => {
    const map = new Map<string, OverviewKpiCell>();
    for (const c of props.overviewKpis?.cells ?? []) {
      map.set(c.semantic_key, c);
    }
    return map;
  }, [props.overviewKpis?.cells]);

  const visibleCards = useMemo(
    () =>
      [...props.layout]
        .filter((e) => e.visible)
        .sort((a, b) => a.order - b.order)
        .filter((e) => isOverviewKpiSemanticKey(e.id)),
    [props.layout],
  );

  const shown = expanded ? visibleCards : visibleCards.slice(0, COLLAPSED_VISIBLE);
  const hiddenCount = Math.max(0, visibleCards.length - COLLAPSED_VISIBLE);

  const formatTrend = (
    cell: OverviewKpiCell,
  ): { label: string; sense: VisualMetricTrendSense } | undefined => {
    if (cell.baseline_available === false) return undefined;
    const pct = trendPercent(cell.value, cell.previous_value);
    if (pct === null) return undefined;
    if (Math.abs(pct) < 0.5) {
      return { label: t("kpi.stable"), sense: "neutral" };
    }
    const rounded = Math.abs(pct) >= 10 ? Math.round(pct) : Math.round(pct * 10) / 10;
    const sign = rounded > 0 ? "+" : "";
    const direction = cell.trend_sense as "up-good" | "up-bad" | "neutral";
    return {
      label: `${sign}${rounded}%`,
      sense: senseForDelta(cell.value - cell.previous_value, direction),
    };
  };

  return (
    <div className="space-y-2">
      <div
        className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-5"
        role="group"
        aria-label={t("ops.alertStripAria")}
      >
        {shown.map((entry) => {
          if (!isOverviewKpiSemanticKey(entry.id)) return null;
          const reg = OVERVIEW_KPI_REGISTRY_BY_KEY.get(entry.id);
          const cell = cellByKey.get(entry.id);
          if (!reg || !cell) return null;

          const available = cell.available;
          const trend = available ? formatTrend(cell) : undefined;
          const unit =
            cell.value_kind === "percent"
              ? t("overviewKpi.unit.percent")
              : reg.unitKey
                ? t(reg.unitKey as never)
                : undefined;

          const series =
            reg.vizKind === "areaWave" || reg.vizKind === "waterfall" || reg.vizKind === "histogram"
              ? seriesUsableForViz(cell.series)
              : undefined;

          return (
            <VisualMetricCard
              key={entry.id}
              title={t(reg.labelKey as never)}
              context={t(reg.contextKey as never)}
              value={available ? formatCellValue(cell, formatCurrency) : "—"}
              available={available}
              unavailableLabel="—"
              vizKind={reg.vizKind}
              onClick={() => navigate(reg.drillPath)}
              {...(isOverviewKpiAccent(entry.accent) ? { accent: entry.accent } : {})}
              {...(unit ? { unit } : {})}
              {...(trend ? { trendLabel: trend.label, trendSense: trend.sense } : {})}
              {...(series ? { series } : {})}
              {...(reg.vizKind === "gauge" ? { gaugeValue: cell.value } : {})}
              {...(reg.vizKind === "alert"
                ? {
                    alertCount: Math.round(cell.value),
                    alertNormalLabel: t("overviewKpi.alertNormal"),
                    alertAttentionLabel: t("overviewKpi.alertAttention"),
                  }
                : {})}
            />
          );
        })}
      </div>
      {hiddenCount > 0 ? (
        <div className="flex justify-center">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="text-text-muted"
            onClick={() => setExpanded((v) => !v)}
          >
            {expanded ? t("overviewKpi.collapse") : t("overviewKpi.expand", { count: hiddenCount })}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
