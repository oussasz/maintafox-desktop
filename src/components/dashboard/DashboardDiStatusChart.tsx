import * as d3 from "d3";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import {
  CHART_PALETTE,
  DEFAULT_MARGIN,
  getInnerDimensions,
  useContainerSize,
} from "@/components/charts/chart-utils";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { getDashboardDiStatusChart } from "@/services/dashboard-service";
import type { DashboardDiStatusChart as DiStatusModel } from "@shared/ipc-types";

const MARGIN = { ...DEFAULT_MARGIN, left: 120, bottom: 28 };

export function DashboardDiStatusChart() {
  const { t } = useTranslation("dashboard");
  const navigate = useNavigate();
  const [model, setModel] = useState<DiStatusModel | null>(null);
  const [loading, setLoading] = useState(true);
  const [containerRef, { width, height: boxH }] = useContainerSize<HTMLDivElement>();
  const xAxisRef = useRef<SVGGElement | null>(null);
  const yAxisRef = useRef<SVGGElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getDashboardDiStatusChart()
      .then((r) => {
        if (!cancelled) setModel(r);
      })
      .catch(() => {
        if (!cancelled) setModel(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const chartHeight = Math.max(200, Math.min(360, (model?.segments.length ?? 1) * 28 + 80));
  const height = Math.max(chartHeight, boxH || chartHeight);
  const { innerWidth, innerHeight } = getInnerDimensions(width, height, MARGIN);

  const segments = model?.segments ?? [];
  const maxC = d3.max(segments, (s) => s.count) ?? 1;

  const yScale = d3
    .scaleBand()
    .domain(segments.map((s) => s.status))
    .range([0, innerHeight])
    .padding(0.25);

  const xScale = d3.scaleLinear().domain([0, maxC]).nice().range([0, innerWidth]);

  useEffect(() => {
    if (!xAxisRef.current || !yAxisRef.current) {
      return;
    }
    d3.select(xAxisRef.current)
      .attr("transform", `translate(0,${innerHeight})`)
      .call(d3.axisBottom(xScale).ticks(4) as never)
      .selectAll("text")
      .attr("class", "fill-current text-[10px]");
    d3.select(yAxisRef.current)
      .call(d3.axisLeft(yScale) as never)
      .selectAll("text")
      .attr("class", "fill-current text-[10px]");
  }, [xScale, yScale, innerHeight]);

  if (loading) {
    return (
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-base font-medium">{t("widgets.diStatus.title")}</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-text-muted">{t("chart.loading")}</p>
        </CardContent>
      </Card>
    );
  }

  if (!model?.available) {
    return null;
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-base font-medium">{t("widgets.diStatus.title")}</CardTitle>
        <button
          type="button"
          className="text-xs font-medium text-primary underline-offset-2 hover:underline"
          onClick={() => navigate("/requests")}
        >
          {t("widgets.diStatus.openList")}
        </button>
      </CardHeader>
      <CardContent className="pb-4">
        {segments.length === 0 ? (
          <p className="text-sm text-text-muted">{t("widgets.diStatus.empty")}</p>
        ) : (
          <div ref={containerRef} className="w-full" style={{ height: chartHeight }}>
            {width > 0 && (
              <svg width={width} height={height} role="img" aria-label={t("widgets.diStatus.aria")}>
                <g transform={`translate(${MARGIN.left},${MARGIN.top})`}>
                  {segments.map((s, i) => {
                    const y = yScale(s.status) ?? 0;
                    const w = xScale(s.count);
                    const h = yScale.bandwidth();
                    const fill = CHART_PALETTE[i % CHART_PALETTE.length];
                    return (
                      <rect key={s.status} x={0} y={y} width={w} height={h} fill={fill} rx={2}>
                        <title>
                          {s.status}: {s.count}
                        </title>
                      </rect>
                    );
                  })}
                  <g ref={xAxisRef} />
                  <g ref={yAxisRef} />
                </g>
              </svg>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
