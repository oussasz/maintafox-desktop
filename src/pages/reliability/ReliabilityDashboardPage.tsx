import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Area,
  Bar,
  BarChart,
  CartesianGrid,
  Line,
  LineChart,
  ReferenceArea,
  ReferenceDot,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import { mfCard, mfChart } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { listAssets } from "@/services/asset-service";
import {
  evaluateMarkovModel,
  getWeibullDashboardPayload,
  getReliabilityRulIndicator,
  listFailureEvents,
  listMarkovModels,
  listReliabilityKpiSnapshots,
  refreshReliabilityKpiSnapshot,
  runWeibullFit,
} from "@/services/reliability-service";
import type {
  Asset,
  ReliabilityKpiSnapshot,
  ReliabilityRulIndicator,
  WeibullDashboardPayload,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { useRequiredRamsEquipmentId } from "./rams-equipment-context";

type ExposureProvenanceView = {
  source?: string;
  schedule_reference_value_id?: number | null;
  utilization_factor?: number;
  last_completed_wo_closed_at?: string | null;
  fallback_reason?: string | null;
};

function parseExposureProvenance(raw: string | null | undefined): ExposureProvenanceView | null {
  if (raw == null || raw.trim() === "") {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as { exposure_provenance?: ExposureProvenanceView };
    return parsed.exposure_provenance ?? null;
  } catch {
    return null;
  }
}

export function ReliabilityDashboardPage() {
  const { t } = useTranslation("reliability");
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canAnalyze = can(P.RAM_ANALYZE);
  const equipmentId = useRequiredRamsEquipmentId();

  const [rows, setRows] = useState<ReliabilityKpiSnapshot[]>([]);
  const [markovPts, setMarkovPts] = useState<{ name: string; p: number }[]>([]);
  const [markovErr, setMarkovErr] = useState<string | null>(null);
  const [rul, setRul] = useState<ReliabilityRulIndicator | null>(null);
  const [weibullDashboard, setWeibullDashboard] = useState<WeibullDashboardPayload | null>(null);
  const [comparisonAssets, setComparisonAssets] = useState<Asset[]>([]);
  const [failureEventCount, setFailureEventCount] = useState(0);
  const [includeCensored, setIncludeCensored] = useState(false);
  const [comparisonEquipmentId, setComparisonEquipmentId] = useState<number | null>(null);
  const [tOffsetHours, setTOffsetHours] = useState(0);
  const [err, setErr] = useState<string | null>(null);
  const [weibullFitFeedback, setWeibullFitFeedback] = useState<{
    adequate: boolean;
    nPoints: number;
    message: string;
    beta: number | null;
    eta: number | null;
  } | null>(null);

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
      const r = await getWeibullDashboardPayload({
        equipment_id: equipmentId,
        include_censored: includeCensored,
        comparison_equipment_id: comparisonEquipmentId,
        t_offset_hours: tOffsetHours,
      });
      setWeibullDashboard(r);
    } catch (e) {
      setWeibullDashboard(null);
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, [comparisonEquipmentId, equipmentId, includeCensored, tOffsetHours]);

  const loadFailureEvents = useCallback(async () => {
    try {
      const events = await listFailureEvents({ equipment_id: equipmentId, limit: 200 });
      setFailureEventCount(events.length);
    } catch {
      setFailureEventCount(0);
    }
  }, [equipmentId]);

  useEffect(() => {
    void loadSnapshots();
  }, [loadSnapshots]);

  useEffect(() => {
    void loadLatestWeibull();
  }, [loadLatestWeibull]);

  useEffect(() => {
    void loadFailureEvents();
  }, [loadFailureEvents]);

  useEffect(() => {
    let mounted = true;
    const run = async () => {
      try {
        const assets = await listAssets(null, null, null, 300);
        if (mounted) {
          setComparisonAssets(assets.filter((a) => a.id !== equipmentId));
        }
      } catch {
        if (mounted) {
          setComparisonAssets([]);
        }
      }
    };
    void run();
    return () => {
      mounted = false;
    };
  }, [equipmentId]);

  const latest = rows[0] ?? null;
  const exposureProvenance = useMemo(
    () => parseExposureProvenance(latest?.analysis_input_spec_json),
    [latest?.analysis_input_spec_json],
  );

  const weibullData = useMemo(
    () =>
      (weibullDashboard?.points ?? []).map((p) => ({
        ...p,
        cmp_r:
          weibullDashboard?.comparison_points != null
            ? (weibullDashboard.comparison_points.find((cp) => cp.t === p.t)?.r ?? null)
            : null,
      })),
    [weibullDashboard],
  );

  const weibullChartMessage = useMemo(() => {
    if (weibullDashboard == null) {
      return null;
    }
    if (weibullData.length > 0) {
      return null;
    }
    return t("dashboard.weibullEmpty");
  }, [weibullDashboard, weibullData.length, t]);

  const onRunWeibull = async () => {
    if (!canAnalyze) {
      return;
    }
    setErr(null);
    setWeibullFitFeedback(null);
    try {
      const fit = await runWeibullFit({
        equipment_id: equipmentId,
        period_start: null,
        period_end: null,
        include_censored: includeCensored,
      });
      setWeibullFitFeedback({
        adequate: fit.adequate_sample,
        nPoints: fit.n_points,
        message: fit.message,
        beta: fit.beta,
        eta: fit.eta,
      });
      const end = new Date();
      const start = new Date(end);
      start.setFullYear(start.getFullYear() - 1);
      await refreshReliabilityKpiSnapshot({
        equipment_id: equipmentId,
        period_start: start.toISOString(),
        period_end: end.toISOString(),
        min_sample_n: 5,
        repeat_lookback_days: 30,
      });
      await Promise.all([loadLatestWeibull(), loadSnapshots(), loadRul()]);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  };

  const loadMarkov = useCallback(async () => {
    if (!can(P.RAM_VIEW)) {
      setMarkovPts([]);
      setMarkovErr(null);
      return;
    }
    setMarkovErr(null);
    try {
      const list = await listMarkovModels({ equipment_id: equipmentId, limit: 3 });
      const first = list[0];
      if (first == null) {
        setMarkovPts([]);
        return;
      }
      if (!canAnalyze) {
        setMarkovPts([]);
        setMarkovErr(t("dashboard.markovPermission"));
        return;
      }
      const ev = await evaluateMarkovModel(first.id);
      const parsed = JSON.parse(ev.result_json) as {
        steady_state?: number[];
        state_labels?: string[];
      };
      const ss = parsed.steady_state ?? [];
      const labels = parsed.state_labels ?? [];
      setMarkovPts(
        ss.map((p, i) => ({
          name: labels[i] ?? `S${i}`,
          p,
        })),
      );
    } catch (e) {
      setMarkovPts([]);
      setMarkovErr(e instanceof Error ? e.message : String(e));
    }
  }, [can, canAnalyze, equipmentId, t]);

  useEffect(() => {
    void loadMarkov();
  }, [loadMarkov]);

  const loadRul = useCallback(async () => {
    try {
      const r = await getReliabilityRulIndicator(equipmentId);
      setRul(r);
    } catch (e) {
      setRul(null);
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, [equipmentId]);

  const refreshAll = useCallback(async () => {
    setErr(null);
    await Promise.all([
      loadSnapshots(),
      loadLatestWeibull(),
      loadMarkov(),
      loadRul(),
      loadFailureEvents(),
    ]);
  }, [loadFailureEvents, loadLatestWeibull, loadMarkov, loadRul, loadSnapshots]);

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
          onClick={() => void refreshAll()}
        >
          {t("dashboard.refresh")}
        </button>
      </div>

      <section className={mfCard.panel}>
        <h2 className="mb-3 text-base font-medium text-text-primary">{t("dashboard.kpiTitle")}</h2>
        {latest ? (
          <>
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
              <Kpi
                label={t("dashboard.betaActual")}
                value={
                  weibullDashboard?.beta_actual != null
                    ? weibullDashboard.beta_actual.toFixed(3)
                    : "—"
                }
              />
              <Kpi
                label={t("dashboard.betaIndustrial")}
                value={
                  weibullDashboard?.beta_industrial_standard != null
                    ? weibullDashboard.beta_industrial_standard.toFixed(3)
                    : "—"
                }
              />
              <Kpi label="DQ" value={latest.data_quality_score.toFixed(2)} accent />
            </div>
            {weibullDashboard?.beta_industrial_source ? (
              <p className="mt-2 text-[11px] text-text-muted">
                {t("dashboard.betaSource")}: {weibullDashboard.beta_industrial_source}
              </p>
            ) : null}
            <div className="mt-4 rounded-lg border border-surface-border bg-surface-1/60 p-3 text-xs">
              <p className="text-[11px] font-semibold text-text-primary">
                {t("dashboard.exposureTitle")}
              </p>
              <div className="mt-2 grid grid-cols-1 gap-x-4 gap-y-1 sm:grid-cols-2">
                <p className="text-text-secondary">
                  {t("dashboard.exposureSource")}:{" "}
                  <span className="font-mono text-text-primary">
                    {exposureProvenance?.source ?? "runtime_exposure_logs"}
                  </span>
                </p>
                <p className="text-text-secondary">
                  {t("dashboard.exposureKu")}:{" "}
                  <span className="font-mono text-text-primary">
                    {exposureProvenance?.utilization_factor != null
                      ? exposureProvenance.utilization_factor.toFixed(2)
                      : "1.00"}
                  </span>
                </p>
                <p className="text-text-secondary">
                  {t("dashboard.exposureSchedule")}:{" "}
                  <span className="font-mono text-text-primary">
                    {exposureProvenance?.schedule_reference_value_id != null
                      ? `#${exposureProvenance.schedule_reference_value_id}`
                      : "—"}
                  </span>
                </p>
                <p className="text-text-secondary">
                  {t("dashboard.exposureLastWo")}:{" "}
                  <span className="font-mono text-text-primary">
                    {exposureProvenance?.last_completed_wo_closed_at != null
                      ? new Date(exposureProvenance.last_completed_wo_closed_at).toLocaleString()
                      : "—"}
                  </span>
                </p>
                {exposureProvenance?.fallback_reason ? (
                  <p className="text-status-warning sm:col-span-2">
                    {t("dashboard.exposureFallback")}: {exposureProvenance.fallback_reason}
                  </p>
                ) : null}
              </div>
            </div>
          </>
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
              disabled={!canAnalyze || permissionsLoading}
              title={
                permissionsLoading
                  ? t("dashboard.weibullPermissionsLoading")
                  : !canAnalyze
                    ? t("dashboard.weibullPermission")
                    : undefined
              }
              className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs text-text-primary disabled:opacity-40"
              onClick={() => void onRunWeibull().then(() => void loadRul())}
            >
              {t("dashboard.weibullRun")}
            </button>
          </div>
          {!canAnalyze && !permissionsLoading ? (
            <p className="mb-2 rounded border border-status-warning/30 bg-status-warning/10 px-2 py-1.5 text-[11px] text-status-warning">
              {t("dashboard.weibullPermission")}
            </p>
          ) : null}
          {weibullFitFeedback ? (
            <p
              className={cn(
                "mb-2 rounded border px-2 py-1.5 text-[11px]",
                weibullFitFeedback.adequate
                  ? "border-status-success/30 bg-status-success/10 text-status-success"
                  : "border-status-warning/30 bg-status-warning/10 text-status-warning",
              )}
            >
              {weibullFitFeedback.adequate
                ? t("dashboard.weibullFitOk", {
                    beta: weibullFitFeedback.beta?.toFixed(3) ?? "—",
                    eta: weibullFitFeedback.eta?.toFixed(1) ?? "—",
                    n: weibullFitFeedback.nPoints,
                  })
                : t("dashboard.weibullFitInadequate", {
                    n: weibullFitFeedback.nPoints,
                    message: weibullFitFeedback.message,
                  })}
            </p>
          ) : null}
          <div className="mb-3 rounded-md border border-surface-border bg-surface-2/40 p-2 text-[11px] text-text-secondary">
            <p className="font-medium text-text-primary">{t("dashboard.weibullPrereqTitle")}</p>
            <ul className="mt-1 list-inside list-disc space-y-0.5">
              <li>{t("dashboard.weibullPrereqEvents", { count: failureEventCount })}</li>
              <li>
                {t("dashboard.weibullPrereqExposure", {
                  hours: (weibullDashboard?.t_effective_hours ?? rul?.t_hours ?? 0).toFixed(1),
                })}
              </li>
              <li>
                {t("dashboard.weibullPrereqSchedule", {
                  value:
                    exposureProvenance?.schedule_reference_value_id != null
                      ? String(exposureProvenance.schedule_reference_value_id)
                      : t("dashboard.weibullPrereqMissing"),
                })}
              </li>
            </ul>
          </div>
          <div className="mb-3 grid gap-2 rounded-md border border-surface-border bg-surface-2/60 p-2 text-[11px] sm:grid-cols-3">
            <label className="flex flex-col gap-1 text-text-secondary">
              <span className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={includeCensored}
                  onChange={(e) => setIncludeCensored(e.target.checked)}
                />
                {t("dashboard.includeCensored")}
              </span>
              <span className="text-[10px] text-text-muted">
                {t("dashboard.weibullCensoredHint")}
              </span>
            </label>
            <label className="flex items-center gap-2 text-text-secondary">
              {t("dashboard.fleetCompare")}
              <select
                value={comparisonEquipmentId ?? ""}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  setComparisonEquipmentId(Number.isFinite(v) && v > 0 ? v : null);
                }}
                className="min-w-[140px] rounded border border-surface-border bg-surface-1 px-2 py-1 text-xs text-text-primary"
              >
                <option value="">{t("dashboard.noneOption")}</option>
                {comparisonAssets.map((asset) => (
                  <option key={asset.id} value={asset.id}>
                    {asset.asset_name}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1 text-text-secondary">
              <span>
                {t("dashboard.strategyOffset")}: {tOffsetHours.toFixed(0)} h
              </span>
              <input
                type="range"
                min={-200}
                max={200}
                step={5}
                value={tOffsetHours}
                onChange={(e) => setTOffsetHours(Number(e.target.value))}
              />
            </label>
          </div>
          <p className="mb-3 text-[11px] text-text-muted">{t("dashboard.weibullHint")}</p>
          {weibullDashboard ? (
            <p className="mb-2 text-[10px] text-text-muted">
              {t("dashboard.policyHint", {
                danger: weibullDashboard.danger_threshold_r.toFixed(2),
                pm: weibullDashboard.pm_threshold_r.toFixed(2),
              })}
            </p>
          ) : null}
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
                      <ReferenceArea
                        y1={0}
                        y2={weibullDashboard?.danger_threshold_r ?? 0.5}
                        fill="#dc2626"
                        fillOpacity={0.08}
                      />
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
                      <Area
                        type="monotone"
                        dataKey="r_high"
                        stroke="none"
                        fill="#93c5fd"
                        fillOpacity={0.25}
                        isAnimationActive={false}
                      />
                      <Area
                        type="monotone"
                        dataKey="r_low"
                        stroke="none"
                        fill="var(--surface-1)"
                        isAnimationActive={false}
                      />
                      <Line
                        type="monotone"
                        dataKey="r"
                        stroke={mfChart.barFill}
                        strokeWidth={2}
                        dot={false}
                        isAnimationActive={false}
                      />
                      <Line
                        type="monotone"
                        dataKey="cmp_r"
                        stroke="#f59e0b"
                        strokeWidth={1.5}
                        dot={false}
                        isAnimationActive={false}
                        connectNulls
                      />
                      {weibullDashboard?.pm_marker ? (
                        <>
                          <ReferenceLine
                            x={weibullDashboard.pm_marker.t}
                            stroke="#22c55e"
                            strokeDasharray="4 3"
                          />
                          <ReferenceDot
                            x={weibullDashboard.pm_marker.t}
                            y={weibullDashboard.pm_marker.r}
                            r={4}
                            fill="#22c55e"
                            stroke="none"
                            label={{
                              value: weibullDashboard.pm_marker.label,
                              position: "top",
                              fontSize: 10,
                            }}
                          />
                        </>
                      ) : null}
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
                  {(weibullDashboard?.rul_live_hours ?? rul.predicted_rul_hours).toFixed(1)} h
                </p>
              ) : (
                <p className="mt-3 text-text-muted">{t("dashboard.rulEmpty")}</p>
              )}
              {(weibullDashboard?.r_live ?? rul?.reliability_at_t) != null ? (
                <p className="mt-2 text-text-secondary">
                  R(t) ≈ {(weibullDashboard?.r_live ?? rul?.reliability_at_t ?? 0).toFixed(4)}
                  {(weibullDashboard?.t_effective_hours ?? rul?.t_hours) != null ? (
                    <span className="text-text-muted">
                      {" "}
                      @ t = {(weibullDashboard?.t_effective_hours ?? rul?.t_hours ?? 0).toFixed(
                        1,
                      )}{" "}
                      h
                    </span>
                  ) : null}
                </p>
              ) : null}
              {(weibullDashboard?.t_effective_hours ?? rul?.t_hours ?? 0) <= 0 ? (
                <p className="mt-2 rounded border border-status-warning/30 bg-status-warning/10 px-2 py-1 text-[10px] text-status-warning">
                  {t("dashboard.exposureZeroHint")}
                  {exposureProvenance?.fallback_reason ? (
                    <span className="mt-1 block text-text-muted">
                      {t("dashboard.exposureFallback")}: {exposureProvenance.fallback_reason}
                    </span>
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
          ) : markovErr ? (
            <p className="py-12 text-center text-xs text-text-danger">
              {t("dashboard.markovError", { message: markovErr })}
            </p>
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
