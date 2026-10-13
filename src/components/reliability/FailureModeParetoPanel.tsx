/**
 * Pareto of unplanned failures by failure mode (count, maintenance cost or downtime) for one
 * equipment, with the cumulative-share line, and the reliability report export (PDF / Excel).
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bar,
  CartesianGrid,
  ComposedChart,
  Line,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import { mfCard, mfChart } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { exportReliabilityReport, getFailureModePareto } from "@/services/reliability-service";
import { downloadExportedDocument } from "@/services/reports-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  FailureModePareto,
  FailureModeParetoMetric,
  FailureModeParetoRow,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const METRICS: FailureModeParetoMetric[] = ["count", "cost", "downtime"];

export interface ParetoPoint {
  label: string;
  value: number;
  cumulativePct: number;
}

/** Chart points; failures without a failure mode get `unclassifiedLabel`. */
export function paretoChartData(
  rows: FailureModeParetoRow[],
  unclassifiedLabel: string,
): ParetoPoint[] {
  return rows.map((r) => ({
    label: r.mode_label ?? r.mode_code ?? unclassifiedLabel,
    value: r.value,
    cumulativePct: Math.round(r.cumulative_share * 1000) / 10,
  }));
}

/** Number of modes that together reach 80 % of the total (the "vital few"). */
export function vitalFewCount(rows: FailureModeParetoRow[]): number {
  const idx = rows.findIndex((r) => r.cumulative_share >= 0.8 - 1e-9);
  return idx < 0 ? rows.length : idx + 1;
}

/** `YYYY-MM-DD` from a date input to an RFC 3339 bound (start / end of day, UTC). */
export function dayBound(day: string, end: boolean): string | null {
  if (day.trim() === "") return null;
  return `${day}T${end ? "23:59:59" : "00:00:00"}Z`;
}

