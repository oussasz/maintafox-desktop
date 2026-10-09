import {
  Activity,
  AlertTriangle,
  CalendarClock,
  ClipboardList,
  Package,
  Settings2,
  Wrench,
} from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { AnalyticsCustomizeDialog } from "@/components/analytics/AnalyticsCustomizeDialog";
import { AnalyticsDiStatusChart } from "@/components/analytics/AnalyticsDiStatusChart";
import { AnalyticsWorkloadChart } from "@/components/analytics/AnalyticsWorkloadChart";
import { CHART_COLORS } from "@/components/charts/chart-utils";
import { KpiCard } from "@/components/dashboard/KpiCard";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import {
  getAnalyticsDashboardLayout,
  getAnalyticsExecutiveDashboard,
} from "@/services/analytics-dashboard-service";
import { listOrgTree } from "@/services/org-node-service";
import { useSessionStore } from "@/store/session-store";
import {
  ANALYTICS_DASHBOARD_WIDGET_IDS,
  ANALYTICS_DASHBOARD_WIDGET_PERMISSION,
  buildRoleDefaultAnalyticsLayout,
  mergeWithDefaultAnalyticsLayout,
  parseAnalyticsDashboardLayoutJson,
  type AnalyticsDashboardLayoutV1,
} from "@shared/analytics-dashboard-layout";
import type { AnalyticsExecutiveDashboard, OrgTreeRow } from "@shared/ipc-types";
import {
  ANALYTICS_RAMS_BASE_PATH,
  DASHBOARD_KPI_DRILL_PATH,
  DASHBOARD_KPI_KEYS,
  RELIABILITY_RAM_EVIDENCE_HASH,
} from "@shared/kpi-definitions";
import { P } from "@shared/rbac/permissions.generated";

type Horizon = 7 | 30 | 90;

const REFRESH_MS = 5 * 60 * 1000;

function trendDelta(kpi: { value: number; previous_value: number }): number {
  return Math.round(kpi.value - kpi.previous_value);
}

