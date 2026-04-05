// src/components/charts/BarChart.tsx
// React-managed SVG bar chart. D3 is used only for scales and axes.

import * as d3 from "d3";
import { useEffect, useRef } from "react";

import { cn } from "@/lib/utils";

import {
  CHART_COLORS,
  DEFAULT_MARGIN,
  type ChartMargin,
  getInnerDimensions,
  renderXAxis,
  renderYAxis,
  useContainerSize,
} from "./chart-utils";

// ─── Types ────────────────────────────────────────────────────────

export interface BarChartDatum {
  label: string;
  value: number;
}

export interface BarChartProps {
  data: BarChartDatum[];
  /** Explicit width in px. If omitted, fills container. */
  width?: number;
  /** Explicit height in px. If omitted, fills container. */
  height?: number;
  /** Bar fill color (defaults to design-token primary). */
  color?: string;
  /** Chart margins override. */
  margin?: ChartMargin;
  /** Additional className on the outer wrapper. */
  className?: string;
}

// ─── Component ────────────────────────────────────────────────────

export function BarChart({
  data,
  width: explicitWidth,
  height: explicitHeight,
  color = CHART_COLORS.primary,
  margin = DEFAULT_MARGIN,
  className,
}: BarChartProps) {
  const [containerRef, containerSize] = useContainerSize<HTMLDivElement>();
  const xAxisRef = useRef<SVGGElement | null>(null);
  const yAxisRef = useRef<SVGGElement | null>(null);

  const width = explicitWidth ?? containerSize.width;
  const height = explicitHeight ?? containerSize.height;
  const { innerWidth, innerHeight } = getInnerDimensions(width, height, margin);

  // ── Scales ────────────────────────────────────────────────────
  const xScale = d3
    .scaleBand()
    .domain(data.map((d) => d.label))
    .range([0, innerWidth])
    .padding(0.2);

  const yScale = d3
    .scaleLinear()
    .domain([0, d3.max(data, (d) => d.value) ?? 0])
    .nice()
    .range([innerHeight, 0]);

  // ── Axes (D3 renders into React-owned <g> refs) ──────────────
  useEffect(() => {
    renderXAxis(xAxisRef.current, xScale, innerHeight);
    renderYAxis(yAxisRef.current, yScale);
  }, [data, innerWidth, innerHeight]); // eslint-disable-line react-hooks/exhaustive-deps

  if (width === 0 || height === 0) {
    return <div ref={containerRef} className={cn("h-full w-full", className)} />;
  }

  return (
    <div ref={containerRef} className={cn("h-full w-full", className)}>
      <svg width={width} height={height} role="img" aria-label="Bar chart">
        <g transform={`translate(${margin.left},${margin.top})`}>
          {/* Bars — React renders each <rect> */}
          {data.map((d) => {
            const x = xScale(d.label);
            const barHeight = innerHeight - yScale(d.value);
            return (
              <rect
                key={d.label}
                x={x}
                y={yScale(d.value)}
                width={xScale.bandwidth()}
                height={Math.max(0, barHeight)}
                fill={color}
                rx={2}
              />
            );
          })}
          {/* Axes */}
          <g ref={xAxisRef} />
          <g ref={yAxisRef} />
        </g>
      </svg>
    </div>
  );
}
