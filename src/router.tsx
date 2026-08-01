import { Suspense, lazy } from "react";
import { Navigate, Outlet, type RouteObject, createBrowserRouter } from "react-router-dom";

import { AuthGuard } from "@/components/auth/AuthGuard";
import { PermissionRoute } from "@/components/auth/PermissionRoute";
import { ProductLicenseGate } from "@/components/auth/ProductLicenseGate";
import { AppShell } from "@/components/layout/AppShell";
import { DashboardPage } from "@/pages/DashboardPage";
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
const PermitsPage = lazy(() =>
  import("@/pages/PermitsPage").then((m) => ({ default: m.PermitsPage })),
);
const InspectionsPage = lazy(() =>
  import("@/pages/InspectionsPage").then((m) => ({
    default: m.InspectionsPage,
  })),
);
const InventoryPage = lazy(() =>
  import("@/pages/InventoryPage").then((m) => ({ default: m.InventoryPage })),
);
const AnalyticsPage = lazy(() =>
  import("@/pages/AnalyticsPage").then((m) => ({ default: m.AnalyticsPage })),
);
const ReliabilityModuleLayout = lazy(() =>
  import("@/pages/reliability/ReliabilityModuleLayout").then((m) => ({
    default: m.ReliabilityModuleLayout,
  })),
);
const ReliabilityDashboardPage = lazy(() =>
  import("@/pages/reliability/ReliabilityDashboardPage").then((m) => ({
    default: m.ReliabilityDashboardPage,
  })),
);
const ReliabilityFoundationPage = lazy(() =>
  import("@/pages/reliability/ReliabilityFoundationPage").then((m) => ({
    default: m.ReliabilityFoundationPage,
  })),
);
const ReliabilityVisualLabPage = lazy(() =>
  import("@/pages/reliability/ReliabilityVisualLabPage").then((m) => ({
    default: m.ReliabilityVisualLabPage,
  })),
);
const ReliabilityAdvancedPage = lazy(() =>
  import("@/pages/reliability/ReliabilityAdvancedPage").then((m) => ({
    default: m.ReliabilityAdvancedPage,
  })),
);
const ReliabilityGovernancePage = lazy(() =>
  import("@/pages/reliability/ReliabilityGovernancePage").then((m) => ({
    default: m.ReliabilityGovernancePage,
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
const NotificationsPage = lazy(() =>
  import("@/pages/NotificationsPage").then((m) => ({
    default: m.NotificationsPage,
  })),
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
const ActivityPage = lazy(() =>
  import("@/pages/ActivityPage").then((m) => ({ default: m.ActivityPage })),
);
const SettingsPage = lazy(() =>
  import("@/pages/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);
const ProfilePage = lazy(() =>
  import("@/pages/ProfilePage").then((m) => ({ default: m.ProfilePage })),
);
const DiagnosticsPage = lazy(() =>
  import("@/pages/DiagnosticsPage").then((m) => ({ default: m.DiagnosticsPage })),
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
                      { path: "equipment/import", element: <AssetImportPage /> },
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
                    children: [
                      {
                        path: "training",
                        element: <Navigate to="/personnel?tab=training" replace />,
                      },
                    ],
                  },
                  {
                    element: <PermissionRoute permission={P.INV_VIEW} />,
                    children: [{ path: "inventory", element: <InventoryPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.REP_VIEW} />,
                    children: [
                      { path: "analytics", element: <AnalyticsPage /> },
                      {
                        path: "reliability",
                        element: <ReliabilityModuleLayout />,
                        children: [
                          { index: true, element: <Navigate to="dashboard" replace /> },
                          { path: "dashboard", element: <ReliabilityDashboardPage /> },
                          { path: "foundation", element: <ReliabilityFoundationPage /> },
                          { path: "lab", element: <ReliabilityVisualLabPage /> },
                          { path: "advanced", element: <ReliabilityAdvancedPage /> },
                          { path: "governance", element: <ReliabilityGovernancePage /> },
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
                    element: <PermissionRoute anyOf={[P.ADM_USERS, P.ADM_ROLES]} />,
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
                    element: <PermissionRoute permission={P.LOG_VIEW} />,
                    children: [{ path: "activity", element: <ActivityPage /> }],
                  },
                  {
                    element: <PermissionRoute permission={P.ADM_SETTINGS} />,
                    children: [{ path: "settings", element: <SettingsPage /> }],
                  },
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
                    element: <PermissionRoute permission={P.ADM_SETTINGS} />,
                    children: [{ path: "diagnostics", element: <DiagnosticsPage /> }],
                  },
                  { path: "profile", element: <ProfilePage /> },
                  { path: "unauthorized", element: <UnauthorizedPage /> },
                ],
              },
            ],
          },
        ],
      },
    ],
  },
];

export const router = createBrowserRouter(routes);
