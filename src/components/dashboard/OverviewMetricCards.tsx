import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { smallCountChange } from "@/components/analytics/analytics-kpi-trend";
import {
  VisualMetricCard,
  type VisualMetricTrendSense,
} from "@/components/metrics/VisualMetricCard";
import { useFleetKpiText } from "@/components/reliability/use-fleet-kpi-text";
import { Button } from "@/components/ui/button";
import { useFormatters, type Formatters } from "@/hooks/use-formatters";
import type { OverviewKpiCell, OverviewKpisPayload } from "@shared/ipc-types";
import {
  DEFAULT_KPI_STABLE_BELOW,
  OVERVIEW_KPI_REGISTRY_BY_KEY,
  OVERVIEW_KPI_SEMANTIC_KEYS,
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
  f: Pick<Formatters, "formatCurrency" | "formatDecimal" | "formatNumber">,
): string {
  switch (cell.value_kind) {
    case "currency":
      return f.formatCurrency(cell.value);
    case "percent":
      return f.formatDecimal(cell.value, 1);
    case "hours":
      return cell.value >= 100
        ? f.formatNumber(Math.round(cell.value), { maximumFractionDigits: 0 })
        : f.formatDecimal(cell.value, 1);
    default:
      return f.formatNumber(Math.round(cell.value), { maximumFractionDigits: 0 });
  }
}

export function OverviewMetricCards(props: {
  overviewKpis: OverviewKpisPayload;
  layout: OverviewKpiLayoutEntry[];
}) {
  const { t } = useTranslation("dashboard");
  const { t: tr } = useTranslation("reports");
  const fleetText = useFleetKpiText();
  const navigate = useNavigate();
  const formatters = useFormatters();
  const { formatNumber } = formatters;
  const [expanded, setExpanded] = useState(false);

  const cellByKey = useMemo(() => {
    const map = new Map<string, OverviewKpiCell>();
    for (const c of props.overviewKpis.cells) {
      map.set(c.semantic_key, c);
    }
    return map;
  }, [props.overviewKpis.cells]);

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
    reg: { stableBelow?: number; trendSense: VisualMetricTrendSense },
  ): { label: string; sense: VisualMetricTrendSense } | undefined => {
    if (cell.baseline_available === false) return undefined;
    // The registry is the only place a sense is written (the cell's own copy may be stale).
    const direction = reg.trendSense;
    const stableBelow = reg.stableBelow ?? DEFAULT_KPI_STABLE_BELOW;
    // A percentage is compared in points ("+8 pts"), never as "+10 %" of a percentage.
    if (cell.value_kind === "percent") {
      const points = cell.value - cell.previous_value;
      if (!Number.isFinite(points)) return undefined;
      if (Math.abs(points) < stableBelow) return { label: t("kpi.stable"), sense: "neutral" };
      const shown = Math.round(points * 10) / 10;
      return {
        label: t("kpi.points", { change: `${shown > 0 ? "+" : ""}${formatNumber(shown)}` }),
        sense: senseForDelta(points, direction),
      };
    }
    // A small count changes in units ("+3"), uncoloured: never "+300 %" for 1 to 4 (L7-08).
    if (cell.value_kind === "count") {
      const delta = smallCountChange(cell.value, cell.previous_value);
      if (delta === 0) return { label: t("kpi.stable"), sense: "neutral" };
      if (delta !== null) {
        return { label: `${delta > 0 ? "+" : ""}${formatNumber(delta)}`, sense: "neutral" };
      }
    }
    const pct = trendPercent(cell.value, cell.previous_value);
    if (pct === null) return undefined;
    if (Math.abs(pct) < stableBelow) {
      return { label: t("kpi.stable"), sense: "neutral" };
    }
    const rounded = Math.abs(pct) >= 10 ? Math.round(pct) : Math.round(pct * 10) / 10;
    const sign = rounded > 0 ? "+" : "";
    return {
      label: `${sign}${formatNumber(rounded)} %`,
      sense: senseForDelta(cell.value - cell.previous_value, direction),
    };
  };

  /**
   * Context of a card. A fleet reliability cell is worded as on the RAMS home from its live figures
   * (`study::fleet_kpis`): MTBF and Aₒ with window and hours basis, MTTR with its definition, each
   * with its exclusions; data quality with its window only. The others: their registry text.
   */
  const contextOf = (cell: OverviewKpiCell, contextKey: string): string => {
    const fleet = cell.fleet;
    if (fleet == null) return t(contextKey as never);
    const period = fleetText.months(fleet.window_months);
    switch (cell.semantic_key) {
      case OVERVIEW_KPI_SEMANTIC_KEYS.MTBF:
        return fleetText.rateContext("mtbf", fleet.kpis, period);
      case OVERVIEW_KPI_SEMANTIC_KEYS.MTTR:
        return fleetText.rateContext("mttr", fleet.kpis, period);
      case OVERVIEW_KPI_SEMANTIC_KEYS.AVAILABILITY:
        return fleetText.rateContext("availability", fleet.kpis, period);
      default:
        return period;
    }
  };

  /** The KPI's "stable" band, worded from the registry value. */
  const bandTitle = (cell: OverviewKpiCell, reg: { stableBelow?: number }) => {
    const band = reg.stableBelow ?? DEFAULT_KPI_STABLE_BELOW;
    const shown = formatters.formatDecimal(band, band < 0.1 ? 2 : 1);
    return cell.value_kind === "percent"
      ? tr("smrp.stableBand", { points: shown })
      : tr("smrp.stableBandPercent", { percent: shown });
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
          const trend = available ? formatTrend(cell, reg) : undefined;
          const unit =
            cell.value_kind === "percent"
              ? t("overviewKpi.unit.percent")
              : reg.unitKey
                ? t(reg.unitKey as never)
                : undefined;

          return (
            <VisualMetricCard
              key={entry.id}
              title={t(reg.labelKey as never)}
              context={contextOf(cell, reg.contextKey)}
              value={available ? formatCellValue(cell, formatters) : "—"}
              available={available}
              unavailableLabel="—"
              vizKind={reg.vizKind}
              onClick={() => navigate(reg.drillPath)}
              {...(isOverviewKpiAccent(entry.accent) ? { accent: entry.accent } : {})}
              {...(unit ? { unit } : {})}
              {...(trend
                ? {
                    trendLabel: trend.label,
                    trendSense: trend.sense,
                    trendTitle: bandTitle(cell, reg),
                  }
                : {})}
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
