import * as d3 from "d3";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import {
  CHART_COLORS,
  DEFAULT_MARGIN,
  getInnerDimensions,
  renderXAxis,
  renderYAxis,
  useContainerSize,
} from "@/components/charts/chart-utils";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { AnalyticsWorkloadDay } from "@shared/ipc-types";

const SERIES = [
  { key: "di_created" as const, color: CHART_COLORS.primary, labelKey: "chart.diCreated" as const },
  {
    key: "wo_completed" as const,
    color: CHART_COLORS.success,
    labelKey: "chart.woCompleted" as const,
  },
  { key: "pm_due" as const, color: CHART_COLORS.warning, labelKey: "chart.pmDue" as const },
];

const MARGIN = { ...DEFAULT_MARGIN, bottom: 40 };

export function AnalyticsWorkloadChart(props: {
  days: AnalyticsWorkloadDay[];
  horizonDays: number;
  qualityBadge?: string | null;
}) {
  const { t } = useTranslation("dashboard");
  const { days, horizonDays, qualityBadge } = props;
  const [containerRef, containerSize] = useContainerSize<HTMLDivElement>();
  const xAxisRef = useRef<SVGGElement | null>(null);
  const yAxisRef = useRef<SVGGElement | null>(null);

  const width = containerSize.width;
  const height = 250;
  const { innerWidth, innerHeight } = getInnerDimensions(width, height, MARGIN);

  const xScale = d3
    .scaleBand()
    .domain(days.map((d) => d.date))
    .range([0, innerWidth])
    .padding(days.length > 40 ? 0.08 : 0.25);

  const maxY = d3.max(days, (d) => d.di_created + d.wo_completed + d.pm_due) ?? 1;
  const yScale = d3.scaleLinear().domain([0, maxY]).nice().range([innerHeight, 0]);

  useEffect(() => {
    renderXAxis(xAxisRef.current, xScale, innerHeight);
    renderYAxis(yAxisRef.current, yScale);
  }, [xScale, yScale, innerHeight]);

  const hasData = days.some((d) => d.di_created + d.wo_completed + d.pm_due > 0);

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-base font-medium">
          {t("chart.title", { days: horizonDays })}
        </CardTitle>
        {qualityBadge ? (
          <p className="text-xs text-tone-warning-fg">{t(`kpi.qualityBadge.${qualityBadge}`)}</p>
        ) : null}
      </CardHeader>
      <CardContent className="pb-4">
        {!hasData ? (
          <div className="flex h-[250px] items-center justify-center text-sm text-text-muted">
            {t("chart.empty")}
          </div>
        ) : (
          <>
            <div ref={containerRef} className="h-[250px] w-full">
              {width > 0 ? (
                <svg
                  width={width}
                  height={height}
                  role="img"
                  aria-label={t("chart.title", { days: horizonDays })}
                >
                  <g transform={`translate(${MARGIN.left},${MARGIN.top})`}>
                    <g ref={xAxisRef} />
                    <g ref={yAxisRef} />
                    {days.map((day) => {
                      const x0 = xScale(day.date) ?? 0;
                      let y0 = innerHeight;
                      return SERIES.map((s) => {
                        const v = day[s.key];
                        const h = innerHeight - yScale(v);
                        y0 -= h;
                        return (
                          <rect
                            key={`${day.date}-${s.key}`}
                            x={x0}
                            y={y0}
                            width={xScale.bandwidth()}
                            height={Math.max(0, h)}
                            fill={s.color}
                          />
                        );
                      });
                    })}
                  </g>
                </svg>
              ) : null}
            </div>
            <div className="mt-2 flex flex-wrap gap-3 text-xs text-text-muted">
              {SERIES.map((s) => (
                <span key={s.key} className="inline-flex items-center gap-1.5">
                  <span
                    className="inline-block h-2.5 w-2.5 rounded-sm"
                    style={{ background: s.color }}
                  />
                  {t(s.labelKey)}
                </span>
              ))}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
