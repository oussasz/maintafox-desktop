/**
 * Trend badge for an Executive Dashboard KPI — shown only when the projection computed a real
 * previous-window value (never "no change" for a figure that has no baseline).
 */
import type { VisualMetricTrendSense } from "@/components/metrics/VisualMetricCard";
import type { AnalyticsKpiValue } from "@shared/ipc-types";
import type { QualityBadgeCode } from "@shared/kpi-definitions";
import { DEFAULT_KPI_STABLE_BELOW } from "@shared/overview-kpi-registry";

export interface KpiTrend {
  /** Signed change in percent of the previous window, or 0 when stable. */
  percent: number;
  /** Set for a figure that is itself a percentage: the signed change in points, 0 when stable. */
  points?: number;
  sense: VisualMetricTrendSense;
}

/**
 * `QUALITY_BADGE_CODES.INSUFFICIENT_BASELINE`, typed against the shared code list. Imported as a
 * type only: `kpi-definitions` and `overview-kpi-registry` import each other at runtime.
 */
const INSUFFICIENT_BASELINE: QualityBadgeCode = "insufficient_baseline";

/** Below this relative change the figure is reported as stable (the registry's default band). */
const STABLE_PERCENT = DEFAULT_KPI_STABLE_BELOW;

/** Options for a figure kept as a 0..1 ratio and shown as a percentage. */
export interface RatioTrendOptions {
  /** Change in percentage points below which the figure reads "stable". */
  stablePoints: number;
}

/**
 * @param direction whether a rise is good (`up-good`, e.g. MTBF), bad (`up-bad`, e.g. cost) or
 * neither (`neutral`, e.g. PM due today: the change is shown, never coloured).
 * @returns `null` when there is nothing honest to compare with.
 */
export function periodTrend(
  kpi: AnalyticsKpiValue | null | undefined,
  direction: "up-good" | "up-bad" | "neutral",
  ratio?: RatioTrendOptions,
): KpiTrend | null {
  if (!kpi || !kpi.available) return null;
  if (kpi.quality_badge === INSUFFICIENT_BASELINE) return null;
  const { value, previous_value: previous } = kpi;
  if (!Number.isFinite(value) || !Number.isFinite(previous)) return null;
  if (ratio) {
    // A percentage moves in points ("+8 pts"), not as a relative percent of itself.
    const points = (value - previous) * 100;
    if (Math.abs(points) < ratio.stablePoints) return { percent: 0, points: 0, sense: "neutral" };
    const good = direction === "up-good" ? points > 0 : points < 0;
    if (direction === "neutral") return { percent: 0, points, sense: "neutral" };
    return { percent: 0, points, sense: good ? "up-good" : "up-bad" };
  }
  if (previous === 0) return value === 0 ? { percent: 0, sense: "neutral" } : null;
  const percent = ((value - previous) / Math.abs(previous)) * 100;
  if (Math.abs(percent) < STABLE_PERCENT) return { percent: 0, sense: "neutral" };
  const up = percent > 0;
  const good = direction === "up-good" ? up : !up;
  if (direction === "neutral") return { percent, sense: "neutral" };
  return { percent, sense: good ? "up-good" : "up-bad" };
}
