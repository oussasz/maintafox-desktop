/**
 * Charts of the life analysis (SVG, theme colours, axis titles with units, text alternative):
 * Weibull probability plot, R(t) / h(t) curves and the cost-per-hour curve of the
 * age-replacement model. Every value comes from the backend analysis.
 */
import { useMemo } from "react";

import { ChartDataTable } from "./rams-ui";

const W = 560;

function logTicks(lo: number, hi: number): number[] {
  const out: number[] = [];
  if (!(lo > 0 && hi > lo)) return out;
  for (let e = Math.floor(Math.log10(lo)); e <= Math.ceil(Math.log10(hi)); e += 1) {
    for (const m of [1, 2, 5]) {
      const v = m * 10 ** e;
      if (v >= lo && v <= hi) out.push(v);
    }
  }
  return out;
}

function linearTicks(max: number, count = 5): number[] {
  if (!(max > 0)) return [0];
  const raw = max / count;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let v = 0; v <= max + step * 1e-9; v += step) out.push(v);
  return out;
}

const weibullY = (f: number) => Math.log(-Math.log(1 - f));

export interface WeibullPlotLabels {
  title: string;
  xAxis: string;
  yAxis: string;
  fitted: string;
  failures: string;
  band: string;
  eta: string;
  pointTitle: (time: string, rank: string) => string;
}

export function WeibullProbabilityPlot({
  points,
  band,
  eta,
  formatNumber,
  labels,
}: {
  points: { time: number; median_rank: number }[];
  band: { time: number; unreliability: number; lower: number; upper: number }[];
  eta: number;
  formatNumber: (v: number, digits?: number) => string;
  labels: WeibullPlotLabels;
}) {
  const H = 340;
  const L = 64;
  const R = W - 20;
  const T = 14;
  const B = H - 46;
  const fLo = 0.01;
  const fHi = 0.99;
  const times = [...points.map((p) => p.time), ...band.map((b) => b.time)].filter((t) => t > 0);
  const tLo = times.length ? 10 ** Math.floor(Math.log10(Math.min(...times))) : 1;
  const tHi = times.length ? 10 ** Math.ceil(Math.log10(Math.max(...times))) : 10;
  const x = (t: number) =>
    L + ((Math.log(t) - Math.log(tLo)) / (Math.log(tHi) - Math.log(tLo))) * (R - L);
  const clampF = (f: number) => Math.min(Math.max(f, fLo), fHi);
  const y = (f: number) =>
    B - ((weibullY(clampF(f)) - weibullY(fLo)) / (weibullY(fHi) - weibullY(fLo))) * (B - T);
  const fTicks = [0.01, 0.02, 0.05, 0.1, 0.2, 0.3, 0.5, 0.632, 0.8, 0.9, 0.95, 0.99];
  const line = band
    .filter((b) => b.unreliability >= fLo && b.unreliability <= fHi)
    .map((b, i) => `${i ? "L" : "M"}${x(b.time).toFixed(1)} ${y(b.unreliability).toFixed(1)}`)
    .join(" ");
  const upper = band.map(
    (b, i) => `${i ? "L" : "M"}${x(b.time).toFixed(1)} ${y(b.upper).toFixed(1)}`,
  );
  const lower = [...band].reverse().map((b) => `L${x(b.time).toFixed(1)} ${y(b.lower).toFixed(1)}`);
  const area = band.length ? `${upper.join(" ")} ${lower.join(" ")} Z` : "";
  return (
    <figure className="m-0">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={labels.title}
        className="block h-auto w-full max-w-full"
      >
        <g className="stroke-surface-border" strokeWidth={1}>
          {logTicks(tLo, tHi).map((t) => (
            <line key={`x${t}`} x1={x(t)} x2={x(t)} y1={T} y2={B} />
          ))}
          {fTicks.map((f) => (
            <line key={`y${f}`} x1={L} x2={R} y1={y(f)} y2={y(f)} />
          ))}
        </g>
        <g className="fill-text-secondary text-[11px]">
          {logTicks(tLo, tHi).map((t) => (
            <text key={`xl${t}`} x={x(t)} y={B + 16} textAnchor="middle">
              {formatNumber(t)}
            </text>
          ))}
          {fTicks.map((f) => (
            <text key={`yl${f}`} x={L - 6} y={y(f) + 4} textAnchor="end">
              {formatNumber(f * 100, f === 0.632 ? 1 : 0)} %
            </text>
          ))}
          <text x={(L + R) / 2} y={H - 6} textAnchor="middle" className="text-xs">
            {labels.xAxis}
          </text>
          <text
            x={14}
            y={(T + B) / 2}
            textAnchor="middle"
            transform={`rotate(-90 14 ${(T + B) / 2})`}
            className="text-xs"
          >
            {labels.yAxis}
          </text>
        </g>
        {area ? <path d={area} className="fill-primary/10" stroke="none" /> : null}
        {line ? <path d={line} className="stroke-primary" strokeWidth={2.5} fill="none" /> : null}
        {eta >= tLo && eta <= tHi ? (
          <g>
            <line
              x1={x(eta)}
              x2={x(eta)}
              y1={y(0.632)}
              y2={B}
              className="stroke-primary"
              strokeDasharray="4 3"
            />
            <text x={x(eta) + 5} y={B - 6} className="fill-primary text-[11px]">
              {labels.eta}
            </text>
          </g>
        ) : null}
        {points.map((p) => (
          <circle
            key={`${p.time}-${p.median_rank}`}
            cx={x(p.time)}
            cy={y(p.median_rank)}
            r={5}
            className="fill-accent stroke-surface-1"
            strokeWidth={1.5}
          >
            <title>
              {labels.pointTitle(formatNumber(p.time), formatNumber(p.median_rank * 100, 1))}
            </title>
          </circle>
        ))}
      </svg>
      <figcaption className="mt-1.5 flex flex-wrap gap-4 text-xs text-text-secondary">
        <LegendSwatch className="bg-primary">{labels.fitted}</LegendSwatch>
        <LegendSwatch className="bg-accent" dot>
          {labels.failures}
        </LegendSwatch>
        <LegendSwatch className="bg-primary/20">{labels.band}</LegendSwatch>
      </figcaption>
      <ChartDataTable
        caption={labels.title}
        headers={[labels.xAxis, labels.yAxis]}
        rows={points.map((p) => [
          formatNumber(p.time),
          `${formatNumber(p.median_rank * 100, 1)} %`,
        ])}
      />
    </figure>
  );
}

