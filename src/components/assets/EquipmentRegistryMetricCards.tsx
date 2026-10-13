/**
 * Equipment registry KPI strip — VisualMetricCard adapters for KPI-047…051.
 * Click-to-filter toggles `registry_focus` on the asset search store.
 */

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  VisualMetricCard,
  type VisualMetricTrendSense,
} from "@/components/metrics/VisualMetricCard";
import { seriesUsableForViz } from "@/components/metrics/visual-metric-viz";
import { mfVisualMetric } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { getEquipmentRegistryMetrics } from "@/services/asset-search-service";
import { useAssetSearchStore } from "@/stores/asset-search-store";
import {
  EQUIPMENT_KPI_REGISTRY,
  type EquipmentKpiSemanticKey,
} from "@shared/equipment-kpi-registry";
import type {
  AssetSearchFilters,
  EquipmentRegistryMetricCell,
  EquipmentRegistryMetricsPayload,
} from "@shared/ipc-types";

type RegistryFocus = NonNullable<AssetSearchFilters["registry_focus"]>;

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

/** Metrics use SmartFilterBar scope without the KPI drill focus (strip stays stable). */
function metricsScopeFilters(filters: AssetSearchFilters): AssetSearchFilters {
  return { ...filters, registry_focus: null };
}

function formatCellValue(cell: EquipmentRegistryMetricCell): string {
  if (cell.value_kind === "percent") {
    return cell.value.toFixed(0);
  }
  return Math.round(cell.value).toLocaleString();
}

function MetricStripSkeleton() {
  return (
    <div
      className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-5"
      role="status"
      aria-busy="true"
    >
      {Array.from({ length: 5 }).map((_, i) => (
        <div key={i} className={cn(mfVisualMetric.cardStatic, "animate-pulse")} aria-hidden>
          <div className={mfVisualMetric.body}>
            <div className="min-w-0 flex-1 space-y-2">
              <div className="h-3 w-24 rounded bg-surface-2" />
              <div className="h-2.5 w-32 rounded bg-surface-2/80" />
              <div className="h-7 w-16 rounded bg-surface-2" />
            </div>
            <div className="h-[3.25rem] w-[7rem] shrink-0 rounded bg-surface-2/60" />
          </div>
        </div>
      ))}
    </div>
  );
}

export function EquipmentRegistryMetricCards() {
  const { t } = useTranslation("equipment");
  const filters = useAssetSearchStore((s) => s.filters);
  const updateFilters = useAssetSearchStore((s) => s.updateFilters);

  const [payload, setPayload] = useState<EquipmentRegistryMetricsPayload | null>(null);
  const [loading, setLoading] = useState(true);

  const scopeKey = useMemo(() => {
    const scope = metricsScopeFilters(filters);
    return JSON.stringify({
      query: scope.query ?? null,
      class_codes: scope.class_codes ?? null,
      family_codes: scope.family_codes ?? null,
      status_codes: scope.status_codes ?? null,
      org_node_ids: scope.org_node_ids ?? null,
      include_decommissioned: scope.include_decommissioned ?? false,
      limit: scope.limit ?? null,
    });
  }, [filters]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void (async () => {
      try {
        const next = await getEquipmentRegistryMetrics(metricsScopeFilters(filters));
        if (!cancelled) setPayload(next);
      } catch {
        if (!cancelled) setPayload(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // scopeKey captures the relevant filter fields for metrics (excludes registry_focus).
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional scopeKey gate
  }, [scopeKey]);

  const cellByKey = useMemo(() => {
    const map = new Map<string, EquipmentRegistryMetricCell>();
    for (const c of payload?.cells ?? []) {
      map.set(c.semantic_key, c);
    }
    return map;
  }, [payload]);

  const activeFocus = filters.registry_focus ?? null;

  const onCardClick = (drillFilter: (typeof EQUIPMENT_KPI_REGISTRY)[number]["drillFilter"]) => {
    if (drillFilter === "none") return;
    const focus = drillFilter as RegistryFocus;
    if (activeFocus === focus) {
      void updateFilters({ registry_focus: null });
      return;
    }
    // Avoid empty AND with an explicit status select when drilling to down assets.
    if (focus === "status_down") {
      void updateFilters({ registry_focus: focus, status_codes: null });
      return;
    }
    void updateFilters({ registry_focus: focus });
  };

  if (loading && !payload) {
    return <MetricStripSkeleton />;
  }

  return (
    <div
      className={cn("relative", loading && payload ? "opacity-70" : undefined)}
      role="group"
      aria-label={t("metrics.stripAria")}
      aria-busy={loading}
    >
      {loading && payload ? (
        <div
          className="pointer-events-none absolute inset-0 z-10 animate-pulse rounded-xl bg-surface-1/40"
          aria-hidden
        />
      ) : null}
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-5">
        {EQUIPMENT_KPI_REGISTRY.map((reg) => {
          const cell = cellByKey.get(reg.semanticKey as EquipmentKpiSemanticKey);
          const available = Boolean(cell?.available);
          const value = cell && available ? formatCellValue(cell) : "—";

          let trend: { label: string; sense: VisualMetricTrendSense } | undefined;
          if (cell && available) {
            const pct = trendPercent(cell.value, cell.previous_value);
            if (pct !== null && Math.abs(pct) >= 0.5) {
              const rounded = Math.abs(pct) >= 10 ? Math.round(pct) : Math.round(pct * 10) / 10;
              const sign = rounded > 0 ? "+" : "";
              trend = {
                label: `${sign}${rounded}%`,
                sense: senseForDelta(cell.value - cell.previous_value, reg.trendSense),
              };
            } else if (pct !== null) {
              trend = { label: t("metrics.stable"), sense: "neutral" };
            }
          }

          const series =
            reg.vizKind === "histogram" ? seriesUsableForViz(cell?.series ?? undefined) : undefined;

          const isActive = reg.drillFilter !== "none" && activeFocus === reg.drillFilter;
          const clickable = reg.drillFilter !== "none";

          let context: string = t(reg.contextKey as "metrics.contextStatus");
          if (
            reg.semanticKey === "EQUIP_STATUS_SUMMARY" &&
            cell?.active_count != null &&
            cell.down_count != null
          ) {
            context = t("metrics.contextStatusDetail", {
              active: cell.active_count,
              down: cell.down_count,
            });
          }

          return (
            <VisualMetricCard
              key={reg.semanticKey}
              title={t(reg.labelKey as never)}
              context={context}
              value={value}
              available={available}
              unavailableLabel="—"
              vizKind={reg.vizKind}
              {...(reg.valueKind === "percent" ? { unit: t("metrics.unit.percent") } : {})}
              {...(trend ? { trendLabel: trend.label, trendSense: trend.sense } : {})}
              {...(series ? { series } : {})}
              {...(reg.vizKind === "gauge" && cell ? { gaugeValue: cell.value } : {})}
              {...(reg.vizKind === "alert" && cell
                ? {
                    alertCount: Math.round(cell.value),
                    alertNormalLabel: t("metrics.alertNormal"),
                    alertAttentionLabel: t("metrics.alertAttention"),
                  }
                : {})}
              {...(clickable ? { onClick: () => onCardClick(reg.drillFilter) } : {})}
              className={
                isActive ? "ring-2 ring-primary/50 ring-offset-2 ring-offset-background" : undefined
              }
              {...(isActive ? { accent: "amber" as const } : {})}
            />
          );
        })}
      </div>
    </div>
  );
}
