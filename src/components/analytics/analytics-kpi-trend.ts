/**
 * Trend badge for an Executive Dashboard KPI — shown only when the projection computed a real
 * previous-window value (never "no change" for a figure that has no baseline).
 */
import type { VisualMetricTrendSense } from "@/components/metrics/VisualMetricCard";
import type { AnalyticsKpiValue } from "@shared/ipc-types";
import type { QualityBadgeCode } from "@shared/kpi-definitions";

export interface KpiTrend {
  /** Signed change in percent of the previous window, or 0 when stable. */
  percent: number;
  sense: VisualMetricTrendSense;
}

/**
 * `QUALITY_BADGE_CODES.INSUFFICIENT_BASELINE`, typed against the shared code list. Imported as a
 * type only: `kpi-definitions` and `overview-kpi-registry` import each other at runtime.
 */
const INSUFFICIENT_BASELINE: QualityBadgeCode = "insufficient_baseline";

/** Below this relative change the figure is reported as stable. */
const STABLE_PERCENT = 0.5;

/**
 * @param direction whether a rise is good (`up-good`, e.g. MTBF) or bad (`up-bad`, e.g. cost).
 * @returns `null` when there is nothing honest to compare with.
 */
export function periodTrend(
  kpi: AnalyticsKpiValue | null | undefined,
  direction: "up-good" | "up-bad",
): KpiTrend | null {
  if (!kpi || !kpi.available) return null;
  if (kpi.quality_badge === INSUFFICIENT_BASELINE) return null;
  const { value, previous_value: previous } = kpi;
  if (!Number.isFinite(value) || !Number.isFinite(previous)) return null;
  if (previous === 0) return value === 0 ? { percent: 0, sense: "neutral" } : null;
  const percent = ((value - previous) / Math.abs(previous)) * 100;
  if (Math.abs(percent) < STABLE_PERCENT) return { percent: 0, sense: "neutral" };
  const up = percent > 0;
  const good = direction === "up-good" ? up : !up;
  return { percent, sense: good ? "up-good" : "up-bad" };
}
