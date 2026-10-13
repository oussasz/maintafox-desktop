import * as d3 from "d3";
import { useId, useMemo } from "react";

import { mfVisualMetric } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import type { OverviewKpiVizKind } from "@shared/overview-kpi-registry";

const VIZ_W = 320;
const VIZ_H = 44;

function AreaWave({ values }: { values: number[] }) {
  const gradId = useId();
  const d = useMemo(() => {
    const x = d3
      .scaleLinear()
      .domain([0, values.length - 1])
      .range([0, VIZ_W]);
    const min = d3.min(values) ?? 0;
    const max = d3.max(values) ?? 1;
    const y = d3
      .scaleLinear()
      .domain([min, max === min ? min + 1 : max])
      .range([VIZ_H - 2, 4]);
    const line = d3
      .line<number>()
      .x((_, i) => x(i))
      .y((v) => y(v))
      .curve(d3.curveMonotoneX);
    const area = d3
      .area<number>()
      .x((_, i) => x(i))
      .y0(VIZ_H)
      .y1((v) => y(v))
      .curve(d3.curveMonotoneX);
    return { line: line(values) ?? "", area: area(values) ?? "" };
  }, [values]);

  return (
    <svg
      viewBox={`0 0 ${VIZ_W} ${VIZ_H}`}
      preserveAspectRatio="none"
      className={cn(mfVisualMetric.viz, "text-primary")}
      aria-hidden
    >
      <defs>
        <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="currentColor" stopOpacity="0.28" />
          <stop offset="100%" stopColor="currentColor" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={d.area} fill={`url(#${gradId})`} />
      <path
        d={d.line}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

function WaterfallSteps({ values }: { values: number[] }) {
  const d = useMemo(() => {
    let sum = 0;
    const cumulative = values.map((v) => {
      sum += Math.max(0, v);
      return sum;
    });
    const x = d3
      .scaleLinear()
      .domain([0, cumulative.length - 1])
      .range([0, VIZ_W]);
    const max = d3.max(cumulative) ?? 1;
    const y = d3
      .scaleLinear()
      .domain([0, max === 0 ? 1 : max])
      .range([VIZ_H - 2, 4]);
    const line = d3
      .line<number>()
      .x((_, i) => x(i))
      .y((v) => y(v))
      .curve(d3.curveStepAfter);
    return { path: line(cumulative) ?? "", lastY: y(cumulative[cumulative.length - 1] ?? 0) };
  }, [values]);

  return (
    <svg
      viewBox={`0 0 ${VIZ_W} ${VIZ_H}`}
      preserveAspectRatio="none"
      className={cn(mfVisualMetric.viz, "text-primary")}
      aria-hidden
    >
      <path
        d={d.path}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
      />
      <circle
        cx={VIZ_W}
        cy={d.lastY}
        r={2.5}
        fill="currentColor"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

function DensityHistogram({ values }: { values: number[] }) {
  const bars = useMemo(() => {
    const max = Math.max(...values, 1e-9);
    const n = values.length;
    const gap = 1;
    const barW = Math.max(1, (VIZ_W - gap * (n - 1)) / n);
    return values.map((v, i) => {
      const h = Math.max(2, (v / max) * (VIZ_H - 4));
      return {
        x: i * (barW + gap),
        y: VIZ_H - h,
        w: barW,
        h,
      };
    });
  }, [values]);

  return (
    <svg
      viewBox={`0 0 ${VIZ_W} ${VIZ_H}`}
      preserveAspectRatio="none"
      className={cn(mfVisualMetric.viz, "text-primary")}
      aria-hidden
    >
      {bars.map((b, i) => (
        <rect
          key={i}
          x={b.x}
          y={b.y}
          width={b.w}
          height={b.h}
          rx={0.5}
          fill="currentColor"
          opacity={0.55 + (0.45 * b.h) / VIZ_H}
        />
      ))}
    </svg>
  );
}

function SemiGauge({ value }: { value: number }) {
  const pct = Math.max(0, Math.min(100, value)) / 100;
  const cx = 40;
  const cy = 38;
  const r = 28;
  const start = Math.PI;
  const end = 0;
  const angle = start + (end - start) * pct;

  const polar = (a: number) => ({
    x: cx + r * Math.cos(a),
    y: cy + r * Math.sin(a),
  });

  const trackStart = polar(start);
  const trackEnd = polar(end);
  const fillEnd = polar(angle);
  const largeArc = 0;

  const track = `M ${trackStart.x} ${trackStart.y} A ${r} ${r} 0 ${largeArc} 1 ${trackEnd.x} ${trackEnd.y}`;
  const fill =
    pct <= 0
      ? ""
      : `M ${trackStart.x} ${trackStart.y} A ${r} ${r} 0 ${pct > 0.5 ? 1 : 0} 1 ${fillEnd.x} ${fillEnd.y}`;

  return (
    <svg viewBox="0 0 80 44" className={cn(mfVisualMetric.viz, "text-primary")} aria-hidden>
      <path
        d={track}
        fill="none"
        stroke="currentColor"
        strokeOpacity={0.18}
        strokeWidth={5}
        strokeLinecap="round"
      />
      {fill ? (
        <path d={fill} fill="none" stroke="currentColor" strokeWidth={5} strokeLinecap="round" />
      ) : null}
    </svg>
  );
}

export function VisualMetricAlertBadge(props: {
  count: number;
  normalLabel: string;
  attentionLabel: string;
}) {
  if (props.count <= 0) {
    return <span className={mfVisualMetric.alertNormal}>{props.normalLabel}</span>;
  }
  const tone = props.count >= 5 ? mfVisualMetric.alertDanger : mfVisualMetric.alertWarn;
  return <span className={tone}>{props.attentionLabel}</span>;
}

export function VisualMetricViz(props: {
  kind: OverviewKpiVizKind;
  series?: number[] | undefined;
  gaugeValue?: number | undefined;
  alertCount?: number | undefined;
  alertNormalLabel?: string | undefined;
  alertAttentionLabel?: string | undefined;
}) {
  switch (props.kind) {
    case "areaWave":
      return props.series ? <AreaWave values={props.series} /> : null;
    case "waterfall":
      return props.series ? <WaterfallSteps values={props.series} /> : null;
    case "histogram":
      return props.series ? <DensityHistogram values={props.series} /> : null;
    case "gauge":
      return <SemiGauge value={props.gaugeValue ?? 0} />;
    case "alert":
      return (
        <VisualMetricAlertBadge
          count={props.alertCount ?? 0}
          normalLabel={props.alertNormalLabel ?? "Normal"}
          attentionLabel={props.alertAttentionLabel ?? "Attention"}
        />
      );
    default:
      return null;
  }
}

export function seriesUsableForViz(values: number[] | undefined): number[] | undefined {
  if (!values?.length) return undefined;
  const clean = values.filter((v) => Number.isFinite(v));
  if (clean.length < 4) return undefined;
  const min = Math.min(...clean);
  const max = Math.max(...clean);
  if (max - min < 1e-9) return undefined;
  return clean;
}
