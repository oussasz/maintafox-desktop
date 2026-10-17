import {
  BarChart3,
  CalendarClock,
  FileStack,
  History,
  LayoutDashboard,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { NavLink, Navigate, Outlet } from "react-router-dom";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { P } from "@shared/rbac/permissions.generated";

const TABS: { to: string; key: string; icon: LucideIcon }[] = [
  { to: "/analytics/dashboards", key: "hub.tabs.dashboards", icon: LayoutDashboard },
  { to: "/analytics/templates", key: "hub.tabs.templates", icon: FileStack },
  { to: "/analytics/schedules", key: "hub.tabs.schedules", icon: CalendarClock },
  { to: "/analytics/history", key: "hub.tabs.history", icon: History },
];

export function AnalyticsModuleLayout() {
  const { t } = useTranslation("reports");
  const { can } = usePermissions();
  const canView = can(P.REP_VIEW);

  if (!canView) {
    return (
      <ModulePageShell icon={BarChart3} title={t("hub.title")} bodyClassName="p-4">
        <p className="text-sm text-text-muted">{t("page.noAccess")}</p>
      </ModulePageShell>
    );
  }

  return (
    <ModulePageShell
      icon={BarChart3}
      title={t("hub.title")}
      description={t("hub.subtitle")}
      bodyClassName="flex min-h-0 flex-1 flex-col p-4"
    >
      <div className="mb-4 shrink-0 border-b border-surface-border">
        <nav className="flex flex-wrap gap-1" aria-label={t("hub.navAria")}>
          {TABS.map(({ to, key, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) => {
                return cn(
                  "inline-flex min-h-[44px] items-center gap-1.5 rounded-t-md px-3 py-2 text-sm transition-colors",
                  isActive
                    ? "border border-b-0 border-surface-border bg-surface-1 font-medium text-text-primary"
                    : "text-text-muted hover:bg-surface-2/70 hover:text-text-primary",
                );
              }}
            >
              <Icon className="h-3.5 w-3.5 shrink-0 opacity-90" aria-hidden />
              {t(key)}
            </NavLink>
          ))}
        </nav>
      </div>
      <div className="min-h-0 min-w-0 flex-1">
        <Outlet />
      </div>
    </ModulePageShell>
  );
}

export function AnalyticsIndexRedirect() {
  return <Navigate to="/analytics/dashboards" replace />;
}
