import { Suspense, lazy } from "react";
import { Navigate, Outlet, type RouteObject, createBrowserRouter } from "react-router-dom";

import { AuthGuard } from "@/components/auth/AuthGuard";
import { PermissionRoute } from "@/components/auth/PermissionRoute";
import { ProductLicenseGate } from "@/components/auth/ProductLicenseGate";
import { AppShell } from "@/components/layout/AppShell";
import { DashboardPage } from "@/pages/DashboardPage";

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
                    element: <PermissionRoute permission="eq.view" />,
                    children: [
                      { path: "equipment", element: <EquipmentPage /> },
                      { path: "equipment/import", element: <AssetImportPage /> },
                    ],
                  },
                  {
                    element: <PermissionRoute permission="di.view" />,
                    children: [{ path: "requests", element: <RequestsPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="ot.view" />,
                    children: [{ path: "work-orders", element: <WorkOrdersPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="plan.view" />,
                    children: [{ path: "planning", element: <PlanningPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="pm.view" />,
                    children: [{ path: "pm", element: <PmPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="ptw.view" />,
                    children: [{ path: "permits", element: <PermitsPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="ins.view" />,
                    children: [{ path: "inspections", element: <InspectionsPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="trn.view" />,
                    children: [
                      {
                        path: "training",
                        element: <Navigate to="/personnel?tab=training" replace />,
                      },
                    ],
                  },
                  {
                    element: <PermissionRoute permission="inv.view" />,
                    children: [{ path: "inventory", element: <InventoryPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="rep.view" />,
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
                    element: <PermissionRoute permission="fin.view" />,
                    children: [{ path: "budget", element: <BudgetPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="per.view" />,
                    children: [{ path: "personnel", element: <PersonnelPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="adm.users" />,
                    children: [
                      {
                        path: "users",
                        element: <Navigate to="/admin?tab=users" replace />,
                      },
                    ],
                  },
                  {
                    element: <PermissionRoute anyOf={["adm.users", "adm.roles"]} />,
                    children: [{ path: "admin", element: <AdminPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="org.view" />,
                    children: [{ path: "org", element: <OrgPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="ref.view" />,
                    children: [{ path: "lookups", element: <LookupsPage /> }],
                  },
                  { path: "notifications", element: <NotificationsPage /> },
                  {
                    element: <PermissionRoute permission="doc.view" />,
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
                    element: <PermissionRoute permission="arc.view" />,
                    children: [{ path: "archive", element: <ArchivePage /> }],
                  },
                  {
                    element: <PermissionRoute permission="log.view" />,
                    children: [{ path: "activity", element: <ActivityPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="adm.settings" />,
                    children: [{ path: "settings", element: <SettingsPage /> }],
                  },
                  {
                    element: <PermissionRoute permission="cfg.view" />,
                    children: [
                      {
                        path: "configuration",
                        element: <Navigate to="/settings" replace />,
                      },
                    ],
                  },
                  { path: "diagnostics", element: <DiagnosticsPage /> },
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
