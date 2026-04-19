import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import { mfCard, mfChart } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  evaluateMarkovModel,
  getLatestWeibullFitForEquipment,
  getReliabilityRulIndicator,
  listMarkovModels,
  listReliabilityKpiSnapshots,
  runWeibullFit,
} from "@/services/reliability-service";
import type {
  ReliabilityKpiSnapshot,
  ReliabilityRulIndicator,
  WeibullFitRecord,
} from "@shared/ipc-types";

import { useRequiredRamsEquipmentId } from "./rams-equipment-context";

function weibullReliabilityPoints(beta: number, eta: number, n = 80) {
  const tMax = Math.max(eta * 4, 1);
  const pts: { t: number; R: number }[] = [];
  for (let i = 0; i <= n; i++) {
    const t = (tMax * i) / n;
    const R = Math.exp(-Math.pow(t / eta, beta));
    pts.push({ t, R });
  }
  return pts;
}

export function ReliabilityDashboardPage() {
  const { t } = useTranslation("reliability");
  const { can } = usePermissions();
  const canAnalyze = can("ram.analyze");
  const equipmentId = useRequiredRamsEquipmentId();

  const [rows, setRows] = useState<ReliabilityKpiSnapshot[]>([]);
  const [wb, setWb] = useState<WeibullFitRecord | null>(null);
  const [markovPts, setMarkovPts] = useState<{ name: string; p: number }[]>([]);
  const [rul, setRul] = useState<ReliabilityRulIndicator | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const loadSnapshots = useCallback(async () => {
    setErr(null);
    try {
      const list = await listReliabilityKpiSnapshots({ equipment_id: equipmentId, limit: 12 });
      setRows(list);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, [equipmentId]);

  const loadLatestWeibull = useCallback(async () => {
    try {
      const r = await getLatestWeibullFitForEquipment(equipmentId);
      setWb(r);
    } catch {
      setWb(null);
    }
  }, [equipmentId]);

  useEffect(() => {
    void loadSnapshots();
  }, [loadSnapshots]);

  useEffect(() => {
    void loadLatestWeibull();
  }, [loadLatestWeibull]);

  const latest = rows[0] ?? null;

  const weibullData = useMemo(() => {
    if (wb?.beta != null && wb?.eta != null && wb.beta > 0 && wb.eta > 0) {
      return weibullReliabilityPoints(wb.beta, wb.eta);
    }
    return [];
  }, [wb]);

  const weibullChartMessage = useMemo(() => {
    if (wb == null) {
      return null;
    }
    if (weibullData.length > 0) {
      return null;
    }
    if (wb.message.trim() !== "") {
      return wb.message;
    }
    return null;
  }, [wb, weibullData.length]);

  const onRunWeibull = async () => {
    if (!canAnalyze) {
      return;
    }
    setErr(null);
    try {
      const r = await runWeibullFit({
        equipment_id: equipmentId,
        period_start: null,
        period_end: null,
      });
      setWb(r);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  const loadMarkov = useCallback(async () => {
    if (!canAnalyze) {
      setMarkovPts([]);
      return;
    }
    try {
      const list = await listMarkovModels({ equipment_id: equipmentId, limit: 3 });
      const first = list[0];
      if (first == null) {
        setMarkovPts([]);
        return;
      }
      const ev = await evaluateMarkovModel(first.id);
      const parsed = JSON.parse(ev.result_json) as { steady_state?: number[] };
      const ss = parsed.steady_state ?? [];
      setMarkovPts(
        ss.map((p, i) => ({
          name: `S${i}`,
          p,
        })),
      );
    } catch {
      setMarkovPts([]);
    }
  }, [canAnalyze, equipmentId]);

  useEffect(() => {
    void loadMarkov();
  }, [loadMarkov]);

  const loadRul = useCallback(async () => {
    try {
      const r = await getReliabilityRulIndicator(equipmentId);
      setRul(r);
    } catch {
      setRul(null);
    }
  }, [equipmentId]);

  useEffect(() => {
    void loadRul();
  }, [loadRul]);

  return (
    <div className="space-y-6 p-6 text-sm text-text-primary">
      {err ? <p className="text-sm text-text-danger">{err}</p> : null}

      <div className="flex flex-wrap items-end gap-3">
        <button
          type="button"
          className="rounded-md border border-surface-border bg-surface-2 px-3 py-1.5 text-xs text-text-primary hover:bg-surface-3"
          onClick={() => void loadSnapshots()}
        >
          {t("dashboard.refresh")}
        </button>
      </div>

      <section className={mfCard.panel}>
        <h2 className="mb-3 text-base font-medium text-text-primary">{t("dashboard.kpiTitle")}</h2>
        {latest ? (
          <div className="grid grid-cols-2 gap-3 gap-y-4 sm:grid-cols-3 lg:grid-cols-6">
            <Kpi
              label={t("dashboard.mtbf")}
              value={latest.mtbf != null ? latest.mtbf.toFixed(1) : "—"}
            />
            <Kpi
              label={t("dashboard.mttr")}
              value={latest.mttr != null ? latest.mttr.toFixed(1) : "—"}
            />
            <Kpi
              label={t("dashboard.availability")}
              value={latest.availability != null ? latest.availability.toFixed(3) : "—"}
            />
            <Kpi
              label={t("dashboard.failureRate")}
              value={latest.failure_rate != null ? latest.failure_rate.toExponential(2) : "—"}
            />
            <Kpi label={t("dashboard.events")} value={String(latest.event_count)} />
            <Kpi label="DQ" value={latest.data_quality_score.toFixed(2)} accent />
          </div>
        ) : (
          <p className="text-xs text-text-muted">{t("dashboard.noSnapshot")}</p>
        )}
      </section>

      <div className="grid grid-cols-1 gap-6 xl:grid-cols-2">
        <section className={mfCard.panel}>
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <h2 className="text-base font-medium text-text-primary">
              {t("dashboard.weibullTitle")}
            </h2>
            <button
              type="button"
              disabled={!canAnalyze}
              className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs text-text-primary disabled:opacity-40"
              onClick={() => void onRunWeibull().then(() => void loadRul())}
            >
              {t("dashboard.weibullRun")}
            </button>
          </div>
          <p className="mb-3 text-[11px] text-text-muted">{t("dashboard.weibullHint")}</p>
          <div className="grid gap-4 xl:grid-cols-3">
            <div className="min-h-[240px] xl:col-span-2">
              {weibullData.length > 0 ? (
                <div className="h-64 w-full">
                  <ResponsiveContainer width="100%" height="100%">
                    <LineChart
                      data={weibullData}
                      margin={{ top: 8, right: 12, left: 0, bottom: 0 }}
                    >
                      <CartesianGrid stroke={mfChart.gridStroke} strokeDasharray="3 3" />
                      <XAxis dataKey="t" tick={{ fontSize: 10, fill: mfChart.axisTickFill }} />
                      <YAxis domain={[0, 1]} tick={{ fontSize: 10, fill: mfChart.axisTickFill }} />
                      <Tooltip
                        contentStyle={{
                          backgroundColor: mfChart.tooltipBg,
                          border: `1px solid ${mfChart.tooltipBorder}`,
                          fontSize: 11,
                          color: "var(--text-primary)",
                        }}
                      />
                      <Line
                        type="monotone"
                        dataKey="R"
                        stroke={mfChart.barFill}
                        strokeWidth={2}
                        dot={false}
                        isAnimationActive={false}
                      />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
              ) : weibullChartMessage ? (
                <div className="flex min-h-[240px] flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-surface-border bg-surface-2/40 px-4 text-center">
                  <p className="text-xs font-medium text-text-primary">
                    {t("dashboard.weibullBlockedTitle")}
                  </p>
                  <p className="max-w-prose text-xs leading-relaxed text-text-secondary">
                    {weibullChartMessage}
                  </p>
                </div>
              ) : (
                <p className="py-12 text-center text-xs text-text-muted">
                  {t("dashboard.weibullEmpty")}
                </p>
              )}
            </div>
            <div className="rounded-lg border border-surface-border bg-surface-2/80 p-3 text-xs">
              <h3 className="text-sm font-semibold text-text-primary">{t("dashboard.rulTitle")}</h3>
              <p className="mt-1 text-[11px] text-text-muted">{t("dashboard.rulHint")}</p>
              {rul?.predicted_rul_hours != null && rul.predicted_rul_hours >= 0 ? (
                <p className="mt-3 font-mono text-lg font-semibold tabular-nums text-text-primary">
                  {rul.predicted_rul_hours.toFixed(1)} h
                </p>
              ) : (
                <p className="mt-3 text-text-muted">{t("dashboard.rulEmpty")}</p>
              )}
              {rul?.reliability_at_t != null ? (
                <p className="mt-2 text-text-secondary">
                  R(t) ≈ {rul.reliability_at_t.toFixed(4)}
                  {rul.t_hours != null ? (
                    <span className="text-text-muted"> @ t = {rul.t_hours.toFixed(1)} h</span>
                  ) : null}
                </p>
              ) : null}
              {rul?.message ? (
                <p className="mt-2 text-[10px] text-text-muted">{rul.message}</p>
              ) : null}
            </div>
          </div>
        </section>

        <section className={mfCard.panel}>
          <h2 className="mb-1 text-base font-medium text-text-primary">
            {t("dashboard.markovTitle")}
          </h2>
          <p className="mb-3 text-[11px] text-text-muted">{t("dashboard.markovHint")}</p>
          {markovPts.length > 0 ? (
            <div className="h-64 w-full">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={markovPts} margin={{ top: 8, right: 12, left: 0, bottom: 0 }}>
                  <CartesianGrid stroke={mfChart.gridStroke} strokeDasharray="3 3" />
                  <XAxis dataKey="name" tick={{ fontSize: 10, fill: mfChart.axisTickFill }} />
                  <YAxis tick={{ fontSize: 10, fill: mfChart.axisTickFill }} />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: mfChart.tooltipBg,
                      border: `1px solid ${mfChart.tooltipBorder}`,
                      fontSize: 11,
                      color: "var(--text-primary)",
                    }}
                  />
                  <Bar dataKey="p" fill={mfChart.barFill} radius={[2, 2, 0, 0]} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          ) : (
            <p className="py-12 text-center text-xs text-text-muted">
              {t("dashboard.markovEmpty")}
            </p>
          )}
        </section>
      </div>
    </div>
  );
}

function Kpi({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div
      className={cn(
        "rounded-lg border border-surface-border px-3 py-2",
        accent ? "border-primary/30 bg-surface-2 shadow-sm" : "bg-surface-1",
      )}
    >
      <p className="text-[10px] uppercase tracking-wide text-text-muted">{label}</p>
      <p className="font-mono text-sm font-semibold text-text-primary">{value}</p>
    </div>
  );
}
