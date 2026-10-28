import { Suspense, lazy } from "react";
import { Navigate, Outlet, type RouteObject, createBrowserRouter } from "react-router-dom";

import { AuthGuard } from "@/components/auth/AuthGuard";
import { PermissionRoute } from "@/components/auth/PermissionRoute";
import { ProductLicenseGate } from "@/components/auth/ProductLicenseGate";
import { AppShell } from "@/components/layout/AppShell";
import { DashboardPage } from "@/pages/DashboardPage";
import { NotFoundPage, RouteErrorPage } from "@/pages/NotFoundPage";
import { P } from "@shared/rbac/permissions.generated";

// Lazy-load all module pages
const EquipmentPage = lazy(() =>
  import("@/pages/EquipmentPage").then((m) => ({ default: m.EquipmentPage })),
);
const AssetImportPage = lazy(() =>
  import("@/pages/assets/AssetImportPage").then((m) => ({
    default: m.AssetImportPage,
  })),
);
const RequestsPage = lazy(() =>
  import("@/pages/RequestsPage").then((m) => ({ default: m.RequestsPage })),
);
const WorkOrdersPage = lazy(() =>
  import("@/pages/WorkOrdersPage").then((m) => ({
    default: m.WorkOrdersPage,
  })),
);
const PlanningPage = lazy(() =>
  import("@/pages/PlanningPage").then((m) => ({ default: m.PlanningPage })),
);
const PmPage = lazy(() => import("@/pages/PmPage").then((m) => ({ default: m.PmPage })));
const ComponentShowcase = lazy(() =>
  import("@/pages/dev/ComponentShowcase").then((m) => ({ default: m.ComponentShowcase })),
);
const PermitsPage = lazy(() =>
  import("@/pages/PermitsPage").then((m) => ({ default: m.PermitsPage })),
);
const InspectionsPage = lazy(() =>
  import("@/pages/InspectionsPage").then((m) => ({
    default: m.InspectionsPage,
  })),
);
const TrainingPage = lazy(() =>
  import("@/pages/TrainingPage").then((m) => ({ default: m.TrainingPage })),
);
const InventoryPage = lazy(() =>
  import("@/pages/InventoryPage").then((m) => ({ default: m.InventoryPage })),
);
const AnalyticsModuleLayout = lazy(() =>
  import("@/pages/analytics/AnalyticsModuleLayout").then((m) => ({
    default: m.AnalyticsModuleLayout,
  })),
);
const AnalyticsIndexRedirect = lazy(() =>
  import("@/pages/analytics/AnalyticsModuleLayout").then((m) => ({
    default: m.AnalyticsIndexRedirect,
  })),
);
const AnalyticsDashboardsPage = lazy(() =>
  import("@/pages/analytics/AnalyticsDashboardsPage").then((m) => ({
    default: m.AnalyticsDashboardsPage,
  })),
);
const AnalyticsTemplatesPage = lazy(() =>
  import("@/pages/analytics/AnalyticsTemplatesPage").then((m) => ({
    default: m.AnalyticsTemplatesPage,
  })),
);
const AnalyticsSchedulesPage = lazy(() =>
  import("@/pages/analytics/AnalyticsSchedulesPage").then((m) => ({
    default: m.AnalyticsSchedulesPage,
  })),
);
const AnalyticsHistoryPage = lazy(() =>
  import("@/pages/analytics/AnalyticsHistoryPage").then((m) => ({
    default: m.AnalyticsHistoryPage,
  })),
);
const RamsFleetPage = lazy(() =>
  import("@/pages/reliability/RamsFleetPage").then((m) => ({
    default: m.RamsFleetPage,
  })),
);
const RamsDataQualityPage = lazy(() =>
  import("@/pages/reliability/RamsDataQualityPage").then((m) => ({
    default: m.RamsDataQualityPage,
  })),
);
const RamsStudyPage = lazy(() =>
  import("@/pages/reliability/RamsStudyPage").then((m) => ({
    default: m.RamsStudyPage,
  })),
);
const BudgetPage = lazy(() =>
  import("@/pages/BudgetPage").then((m) => ({ default: m.BudgetPage })),
);
const PersonnelPage = lazy(() =>
  import("@/pages/PersonnelPage").then((m) => ({ default: m.PersonnelPage })),
);
const AdminPage = lazy(() => import("@/pages/AdminPage").then((m) => ({ default: m.AdminPage })));
const UnauthorizedPage = lazy(() =>
  import("@/pages/UnauthorizedPage").then((m) => ({ default: m.UnauthorizedPage })),
);
const OrgPage = lazy(() => import("@/pages/OrgPage").then((m) => ({ default: m.OrgPage })));
const LookupsPage = lazy(() =>
  import("@/pages/LookupsPage").then((m) => ({ default: m.LookupsPage })),
);
const DocumentationModuleLayout = lazy(() =>
  import("@/pages/documentation/DocumentationModuleLayout").then((m) => ({
    default: m.DocumentationModuleLayout,
  })),
);
const DocumentationCategoryPage = lazy(() =>
  import("@/pages/documentation/DocumentationCategoryPage").then((m) => ({
    default: m.DocumentationCategoryPage,
  })),
);
const DocumentationIndexRedirect = lazy(() =>
  import("@/pages/documentation/DocumentationModuleLayout").then((m) => ({
    default: m.DocumentationIndexRedirect,
  })),
);
const ArchivePage = lazy(() =>
  import("@/pages/ArchivePage").then((m) => ({ default: m.ArchivePage })),
);
const NotificationsPage = lazy(() =>
  import("@/pages/NotificationsPage").then((m) => ({ default: m.NotificationsPage })),
);
const SettingsPage = lazy(() =>
  import("@/pages/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);
const LoginPage = lazy(() =>
  import("@/pages/auth/LoginPage").then((m) => ({ default: m.LoginPage })),
);
const SetupInitialAdminPage = lazy(() =>
  import("@/pages/auth/SetupInitialAdminPage").then((m) => ({ default: m.SetupInitialAdminPage })),
);

function PageSuspense() {
  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
        </div>
      }
    >
      <Outlet />
    </Suspense>
  );
}

