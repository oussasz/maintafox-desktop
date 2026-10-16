import { Settings2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { AnalyticsCustomizeDialog } from "@/components/analytics/AnalyticsCustomizeDialog";
import { AnalyticsDiStatusChart } from "@/components/analytics/AnalyticsDiStatusChart";
import { AnalyticsWorkloadChart } from "@/components/analytics/AnalyticsWorkloadChart";
import { SmrpIndicatorsSection } from "@/components/analytics/SmrpIndicatorsSection";
import { type KpiTrend, periodTrend } from "@/components/analytics/analytics-kpi-trend";
import { VisualMetricCard } from "@/components/metrics/VisualMetricCard";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import {
  getAnalyticsDashboardLayout,
  getAnalyticsExecutiveDashboard,
  type AnalyticsExecutiveDashboardWithSmrp,
} from "@/services/analytics-dashboard-service";
import { listOrgTree } from "@/services/org-node-service";
import { useSessionStore } from "@/store/session-store";
import {
  ANALYTICS_DASHBOARD_WIDGET_IDS,
  ANALYTICS_DASHBOARD_WIDGET_PERMISSION,
  type AnalyticsDashboardLayoutV1,
  buildRoleDefaultAnalyticsLayout,
  mergeWithDefaultAnalyticsLayout,
  parseAnalyticsDashboardLayoutJson,
} from "@shared/analytics-dashboard-layout";
import type { OrgTreeRow } from "@shared/ipc-types";
import {
  ANALYTICS_RAMS_BASE_PATH,
  DASHBOARD_KPI_DRILL_PATH,
  DASHBOARD_KPI_KEYS,
  RELIABILITY_RAM_EVIDENCE_HASH,
} from "@shared/kpi-definitions";
import { P } from "@shared/rbac/permissions.generated";

type Horizon = 7 | 30 | 90;

const REFRESH_MS = 5 * 60 * 1000;
/** Select value for "no organization filter" (Radix Select does not accept ""). */
const ALL_ORGS = "all";

interface KpiProps {
  title: string;
  /** Period or scope of the figure, e.g. "Now" or "Last 30 days". */
  context: string;
  value: string;
  available: boolean;
  unit?: string;
  /**
   * Change vs the same measure at the start of the horizon (counts are rebuilt at that instant
   * from lifecycle dates by the backend, `analytics::as_of`).
   */
  trend?: KpiTrend | null;
  onOpen?: () => void;
}

export function AnalyticsDashboardsPage() {
  const { t } = useTranslation("reports");
  const { t: td } = useTranslation("dashboard");
  const { can, canAny } = usePermissions();
  const { formatCurrency, formatNumber, formatDecimal, formatPercent } = useFormatters();
  const navigate = useNavigate();
  const sessionAuthenticated = useSessionStore((s) => s.info?.is_authenticated === true);
  const sessionLocked = useSessionStore((s) => s.info?.is_locked === true);
  const isAdmin = useSessionStore((s) => s.info?.is_admin === true);

  const [horizon, setHorizon] = useState<Horizon>(30);
  const [orgNodeId, setOrgNodeId] = useState<number | null>(null);
  const [orgRows, setOrgRows] = useState<OrgTreeRow[]>([]);
  const [data, setData] = useState<AnalyticsExecutiveDashboardWithSmrp | null>(null);
  const [layout, setLayout] = useState<AnalyticsDashboardLayoutV1 | null>(null);
  const [layoutSaved, setLayoutSaved] = useState(false);
  const [error, setError] = useState<unknown>(null);
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
        setError(e);
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
        // The server returns the default layout JSON until the user saves one; both are merged
        // with the role default so new widgets appear.
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

  const trendLabel = (trend: KpiTrend) => {
    if (trend.percent === 0) return t("hub.dashboards.trend.stable");
    const rounded =
      Math.abs(trend.percent) >= 10
        ? Math.round(trend.percent)
        : Math.round(trend.percent * 10) / 10;
    return t("hub.dashboards.trend.vsPrevious", {
      change: `${rounded > 0 ? "+" : ""}${formatPercent(rounded)}`,
    });
  };

  const renderKpi = ({ title, context, value, available, unit, trend, onOpen }: KpiProps) => (
    <VisualMetricCard
      key={title}
      title={title}
      context={context}
      value={value}
      available={available}
      unavailableLabel="—"
      {...(unit ? { unit } : {})}
      {...(trend ? { trendLabel: trendLabel(trend), trendSense: trend.sense } : {})}
      {...(onOpen && available ? { onClick: onOpen } : {})}
    />
  );

  const count = (v: number | undefined) => formatNumber(Math.round(v ?? 0));
  const hours = (v: number | undefined) => (v != null ? formatDecimal(v, 1) : "—");
  /** Projection ratios are 0..1; shown as a percentage like the Overview. */
  const ratio = (v: number | undefined) => (v != null ? formatPercent(v * 100) : "—");
  const now = t("hub.dashboards.context.now");
  const horizonText = t("hub.dashboards.context.horizon", { days: horizon });
  const openRamsDashboard = () => navigate(`${ANALYTICS_RAMS_BASE_PATH}/dashboard`);
  const openRamsEvidence = () =>
    navigate(`${ANALYTICS_RAMS_BASE_PATH}/foundation${RELIABILITY_RAM_EVIDENCE_HASH}`);
  const reliabilityAvailable = data?.avg_mtbf_hours.available === true;

  const renderWidget = (id: string) => {
    switch (id) {
      case ANALYTICS_DASHBOARD_WIDGET_IDS.OPS_KPIS:
        return (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {renderKpi({
              title: td("kpi.openDis"),
              context: now,
              value: count(data?.open_dis.value),
              available: data?.open_dis.available ?? false,
              trend: periodTrend(data?.open_dis, "up-bad"),
              onOpen: () => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OPEN_DIS]),
            })}
            {renderKpi({
              title: td("kpi.openWos"),
              context: now,
              value: count(data?.open_wos.value),
              available: data?.open_wos.available ?? false,
              trend: periodTrend(data?.open_wos, "up-bad"),
              onOpen: () => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OPEN_WOS]),
            })}
            {renderKpi({
              title: td("kpi.totalAssets"),
              context: t("hub.dashboards.context.inService"),
              value: count(data?.total_assets.value),
              available: data?.total_assets.available ?? false,
              trend: periodTrend(data?.total_assets, "up-good"),
              onOpen: () => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.TOTAL_ASSETS]),
            })}
            {renderKpi({
              title: td("kpi.overdueItems"),
              context: t("hub.dashboards.context.overdue"),
              value: count(data?.overdue_items.value),
              available: data?.overdue_items.available ?? false,
              trend: periodTrend(data?.overdue_items, "up-bad"),
              onOpen: () => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OVERDUE_ITEMS]),
            })}
          </div>
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.PM_DUE:
        return (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            {renderKpi({
              title: t("hub.dashboards.kpi.pmOverdue"),
              context: now,
              value: count(data?.pm_overdue.value),
              available: data?.pm_overdue.available ?? false,
              trend: periodTrend(data?.pm_overdue, "up-bad"),
              onOpen: () => navigate("/pm"),
            })}
            {renderKpi({
              title: t("hub.dashboards.kpi.pmDueToday"),
              context: t("hub.dashboards.context.today"),
              value: count(data?.pm_due_today.value),
              available: data?.pm_due_today.available ?? false,
              trend: periodTrend(data?.pm_due_today, "up-bad"),
              onOpen: () => navigate("/pm"),
            })}
            {renderKpi({
              title: t("hub.dashboards.kpi.pmDueNext7d"),
              context: t("hub.dashboards.context.next7"),
              value: count(data?.pm_due_next_7d.value),
              available: data?.pm_due_next_7d.available ?? false,
              trend: periodTrend(data?.pm_due_next_7d, "up-bad"),
              onOpen: () => navigate("/pm"),
            })}
          </div>
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.RELIABILITY_RAMS:
        return (
          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-base font-medium">
                {t("hub.dashboards.widgets.reliability")}
              </CardTitle>
              <Button
                type="button"
                variant="link"
                className="h-10 px-2 text-xs"
                onClick={openRamsEvidence}
              >
                {td("widgets.reliability.openEvidence")}
              </Button>
            </CardHeader>
            <CardContent>
              <p className="mb-3 text-xs text-text-muted">
                {t("hub.dashboards.reliabilityHint", { days: horizon })}
              </p>
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                {renderKpi({
                  title: t("hub.dashboards.kpi.mtbf"),
                  context: horizonText,
                  value: hours(data?.avg_mtbf_hours.value),
                  available: data?.avg_mtbf_hours.available ?? false,
                  trend: periodTrend(data?.avg_mtbf_hours, "up-good"),
                  onOpen: openRamsDashboard,
                })}
                {renderKpi({
                  title: t("hub.dashboards.kpi.mttr"),
                  context: horizonText,
                  value: hours(data?.avg_mttr_hours.value),
                  available: data?.avg_mttr_hours.available ?? false,
                  trend: periodTrend(data?.avg_mttr_hours, "up-bad"),
                  onOpen: openRamsDashboard,
                })}
                {renderKpi({
                  title: t("hub.dashboards.kpi.availability"),
                  context: horizonText,
                  value: ratio(data?.avg_availability.value),
                  available: data?.avg_availability.available ?? false,
                  trend: periodTrend(data?.avg_availability, "up-good"),
                  onOpen: openRamsDashboard,
                })}
                {renderKpi({
                  title: t("hub.dashboards.kpi.dataQuality"),
                  context: horizonText,
                  value: ratio(data?.avg_data_quality.value),
                  available: data?.avg_data_quality.available ?? false,
                  trend: periodTrend(data?.avg_data_quality, "up-good"),
                  onOpen: openRamsEvidence,
                })}
                {renderKpi({
                  title: t("hub.dashboards.kpi.snapshots"),
                  context: horizonText,
                  value: count(data?.reliability_snapshot_count),
                  available: reliabilityAvailable || Boolean(data?.reliability_snapshot_count),
                })}
                {renderKpi({
                  title: t("hub.dashboards.kpi.failureEvents"),
                  context: horizonText,
                  value: count(data?.reliability_total_events),
                  available: reliabilityAvailable || Boolean(data?.reliability_total_events),
                  onOpen: () => navigate(`${ANALYTICS_RAMS_BASE_PATH}/foundation`),
                })}
              </div>
            </CardContent>
          </Card>
        );
      case ANALYTICS_DASHBOARD_WIDGET_IDS.MAINTENANCE_COST:
        return (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {renderKpi({
              title: t("hub.dashboards.kpi.maintenanceCost"),
              context:
                (data?.maintenance_cost_pending ?? 0) > 0
                  ? t("hub.dashboards.context.costPending", {
                      days: horizon,
                      count: data?.maintenance_cost_pending ?? 0,
                    })
                  : t("hub.dashboards.context.costSettled", { days: horizon }),
              value: data?.maintenance_cost.available
                ? formatCurrency(data.maintenance_cost.value)
                : "—",
              available: data?.maintenance_cost.available ?? false,
              trend: periodTrend(data?.maintenance_cost, "up-bad"),
              ...(can(P.FIN_VIEW) ? { onOpen: () => navigate("/budget") } : {}),
            })}
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
          <div className="flex flex-col gap-1 text-xs">
            <label htmlFor="analytics-horizon" className="text-text-muted">
              {t("hub.dashboards.filters.horizon")}
            </label>
            <Select value={String(horizon)} onValueChange={(v) => setHorizon(Number(v) as Horizon)}>
              <SelectTrigger id="analytics-horizon" className="h-10 w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="7">{t("hub.dashboards.filters.days7")}</SelectItem>
                <SelectItem value="30">{t("hub.dashboards.filters.days30")}</SelectItem>
                <SelectItem value="90">{t("hub.dashboards.filters.days90")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="flex min-w-[12rem] flex-col gap-1 text-xs">
            <label htmlFor="analytics-org" className="text-text-muted">
              {t("hub.dashboards.filters.orgScope")}
            </label>
            <Select
              value={orgNodeId === null ? ALL_ORGS : String(orgNodeId)}
              onValueChange={(v) => setOrgNodeId(v === ALL_ORGS ? null : Number(v))}
            >
              <SelectTrigger id="analytics-org" className="h-10 w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL_ORGS}>{t("hub.dashboards.filters.allOrgs")}</SelectItem>
                {orgOptions.map((o) => (
                  <SelectItem key={o.id} value={String(o.id)}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <Button
            type="button"
            variant="outline"
            className="h-10"
            onClick={() => setCustomOpen(true)}
          >
            <Settings2 className="mr-1.5 h-4 w-4" aria-hidden />
            {t("hub.dashboards.customize")}
          </Button>
        </div>
      </div>

      {error ? (
        <ErrorState
          error={error}
          title={t("hub.dashboards.loadError")}
          onRetry={loadPayload}
          retrying={loading}
        />
      ) : loading && !data ? (
        <LoadingState label={t("hub.dashboards.loading")} />
      ) : (
        <div className="space-y-4">
          <SmrpIndicatorsSection smrp={data?.smrp ?? null} />
          {visibleWidgets.map((w) => (
            <Fragment key={w.id}>{renderWidget(w.id)}</Fragment>
          ))}
        </div>
      )}

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
