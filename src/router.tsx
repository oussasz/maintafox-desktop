import { lazy, Suspense } from "react";
import { createBrowserRouter, Outlet, type RouteObject } from "react-router-dom";

import { AuthGuard } from "@/components/auth/AuthGuard";
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
const TrainingPage = lazy(() =>
  import("@/pages/TrainingPage").then((m) => ({ default: m.TrainingPage })),
);
const InventoryPage = lazy(() =>
  import("@/pages/InventoryPage").then((m) => ({ default: m.InventoryPage })),
);
const AnalyticsPage = lazy(() =>
  import("@/pages/AnalyticsPage").then((m) => ({ default: m.AnalyticsPage })),
);
const ReliabilityPage = lazy(() =>
  import("@/pages/ReliabilityPage").then((m) => ({
    default: m.ReliabilityPage,
  })),
);
const BudgetPage = lazy(() =>
  import("@/pages/BudgetPage").then((m) => ({ default: m.BudgetPage })),
);
const PersonnelPage = lazy(() =>
  import("@/pages/PersonnelPage").then((m) => ({ default: m.PersonnelPage })),
);
const UsersPage = lazy(() => import("@/pages/UsersPage").then((m) => ({ default: m.UsersPage })));
const OrgPage = lazy(() => import("@/pages/OrgPage").then((m) => ({ default: m.OrgPage })));
const LookupsPage = lazy(() =>
  import("@/pages/LookupsPage").then((m) => ({ default: m.LookupsPage })),
);
const NotificationsPage = lazy(() =>
  import("@/pages/NotificationsPage").then((m) => ({
    default: m.NotificationsPage,
  })),
);
const DocumentationPage = lazy(() =>
  import("@/pages/DocumentationPage").then((m) => ({
    default: m.DocumentationPage,
  })),
);
const IotPage = lazy(() => import("@/pages/IotPage").then((m) => ({ default: m.IotPage })));
const ErpPage = lazy(() => import("@/pages/ErpPage").then((m) => ({ default: m.ErpPage })));
const ArchivePage = lazy(() =>
  import("@/pages/ArchivePage").then((m) => ({ default: m.ArchivePage })),
);
const ActivityPage = lazy(() =>
  import("@/pages/ActivityPage").then((m) => ({ default: m.ActivityPage })),
);
const SettingsPage = lazy(() =>
  import("@/pages/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);
const ConfigurationPage = lazy(() =>
  import("@/pages/ConfigurationPage").then((m) => ({
    default: m.ConfigurationPage,
  })),
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
  // ── Public routes (no shell, no auth required) ───────────────────────
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
  // ── Protected routes (auth required → shell layout) ──────────────────
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
              { path: "equipment", element: <EquipmentPage /> },
              { path: "equipment/import", element: <AssetImportPage /> },
              { path: "requests", element: <RequestsPage /> },
              { path: "work-orders", element: <WorkOrdersPage /> },
              { path: "planning", element: <PlanningPage /> },
              { path: "pm", element: <PmPage /> },
              { path: "permits", element: <PermitsPage /> },
              { path: "inspections", element: <InspectionsPage /> },
              { path: "training", element: <TrainingPage /> },
              { path: "inventory", element: <InventoryPage /> },
              { path: "analytics", element: <AnalyticsPage /> },
              { path: "reliability", element: <ReliabilityPage /> },
              { path: "budget", element: <BudgetPage /> },
              { path: "personnel", element: <PersonnelPage /> },
              { path: "users", element: <UsersPage /> },
              { path: "org", element: <OrgPage /> },
              { path: "lookups", element: <LookupsPage /> },
              { path: "notifications", element: <NotificationsPage /> },
              { path: "documentation", element: <DocumentationPage /> },
              { path: "iot", element: <IotPage /> },
              { path: "erp", element: <ErpPage /> },
              { path: "archive", element: <ArchivePage /> },
              { path: "activity", element: <ActivityPage /> },
              { path: "settings", element: <SettingsPage /> },
              { path: "configuration", element: <ConfigurationPage /> },
              { path: "diagnostics", element: <DiagnosticsPage /> },
              { path: "profile", element: <ProfilePage /> },
            ],
          },
        ],
      },
    ],
  },
];

export const router = createBrowserRouter(routes);