export function FailureModeParetoPanel({ equipmentId }: { equipmentId: number }) {
  const { t } = useTranslation("reliability");
  const { can } = usePermissions();
  const canExport = can(P.REP_EXPORT);
  const [metric, setMetric] = useState<FailureModeParetoMetric>("count");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [data, setData] = useState<FailureModePareto | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [exporting, setExporting] = useState<"pdf" | "xlsx" | null>(null);
  const [exportErr, setExportErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setErr(null);
    try {
      setData(
        await getFailureModePareto({
          metric,
          equipment_id: equipmentId,
          period_start: dayBound(from, false),
          period_end: dayBound(to, true),
        }),
      );
    } catch (e) {
      setData(null);
      setErr(toErrorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [metric, equipmentId, from, to]);

  useEffect(() => {
    void load();
  }, [load]);

  const onExport = async (fmt: "pdf" | "xlsx") => {
    setExporting(fmt);
    setExportErr(null);
    try {
      downloadExportedDocument(await exportReliabilityReport(equipmentId, fmt));
    } catch (e) {
      setExportErr(toErrorMessage(e));
    } finally {
      setExporting(null);
    }
  };

  const points = data ? paretoChartData(data.rows, t("pareto.unclassified")) : [];

  return (
    <section className={mfCard.panel} aria-labelledby="pareto-title">
      <div className="mb-3 flex flex-wrap items-end justify-between gap-3">
        <h2 id="pareto-title" className="text-base font-medium text-text-primary">
          {t("pareto.title")}
        </h2>
        {canExport ? (
          <div className="flex gap-2">
            {(["pdf", "xlsx"] as const).map((fmt) => (
              <button
                key={fmt}
                type="button"
                disabled={exporting != null}
                className="rounded-md border border-surface-border bg-surface-2 px-3 py-1.5 text-xs text-text-primary hover:bg-surface-3 disabled:opacity-50"
                onClick={() => void onExport(fmt)}
              >
                {exporting === fmt ? t("pareto.exporting") : t(`pareto.export.${fmt}`)}
              </button>
            ))}
          </div>
        ) : null}
      </div>
      {exportErr ? (
        <p role="alert" className="mb-2 text-xs text-text-danger">
          {t("pareto.exportFailed", { message: exportErr })}
        </p>
      ) : null}

      <div className="mb-3 flex flex-wrap items-end gap-3 text-xs">
        <label className="flex flex-col gap-1">
          <span className="text-text-muted">{t("pareto.metricLabel")}</span>
          <select
            className="rounded-md border border-surface-border bg-surface-1 px-2 py-1"
            value={metric}
            onChange={(e) => setMetric(e.target.value as FailureModeParetoMetric)}
          >
            {METRICS.map((m) => (
              <option key={m} value={m}>
                {t(`pareto.metric.${m}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-text-muted">{t("pareto.from")}</span>
          <input
            type="date"
            className="rounded-md border border-surface-border bg-surface-1 px-2 py-1"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-text-muted">{t("pareto.to")}</span>
          <input
            type="date"
            className="rounded-md border border-surface-border bg-surface-1 px-2 py-1"
            value={to}
            onChange={(e) => setTo(e.target.value)}
          />
        </label>
        {from === "" && to === "" ? (
          <span className="text-text-muted">{t("pareto.allTime")}</span>
        ) : null}
      </div>

      {err ? (
        <p role="alert" className="text-xs text-text-danger">
          {t("pareto.loadFailed", { message: err })}
        </p>
      ) : loading && data == null ? (
        <p className="py-8 text-center text-xs text-text-muted">{t("pareto.loading")}</p>
      ) : data == null || data.rows.length === 0 ? (
        <p className="py-8 text-center text-xs text-text-muted">{t("pareto.empty")}</p>
      ) : (
        <>
          <p className="mb-2 text-xs text-text-secondary">
            {t("pareto.vitalFew", { count: vitalFewCount(data.rows), total: data.rows.length })}
          </p>
          <div className="h-64 w-full">
            <ResponsiveContainer width="100%" height="100%">
              <ComposedChart data={points} margin={{ top: 8, right: 8, bottom: 8, left: 0 }}>
                <CartesianGrid stroke={mfChart.gridStroke} strokeDasharray="3 3" />
                <XAxis
                  dataKey="label"
                  tick={{ fill: mfChart.axisTickFill, fontSize: 10 }}
                  interval={0}
                />
                <YAxis yAxisId="v" tick={{ fill: mfChart.axisTickFill, fontSize: 10 }} />
                <YAxis
                  yAxisId="c"
                  orientation="right"
                  domain={[0, 100]}
                  unit="%"
                  tick={{ fill: mfChart.axisTickFill, fontSize: 10 }}
                />
                <Tooltip
                  contentStyle={{
                    backgroundColor: mfChart.tooltipBg,
                    border: `1px solid ${mfChart.tooltipBorder}`,
                  }}
                />
                <ReferenceLine
                  yAxisId="c"
                  y={80}
                  stroke={mfChart.axisTickFill}
                  strokeDasharray="4 4"
                />
                <Bar
                  yAxisId="v"
                  dataKey="value"
                  name={t(`pareto.metric.${metric}`)}
                  fill={mfChart.barFill}
                  radius={[2, 2, 0, 0]}
                />
                <Line
                  yAxisId="c"
                  dataKey="cumulativePct"
                  name={t("pareto.cumulative")}
                  stroke="var(--status-warning, #f59e0b)"
                  dot={false}
                />
              </ComposedChart>
            </ResponsiveContainer>
          </div>
          <table className="mt-3 w-full text-left text-xs">
            <thead className="text-text-muted">
              <tr>
                <th className="py-1">{t("pareto.col.mode")}</th>
                <th className="py-1 text-right">{t("pareto.col.failures")}</th>
                <th className="py-1 text-right">{t("pareto.col.cost")}</th>
                <th className="py-1 text-right">{t("pareto.col.downtime")}</th>
                <th className="py-1 text-right">{t("pareto.col.cumulative")}</th>
              </tr>
            </thead>
            <tbody>
              {data.rows.map((r, i) => (
                <tr
                  key={r.failure_mode_id ?? `none-${i}`}
                  className="border-t border-surface-border"
                >
                  <td className="py-1">
                    {r.mode_label ?? r.mode_code ?? t("pareto.unclassified")}
                  </td>
                  <td className="py-1 text-right font-mono">{r.failure_count}</td>
                  <td className="py-1 text-right font-mono">{r.total_cost.toFixed(2)}</td>
                  <td className="py-1 text-right font-mono">{r.downtime_hours.toFixed(1)}</td>
                  <td className="py-1 text-right font-mono">
                    {(r.cumulative_share * 100).toFixed(1)} %
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
}