function ShellLayout() {
  return (
    <AppShell>
      <Outlet />
    </AppShell>
  );
}

const routes: RouteObject[] = [
  {
    element: (
      <ProductLicenseGate>
        <Outlet />
      </ProductLicenseGate>
    ),
    errorElement: <RouteErrorPage />,
    children: [
      // ── Public routes (license-gated, no shell, no auth required) ───────
      {
        path: "login",
        element: (
          <Suspense
            fallback={
              <div className="flex h-screen items-center justify-center bg-surface-0">
                <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
              </div>
            }
          >
            <LoginPage />
          </Suspense>
        ),
      },
      {
        path: "admin-setup",
        element: (
          <Suspense
            fallback={
              <div className="flex h-screen items-center justify-center bg-surface-0">
                <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
              </div>
            }
          >
            <SetupInitialAdminPage />
          </Suspense>
        ),
      },
      {
        path: "setup-admin",
        element: <Navigate to="/admin-setup" replace />,
      },
      // ── Protected routes (license + auth required → shell layout) ───────
      {
        element: <AuthGuard />,
        children: [
          {
            element: <ShellLayout />,
            children: [
              { index: true, element: <DashboardPage /> },
              {
                element: <PageSuspense />,
                children: [
                  {
                    element: <PermissionRoute permission={P.EQ_VIEW} />,
                    children: [
                      { path: "equipment", element: <EquipmentPage /> },
                      // The import screen needs the import right (the backend commands do too).
                      {
                        element: <PermissionRoute permission={P.EQ_IMPORT} />,
                        children: [{ path: "equipment/import", element: <AssetImportPage /> }],
                      },
                    ],
                  },
                  {
                    element: <PermissionRoute permission={P.DI_VIEW} />,
                    children: [{ path: "requests", element: <RequestsPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.OT_VIEW} />,
                    children: [{ path: "work-orders", element: <WorkOrdersPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.PLAN_VIEW} />,
                    children: [{ path: "planning", element: <PlanningPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.PM_VIEW} />,
                    children: [{ path: "pm", element: <PmPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.PTW_VIEW} />,
                    children: [{ path: "permits", element: <PermitsPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.INS_VIEW} />,
                    children: [{ path: "inspections", element: <InspectionsPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.TRN_VIEW} />,
                    children: [{ path: "training", element: <TrainingPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.INV_VIEW} />,
                    children: [{ path: "inventory", element: <InventoryPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.REP_VIEW} />,
                    children: [
                      {
                        path: "analytics",
                        element: <AnalyticsModuleLayout />,
                        children: [
                          { index: true, element: <AnalyticsIndexRedirect /> },
                          { path: "dashboards", element: <AnalyticsDashboardsPage /> },
                          { path: "templates", element: <AnalyticsTemplatesPage /> },
                          { path: "schedules", element: <AnalyticsSchedulesPage /> },
                          { path: "history", element: <AnalyticsHistoryPage /> },
                        ],
                      },
                    ],
                  },
                  {
                    element: <PermissionRoute permission={P.RAM_VIEW} />,
                    children: [
                      {
                        path: "reliability",
                        children: [
                          { index: true, element: <RamsFleetPage /> },
                          { path: "studies/:studyId", element: <RamsStudyPage /> },
                          { path: "data-quality", element: <RamsDataQualityPage /> },
                          // Former screens, now inside the module (study steps, data quality).
                          { path: "dashboard", element: <Navigate to="/reliability" replace /> },
                          {
                            path: "foundation",
                            element: <Navigate to="/reliability/data-quality" replace />,
                          },
                          { path: "lab", element: <Navigate to="/reliability" replace /> },
                          { path: "advanced", element: <Navigate to="/reliability" replace /> },
                          { path: "governance", element: <Navigate to="/reliability" replace /> },
                        ],
                      },
                      // The module used to live under the analytics hub: old links redirect with
                      // the reliability permission only.
                      {
                        path: "analytics/rams",
                        children: [
                          { index: true, element: <Navigate to="/reliability" replace /> },
                          { path: "dashboard", element: <Navigate to="/reliability" replace /> },
                          {
                            path: "foundation",
                            element: <Navigate to="/reliability/data-quality" replace />,
                          },
                          { path: "lab", element: <Navigate to="/reliability" replace /> },
                          { path: "advanced", element: <Navigate to="/reliability" replace /> },
                          { path: "governance", element: <Navigate to="/reliability" replace /> },
                        ],
                      },
                    ],
                  },
                  {
                    element: <PermissionRoute permission={P.FIN_VIEW} />,
                    children: [{ path: "budget", element: <BudgetPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.PER_VIEW} />,
                    children: [{ path: "personnel", element: <PersonnelPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.ADM_USERS} />,
                    children: [
                      {
                        path: "users",
                        element: <Navigate to="/admin?tab=users" replace />,
                      },
                    ],
                  },
                  {
                    element: (
                      <PermissionRoute anyOf={[P.ADM_USERS, P.ADM_ROLES, P.ADM_PERMISSIONS]} />
                    ),
                    children: [{ path: "admin", element: <AdminPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.ORG_VIEW} />,
                    children: [{ path: "org", element: <OrgPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.REF_VIEW} />,
                    children: [{ path: "lookups", element: <LookupsPage /> }],
                  },
                  { path: "notifications", element: <NotificationsPage /> },
                  {
                    element: <PermissionRoute permission={P.DOC_VIEW} />,
                    children: [
                      {
                        path: "documentation",
                        element: <DocumentationModuleLayout />,
                        children: [
                          {
                            index: true,
                            element: <DocumentationIndexRedirect />,
                          },
                          {
                            path: ":categorySlug",
                            element: <DocumentationCategoryPage />,
                          },
                        ],
                      },
                    ],
                  },
                  {
                    path: "iot",
                    element: <Navigate to="/" replace />,
                  },
                  {
                    path: "erp",
                    element: <Navigate to="/" replace />,
                  },
                  {
                    element: <PermissionRoute permission={P.ARC_VIEW} />,
                    children: [{ path: "archive", element: <ArchivePage /> }],
                  },
                  {
                    path: "activity",
                    element: <Navigate to="/settings?tab=activity" replace />,
                  },
                  { path: "settings", element: <SettingsPage /> },
                  // Living design-system reference (docs/DESIGN_SYSTEM.md). Development builds only:
                  // the import is tree-shaken from production bundles.
                  ...(import.meta.env.DEV
                    ? [{ path: "__dev/components", element: <ComponentShowcase /> }]
                    : []),
                  {
                    element: <PermissionRoute permission={P.CFG_VIEW} />,
                    children: [
                      {
                        path: "configuration",
                        element: <Navigate to="/settings" replace />,
                      },
                    ],
                  },
                  {
                    path: "diagnostics",
                    element: <Navigate to="/settings?tab=integrations" replace />,
                  },
                  {
                    path: "profile",
                    element: <Navigate to="/settings?tab=profile" replace />,
                  },
                  { path: "unauthorized", element: <UnauthorizedPage /> },
                  // Any address without a screen stays inside the shell with a way back.
                  { path: "*", element: <NotFoundPage /> },
                ],
              },
            ],
          },
        ],
      },
    ],
  },
];

/** Route tree, exported for tests (e.g. every dashboard drill link must resolve to a screen). */
export const appRouteTree = routes;

export const router = createBrowserRouter(routes);
