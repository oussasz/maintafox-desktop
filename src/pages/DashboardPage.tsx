import {
  AlertTriangle,
  ClipboardList,
  LayoutDashboard,
  Package,
  Settings2,
  Wrench,
} from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { CHART_COLORS } from "@/components/charts/chart-utils";
import { DashboardCustomizeDialog } from "@/components/dashboard/DashboardCustomizeDialog";
import { DashboardDiStatusChart } from "@/components/dashboard/DashboardDiStatusChart";
import { DashboardDisTriageInboxPanel } from "@/components/dashboard/DashboardDisTriageInboxPanel";
import { DashboardReliabilitySnapshotCard } from "@/components/dashboard/DashboardReliabilitySnapshotCard";
import { DashboardWorkloadChart } from "@/components/dashboard/DashboardWorkloadChart";
import { KpiCard } from "@/components/dashboard/KpiCard";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Button } from "@/components/ui/button";
import { usePermissions } from "@/hooks/use-permissions";
import {
  getDashboardKpiValidation,
  getDashboardKpis,
  getDashboardLayout,
} from "@/services/dashboard-service";
import { useAppStore } from "@/store/app-store";
import {
  DASHBOARD_WIDGET_IDS,
  DASHBOARD_WIDGET_PERMISSION,
  DEFAULT_DASHBOARD_LAYOUT,
  mergeWithDefaultLayout,
  parseDashboardLayoutJson,
  type DashboardLayoutV1,
} from "@shared/dashboard-layout";
import type { DashboardKpiValidation, DashboardKpis } from "@shared/ipc-types";
import { DASHBOARD_KPI_DRILL_PATH, DASHBOARD_KPI_KEYS } from "@shared/kpi-definitions";

const KPI_REFRESH_MS = 5 * 60 * 1000;

export function DashboardPage() {
  const { t } = useTranslation("dashboard");
  const { can, canAny } = usePermissions();
  const displayName = useAppStore((s) => s.currentUserDisplayName);
  const navigate = useNavigate();

  const [kpis, setKpis] = useState<DashboardKpis | null>(null);
  const [layout, setLayout] = useState<DashboardLayoutV1>(DEFAULT_DASHBOARD_LAYOUT);
  const [customOpen, setCustomOpen] = useState(false);
  const [kpiValOpen, setKpiValOpen] = useState(false);
  const [kpiValidation, setKpiValidation] = useState<DashboardKpiValidation | null>(null);
  const [kpiValErr, setKpiValErr] = useState<string | null>(null);
  const kpiValidationFetchStarted = useRef(false);

  const loadKpis = useCallback(() => {
    getDashboardKpis()
      .then(setKpis)
      .catch(() => setKpis(null));
  }, []);

  useEffect(() => {
    const perf = typeof performance !== "undefined" ? performance : undefined;
    perf?.mark?.("dashboard-load-start");
    void Promise.all([getDashboardKpis(), getDashboardLayout()])
      .then(([k, layoutPayload]) => {
        setKpis(k);
        setLayout(parseDashboardLayoutJson(layoutPayload.layout_json));
      })
      .catch(() => {
        setKpis(null);
        setLayout(DEFAULT_DASHBOARD_LAYOUT);
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
  }, [loadKpis]);

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
      if (id === DASHBOARD_WIDGET_IDS.KPIS || id === DASHBOARD_WIDGET_IDS.WORKLOAD) {
        return canAny("di.view", "ot.view", "eq.view", "pm.view");
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
        return (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <KpiCard
              title={t("kpi.openDis")}
              value={kpis?.open_dis.value ?? 0}
              trend={(kpis?.open_dis.value ?? 0) - (kpis?.open_dis.previous_value ?? 0)}
              icon={ClipboardList}
              color={CHART_COLORS.primary}
              trendDirection="up-bad"
              available={kpis?.open_dis.available ?? true}
              qualityBadge={kpis?.open_dis.quality_badge}
              onOpenEvidence={() => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OPEN_DIS])}
            />
            <KpiCard
              title={t("kpi.openWos")}
              value={kpis?.open_wos.value ?? 0}
              trend={(kpis?.open_wos.value ?? 0) - (kpis?.open_wos.previous_value ?? 0)}
              icon={Wrench}
              color={CHART_COLORS.accent}
              trendDirection="up-bad"
              available={kpis?.open_wos.available ?? false}
              qualityBadge={kpis?.open_wos.quality_badge}
              onOpenEvidence={() => navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OPEN_WOS])}
            />
            <KpiCard
              title={t("kpi.totalAssets")}
              value={kpis?.total_assets.value ?? 0}
              trend={(kpis?.total_assets.value ?? 0) - (kpis?.total_assets.previous_value ?? 0)}
              icon={Package}
              color={CHART_COLORS.success}
              trendDirection="up-good"
              available={kpis?.total_assets.available ?? true}
              qualityBadge={kpis?.total_assets.quality_badge}
              onOpenEvidence={() =>
                navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.TOTAL_ASSETS])
              }
            />
            <KpiCard
              title={t("kpi.overdueItems")}
              value={kpis?.overdue_items.value ?? 0}
              trend={(kpis?.overdue_items.value ?? 0) - (kpis?.overdue_items.previous_value ?? 0)}
              icon={AlertTriangle}
              color={CHART_COLORS.danger}
              trendDirection="up-bad"
              available={kpis?.overdue_items.available ?? true}
              qualityBadge={kpis?.overdue_items.quality_badge}
              onOpenEvidence={() =>
                navigate(DASHBOARD_KPI_DRILL_PATH[DASHBOARD_KPI_KEYS.OVERDUE_ITEMS])
              }
            />
          </div>
        );
      case DASHBOARD_WIDGET_IDS.WORKLOAD:
        return <DashboardWorkloadChart />;
      case DASHBOARD_WIDGET_IDS.DI_STATUS:
        return <DashboardDiStatusChart />;
      case DASHBOARD_WIDGET_IDS.RELIABILITY_SNAPSHOT:
        return <DashboardReliabilitySnapshotCard />;
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
      bodyClassName="space-y-6 p-4"
    >
      <DashboardCustomizeDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        layout={layout}
        onSaved={setLayout}
      />

      {visibleWidgets.map((w) => (
        <Fragment key={w.id}>{renderWidget(w.id)}</Fragment>
      ))}

      <PermissionGate anyOf={["di.screen", "di.review"]}>
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

      <div className="space-y-2">
        <h2 className="text-sm font-medium text-text-muted">{t("quickActions.title")}</h2>
        <div className="flex flex-wrap gap-3">
          <PermissionGate permission="di.create">
            <Button variant="outline" size="sm" onClick={() => navigate("/requests?action=create")}>
              {t("quickActions.newDi")}
            </Button>
          </PermissionGate>
          <PermissionGate permission="ot.create">
            <Button
              variant="outline"
              size="sm"
              onClick={() => navigate("/work-orders?action=create")}
            >
              {t("quickActions.newWo")}
            </Button>
          </PermissionGate>
          <PermissionGate permission="eq.manage">
            <Button
              variant="outline"
              size="sm"
              onClick={() => navigate("/equipment?action=create")}
            >
              {t("quickActions.newAsset")}
            </Button>
          </PermissionGate>
        </div>
      </div>
    </ModulePageShell>
  );
}