export function AnalyticsDashboardsPage() {
  const { t } = useTranslation("reports");
  const { t: td } = useTranslation("dashboard");
  const { can, canAny } = usePermissions();
  const { formatCurrency } = useFormatters();
  const navigate = useNavigate();
  const sessionAuthenticated = useSessionStore((s) => s.info?.is_authenticated === true);
  const sessionLocked = useSessionStore((s) => s.info?.is_locked === true);
  const isAdmin = useSessionStore((s) => s.info?.is_admin === true);

  const [horizon, setHorizon] = useState<Horizon>(30);
  const [orgNodeId, setOrgNodeId] = useState<number | null>(null);
  const [orgRows, setOrgRows] = useState<OrgTreeRow[]>([]);
  const [data, setData] = useState<AnalyticsExecutiveDashboard | null>(null);
  const [layout, setLayout] = useState<AnalyticsDashboardLayoutV1 | null>(null);
  const [layoutSaved, setLayoutSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [customOpen, setCustomOpen] = useState(false);

  const isPlannerHeavy = canAny(P.PLAN_VIEW, P.PM_VIEW, P.PM_EDIT);
  const isSupervisorHeavy = canAny(P.DI_REVIEW, P.OT_CLOSE, P.DI_APPROVE) && !isPlannerHeavy;

  const roleDefault = useMemo(
    () =>
      buildRoleDefaultAnalyticsLayout({
        canDi: can(P.DI_VIEW),
        canOt: can(P.OT_VIEW),
        canPm: can(P.PM_VIEW),
        canRep: can(P.REP_VIEW),
        isPlannerHeavy,
        isSupervisorHeavy: isSupervisorHeavy || Boolean(isAdmin),
      }),
    [can, isAdmin, isPlannerHeavy, isSupervisorHeavy],
  );

  const effectiveLayout = useMemo(() => {
    if (layoutSaved && layout) {
      return mergeWithDefaultAnalyticsLayout(layout);
    }
    return roleDefault;
  }, [layout, layoutSaved, roleDefault]);

  const loadPayload = useCallback(() => {
    if (!isSessionActiveForBackgroundWork()) {
      return;
    }
    setLoading(true);
    setError(null);
    getAnalyticsExecutiveDashboard({
      horizon_days: horizon,
      org_node_id: orgNodeId,
    })
      .then(setData)
      .catch((e: unknown) => {
        setData(null);
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => setLoading(false));
  }, [horizon, orgNodeId]);

  useEffect(() => {
    if (!sessionAuthenticated || sessionLocked) {
      return;
    }
    void listOrgTree()
      .then(setOrgRows)
      .catch(() => setOrgRows([]));
    void getAnalyticsDashboardLayout()
      .then((p) => {
        const parsed = parseAnalyticsDashboardLayoutJson(p.layout_json);
        // Distinguish default seed vs user-saved: empty table returns default JSON from server.
        // Treat any successful load as saved if it differs from role default after merge — simpler:
        // always start from server layout (defaults until user customizes).
        setLayout(parsed);
        setLayoutSaved(true);
      })
      .catch(() => {
        setLayout(null);
        setLayoutSaved(false);
      });
  }, [sessionAuthenticated, sessionLocked]);

  useEffect(() => {
    if (!sessionAuthenticated || sessionLocked) {
      return;
    }
    loadPayload();
    const id = setInterval(loadPayload, REFRESH_MS);
    return () => clearInterval(id);
  }, [loadPayload, sessionAuthenticated, sessionLocked]);

  const canShowWidget = useCallback(
    (id: string) => {
      if (id === ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS) {
        return canAny(P.DI_VIEW, P.OT_VIEW, P.EQ_VIEW, P.PM_VIEW);
      }
      if (id === ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD) {
        return canAny(P.DI_VIEW, P.OT_VIEW, P.PM_VIEW);
      }
      const p = ANALYTICS_DASHBOARD_WIDGET_PERMISSION[id];
      return !p || can(p);
    },
    [can, canAny],
  );

  const visibleWidgets = useMemo(
    () =>
      effectiveLayout.widgets
        .filter((w) => w.visible && canShowWidget(w.id))
        .sort((a, b) => a.order - b.order),
    [effectiveLayout, canShowWidget],
  );

  const orgOptions = useMemo(() => {
    return orgRows
      .map((r) => ({
        id: r.node.id,
        label: r.node.name || r.node.code,
      }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }, [orgRows]);

  const renderWidget = (id: string) => {
    switch (id) {
      case ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS:
        return (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <KpiCard
              title={td("kpi.openDis")}
              value={Math.round(data?.open_dis.value ?? 0)}
              trend={data ? trendDelta(data.open_dis) : 0}
              icon={ClipboardList}
              color={CHART_COLORS.primary}
              trendDirection="up-bad"
              available={data?.open_dis.available ?? false}
              qualityBadge={data?.open_dis.quality_badge}
              onOpenEvidence={() => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OPEN_DIS])}
            />
            <KpiCard
              title={td("kpi.openWos")}
              value={Math.round(data?.open_wos.value ?? 0)}
              trend={data ? trendDelta(data.open_wos) : 0}
              icon={Wrench}
              color={CHART_COLORS.accent}
              trendDirection="up-bad"
              available={data?.open_wos.available ?? false}
              qualityBadge={data?.open_wos.quality_badge}
              onOpenEvidence={() => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OPEN_WOS])}
            />
            <KpiCard
              title={td("kpi.totalAssets")}
              value={Math.round(data?.total_assets.value ?? 0)}
              trend={data ? trendDelta(data.total_assets) : 0}
              icon={Package}
              color={CHART_COLORS.success}
              trendDirection="up-good"
              available={data?.total_assets.available ?? false}
              qualityBadge={data?.total_assets.quality_badge}
              onOpenEvidence={() =>
                navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.TOTAL_ASSETS])
              }
            />
            <KpiCard
              title={td("kpi.overdueItems")}
              value={Math.round(data?.overdue_items.value ?? 0)}
              trend={data ? trendDelta(data.overdue_items) : 0}
              icon={AlertTriangle}
              color={CHART_COLORS.danger}
              trendDirection="up-bad"
              available={data?.overdue_items.available ?? false}
              qualityBadge={data?.overdue_items.quality_badge}
              onOpenEvidence={() =>
                navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OVERDUE_ITEMS])
              }
            />
          </div>
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE:
        return (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
            <KpiCard
              title={t("hub.dashboards.kpi.pmOverdue")}
              value={Math.round(data?.pm_overdue.value ?? 0)}
              trend={0}
              icon={CalendarClock}
              color={CHART_COLORS.danger}
              trendDirection="up-bad"
              available={data?.pm_overdue.available ?? false}
              qualityBadge={data?.pm_overdue.quality_badge}
              onOpenEvidence={() => navigate("/pm")}
            />
            <KpiCard
              title={t("hub.dashboards.kpi.pmDueToday")}
              value={Math.round(data?.pm_due_today.value ?? 0)}
              trend={0}
              icon={CalendarClock}
              color={CHART_COLORS.warning}
              trendDirection="up-bad"
              available={data?.pm_due_today.available ?? false}
              onOpenEvidence={() => navigate("/pm")}
            />
            <KpiCard
              title={t("hub.dashboards.kpi.pmDueNext7d")}
              value={Math.round(data?.pm_due_next_7d.value ?? 0)}
              trend={0}
              icon={CalendarClock}
              color={CHART_COLORS.accent}
              trendDirection="up-bad"
              available={data?.pm_due_next_7d.available ?? false}
              onOpenEvidence={() => navigate("/pm")}
            />
          </div>
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS:
        return (
          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-base font-medium">
                {t("hub.dashboards.widgets.reliability")}
              </CardTitle>
              <button
                type="button"
                className="text-xs font-medium text-primary underline-offset-2 hover:underline"
                onClick={() =>
                  navigate(`${ANALYTICS_RAMS_BASE_PATH}/foundation${RELIABILITY_RAM_EVIDENCE_HASH}`)
                }
              >
                {td("widgets.reliability.openEvidence")}
              </button>
            </CardHeader>
            <CardContent>
              <p className="mb-3 text-xs text-text-muted">
                {t("hub.dashboards.reliabilityHint", { days: horizon })}
              </p>
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
                <KpiCard
                  title={t("hub.dashboards.kpi.mtbf")}
                  value={data?.avg_mtbf_hours.value ?? 0}
                  trend={
                    data
                      ? Math.round(data.avg_mtbf_hours.value - data.avg_mtbf_hours.previous_value)
                      : 0
                  }
                  icon={Activity}
                  color={CHART_COLORS.primary}
                  trendDirection="up-good"
                  available={data?.avg_mtbf_hours.available ?? false}
                  qualityBadge={data?.avg_mtbf_hours.quality_badge}
                  formatValue={(v) => (Number.isFinite(v) ? v.toFixed(1) : "—")}
                  onOpenEvidence={() => navigate(`${ANALYTICS_RAMS_BASE_PATH}/dashboard`)}
                />
                <KpiCard
                  title={t("hub.dashboards.kpi.mttr")}
                  value={data?.avg_mttr_hours.value ?? 0}
                  trend={
                    data
                      ? Math.round(data.avg_mttr_hours.value - data.avg_mttr_hours.previous_value)
                      : 0
                  }
                  icon={Activity}
                  color={CHART_COLORS.warning}
                  trendDirection="up-bad"
                  available={data?.avg_mttr_hours.available ?? false}
                  formatValue={(v) => (Number.isFinite(v) ? v.toFixed(1) : "—")}
                  onOpenEvidence={() => navigate(`${ANALYTICS_RAMS_BASE_PATH}/dashboard`)}
                />
                <KpiCard
                  title={t("hub.dashboards.kpi.availability")}
                  value={(data?.avg_availability.value ?? 0) * 100}
                  trend={0}
                  icon={Activity}
                  color={CHART_COLORS.success}
                  trendDirection="up-good"
                  available={data?.avg_availability.available ?? false}
                  formatValue={(v) => `${v.toFixed(1)}%`}
                  onOpenEvidence={() => navigate(`${ANALYTICS_RAMS_BASE_PATH}/dashboard`)}
                />
                <KpiCard
                  title={t("hub.dashboards.kpi.dataQuality")}
                  value={data?.avg_data_quality.value ?? 0}
                  trend={0}
                  icon={Activity}
                  color={CHART_COLORS.accent}
                  trendDirection="up-good"
                  available={data?.avg_data_quality.available ?? false}
                  formatValue={(v) => v.toFixed(2)}
                  onOpenEvidence={() =>
                    navigate(
                      `${ANALYTICS_RAMS_BASE_PATH}/foundation${RELIABILITY_RAM_EVIDENCE_HASH}`,
                    )
                  }
                />
                <KpiCard
                  title={t("hub.dashboards.kpi.snapshots")}
                  value={data?.reliability_snapshot_count ?? 0}
                  trend={0}
                  icon={Activity}
                  color={CHART_COLORS.neutral}
                  trendDirection="up-good"
                  available={Boolean(data)}
                />
                <KpiCard
                  title={t("hub.dashboards.kpi.failureEvents")}
                  value={data?.reliability_total_events ?? 0}
                  trend={0}
                  icon={AlertTriangle}
                  color={CHART_COLORS.danger}
                  trendDirection="up-bad"
                  available={Boolean(data)}
                  onOpenEvidence={() => navigate(`${ANALYTICS_RAMS_BASE_PATH}/foundation`)}
                />
              </div>
            </CardContent>
          </Card>
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST:
        return (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <KpiCard
              title={t("hub.dashboards.kpi.maintenanceCost")}
              value={data?.maintenance_cost.value ?? 0}
              trend={
                data
                  ? Math.round(data.maintenance_cost.value - data.maintenance_cost.previous_value)
                  : 0
              }
              icon={Wrench}
              color={CHART_COLORS.accent}
              trendDirection="up-bad"
              available={data?.maintenance_cost.available ?? false}
              qualityBadge={data?.maintenance_cost.quality_badge}
              formatValue={(v) => formatCurrency(v)}
              onOpenEvidence={() => navigate("/work-orders")}
            />
          </div>
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.WORKLOAD:
        return (
          <AnalyticsWorkloadChart
            days={data?.workload_days ?? []}
            horizonDays={horizon}
            qualityBadge={data?.workload_quality_badge ?? null}
          />
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.DI_STATUS:
        return <AnalyticsDiStatusChart segments={data?.di_status_segments ?? []} />;
      default:
        return null;
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 className="text-base font-medium text-text-primary">{t("hub.dashboards.title")}</h2>
          <p className="text-xs text-text-muted">{t("hub.dashboards.subtitle")}</p>
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <label className="flex flex-col gap-1 text-xs">
            <span className="text-text-muted">{t("hub.dashboards.filters.horizon")}</span>
            <select
              className="rounded border border-surface-border bg-surface-0 px-2 py-1.5 text-sm"
              value={horizon}
              onChange={(e) => setHorizon(Number(e.target.value) as Horizon)}
            >
              <option value={7}>{t("hub.dashboards.filters.days7")}</option>
              <option value={30}>{t("hub.dashboards.filters.days30")}</option>
              <option value={90}>{t("hub.dashboards.filters.days90")}</option>
            </select>
          </label>
          <label className="flex min-w-[12rem] flex-col gap-1 text-xs">
            <span className="text-text-muted">{t("hub.dashboards.filters.orgScope")}</span>
            <select
              className="rounded border border-surface-border bg-surface-0 px-2 py-1.5 text-sm"
              value={orgNodeId ?? ""}
              onChange={(e) => {
                const v = e.target.value;
                setOrgNodeId(v === "" ? null : Number(v));
              }}
            >
              <option value="">{t("hub.dashboards.filters.allOrgs")}</option>
              {orgOptions.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
          <Button type="button" variant="outline" size="sm" onClick={() => setCustomOpen(true)}>
            <Settings2 className="mr-1.5 h-3.5 w-3.5" aria-hidden />
            {t("hub.dashboards.customize")}
          </Button>
        </div>
      </div>

      {error ? <p className="text-sm text-text-danger">{error}</p> : null}
      {loading && !data ? <p className="text-sm text-text-muted">{td("chart.loading")}</p> : null}

      <div className="space-y-4">
        {visibleWidgets.map((w) => (
          <Fragment key={w.id}>{renderWidget(w.id)}</Fragment>
        ))}
      </div>

      <AnalyticsCustomizeDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        layout={effectiveLayout}
        onSaved={(next) => {
          setLayout(next);
          setLayoutSaved(true);
        }}
      />
    </div>
  );
}
