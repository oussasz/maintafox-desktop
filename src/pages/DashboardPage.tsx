import { AlertTriangle, ClipboardList, Package, Wrench } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { CHART_COLORS } from "@/components/charts/chart-utils";
import { DashboardWorkloadChart } from "@/components/dashboard/DashboardWorkloadChart";
import { KpiCard } from "@/components/dashboard/KpiCard";
import { Button } from "@/components/ui/button";
import { getDashboardKpis } from "@/services/dashboard-service";
import { useAppStore } from "@/store/app-store";
import type { DashboardKpis } from "@shared/ipc-types";

const KPI_REFRESH_MS = 5 * 60 * 1000; // 5 minutes

export function DashboardPage() {
  const { t } = useTranslation("dashboard");
  const displayName = useAppStore((s) => s.currentUserDisplayName);
  const navigate = useNavigate();

  const [kpis, setKpis] = useState<DashboardKpis | null>(null);

  const loadKpis = useCallback(() => {
    getDashboardKpis()
      .then(setKpis)
      .catch(() => setKpis(null));
  }, []);

  useEffect(() => {
    loadKpis();
    const interval = setInterval(loadKpis, KPI_REFRESH_MS);
    return () => clearInterval(interval);
  }, [loadKpis]);

  return (
    <div className="p-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold text-text-primary">{t("page.title")}</h1>
        {displayName && (
          <p className="text-sm text-text-muted">{t("page.welcome", { name: displayName })}</p>
        )}
      </div>

      {/* KPI Grid */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <KpiCard
          title={t("kpi.openDis")}
          value={kpis?.open_dis.value ?? 0}
          trend={(kpis?.open_dis.value ?? 0) - (kpis?.open_dis.previous_value ?? 0)}
          icon={ClipboardList}
          color={CHART_COLORS.primary}
          trendDirection="up-bad"
          available={kpis?.open_dis.available ?? true}
        />
        <KpiCard
          title={t("kpi.openWos")}
          value={kpis?.open_wos.value ?? 0}
          trend={(kpis?.open_wos.value ?? 0) - (kpis?.open_wos.previous_value ?? 0)}
          icon={Wrench}
          color={CHART_COLORS.accent}
          trendDirection="up-bad"
          available={kpis?.open_wos.available ?? false}
        />
        <KpiCard
          title={t("kpi.totalAssets")}
          value={kpis?.total_assets.value ?? 0}
          trend={(kpis?.total_assets.value ?? 0) - (kpis?.total_assets.previous_value ?? 0)}
          icon={Package}
          color={CHART_COLORS.success}
          trendDirection="up-good"
          available={kpis?.total_assets.available ?? true}
        />
        <KpiCard
          title={t("kpi.overdueItems")}
          value={kpis?.overdue_items.value ?? 0}
          trend={(kpis?.overdue_items.value ?? 0) - (kpis?.overdue_items.previous_value ?? 0)}
          icon={AlertTriangle}
          color={CHART_COLORS.danger}
          trendDirection="up-bad"
          available={kpis?.overdue_items.available ?? true}
        />
      </div>

      {/* Workload Chart */}
      <DashboardWorkloadChart />

      {/* Quick Actions */}
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
    </div>
  );
}
