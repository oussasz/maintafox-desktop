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
// CSS variables from src/styles/globals.css, so charts follow the brand and the status tones
// (one colour vocabulary, light + dark theme). SVG fill/stroke accept `var(--…)`.

export const CHART_COLORS = {
  primary: "var(--color-primary)",
  primaryLight: "var(--color-primary-light)",
  primaryDark: "var(--color-primary-dark)",
  /** Logo orange: the one attention point of a screen. */
  accent: "var(--color-accent)",
  accentDark: "var(--color-accent-dark)",
  success: "var(--tone-success-fg)",
  danger: "var(--tone-danger-fg)",
  warning: "var(--tone-warning-fg)",
  info: "var(--tone-info-fg)",
  progress: "var(--tone-progress-fg)",
  neutral: "var(--tone-neutral-fg)",
} as const;

/** Ordered color sequence for multi-series charts (orange kept last: attention only). */
export const CHART_PALETTE = [
  CHART_COLORS.primary,
  CHART_COLORS.progress,
  CHART_COLORS.success,
  CHART_COLORS.neutral,
  CHART_COLORS.primaryLight,
  CHART_COLORS.warning,
  CHART_COLORS.danger,
  CHART_COLORS.accent,
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

export interface YAxisOptions {
  /** Only whole-number ticks (counts: no "0,5 work orders"). */
  integers?: boolean;
  /** Tick text in the app language (a `useFormatters().formatNumber`); default is the raw number. */
  format?: (value: number) => string;
}

export function renderYAxis(
  gRef: SVGGElement | null,
  scale: d3.AxisScale<d3.NumberValue>,
  options: YAxisOptions = {},
): void {
  if (!gRef) return;
  const sel = d3.select(gRef);
  const axis = d3.axisLeft(scale);
  if (options.integers) {
    const ticks = (scale as d3.ScaleLinear<number, number>).ticks().filter(Number.isInteger);
    axis.tickValues(ticks);
  }
  if (options.format) {
    const format = options.format;
    axis.tickFormat((v) => format(Number(v)));
  }
  sel.call(axis as never);
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
