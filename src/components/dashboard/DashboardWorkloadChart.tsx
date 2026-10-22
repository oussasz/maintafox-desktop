/**
 * Activity chart (`get_dashboard_workload_chart`, one query): requests submitted, work orders
 * closed and PM occurrences due per day. A series the user may not see is not drawn (the backend
 * sends `null`, never 0). Loading, error with retry and empty states are the panel's own.
 */
import * as d3 from "d3";
import { BarChart3 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  CHART_COLORS,
  DEFAULT_MARGIN,
  getInnerDimensions,
  renderYAxis,
  useContainerSize,
} from "@/components/charts/chart-utils";
import { useFormatters } from "@/hooks/use-formatters";
import { getDashboardWorkloadChart } from "@/services/dashboard-service";
import type { HomePeriod } from "@/services/home-service";
import type { DashboardWorkloadChart as Chart } from "@shared/ipc-types";

import { HomeNote, HomePanel, HomeSection, usePanelLoad } from "./home/HomeParts";

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

export function DashboardWorkloadChart({ refreshSignal = 0 }: { refreshSignal?: number }) {
  const { t } = useTranslation("dashboard");
  const [period, setPeriod] = useState<HomePeriod>(7);
  const load = useCallback(() => getDashboardWorkloadChart(period), [period]);
  const state = usePanelLoad<Chart>(load, refreshSignal);
  return (
    <HomePanel
      icon={BarChart3}
      title={t("chart.title", { days: period })}
      period={period}
      onPeriodChange={setPeriod}
      state={state}
      testId="home-workload"
    >
      {state.data ? <WorkloadBody chart={state.data} /> : null}
    </HomePanel>
  );
}

function WorkloadBody({ chart }: { chart: Chart }) {
  const { t } = useTranslation("dashboard");
  const { formatDate, formatNumber } = useFormatters();
  const [containerRef, containerSize] = useContainerSize<HTMLDivElement>();
  const xAxisRef = useRef<SVGGElement | null>(null);
  const yAxisRef = useRef<SVGGElement | null>(null);
  const { days } = chart;
  const series = SERIES.filter((s) => days.some((d) => d[s.key] !== null));
  const total = (d: (typeof days)[number]) => series.reduce((acc, s) => acc + (d[s.key] ?? 0), 0);

  const width = containerSize.width;
  const height = Math.max(250, containerSize.height);
  const { innerWidth, innerHeight } = getInnerDimensions(width, height, MARGIN);
  const xScale = d3
    .scaleBand()
    .domain(days.map((d) => d.date))
    .range([0, innerWidth])
    .padding(0.3);
  const maxY = d3.max(days, total) ?? 1;
  const yScale = d3
    .scaleLinear()
    .domain([0, Math.max(1, maxY)])
    .nice()
    .range([innerHeight, 0]);
  const tickEvery = Math.max(1, Math.ceil(days.length / 10));

  useEffect(() => {
    if (!xAxisRef.current) return;
    d3.select(xAxisRef.current)
      .attr("transform", `translate(0,${innerHeight})`)
      .call(
        d3
          .axisBottom(xScale)
          .tickValues(days.filter((_, i) => i % tickEvery === 0).map((d) => d.date))
          .tickFormat((v) => formatDate(`${String(v)}T12:00:00`)) as never,
      )
      .selectAll("text")
      .attr("class", "fill-current text-[10px]");
    renderYAxis(yAxisRef.current, yScale, { integers: true, format: formatNumber });
  }, [xScale, yScale, innerHeight, days, tickEvery, formatDate, formatNumber]);

  const hasData = days.some((d) => total(d) > 0);

  return (
    <HomeSection
      title={t("chart.sectionTitle")}
      definitions={[
        { label: t("chart.diCreated"), text: t("home.def.chartDi") },
        { label: t("chart.woCompleted"), text: t("home.def.chartWo") },
        { label: t("chart.pmDue"), text: t("home.def.chartPm") },
      ]}
    >
      {series.length === 0 ? (
        <HomeNote>{t("chart.noSeries")}</HomeNote>
      ) : !hasData ? (
        <HomeNote>{t("chart.emptyPeriod", { count: chart.period_days })}</HomeNote>
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
                    return series.map((s) => {
                      const val = d[s.key] ?? 0;
                      if (val === 0) return null;
                      const barH = innerHeight - yScale(val);
                      y0 -= barH;
                      return (
                        <rect
                          key={`${d.date}-${s.key}`}
                          x={x}
                          y={y0}
                          width={bw}
                          height={Math.max(0, barH)}
                          fill={s.color}
                          rx={2}
                        >
                          <title>
                            {formatDate(`${d.date}T12:00:00`)} — {t(s.labelKey)} : {val}
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
          <div className="mt-3 flex flex-wrap gap-4 text-xs text-text-muted">
            {series.map((s) => (
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
    </HomeSection>
  );
}
