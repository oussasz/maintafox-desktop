import { LayoutDashboard, Settings2 } from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { DashboardCustomizeDialog } from "@/components/dashboard/DashboardCustomizeDialog";
import { DashboardDiStatusChart } from "@/components/dashboard/DashboardDiStatusChart";
import { DashboardDisTriageInboxPanel } from "@/components/dashboard/DashboardDisTriageInboxPanel";
import { DashboardWorkloadChart } from "@/components/dashboard/DashboardWorkloadChart";
import { OverviewMetricCards } from "@/components/dashboard/OverviewMetricCards";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { usePermissions } from "@/hooks/use-permissions";
import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import {
  getDashboardKpiValidation,
  getDashboardLayout,
  getOverviewKpiLayout,
  getOverviewKpis,
} from "@/services/dashboard-service";
import { useAppStore } from "@/store/app-store";
import { useSessionStore } from "@/store/session-store";
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
  DASHBOARD_WIDGET_PERMISSION,
  DEFAULT_DASHBOARD_LAYOUT,
  mergeWithDefaultLayout,
  parseDashboardLayoutJson,
  type DashboardLayoutV1,
} from "@shared/dashboard-layout";
import type { DashboardKpiValidation, OverviewKpisPayload } from "@shared/ipc-types";
import type { OverviewKpiLayoutEntry } from "@shared/overview-kpi-registry";
import { P } from "@shared/rbac/permissions.generated";

const KPI_REFRESH_MS = 5 * 60 * 1000;

