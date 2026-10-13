import * as d3 from "d3";
import { useEffect, useRef } from "react";

import { cn } from "@/lib/utils";

export interface SparklineProps {
  values: number[];
  className?: string | undefined;
}

/** Neutral inline sparkline. Color is not used for trend direction. */
export function Sparkline({ values, className }: SparklineProps) {
  const ref = useRef<SVGSVGElement | null>(null);
  const clean = values.filter((v) => Number.isFinite(v));

  useEffect(() => {
    const svg = ref.current;
    if (!svg || clean.length < 2) {
      if (svg) {
        d3.select(svg).selectAll("*").remove();
      }
      return;
    }
    const width = 72;
    const height = 22;
    const pad = 1;
    const x = d3
      .scaleLinear()
      .domain([0, clean.length - 1])
      .range([pad, width - pad]);
    const [y0, y1] = d3.extent(clean) as [number, number];
    const y = d3
      .scaleLinear()
      .domain(y0 === y1 ? [y0 - 1, y1 + 1] : [y0, y1])
      .range([height - pad, pad]);
    const line = d3
      .line<number>()
      .x((_, i) => x(i))
      .y((v) => y(v))
      .curve(d3.curveMonotoneX);

    const sel = d3.select(svg);
    sel.selectAll("*").remove();
    sel.attr("viewBox", `0 0 ${width} ${height}`).attr("width", width).attr("height", height);
    sel
      .append("path")
      .attr("d", line(clean) ?? "")
      .attr("fill", "none")
      .attr("stroke", "currentColor")
      .attr("stroke-width", 1.25)
      .attr("stroke-linecap", "round");
  }, [clean]);

  if (clean.length < 2) {
    return null;
  }

  return (
    <svg
      ref={ref}
      className={cn("h-[22px] w-[72px] shrink-0 text-text-muted", className)}
      role="img"
      aria-hidden
    />
  );
}
