import { mfVisualMetric } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import type { OverviewKpiAccent, OverviewKpiVizKind } from "@shared/overview-kpi-registry";
import { OVERVIEW_KPI_ACCENTS } from "@shared/overview-kpi-registry";

import { VisualMetricViz, seriesUsableForViz } from "./visual-metric-viz";

export type VisualMetricTrendSense = "up-good" | "up-bad" | "neutral";

export interface VisualMetricCardProps {
  title: string;
  value: string;
  unit?: string | undefined;
  available?: boolean | undefined;
  unavailableLabel?: string | undefined;
  context?: string | undefined;
  trendLabel?: string | undefined;
  trendSense?: VisualMetricTrendSense | undefined;
  /** Typed right-column micro-viz. */
  vizKind?: OverviewKpiVizKind | undefined;
  /** Daily (or equally spaced) series for areaWave / waterfall / histogram. */
  series?: number[] | undefined;
  /** 0–100 for gauge. */
  gaugeValue?: number | undefined;
  /** Count for alert micro-viz. */
  alertCount?: number | undefined;
  alertNormalLabel?: string | undefined;
  alertAttentionLabel?: string | undefined;
  /** Optional user accent for title + micro-viz stroke/fill. */
  accent?: OverviewKpiAccent | undefined;
  onClick?: (() => void) | undefined;
  className?: string | undefined;
}

export function VisualMetricCard(props: VisualMetricCardProps) {
  const {
    title,
    value,
    unit,
    available = true,
    unavailableLabel = "—",
    context,
    trendLabel,
    trendSense = "neutral",
    vizKind,
    series,
    gaugeValue,
    alertCount,
    alertNormalLabel,
    alertAttentionLabel,
    accent = OVERVIEW_KPI_ACCENTS.DEFAULT,
    onClick,
    className,
  } = props;

  const accentTone = mfVisualMetric.accentTone[accent] ?? mfVisualMetric.accentTone.default;

  const badgeTone =
    trendSense === "neutral"
      ? undefined
      : trendSense === "up-good"
        ? mfVisualMetric.badgeUp
        : mfVisualMetric.badgeDown;

  const usableSeries =
    vizKind === "areaWave" || vizKind === "waterfall" || vizKind === "histogram"
      ? seriesUsableForViz(series)
      : undefined;

  const showViz =
    available &&
    vizKind != null &&
    (vizKind === "gauge" || vizKind === "alert" || Boolean(usableSeries));

  const inner = (
    <div className={mfVisualMetric.body}>
      <div className={showViz ? mfVisualMetric.dataWithViz : mfVisualMetric.data}>
        <p className={cn(mfVisualMetric.title, accentTone)}>{title}</p>
        {context ? <p className={mfVisualMetric.context}>{context}</p> : null}
        <div className={mfVisualMetric.valueRow}>
          <p className={mfVisualMetric.value}>
            {available ? value : unavailableLabel}
            {available && unit ? <span className={mfVisualMetric.unit}> {unit}</span> : null}
          </p>
          {available && trendLabel ? (
            <span className={cn(mfVisualMetric.badge, badgeTone ?? accentTone)}>{trendLabel}</span>
          ) : null}
        </div>
      </div>
      {showViz && vizKind ? (
        <div className={cn(mfVisualMetric.vizWrap, accentTone)}>
          <VisualMetricViz
            kind={vizKind}
            {...(usableSeries ? { series: usableSeries } : {})}
            {...(gaugeValue != null ? { gaugeValue } : {})}
            {...(alertCount != null ? { alertCount } : {})}
            {...(alertNormalLabel ? { alertNormalLabel } : {})}
            {...(alertAttentionLabel ? { alertAttentionLabel } : {})}
          />
        </div>
      ) : null}
    </div>
  );

  if (onClick) {
    return (
      <button type="button" onClick={onClick} className={cn(mfVisualMetric.card, className)}>
        {inner}
      </button>
    );
  }

  return <div className={cn(mfVisualMetric.cardStatic, className)}>{inner}</div>;
}