export function DashboardPage() {
  const { t } = useTranslation("dashboard");
  const { can, canAny } = usePermissions();
  const displayName = useAppStore((s) => s.currentUserDisplayName);
  const navigate = useNavigate();
  const sessionAuthenticated = useSessionStore((s) => s.info?.is_authenticated === true);
  const sessionLocked = useSessionStore((s) => s.info?.is_locked === true);

  const [overviewKpis, setOverviewKpis] = useState<OverviewKpisPayload | null>(null);
  const [overviewKpiLayout, setOverviewKpiLayout] = useState<OverviewKpiLayoutEntry[]>([]);
  const [layout, setLayout] = useState<DashboardLayoutV1>(DEFAULT_DASHBOARD_LAYOUT);
  const [customOpen, setCustomOpen] = useState(false);
  const [kpiValOpen, setKpiValOpen] = useState(false);
  const [kpiValidation, setKpiValidation] = useState<DashboardKpiValidation | null>(null);
  const [kpiValErr, setKpiValErr] = useState<string | null>(null);
  const kpiValidationFetchStarted = useRef(false);

  const loadKpis = useCallback(() => {
    if (!isSessionActiveForBackgroundWork()) {
      return;
    }
    getOverviewKpis()
      .then(setOverviewKpis)
      .catch(() => setOverviewKpis(null));
  }, []);

  const roleDefaultOverviewKpis = useMemo(
    () =>
      buildRoleDefaultOverviewKpis({
        canDi: can(P.DI_VIEW),
        canOt: can(P.OT_VIEW),
        canPm: can(P.PM_VIEW),
        canInv: can(P.INV_VIEW),
        canFin: can(P.FIN_VIEW),
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

  useEffect(() => {
    if (!sessionAuthenticated || sessionLocked) {
      return;
    }

    const perf = typeof performance !== "undefined" ? performance : undefined;
    perf?.mark?.("dashboard-load-start");
    void Promise.all([getOverviewKpis(), getDashboardLayout(), getOverviewKpiLayout()])
      .then(([overview, layoutPayload, overviewLayoutPayload]) => {
        setOverviewKpis(overview);
        setLayout(parseDashboardLayoutJson(layoutPayload.layout_json));
        const analyticsLayout = parseAnalyticsDashboardLayoutJson(
          overviewLayoutPayload.layout_json,
        );
        setOverviewKpiLayout(analyticsLayout.overview_kpis ?? []);
      })
      .catch(() => {
        setOverviewKpis(null);
        setLayout(DEFAULT_DASHBOARD_LAYOUT);
        setOverviewKpiLayout([]);
      })
      .finally(() => {
        perf?.mark?.("dashboard-load-end");
        try {
          perf?.measure?.("dashboard-initial-load", "dashboard-load-start", "dashboard-load-end");
        } catch {
          /* duplicate measure */
        }
        if (import.meta.env.DEV && perf?.getEntriesByName) {
          perf.getEntriesByName("dashboard-initial-load").pop();
        }
      });

    const interval = setInterval(loadKpis, KPI_REFRESH_MS);
    return () => clearInterval(interval);
  }, [loadKpis, sessionAuthenticated, sessionLocked]);

  useEffect(() => {
    const onRefresh = () => {
      loadKpis();
    };
    window.addEventListener("mf:dashboard-kpis-refresh", onRefresh);
    return () => window.removeEventListener("mf:dashboard-kpis-refresh", onRefresh);
  }, [loadKpis]);

  useEffect(() => {
    if (!import.meta.env.DEV || !kpiValOpen || kpiValidationFetchStarted.current) {
      return;
    }
    kpiValidationFetchStarted.current = true;
    void getDashboardKpiValidation()
      .then((v) => {
        setKpiValidation(v);
        setKpiValErr(null);
      })
      .catch((e: unknown) => {
        setKpiValErr(e instanceof Error ? e.message : String(e));
      });
  }, [kpiValOpen]);

  const canShowWidget = useCallback(
    (id: string) => {
      if (id === DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT) {
        return false;
      }
      if (id === DASHBOARD_WIDGET_IDS.KPIS || id === DASHBOARD_WIDGET_IDS.WORKLOAD) {
        return canAny(P.DI_VIEW, P.OT_VIEW, P.EQ_VIEW, P.PM_VIEW);
      }
      const p = DASHBOARD_WIDGET_PERMISSION[id];
      return !p || can(p);
    },
    [can, canAny],
  );

  const visibleWidgets = useMemo(() => {
    const merged = mergeWithDefaultLayout(layout);
    return merged.widgets
      .filter((w) => w.visible && canShowWidget(w.id))
      .sort((a, b) => a.order - b.order);
  }, [layout, canShowWidget]);

  const renderWidget = (id: string) => {
    switch (id) {
      case DASHBOARD_WIDGET_IDS.KPIS:
        return <OverviewMetricCards overviewKpis={overviewKpis} layout={resolvedOverviewLayout} />;
      case DASHBOARD_WIDGET_IDS.WORKLOAD:
        return <DashboardWorkloadChart />;
      case DASHBOARD_WIDGET_IDS.DI_STATUS:
        return <DashboardDiStatusChart />;
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
            size="sm"
            onClick={() => setCustomOpen(true)}
            className="gap-1.5"
          >
            <Settings2 className="h-3.5 w-3.5" />
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

      <div className="space-y-2">
        <h2 className="text-sm font-medium text-text-muted">{t("quickActions.title")}</h2>
        <div className="flex flex-wrap gap-3">
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
            <Button variant="ghost" size="sm" onClick={() => navigate("/analytics")}>
              {t("quickActions.openAnalytics")}
            </Button>
          </PermissionGate>
        </div>
      </div>

      {visibleWidgets.map((w) => (
        <Fragment key={w.id}>{renderWidget(w.id)}</Fragment>
      ))}

      <PermissionGate anyOf={[P.DI_SCREEN, P.DI_REVIEW]}>
        <DashboardDisTriageInboxPanel />
      </PermissionGate>

      {import.meta.env.DEV ? (
        <details
          className="rounded border border-surface-border bg-surface-0/50 p-3 text-xs text-text-muted"
          onToggle={(e) => {
            const open = (e.target as HTMLDetailsElement).open;
            setKpiValOpen(open);
            if (!open) {
              kpiValidationFetchStarted.current = false;
              setKpiValidation(null);
              setKpiValErr(null);
            }
          }}
        >
          <summary className="cursor-pointer font-medium text-text-primary">
            {t("dev.kpiValidation")}
          </summary>
          <p className="mt-2 text-text-muted">{t("dev.kpiValidationHint")}</p>
          {kpiValErr ? <p className="mt-2 text-red-600">{kpiValErr}</p> : null}
          {kpiValidation ? (
            <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap rounded bg-surface-1 p-2 font-mono text-[10px] text-text-primary">
              {JSON.stringify(kpiValidation, null, 2)}
            </pre>
          ) : kpiValOpen && !kpiValErr ? (
            <p className="mt-2 text-text-muted">…</p>
          ) : null}
        </details>
      ) : null}
    </ModulePageShell>
  );
}
