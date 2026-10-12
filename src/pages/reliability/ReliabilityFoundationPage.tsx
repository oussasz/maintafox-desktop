import { ExternalLink, Gauge, RefreshCw, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfCard } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { seedRamsPresentationData } from "@/services/diagnostics-service";
import {
  getRamEquipmentQualityBadge,
  iso14224FailureDatasetCompleteness,
  listComputationJobs,
  listFailureEvents,
  listFtaModels,
  listMarkovModels,
  listMcModels,
  listRbdModels,
  listRamDataQualityIssues,
  listReliabilityKpiSnapshots,
  listRuntimeExposureLogs,
  refreshReliabilityKpiSnapshot,
} from "@/services/reliability-service";
import { P } from "@shared/rbac/permissions.generated";

import { useRequiredRamsEquipmentId } from "./rams-equipment-context";

type FoundationTab = "quality" | "events" | "models" | "kpis";

type MetricCell = {
  label: string;
  value: string;
};

function scoreColor(score: number | null): string {
  if (score == null) return "text-slate-500";
  if (score >= 0.8) return "text-emerald-600 dark:text-emerald-400";
  if (score >= 0.6) return "text-amber-600 dark:text-amber-400";
  return "text-rose-600 dark:text-rose-400";
}

function qualityBadgeClass(badge: string): string {
  const token = badge.toLowerCase();
  if (token.includes("green")) return "border-emerald-500/30 bg-emerald-500/10 text-emerald-300";
  if (token.includes("amber") || token.includes("yellow"))
    return "border-amber-500/30 bg-amber-500/10 text-amber-300";
  if (token.includes("red")) return "border-rose-500/30 bg-rose-500/10 text-rose-300";
  return "border-slate-500/30 bg-slate-500/10 text-slate-300";
}

