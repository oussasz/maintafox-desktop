// src/components/charts/chart-utils.ts
// Shared utilities for D3-powered chart components.
// Charts use D3 only for scales and axes — React owns the SVG DOM.

import * as d3 from "d3";
import { useEffect, useRef, useState } from "react";

// ─── Default margins ──────────────────────────────────────────────

export interface ChartMargin {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export const DEFAULT_MARGIN: ChartMargin = {
  top: 16,
  right: 16,
  bottom: 32,
  left: 40,
};

// ─── Design-token color palette ───────────────────────────────────
// Mirrors the Maintafox brand tokens from tailwind.config.ts.
// Charts reference these directly (SVG fill/stroke) since CSS custom
// properties are not available outside the DOM cascade.

export const CHART_COLORS = {
  primary: "#003d8f",
  primaryLight: "#4d7bc5",
  primaryDark: "#002b6a",
  accent: "#f0a500",
  accentDark: "#c47f00",
  success: "#198754",
  danger: "#dc3545",
  warning: "#ffc107",
  info: "#0dcaf0",
  neutral: "#6c757d",
} as const;

/** Ordered color sequence for multi-series charts. */
export const CHART_PALETTE = [
  CHART_COLORS.primary,
  CHART_COLORS.accent,
  CHART_COLORS.success,
  CHART_COLORS.info,
  CHART_COLORS.danger,
  CHART_COLORS.warning,
  CHART_COLORS.primaryLight,
  CHART_COLORS.accentDark,
  CHART_COLORS.neutral,
] as const;

// ─── Inner dimensions helper ──────────────────────────────────────

export interface InnerDimensions {
  innerWidth: number;
  innerHeight: number;
}

export function getInnerDimensions(
  width: number,
  height: number,
  margin: ChartMargin = DEFAULT_MARGIN,
): InnerDimensions {
  return {
    innerWidth: Math.max(0, width - margin.left - margin.right),
    innerHeight: Math.max(0, height - margin.top - margin.bottom),
  };
}

// ─── Axis renderers ───────────────────────────────────────────────
// These are called inside useEffect to let D3 paint <g> axis ticks.
// The containing <g> ref is owned by React; D3 only populates children.

export function renderXAxis(
  gRef: SVGGElement | null,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  scale: d3.AxisScale<any>,
  innerHeight: number,
): void {
  if (!gRef) return;
  const sel = d3.select(gRef);
  sel.attr("transform", `translate(0,${innerHeight})`);
  sel.call(d3.axisBottom(scale));
  sel.selectAll("text").attr("class", "fill-current text-[10px]");
}

export function renderYAxis(gRef: SVGGElement | null, scale: d3.AxisScale<d3.NumberValue>): void {
  if (!gRef) return;
  const sel = d3.select(gRef);
  sel.call(d3.axisLeft(scale) as never);
  sel.selectAll("text").attr("class", "fill-current text-[10px]");
}

// ─── Responsive container hook ────────────────────────────────────

interface ContainerSize {
  width: number;
  height: number;
}

/**
 * Observes the container element and returns its current size.
 * When no explicit width/height is provided to a chart, this
 * hook makes it fill its parent responsively.
 */
export function useContainerSize<T extends HTMLElement>(): [React.RefObject<T>, ContainerSize] {
  // Non-null assertion init to satisfy React 18 LegacyRef<T> type.
  // The ref is immediately attached by React before any read occurs.
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
  const containerRef = useRef<T>(null!);
  const [size, setSize] = useState<ContainerSize>({ width: 0, height: 0 });

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) {
        setSize({
          width: Math.round(entry.contentRect.width),
          height: Math.round(entry.contentRect.height),
        });
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return [containerRef, size];
}
