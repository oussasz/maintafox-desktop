// src/components/charts/LineChart.tsx
// React-managed SVG line chart. D3 is used only for scales, axes, and the line generator.

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

export interface LineChartDatum {
  x: number | Date;
  y: number;
}

export interface LineChartProps {
  data: LineChartDatum[];
  /** Explicit width in px. If omitted, fills container. */
  width?: number;
  /** Explicit height in px. If omitted, fills container. */
  height?: number;
  /** Stroke color (defaults to design-token primary). */
  color?: string;
  /** Chart margins override. */
  margin?: ChartMargin;
  /** Show dots at each data point. */
  showDots?: boolean;
  /** Additional className on the outer wrapper. */
  className?: string;
}

// ─── Component ────────────────────────────────────────────────────

export function LineChart({
  data,
  width: explicitWidth,
  height: explicitHeight,
  color = CHART_COLORS.primary,
  margin = DEFAULT_MARGIN,
  showDots = false,
  className,
}: LineChartProps) {
  const [containerRef, containerSize] = useContainerSize<HTMLDivElement>();
  const xAxisRef = useRef<SVGGElement | null>(null);
  const yAxisRef = useRef<SVGGElement | null>(null);

  const width = explicitWidth ?? containerSize.width;
  const height = explicitHeight ?? containerSize.height;
  const { innerWidth, innerHeight } = getInnerDimensions(width, height, margin);

  // ── Scales ────────────────────────────────────────────────────
  const xExtent = d3.extent(data, (d) => (d.x instanceof Date ? d.x.getTime() : d.x)) as [
    number,
    number,
  ];

  const xScale = d3.scaleLinear().domain(xExtent).range([0, innerWidth]);

  const yScale = d3
    .scaleLinear()
    .domain([0, d3.max(data, (d) => d.y) ?? 0])
    .nice()
    .range([innerHeight, 0]);

  // ── Line generator ────────────────────────────────────────────
  const lineGenerator = d3
    .line<LineChartDatum>()
    .x((d) => xScale(d.x instanceof Date ? d.x.getTime() : d.x))
    .y((d) => yScale(d.y));

  const linePath = lineGenerator(data) ?? "";

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
      <svg width={width} height={height} role="img" aria-label="Line chart">
        <g transform={`translate(${margin.left},${margin.top})`}>
          {/* Line path */}
          <path d={linePath} fill="none" stroke={color} strokeWidth={2} />
          {/* Optional dots */}
          {showDots &&
            data.map((d, i) => (
              <circle
                key={i}
                cx={xScale(d.x instanceof Date ? d.x.getTime() : d.x)}
                cy={yScale(d.y)}
                r={3}
                fill={color}
              />
            ))}
          {/* Axes */}
          <g ref={xAxisRef} />
          <g ref={yAxisRef} />
        </g>
      </svg>
    </div>
  );
}
