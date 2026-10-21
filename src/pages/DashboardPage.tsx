/**
 * Home page (start centre) — each person's starting page, by role (Maximo Start Center / SAP
 * Fiori launchpad spirit):
 * - My work (anyone with personal work: OT, DI, inspections, PM, notifications);
 * - Supervision (supervising or managing permissions);
 * - Planning (planning module);
 * - shared widgets: indicators, activity chart, open requests by status.
 * Which panels show follows the user's permissions (`DASHBOARD_WIDGET_PERMISSIONS`; the server
 * checks every section again); order and visibility are the user's layout. Each panel loads
 * with one backend call and has its own loading, error-with-retry and empty states.
 */
import { Gauge, LayoutDashboard, Settings2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { DashboardCustomizeDialog } from "@/components/dashboard/DashboardCustomizeDialog";
import { DashboardDiStatusChart } from "@/components/dashboard/DashboardDiStatusChart";
import { DashboardWorkloadChart } from "@/components/dashboard/DashboardWorkloadChart";
import { OverviewMetricCards } from "@/components/dashboard/OverviewMetricCards";
import { HomeMyWorkPanel } from "@/components/dashboard/home/HomeMyWorkPanel";
import { HomePanel, usePanelLoad } from "@/components/dashboard/home/HomeParts";
import { HomePlanningPanel } from "@/components/dashboard/home/HomePlanningPanel";
import { HomeSupervisionPanel } from "@/components/dashboard/home/HomeSupervisionPanel";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { usePermissions } from "@/hooks/use-permissions";
import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import {
  getDashboardLayout,
  getOverviewKpiLayout,
  getOverviewKpis,
} from "@/services/dashboard-service";
import { useAppStore } from "@/store/app-store";
import { useAssetStore } from "@/stores/asset-store";
import { useDiStore } from "@/stores/di-store";
import { useWoStore } from "@/stores/wo-store";
import {
  buildRoleDefaultOverviewKpis,
  parseAnalyticsDashboardLayoutJson,
  resolveOverviewKpiLayout,
} from "@shared/analytics-dashboard-layout";
import {
  DASHBOARD_WIDGET_IDS,
  DASHBOARD_WIDGET_PERMISSIONS,
  DEFAULT_DASHBOARD_LAYOUT,
  type DashboardLayoutV1,
  isDashboardWidgetId,
  mergeWithDefaultLayout,
  parseDashboardLayoutJson,
} from "@shared/dashboard-layout";
import type { OverviewKpisPayload } from "@shared/ipc-types";
import type { OverviewKpiLayoutEntry } from "@shared/overview-kpi-registry";
import { P } from "@shared/rbac/permissions.generated";

/** Figures are refreshed every 5 minutes while the session is active. */
const REFRESH_MS = 5 * 60 * 1000;

interface SavedLayouts {
  layout: DashboardLayoutV1;
  overviewKpis: OverviewKpiLayoutEntry[];
}

async function loadLayouts(): Promise<SavedLayouts> {
  const [page, overview] = await Promise.all([getDashboardLayout(), getOverviewKpiLayout()]);
  return {
    layout: parseDashboardLayoutJson(page.layout_json),
    overviewKpis: parseAnalyticsDashboardLayoutJson(overview.layout_json).overview_kpis ?? [],
  };
}

function IndicatorsPanel(props: { layout: OverviewKpiLayoutEntry[]; refreshSignal: number }) {
  const { t } = useTranslation("dashboard");
  const load = useCallback(() => getOverviewKpis(), []);
  const state = usePanelLoad<OverviewKpisPayload>(load, props.refreshSignal);
  return (
    <HomePanel
      icon={Gauge}
      title={t("home.indicators.title")}
      subtitle={t("home.indicators.subtitle", { count: state.data?.horizon_days ?? 30 })}
      state={state}
      testId="home-indicators"
    >
      {state.data ? <OverviewMetricCards overviewKpis={state.data} layout={props.layout} /> : null}
    </HomePanel>
  );
}

export function DashboardPage() {
  const { t } = useTranslation("dashboard");
  const { can, canAny, isLoading: permissionsLoading } = usePermissions();
  const displayName = useAppStore((s) => s.currentUserDisplayName);
  const navigate = useNavigate();

  const [refreshSignal, setRefreshSignal] = useState(0);
  const [customOpen, setCustomOpen] = useState(false);
  const layouts = usePanelLoad<SavedLayouts>(loadLayouts);
  const [layout, setLayout] = useState<DashboardLayoutV1>(DEFAULT_DASHBOARD_LAYOUT);
  const [overviewKpiLayout, setOverviewKpiLayout] = useState<OverviewKpiLayoutEntry[]>([]);

  useEffect(() => {
    if (layouts.data) {
      setLayout(layouts.data.layout);
      setOverviewKpiLayout(layouts.data.overviewKpis);
    }
  }, [layouts.data]);

  useEffect(() => {
    const refresh = () => {
      if (isSessionActiveForBackgroundWork()) setRefreshSignal((n) => n + 1);
    };
    const interval = setInterval(refresh, REFRESH_MS);
    window.addEventListener("mf:dashboard-kpis-refresh", refresh);
    return () => {
      clearInterval(interval);
      window.removeEventListener("mf:dashboard-kpis-refresh", refresh);
    };
  }, []);

  const roleDefaultOverviewKpis = useMemo(
    () =>
      buildRoleDefaultOverviewKpis({
        canDi: can(P.DI_VIEW),
        canOt: can(P.OT_VIEW),
        canPm: can(P.PM_VIEW),
        canInv: can(P.INV_VIEW),
        canRam: can(P.RAM_VIEW),
        canRep: can(P.REP_VIEW),
        isSupervisorHeavy: can(P.DI_REVIEW) || can(P.OT_CLOSE),
      }),
    [can],
  );

  const resolvedOverviewLayout = useMemo(
    () =>
      resolveOverviewKpiLayout(
        { version: 1, widgets: [], overview_kpis: overviewKpiLayout },
        roleDefaultOverviewKpis,
      ),
    [overviewKpiLayout, roleDefaultOverviewKpis],
  );

  // Panels wait for the saved layout (or its failure, shown above them with the default layout)
  // so a hidden panel never loads.
  const ready = !permissionsLoading && (layouts.data != null || layouts.error != null);

  const visibleWidgets = useMemo(() => {
    const merged = mergeWithDefaultLayout(layout);
    return merged.widgets
      .filter(
        (w) =>
          w.visible && isDashboardWidgetId(w.id) && canAny(...DASHBOARD_WIDGET_PERMISSIONS[w.id]),
      )
      .sort((a, b) => a.order - b.order);
  }, [layout, canAny]);

  const renderWidget = (id: string) => {
    switch (id) {
      case DASHBOARD_WIDGET_IDS.MY_WORK:
        return <HomeMyWorkPanel refreshSignal={refreshSignal} />;
      case DASHBOARD_WIDGET_IDS.SUPERVISION:
        return <HomeSupervisionPanel refreshSignal={refreshSignal} />;
      case DASHBOARD_WIDGET_IDS.PLANNING:
        return <HomePlanningPanel refreshSignal={refreshSignal} />;
      case DASHBOARD_WIDGET_IDS.KPIS:
        return <IndicatorsPanel layout={resolvedOverviewLayout} refreshSignal={refreshSignal} />;
      case DASHBOARD_WIDGET_IDS.WORKLOAD:
        return <DashboardWorkloadChart refreshSignal={refreshSignal} />;
      case DASHBOARD_WIDGET_IDS.DI_STATUS:
        return <DashboardDiStatusChart refreshSignal={refreshSignal} />;
      default:
        return null;
    }
  };

  return (
    <ModulePageShell
      icon={LayoutDashboard}
      title={t("page.title")}
      actions={
        <>
          {displayName ? (
            <p className="hidden text-sm text-text-muted sm:block">
              {t("page.welcome", { name: displayName })}
            </p>
          ) : null}
          <Button
            type="button"
            variant="outline"
            onClick={() => setCustomOpen(true)}
            className="h-11 gap-1.5"
          >
            <Settings2 className="h-4 w-4" aria-hidden />
            {t("layout.customize")}
          </Button>
        </>
      }
      bodyClassName="space-y-4 p-4"
    >
      <DashboardCustomizeDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        layout={layout}
        overviewKpiLayout={resolvedOverviewLayout}
        onSaved={setLayout}
        onOverviewKpiSaved={setOverviewKpiLayout}
      />

      {layouts.error != null ? (
        <ErrorState
          className="py-4"
          error={layouts.error}
          title={t("home.layoutError")}
          onRetry={layouts.reload}
          retrying={layouts.loading}
        />
      ) : null}

      <div
        className="flex flex-wrap items-center gap-3"
        aria-label={t("quickActions.title")}
        role="group"
      >
        <PermissionGate permission={P.DI_CREATE}>
          <Button
            variant="outline"
            className="h-11"
            onClick={() => {
              useDiStore.getState().openCreateForm();
              void navigate("/requests");
            }}
          >
            {t("quickActions.newDi")}
          </Button>
        </PermissionGate>
        <PermissionGate permission={P.OT_CREATE}>
          <Button
            variant="outline"
            className="h-11"
            onClick={() => {
              useWoStore.getState().openCreateForm();
              void navigate("/work-orders");
            }}
          >
            {t("quickActions.newWo")}
          </Button>
        </PermissionGate>
        <PermissionGate permission={P.EQ_MANAGE}>
          <Button
            variant="outline"
            className="h-11"
            onClick={() => {
              useAssetStore.getState().openCreateForm();
              void navigate("/equipment");
            }}
          >
            {t("quickActions.newAsset")}
          </Button>
        </PermissionGate>
        <PermissionGate permission={P.REP_VIEW}>
          <Button variant="ghost" className="h-11" onClick={() => void navigate("/analytics")}>
            {t("quickActions.openAnalytics")}
          </Button>
        </PermissionGate>
      </div>

      {ready && visibleWidgets.length === 0 ? (
        <p className="rounded-md border border-dashed border-surface-border px-4 py-8 text-center text-sm text-text-muted">
          {t("home.nothingToShow")}
        </p>
      ) : null}

      {ready
        ? visibleWidgets.map((w) => <Fragment key={w.id}>{renderWidget(w.id)}</Fragment>)
        : null}
    </ModulePageShell>
  );
}
