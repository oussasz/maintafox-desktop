import * as d3 from "d3";
import { useEffect, useRef, useState } from "react";
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
import { cn } from "@/lib/utils";
import { getDashboardWorkloadChart } from "@/services/dashboard-service";
import type { WorkloadDay } from "@shared/ipc-types";

type Period = 7 | 30;

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

export function DashboardWorkloadChart() {
  const { t } = useTranslation("dashboard");
  const [period, setPeriod] = useState<Period>(7);
  const [days, setDays] = useState<WorkloadDay[]>([]);
  const [loading, setLoading] = useState(true);

  const [containerRef, containerSize] = useContainerSize<HTMLDivElement>();
  const xAxisRef = useRef<SVGGElement | null>(null);
  const yAxisRef = useRef<SVGGElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    getDashboardWorkloadChart(period)
      .then((result) => {
        if (!cancelled) setDays(result.days);
      })
      .catch(() => {
        if (!cancelled) setDays([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [period]);

  const width = containerSize.width;
  const height = Math.max(250, containerSize.height);
  const { innerWidth, innerHeight } = getInnerDimensions(width, height, MARGIN);

  // Scales
  const xScale = d3
    .scaleBand()
    .domain(days.map((d) => d.date))
    .range([0, innerWidth])
    .padding(0.3);

  const maxY = d3.max(days, (d) => d.di_created + d.wo_completed + d.pm_due) ?? 1;
  const yScale = d3.scaleLinear().domain([0, maxY]).nice().range([innerHeight, 0]);

  useEffect(() => {
    renderXAxis(xAxisRef.current, xScale, innerHeight);
    renderYAxis(yAxisRef.current, yScale);
  }, [xScale, yScale, innerHeight]);

  const hasData = days.some((d) => d.di_created + d.wo_completed + d.pm_due > 0);

  // Format x-axis label (short day name)
  const formatDate = (dateStr: string) => {
    try {
      return new Date(`${dateStr}T00:00:00`).toLocaleDateString(undefined, {
        weekday: "short",
      });
    } catch {
      return dateStr;
    }
  };

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <CardTitle className="text-base font-medium">
          {t("chart.title", { days: period })}
        </CardTitle>
        <div className="flex rounded-md border border-surface-border text-xs">
          <button
            type="button"
            className={cn(
              "px-3 py-1 rounded-l-md transition-colors",
              period === 7 ? "bg-primary text-primary-foreground" : "hover:bg-surface-2",
            )}
            onClick={() => setPeriod(7)}
          >
            7d
          </button>
          <button
            type="button"
            className={cn(
              "px-3 py-1 rounded-r-md transition-colors",
              period === 30 ? "bg-primary text-primary-foreground" : "hover:bg-surface-2",
            )}
            onClick={() => setPeriod(30)}
          >
            30d
          </button>
        </div>
      </CardHeader>
      <CardContent className="pb-4">
        {loading ? (
          <div className="flex h-[250px] items-center justify-center text-text-muted">
            {t("chart.loading")}
          </div>
        ) : !hasData ? (
          <div className="flex h-[250px] flex-col items-center justify-center gap-2 text-text-muted">
            <p className="text-sm">{t("chart.empty")}</p>
          </div>
        ) : (
          <>
            <div ref={containerRef} className="h-[250px] w-full">
              {width > 0 && (
                <svg width={width} height={height} role="img" aria-label={t("chart.ariaLabel")}>
                  <g transform={`translate(${MARGIN.left},${MARGIN.top})`}>
                    {days.map((d) => {
                      const x = xScale(d.date) ?? 0;
                      const bw = xScale.bandwidth();
                      let y0 = innerHeight;

                      return SERIES.map((series) => {
                        const val = d[series.key];
                        if (val === 0) return null;
                        const barH = innerHeight - yScale(val);
                        y0 -= barH;
                        return (
                          <rect
                            key={`${d.date}-${series.key}`}
                            x={x}
                            y={y0}
                            width={bw}
                            height={Math.max(0, barH)}
                            fill={series.color}
                            rx={2}
                          >
                            <title>
                              {formatDate(d.date)}: {val}
                            </title>
                          </rect>
                        );
                      });
                    })}
                    <g ref={xAxisRef} />
                    <g ref={yAxisRef} />
                  </g>
                </svg>
              )}
            </div>

            {/* Legend */}
            <div className="mt-3 flex flex-wrap gap-4 text-xs text-text-muted">
              {SERIES.map((s) => (
                <div key={s.key} className="flex items-center gap-1.5">
                  <span
                    className="inline-block h-2.5 w-2.5 rounded-sm"
                    style={{ backgroundColor: s.color }}
                  />
                  {t(s.labelKey)}
                </div>
              ))}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
