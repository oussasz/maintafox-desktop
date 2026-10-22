import { Lock, Shield } from "lucide-react";
import { Suspense, lazy, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { AdminMetricCards, type AdminMetricTarget } from "@/components/admin/AdminMetricCards";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { LoadingState } from "@/components/ui/data-state";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { usePermissions } from "@/hooks/use-permissions";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

// Lazy-load each admin panel
const UserListPanel = lazy(() =>
  import("../components/admin/UserListPanel").then((m) => ({ default: m.UserListPanel })),
);
const RoleEditorPanel = lazy(() =>
  import("../components/admin/RoleEditorPanel").then((m) => ({ default: m.RoleEditorPanel })),
);
const PermissionCatalogPanel = lazy(() =>
  import("../components/admin/PermissionCatalogPanel").then((m) => ({
    default: m.PermissionCatalogPanel,
  })),
);
const SessionVisibilityPanel = lazy(() =>
  import("../components/admin/SessionVisibilityPanel").then((m) => ({
    default: m.SessionVisibilityPanel,
  })),
);
const EmergencyElevationPanel = lazy(() =>
  import("../components/admin/EmergencyElevationPanel").then((m) => ({
    default: m.EmergencyElevationPanel,
  })),
);
const RoleImportExportPanel = lazy(() =>
  import("../components/admin/RoleImportExportPanel").then((m) => ({
    default: m.RoleImportExportPanel,
  })),
);
const AdminAuditTimeline = lazy(() =>
  import("../components/admin/AdminAuditTimeline").then((m) => ({
    default: m.AdminAuditTimeline,
  })),
);

// ── Tab configuration ─────────────────────────────────────────────────────
// Delegated-administration policies (DelegationManagerPanel) are not offered: nothing in the
// permission checks reads them yet, so a policy saved there would change nothing.

interface AdminTab {
  key: string;
  labelKey: string;
  permission: PermissionName | PermissionName[];
  component: React.LazyExoticComponent<React.ComponentType>;
}

const ADMIN_TABS: AdminTab[] = [
  { key: "users", labelKey: "admin:tabs.users", permission: P.ADM_USERS, component: UserListPanel },
  {
    key: "roles",
    labelKey: "admin:tabs.roles",
    permission: P.ADM_ROLES,
    component: RoleEditorPanel,
  },
  {
    key: "permissions",
    labelKey: "admin:tabs.permissions",
    permission: P.ADM_PERMISSIONS,
    component: PermissionCatalogPanel,
  },
  {
    key: "sessions",
    labelKey: "admin:tabs.sessions",
    permission: P.ADM_USERS,
    component: SessionVisibilityPanel,
  },
  {
    key: "emergency",
    labelKey: "admin:tabs.emergency",
    permission: P.ADM_USERS,
    component: EmergencyElevationPanel,
  },
  {
    key: "import-export",
    labelKey: "admin:tabs.importExport",
    permission: P.ADM_ROLES,
    component: RoleImportExportPanel,
  },
  {
    key: "audit",
    labelKey: "admin:tabs.audit",
    permission: [P.ADM_USERS, P.ADM_ROLES, P.ADM_PERMISSIONS],
    component: AdminAuditTimeline,
  },
];

// ── Panel suspense wrapper ────────────────────────────────────────────────

function PanelFallback() {
  return <LoadingState className="h-48" />;
}

// ── AdminPage ─────────────────────────────────────────────────────────────

export function AdminPage() {
  const { t } = useTranslation(["admin", "auth"]);
  const { can, canAny } = usePermissions();
  const [searchParams, setSearchParams] = useSearchParams();

  // Filter tabs to only those the user has permission for
  const visibleTabs = useMemo(
    () =>
      ADMIN_TABS.filter((tab) => {
        if (Array.isArray(tab.permission)) {
          return canAny(...tab.permission);
        }
        return can(tab.permission);
      }),
    [can, canAny],
  );

  // Active tab from URL or first visible tab
  const activeTab = searchParams.get("tab") ?? visibleTabs[0]?.key ?? "";

  const setActiveTab = useCallback(
    (key: string) => {
      setSearchParams({ tab: key }, { replace: true });
    },
    [setSearchParams],
  );

  // If user has no admin permissions at all, show 403
  if (visibleTabs.length === 0) {
    return (
      <div className="flex h-full items-center justify-center px-4">
        <div className="text-center max-w-md">
          <div className="mx-auto mb-6 flex h-16 w-16 items-center justify-center rounded-full bg-tone-danger-bg">
            <Lock className="h-8 w-8 text-tone-danger-fg" aria-hidden />
          </div>
          <h1 className="text-xl font-semibold text-text-primary">
            {t("auth:unauthorized.title")}
          </h1>
          <p className="mt-2 text-sm text-text-secondary">{t("auth:unauthorized.message")}</p>
        </div>
      </div>
    );
  }

  // Resolve the active panel component
  const activeTabDef = visibleTabs.find((t) => t.key === activeTab) ?? visibleTabs[0];
  const PanelComponent = activeTabDef?.component;

  const metricTab: Record<AdminMetricTarget, string> = {
    users: "users",
    roles: "roles",
    sessions: "sessions",
  };

  return (
    <ModulePageShell
      icon={Shield}
      title={t("admin:page.title")}
      description={t("admin:page.subtitle")}
    >
      <div className="flex min-h-0 flex-col gap-4">
        {/* `get_admin_stats` (polled every minute) asks for adm.users. */}
        {can(P.ADM_USERS) ? (
          <AdminMetricCards
            onOpen={(target) => setActiveTab(metricTab[target])}
            canOpen={(target) => visibleTabs.some((tab) => tab.key === metricTab[target])}
          />
        ) : null}

        <Tabs value={activeTabDef?.key ?? ""} onValueChange={setActiveTab}>
          <TabsList className="h-auto w-full flex-wrap justify-start">
            {visibleTabs.map((tab) => (
              <TabsTrigger key={tab.key} value={tab.key} className="min-h-10">
                {t(tab.labelKey)}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>

        <div className="min-h-0 flex-1">
          {PanelComponent && (
            <Suspense fallback={<PanelFallback />}>
              <PanelComponent />
            </Suspense>
          )}
        </div>
      </div>
    </ModulePageShell>
  );
}