function LegendSwatch({
  className,
  dot,
  children,
}: {
  className: string;
  dot?: boolean;
  children: string;
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        aria-hidden
        className={`${dot ? "h-2.5 w-2.5 rounded-full" : "h-[3px] w-3.5 rounded"} inline-block ${className}`}
      />
      {children}
    </span>
  );
}

export interface ReliabilityCurveLabels {
  title: string;
  xAxis: string;
  reliability: string;
  hazard: string;
  hazardUnit: string;
  proposed: string;
  current: string;
}

export function ReliabilityHazardChart({
  curve,
  proposedInterval,
  currentInterval,
  formatNumber,
  labels,
}: {
  curve: { time: number; reliability: number; hazard: number }[];
  proposedInterval: number | null;
  currentInterval: number | null;
  formatNumber: (v: number, digits?: number) => string;
  labels: ReliabilityCurveLabels;
}) {
  const H = 270;
  const L = 52;
  const R = W - 64;
  const T = 26;
  const B = H - 44;
  const tMax = curve.at(-1)?.time ?? 1;
  const hMax = Math.max(...curve.map((c) => (Number.isFinite(c.hazard) ? c.hazard : 0)), 1e-12);
  const x = (t: number) => L + (t / tMax) * (R - L);
  const yR = (r: number) => B - r * (B - T);
  const yH = (h: number) => B - Math.min(h / hMax, 1) * (B - T);
  const rPath = curve
    .map((c, i) => `${i ? "L" : "M"}${x(c.time).toFixed(1)} ${yR(c.reliability).toFixed(1)}`)
    .join(" ");
  const hPath = curve
    .filter((c) => Number.isFinite(c.hazard))
    .map((c, i) => `${i ? "L" : "M"}${x(c.time).toFixed(1)} ${yH(c.hazard).toFixed(1)}`)
    .join(" ");
  const ticks = linearTicks(tMax);
  const marker = (t: number | null, cls: string, text: string, dash?: string) =>
    t != null && t > 0 && t <= tMax ? (
      <g>
        <line
          x1={x(t)}
          x2={x(t)}
          y1={T}
          y2={B}
          className={cls}
          strokeWidth={2}
          strokeDasharray={dash}
        />
        <text x={x(t) + 4} y={T + 12} className="fill-text-secondary text-[11px]">
          {text}
        </text>
      </g>
    ) : null;
  return (
    <figure className="m-0">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={labels.title}
        className="block h-auto w-full"
      >
        <g className="stroke-surface-border">
          {ticks.map((t) => (
            <line key={t} x1={x(t)} x2={x(t)} y1={T} y2={B} />
          ))}
          {[0, 0.25, 0.5, 0.75, 1].map((r) => (
            <line key={r} x1={L} x2={R} y1={yR(r)} y2={yR(r)} />
          ))}
        </g>
        <g className="fill-text-secondary text-[11px]">
          {ticks.map((t) => (
            <text key={t} x={x(t)} y={B + 16} textAnchor="middle">
              {formatNumber(t)}
            </text>
          ))}
          {[0, 0.25, 0.5, 0.75, 1].map((r) => (
            <g key={r}>
              <text x={L - 6} y={yR(r) + 4} textAnchor="end">
                {formatNumber(r * 100)} %
              </text>
              <text x={R + 6} y={yR(r) + 4}>
                {formatNumber(hMax * r * 1000, 2)}
              </text>
            </g>
          ))}
          <text x={R + 6} y={T - 12}>
            {labels.hazardUnit}
          </text>
          <text x={(L + R) / 2} y={H - 6} textAnchor="middle" className="text-xs">
            {labels.xAxis}
          </text>
        </g>
        <path d={rPath} className="stroke-primary" strokeWidth={2.5} fill="none" />
        <path
          d={hPath}
          className="stroke-tone-danger-fg"
          strokeWidth={2}
          strokeDasharray="6 4"
          fill="none"
        />
        {marker(currentInterval, "stroke-text-secondary", labels.current, "3 3")}
        {marker(proposedInterval, "stroke-accent", labels.proposed)}
      </svg>
      <figcaption className="mt-1.5 flex flex-wrap gap-4 text-xs text-text-secondary">
        <LegendSwatch className="bg-primary">{labels.reliability}</LegendSwatch>
        <LegendSwatch className="bg-tone-danger-fg">{labels.hazard}</LegendSwatch>
        {proposedInterval != null ? (
          <LegendSwatch className="bg-accent">{labels.proposed}</LegendSwatch>
        ) : null}
        {currentInterval != null ? (
          <LegendSwatch className="bg-text-secondary">{labels.current}</LegendSwatch>
        ) : null}
      </figcaption>
      <ChartDataTable
        caption={labels.title}
        headers={[labels.xAxis, labels.reliability, `${labels.hazard} (${labels.hazardUnit})`]}
        rows={curve
          .filter((_, i) => i % 8 === 7)
          .map((c) => [
            formatNumber(c.time),
            `${formatNumber(c.reliability * 100, 1)} %`,
            formatNumber(c.hazard * 1000, 3),
          ])}
      />
    </figure>
  );
}