function safeOpenLink(url: string) {
  if (typeof window === "undefined") return;
  if (url.startsWith("maintafox://")) {
    window.location.href = url;
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

function miniCells(cells: MetricCell[]) {
  return (
    <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-[11px]">
      {cells.map((cell) => (
        <div key={cell.label} className="contents">
          <span className="text-text-muted">{cell.label}</span>
          <span className="text-right font-mono tabular-nums text-text-primary">{cell.value}</span>
        </div>
      ))}
    </div>
  );
}

export function ReliabilityFoundationPage() {
  const { t } = useTranslation("reliability");
  const navigate = useNavigate();
  const { can } = usePermissions();
  const canSeedPresentation = can(P.ADM_SETTINGS);
  const equipmentId = useRequiredRamsEquipmentId();

  const [activeTab, setActiveTab] = useState<FoundationTab>("quality");
  const [loading, setLoading] = useState(false);
  const [kpiRefreshing, setKpiRefreshing] = useState(false);
  const [presentationSeeding, setPresentationSeeding] = useState(false);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [qualityBadge, setQualityBadge] = useState<Awaited<
    ReturnType<typeof getRamEquipmentQualityBadge>
  > | null>(null);
  const [iso, setIso] = useState<Awaited<
    ReturnType<typeof iso14224FailureDatasetCompleteness>
  > | null>(null);
  const [issues, setIssues] = useState<Awaited<ReturnType<typeof listRamDataQualityIssues>>>([]);
  const [events, setEvents] = useState<Awaited<ReturnType<typeof listFailureEvents>>>([]);
  const [exposures, setExposures] = useState<Awaited<ReturnType<typeof listRuntimeExposureLogs>>>(
    [],
  );
  const [ftaModels, setFtaModels] = useState<Awaited<ReturnType<typeof listFtaModels>>>([]);
  const [rbdModels, setRbdModels] = useState<Awaited<ReturnType<typeof listRbdModels>>>([]);
  const [mcModels, setMcModels] = useState<Awaited<ReturnType<typeof listMcModels>>>([]);
  const [markovModels, setMarkovModels] = useState<Awaited<ReturnType<typeof listMarkovModels>>>(
    [],
  );
  const [kpiSnapshots, setKpiSnapshots] = useState<
    Awaited<ReturnType<typeof listReliabilityKpiSnapshots>>
  >([]);
  const [jobs, setJobs] = useState<Awaited<ReturnType<typeof listComputationJobs>>>([]);

  const refreshAll = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [
        nextBadge,
        nextIso,
        nextIssues,
        nextEvents,
        nextExposure,
        nextFta,
        nextRbd,
        nextMc,
        nextMarkov,
        nextKpi,
        nextJobs,
      ] = await Promise.all([
        getRamEquipmentQualityBadge(equipmentId),
        iso14224FailureDatasetCompleteness(equipmentId),
        listRamDataQualityIssues({ equipment_id: equipmentId }),
        listFailureEvents({ equipment_id: equipmentId, limit: 25 }),
        listRuntimeExposureLogs({ equipment_id: equipmentId, limit: 25 }),
        listFtaModels({ equipment_id: equipmentId, limit: 10 }),
        listRbdModels({ equipment_id: equipmentId, limit: 10 }),
        listMcModels({ equipment_id: equipmentId, limit: 10 }),
        listMarkovModels({ equipment_id: equipmentId, limit: 10 }),
        listReliabilityKpiSnapshots({ equipment_id: equipmentId, limit: 20 }),
        listComputationJobs(20),
      ]);
      setQualityBadge(nextBadge);
      setIso(nextIso);
      setIssues(nextIssues);
      setEvents(nextEvents);
      setExposures(nextExposure);
      setFtaModels(nextFta);
      setRbdModels(nextRbd);
      setMcModels(nextMc);
      setMarkovModels(nextMarkov);
      setKpiSnapshots(nextKpi);
      setJobs(nextJobs);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [equipmentId]);

  useEffect(() => {
    void refreshAll();
  }, [refreshAll]);

  const scorePercent = useMemo(() => {
    if (qualityBadge?.data_quality_score == null) return null;
    return Math.max(0, Math.min(100, Math.round(qualityBadge.data_quality_score * 100)));
  }, [qualityBadge]);

  const latestKpi = kpiSnapshots[0] ?? null;

  const onRefreshKpiSnapshot = async () => {
    setKpiRefreshing(true);
    setError(null);
    setStatusMessage(null);
    try {
      const end = new Date();
      const start = new Date(end);
      start.setMonth(start.getMonth() - 12);
      const snap = await refreshReliabilityKpiSnapshot({
        equipment_id: equipmentId,
        period_start: start.toISOString(),
        period_end: end.toISOString(),
        min_sample_n: 5,
        repeat_lookback_days: 30,
      });
      setStatusMessage(
        t("foundation.kpiRefreshDone", {
          events: snap.event_count,
          hours: snap.analysis_input_spec_json
            ? (() => {
                try {
                  const parsed = JSON.parse(snap.analysis_input_spec_json) as {
                    exposure_hours?: number;
                  };
                  return parsed.exposure_hours?.toFixed(1) ?? "—";
                } catch {
                  return "—";
                }
              })()
            : "—",
        }),
      );
      await refreshAll();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setKpiRefreshing(false);
    }
  };

  const onSeedPresentation = async () => {
    // DEMO ONLY — explicit button click; never triggered by mount / refreshAll.
    if (!canSeedPresentation) {
      setError(t("foundation.presentationSeedPermission"));
      return;
    }
    setPresentationSeeding(true);
    setError(null);
    setStatusMessage(null);
    try {
      const report = await seedRamsPresentationData({
        equipment_id: equipmentId,
        months_back: 12,
        failure_count: 8,
      });
      if (report.errors.length > 0) {
        setError(report.errors.join(" · "));
      }
      setStatusMessage(
        report.skipped
          ? t("foundation.presentationSeedSkipped")
          : t("foundation.presentationSeedDone", {
              wo: report.work_orders_created,
              beta: report.weibull_beta?.toFixed(3) ?? "—",
              eta: report.weibull_eta?.toFixed(1) ?? "—",
              markov: report.markov_model_id ?? "—",
            }),
      );
      if (report.warnings.length > 0) {
        setStatusMessage((prev) => [prev, report.warnings.join(" · ")].filter(Boolean).join(" · "));
      }
      await refreshAll();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPresentationSeeding(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-surface-0 text-sm text-text-primary">
      <div className="border-b border-surface-border px-4 py-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <h2 className="text-base font-semibold text-text-primary">{t("foundation.title")}</h2>
            <p className="mt-0.5 text-xs text-text-muted">{t("foundation.hint")}</p>
          </div>
          <button
            type="button"
            onClick={() => void refreshAll()}
            className="inline-flex items-center gap-1 rounded-md border border-surface-border bg-surface-1 px-3 py-1.5 text-xs hover:bg-surface-2"
            disabled={loading}
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            {loading ? t("foundationPage.refreshing") : t("foundationPage.refresh")}
          </button>
        </div>
        {error ? <p className="mt-2 text-xs text-status-danger">{error}</p> : null}
        {statusMessage ? <p className="mt-2 text-xs text-text-secondary">{statusMessage}</p> : null}
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-4">
        <Tabs
          value={activeTab}
          onValueChange={(v) => setActiveTab(v as FoundationTab)}
          className="h-full"
        >
          <TabsList className="mb-3 grid w-full grid-cols-2 gap-1 bg-surface-1 p-1 md:grid-cols-4">
            <TabsTrigger value="quality">{t("foundationPage.tabs.quality")}</TabsTrigger>
            <TabsTrigger value="events">{t("foundationPage.tabs.events")}</TabsTrigger>
            <TabsTrigger value="models">{t("foundationPage.tabs.models")}</TabsTrigger>
            <TabsTrigger value="kpis">{t("foundationPage.tabs.kpis")}</TabsTrigger>
          </TabsList>

          <TabsContent value="quality" className="space-y-3">
            <div className="grid gap-3 lg:grid-cols-2">
              <section className={mfCard.panel}>
                <p className="text-xs uppercase tracking-wide text-text-muted">
                  {t("foundationPage.quality.dataHealth")}
                </p>
                <div className="mt-3 flex items-center gap-4">
                  <div
                    className="relative h-20 w-20 rounded-full"
                    style={{
                      background:
                        scorePercent == null
                          ? "conic-gradient(var(--surface-border) 0deg 360deg)"
                          : `conic-gradient(var(--color-primary) ${scorePercent * 3.6}deg, var(--surface-border) 0deg)`,
                    }}
                  >
                    <div className="absolute inset-[6px] flex items-center justify-center rounded-full bg-surface-1">
                      <span
                        className={`font-mono text-xs font-semibold ${scoreColor(qualityBadge?.data_quality_score ?? null)}`}
                      >
                        {scorePercent == null
                          ? t("foundationPage.notAvailable")
                          : `${scorePercent}%`}
                      </span>
                    </div>
                  </div>
                  <div>
                    <span
                      className={`inline-flex rounded border px-2 py-0.5 text-xs ${qualityBadgeClass(qualityBadge?.badge ?? "unknown")}`}
                    >
                      {qualityBadge?.badge ?? t("foundationPage.quality.unknownBadge")}
                    </span>
                    <p className="mt-2 text-xs text-text-muted">
                      {t("foundationPage.quality.activeIssues", {
                        count: issues.length,
                        equipment: equipmentId,
                      })}
                    </p>
                  </div>
                </div>
              </section>

              <section className={mfCard.panel}>
                <p className="text-xs uppercase tracking-wide text-text-muted">
                  {t("foundationPage.quality.isoTitle")}
                </p>
                {iso ? (
                  miniCells([
                    { label: t("foundationPage.iso.records"), value: String(iso.event_count) },
                    {
                      label: t("foundationPage.iso.overall"),
                      value: `${iso.completeness_percent.toFixed(1)}%`,
                    },
                    {
                      label: t("foundationPage.iso.equipment"),
                      value: `${iso.dim_equipment_id_pct.toFixed(1)}%`,
                    },
                    {
                      label: t("foundationPage.iso.interval"),
                      value: `${iso.dim_failure_interval_pct.toFixed(1)}%`,
                    },
                    {
                      label: t("foundationPage.iso.mode"),
                      value: `${iso.dim_failure_mode_pct.toFixed(1)}%`,
                    },
                    {
                      label: t("foundationPage.iso.closure"),
                      value: `${iso.dim_corrective_closure_pct.toFixed(1)}%`,
                    },
                  ])
                ) : (
                  <p className="mt-3 text-xs text-text-muted">{t("foundationPage.noData")}</p>
                )}
              </section>
            </div>

            <section className={mfCard.panel}>
              <p className="text-xs uppercase tracking-wide text-text-muted">
                {t("foundationPage.quality.remediationTitle")}
              </p>
              <div className="mt-2 divide-y divide-surface-border">
                {issues.length === 0 ? (
                  <p className="py-2 text-xs text-text-muted">
                    {t("foundationPage.quality.noIssues")}
                  </p>
                ) : (
                  issues.map((issue) => (
                    <div
                      key={`${issue.issue_code}-${issue.equipment_id}`}
                      className="flex items-center justify-between gap-3 py-2"
                    >
                      <div>
                        <p className="font-mono text-xs text-text-primary">{issue.issue_code}</p>
                        <p className="text-[11px] text-text-muted">
                          {t("foundationPage.quality.severity", { severity: issue.severity })}
                        </p>
                      </div>
                      <button
                        type="button"
                        onClick={() => safeOpenLink(issue.remediation_url)}
                        className="inline-flex items-center gap-1 rounded border border-surface-border px-2 py-1 text-[11px] hover:bg-surface-2"
                      >
                        <Search className="h-3.5 w-3.5" />
                        {t("foundationPage.quality.openAction")}
                      </button>
                    </div>
                  ))
                )}
              </div>
            </section>
          </TabsContent>

          <TabsContent value="events" className="space-y-3">
            <section className={mfCard.panel}>
              <div className="mb-2 flex items-center justify-between">
                <p className="text-xs uppercase tracking-wide text-text-muted">
                  {t("foundationPage.events.title")}
                </p>
                <span className="font-mono text-xs text-text-muted">
                  {t("foundationPage.events.rows", { count: events.length })}
                </span>
              </div>
              <div className="overflow-auto rounded border border-surface-border">
                <table className="min-w-full text-[11px]">
                  <thead className="bg-surface-2 text-text-muted">
                    <tr>
                      <th className="px-2 py-1 text-left">{t("foundationPage.col.id")}</th>
                      <th className="px-2 py-1 text-left">{t("foundationPage.col.source")}</th>
                      <th className="px-2 py-1 text-right">{t("foundationPage.col.downtime")}</th>
                      <th className="px-2 py-1 text-right">{t("foundationPage.col.repair")}</th>
                      <th className="px-2 py-1 text-left">{t("foundationPage.col.status")}</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-surface-border bg-surface-1">
                    {events.map((ev) => (
                      <tr key={ev.id}>
                        <td className="px-2 py-1 font-mono">{ev.id}</td>
                        <td className="px-2 py-1">{ev.source_type}</td>
                        <td className="px-2 py-1 text-right font-mono">
                          {ev.downtime_duration_hours.toFixed(2)}
                        </td>
                        <td className="px-2 py-1 text-right font-mono">
                          {ev.active_repair_hours.toFixed(2)}
                        </td>
                        <td className="px-2 py-1">{ev.verification_status}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </section>

            <section className={mfCard.panel}>
              <div className="mb-2 flex items-center justify-between">
                <p className="text-xs uppercase tracking-wide text-text-muted">
                  {t("foundationPage.exposure.title")}
                </p>
                <span className="font-mono text-xs text-text-muted">
                  {t("foundationPage.exposure.readings", { count: exposures.length })}
                </span>
              </div>
              <div className="overflow-auto rounded border border-surface-border">
                <table className="min-w-full text-[11px]">
                  <thead className="bg-surface-2 text-text-muted">
                    <tr>
                      <th className="px-2 py-1 text-left">{t("foundationPage.col.type")}</th>
                      <th className="px-2 py-1 text-right">{t("foundationPage.col.value")}</th>
                      <th className="px-2 py-1 text-left">{t("foundationPage.col.date")}</th>
                      <th className="px-2 py-1 text-left">{t("foundationPage.col.source")}</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-surface-border bg-surface-1">
                    {exposures.map((x) => (
                      <tr key={x.id}>
                        <td className="px-2 py-1">{x.exposure_type}</td>
                        <td className="px-2 py-1 text-right font-mono">{x.value.toFixed(2)}</td>
                        <td className="px-2 py-1 font-mono">{x.recorded_at}</td>
                        <td className="px-2 py-1">{x.source_type}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </section>
          </TabsContent>

          <TabsContent value="models" className="space-y-3">
            <div className="grid gap-3 lg:grid-cols-2">
              <section className={mfCard.panel}>
                <div className="mb-2 flex items-center justify-between">
                  <p className="text-xs uppercase tracking-wide text-text-muted">
                    {t("foundationPage.models.fta")}
                  </p>
                  <button
                    type="button"
                    onClick={() => void navigate("/analytics/rams/lab")}
                    className="inline-flex items-center gap-1 rounded border border-surface-border px-2 py-1 text-[11px] hover:bg-surface-2"
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                    {t("foundationPage.models.manage")}
                  </button>
                </div>
                <div className="space-y-1 text-[11px]">
                  {ftaModels.length === 0 ? (
                    <p className="text-text-muted">{t("foundationPage.models.noFta")}</p>
                  ) : (
                    ftaModels.map((m) => (
                      <p key={m.id}>
                        #{m.id} {m.title}
                      </p>
                    ))
                  )}
                </div>
              </section>

              <section className={mfCard.panel}>
                <div className="mb-2 flex items-center justify-between">
                  <p className="text-xs uppercase tracking-wide text-text-muted">
                    {t("foundationPage.models.rbd")}
                  </p>
                  <button
                    type="button"
                    onClick={() => void navigate("/analytics/rams/lab")}
                    className="inline-flex items-center gap-1 rounded border border-surface-border px-2 py-1 text-[11px] hover:bg-surface-2"
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                    {t("foundationPage.models.manage")}
                  </button>
                </div>
                <div className="space-y-1 text-[11px]">
                  {rbdModels.length === 0 ? (
                    <p className="text-text-muted">{t("foundationPage.models.noRbd")}</p>
                  ) : (
                    rbdModels.map((m) => (
                      <p key={m.id}>
                        #{m.id} {m.title}
                      </p>
                    ))
                  )}
                </div>
              </section>

              <section className={mfCard.panel}>
                <div className="mb-2 flex items-center justify-between">
                  <p className="text-xs uppercase tracking-wide text-text-muted">
                    {t("foundationPage.models.monteCarlo")}
                  </p>
                  <button
                    type="button"
                    onClick={() => void navigate("/analytics/rams/advanced")}
                    className="inline-flex items-center gap-1 rounded border border-surface-border px-2 py-1 text-[11px] hover:bg-surface-2"
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                    {t("foundationPage.models.manage")}
                  </button>
                </div>
                <div className="overflow-auto rounded border border-surface-border">
                  <table className="min-w-full text-[11px]">
                    <thead className="bg-surface-2 text-text-muted">
                      <tr>
                        <th className="px-2 py-1 text-left">{t("foundationPage.col.title")}</th>
                        <th className="px-2 py-1 text-right">{t("foundationPage.col.trials")}</th>
                        <th className="px-2 py-1 text-left">{t("foundationPage.col.status")}</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-surface-border bg-surface-1">
                      {mcModels.map((m) => (
                        <tr key={m.id}>
                          <td className="px-2 py-1">{m.title}</td>
                          <td className="px-2 py-1 text-right font-mono">{m.trials}</td>
                          <td className="px-2 py-1">{m.status}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>

              <section className={mfCard.panel}>
                <div className="mb-2 flex items-center justify-between">
                  <p className="text-xs uppercase tracking-wide text-text-muted">
                    {t("foundationPage.models.markov")}
                  </p>
                  <button
                    type="button"
                    onClick={() => void navigate("/analytics/rams/advanced")}
                    className="inline-flex items-center gap-1 rounded border border-surface-border px-2 py-1 text-[11px] hover:bg-surface-2"
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                    {t("foundationPage.models.manage")}
                  </button>
                </div>
                <div className="space-y-1 text-[11px]">
                  {markovModels.length === 0 ? (
                    <p className="text-text-muted">{t("foundationPage.models.noMarkov")}</p>
                  ) : (
                    markovModels.map((m) => (
                      <div key={m.id} className="rounded border border-surface-border p-2">
                        <p className="font-medium text-text-primary">{m.title}</p>
                        <p className="mt-1 font-mono text-[10px] text-text-muted">
                          {t("foundationPage.models.status", { status: m.status })}
                        </p>
                      </div>
                    ))
                  )}
                </div>
              </section>
            </div>
          </TabsContent>

          <TabsContent value="kpis" className="space-y-3">
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                disabled={kpiRefreshing || loading}
                onClick={() => void onRefreshKpiSnapshot()}
                className="rounded-md border border-surface-border bg-surface-1 px-3 py-1.5 text-xs hover:bg-surface-2 disabled:opacity-40"
              >
                {kpiRefreshing ? t("foundation.kpiRefreshing") : t("foundation.kpiRefresh")}
              </button>
              <button
                type="button"
                disabled={!canSeedPresentation || presentationSeeding || loading}
                title={
                  !canSeedPresentation ? t("foundation.presentationSeedPermission") : undefined
                }
                onClick={() => void onSeedPresentation()}
                className="rounded-md border border-primary/30 bg-primary/10 px-3 py-1.5 text-xs text-text-primary hover:bg-primary/15 disabled:opacity-40"
              >
                {presentationSeeding
                  ? t("foundation.presentationSeeding")
                  : t("foundation.presentationSeed")}
              </button>
            </div>
            <div className="grid gap-3 lg:grid-cols-2">
              <section className={mfCard.panel}>
                <div className="mb-2 flex items-center gap-2">
                  <Gauge className="h-4 w-4 text-text-muted" />
                  <p className="text-xs uppercase tracking-wide text-text-muted">
                    {t("foundationPage.kpi.latest")}
                  </p>
                </div>
                {latestKpi ? (
                  miniCells([
                    {
                      label: t("foundationPage.kpi.mtbf"),
                      value: latestKpi.mtbf == null ? "—" : latestKpi.mtbf.toFixed(2),
                    },
                    {
                      label: t("foundationPage.kpi.mttr"),
                      value: latestKpi.mttr == null ? "—" : latestKpi.mttr.toFixed(2),
                    },
                    {
                      label: t("foundationPage.kpi.availability"),
                      value:
                        latestKpi.availability == null
                          ? "—"
                          : `${(latestKpi.availability * 100).toFixed(2)}%`,
                    },
                    {
                      label: t("foundationPage.kpi.failureRate"),
                      value:
                        latestKpi.failure_rate == null ? "—" : latestKpi.failure_rate.toFixed(4),
                    },
                    { label: t("foundationPage.kpi.events"), value: String(latestKpi.event_count) },
                    {
                      label: t("foundationPage.kpi.quality"),
                      value: latestKpi.data_quality_score.toFixed(2),
                    },
                  ])
                ) : (
                  <p className="text-xs text-text-muted">{t("foundationPage.kpi.noSnapshot")}</p>
                )}
              </section>

              <section className={mfCard.panel}>
                <p className="text-xs uppercase tracking-wide text-text-muted">
                  {t("foundationPage.jobs.title")}
                </p>
                <div className="mt-2 overflow-auto rounded border border-surface-border">
                  <table className="min-w-full text-[11px]">
                    <thead className="bg-surface-2 text-text-muted">
                      <tr>
                        <th className="px-2 py-1 text-left">{t("foundationPage.col.id")}</th>
                        <th className="px-2 py-1 text-left">{t("foundationPage.col.type")}</th>
                        <th className="px-2 py-1 text-left">{t("foundationPage.col.status")}</th>
                        <th className="px-2 py-1 text-right">%</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-surface-border bg-surface-1">
                      {jobs.map((job) => (
                        <tr key={job.id}>
                          <td className="px-2 py-1 font-mono">{job.id}</td>
                          <td className="px-2 py-1">{job.job_kind}</td>
                          <td className="px-2 py-1">{job.status}</td>
                          <td className="px-2 py-1 text-right font-mono">
                            {job.progress_pct.toFixed(0)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>
            </div>
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
