import { Lock, Shield } from "lucide-react";
import { Suspense, lazy, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { AdminMetricCards } from "@/components/admin/AdminMetricCards";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";

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
const DelegationManagerPanel = lazy(() =>
  import("../components/admin/DelegationManagerPanel").then((m) => ({
    default: m.DelegationManagerPanel,
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

interface AdminTab {
  key: string;
  labelKey: string;
  permission: string | string[];
  component: React.LazyExoticComponent<React.ComponentType>;
}

const ADMIN_TABS: AdminTab[] = [
  { key: "users", labelKey: "admin:tabs.users", permission: "adm.users", component: UserListPanel },
  {
    key: "roles",
    labelKey: "admin:tabs.roles",
    permission: "adm.roles",
    component: RoleEditorPanel,
  },
  {
    key: "permissions",
    labelKey: "admin:tabs.permissions",
    permission: "adm.permissions",
    component: PermissionCatalogPanel,
  },
  {
    key: "sessions",
    labelKey: "admin:tabs.sessions",
    permission: "adm.users",
    component: SessionVisibilityPanel,
  },
  {
    key: "delegation",
    labelKey: "admin:tabs.delegation",
    permission: "adm.roles",
    component: DelegationManagerPanel,
  },
  {
    key: "emergency",
    labelKey: "admin:tabs.emergency",
    permission: "adm.users",
    component: EmergencyElevationPanel,
  },
  {
    key: "import-export",
    labelKey: "admin:tabs.importExport",
    permission: "adm.roles",
    component: RoleImportExportPanel,
  },
  {
    key: "audit",
    labelKey: "admin:tabs.audit",
    permission: ["adm.users", "adm.roles", "adm.permissions"],
    component: AdminAuditTimeline,
  },
];

// ── Panel suspense wrapper ────────────────────────────────────────────────

function PanelFallback() {
  return (
    <div className="flex h-48 items-center justify-center">
      <div className="h-5 w-5 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
    </div>
  );
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
          <div className="mx-auto mb-6 flex h-16 w-16 items-center justify-center rounded-full bg-status-danger/10">
            <Lock className="h-8 w-8 text-status-danger" />
          </div>
          <h1 className="text-xl font-semibold text-text-primary">
            {t("auth:unauthorized.title", "Accès non autorisé")}
          </h1>
          <p className="mt-2 text-sm text-text-secondary">
            {t(
              "auth:unauthorized.message",
              "Vous n'avez pas les permissions nécessaires pour accéder à cette page.",
            )}
          </p>
        </div>
      </div>
    );
  }

  // Resolve the active panel component
  const activeTabDef = visibleTabs.find((t) => t.key === activeTab) ?? visibleTabs[0];
  const PanelComponent = activeTabDef?.component;

  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <Shield className={mfLayout.moduleHeaderIcon} />
          <div>
            <h1 className={mfLayout.moduleTitle}>{t("admin:page.title", "Administration")}</h1>
            <p className="text-sm text-text-secondary">
              {t("admin:page.subtitle", "Gestion des utilisateurs, rôles et permissions")}
            </p>
          </div>
        </div>
      </div>

      <div className={mfLayout.moduleContent}>
        {/* KPI metric cards */}
        <AdminMetricCards />

        {/* Tab bar */}
        <div className="shrink-0 border-b border-surface-border">
          <nav className="-mb-px flex gap-1 overflow-x-auto" role="tablist">
            {visibleTabs.map((tab) => {
              const isActive = tab.key === (activeTabDef?.key ?? "");
              return (
                <button
                  key={tab.key}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  onClick={() => setActiveTab(tab.key)}
                  className={`whitespace-nowrap border-b-2 px-4 py-2.5 text-sm font-medium transition-colors
                  ${
                    isActive
                      ? "border-primary text-primary"
                      : "border-transparent text-text-secondary hover:border-surface-border hover:text-text-primary"
                  }`}
                >
                  {t(tab.labelKey)}
                </button>
              );
            })}
          </nav>
        </div>

        {/* Active panel */}
        <div className="min-h-0 flex-1">
          {PanelComponent && (
            <Suspense fallback={<PanelFallback />}>
              <PanelComponent />
            </Suspense>
          )}
        </div>
      </div>
    </div>
  );
}