export interface CostChartLabels {
  title: string;
  xAxis: string;
  yAxis: string;
  costRate: string;
  runToFailure: string;
  current: string;
  proposed: string;
}

export function CostRateChart({
  curve,
  runToFailureRate,
  currentInterval,
  proposed,
  formatNumber,
  labels,
}: {
  curve: { interval: number; cost_rate: number }[];
  runToFailureRate: number;
  currentInterval: number | null;
  proposed: { interval: number; cost_rate: number } | null;
  formatNumber: (v: number, digits?: number) => string;
  labels: CostChartLabels;
}) {
  const H = 220;
  const L = 74;
  const R = W - 20;
  const T = 26;
  const B = H - 44;
  const data = useMemo(() => curve.filter((c) => Number.isFinite(c.cost_rate)), [curve]);
  const tMax = data.at(-1)?.interval ?? 1;
  const values = data.map((c) => c.cost_rate);
  const cMax = Math.min(Math.max(...values, runToFailureRate), runToFailureRate * 2.2);
  const cMin = Math.min(...values, runToFailureRate) * 0.9;
  const x = (t: number) => L + (t / tMax) * (R - L);
  const y = (c: number) => B - ((Math.min(c, cMax) - cMin) / (cMax - cMin || 1)) * (B - T);
  const path = data
    .map((c, i) => `${i ? "L" : "M"}${x(c.interval).toFixed(1)} ${y(c.cost_rate).toFixed(1)}`)
    .join(" ");
  const ticks = linearTicks(tMax);
  const yTicks = [cMin, (cMin + cMax) / 2, cMax];
  return (
    <figure className="m-0">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={labels.title}
        className="block h-auto w-full"
      >
        <g className="stroke-surface-border">
          {ticks.map((t) => (
            <line key={t} x1={x(t)} x2={x(t)} y1={T} y2={B} />
          ))}
          {yTicks.map((v) => (
            <line key={v} x1={L} x2={R} y1={y(v)} y2={y(v)} />
          ))}
        </g>
        <g className="fill-text-secondary text-[11px]">
          {ticks.map((t) => (
            <text key={t} x={x(t)} y={B + 16} textAnchor="middle">
              {formatNumber(t)}
            </text>
          ))}
          {yTicks.map((v) => (
            <text key={v} x={L - 6} y={y(v) + 4} textAnchor="end">
              {formatNumber(v)}
            </text>
          ))}
          <text x={L - 6} y={T - 12} textAnchor="end">
            {labels.yAxis}
          </text>
          <text x={(L + R) / 2} y={H - 6} textAnchor="middle" className="text-xs">
            {labels.xAxis}
          </text>
        </g>
        <line
          x1={L}
          x2={R}
          y1={y(runToFailureRate)}
          y2={y(runToFailureRate)}
          className="stroke-tone-danger-fg"
          strokeDasharray="5 4"
        />
        {currentInterval != null && currentInterval <= tMax ? (
          <line
            x1={x(currentInterval)}
            x2={x(currentInterval)}
            y1={T}
            y2={B}
            className="stroke-text-secondary"
            strokeDasharray="3 3"
          />
        ) : null}
        <path d={path} className="stroke-primary" strokeWidth={2.5} fill="none" />
        {proposed ? (
          <circle
            cx={x(proposed.interval)}
            cy={y(proposed.cost_rate)}
            r={5}
            className="fill-accent"
          />
        ) : null}
      </svg>
      <figcaption className="mt-1.5 flex flex-wrap gap-4 text-xs text-text-secondary">
        <LegendSwatch className="bg-primary">{labels.costRate}</LegendSwatch>
        <LegendSwatch className="bg-tone-danger-fg">{labels.runToFailure}</LegendSwatch>
        {currentInterval != null ? (
          <LegendSwatch className="bg-text-secondary">{labels.current}</LegendSwatch>
        ) : null}
        {proposed ? (
          <LegendSwatch className="bg-accent" dot>
            {labels.proposed}
          </LegendSwatch>
        ) : null}
      </figcaption>
      <ChartDataTable
        caption={labels.title}
        headers={[labels.xAxis, labels.yAxis]}
        rows={data
          .filter((_, i) => i % 6 === 0)
          .map((c) => [formatNumber(c.interval), formatNumber(c.cost_rate)])}
      />
    </figure>
  );
}
