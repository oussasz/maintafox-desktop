import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { AnalyticsDashboardsPage } from "@/pages/analytics/AnalyticsDashboardsPage";
import type { AnalyticsExecutiveDashboardWithSmrp } from "@/services/analytics-dashboard-service";
import { useSessionStore } from "@/store/session-store";
import type { AnalyticsKpiValue } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "change" in opts
        ? `${key}:${String(opts["change"])}`
        : opts && "count" in opts
          ? `${key}:${String(opts["count"])}`
          : key,
    i18n: { language: "en" },
  }),
}));

const rights = vi.hoisted(() => ({ can: (_p: string): boolean => true }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => rights.can(p),
    canAny: (...ps: string[]) => ps.some((p) => rights.can(p)),
    isLoading: false,
  }),
}));

vi.mock("@/lib/session-ready", () => ({ isSessionActiveForBackgroundWork: () => true }));

const svc = vi.hoisted(() => ({
  getAnalyticsExecutiveDashboard: vi.fn(),
  getAnalyticsDashboardLayout: vi.fn(),
  saveAnalyticsDashboardLayout: vi.fn(),
}));
vi.mock("@/services/analytics-dashboard-service", () => svc);
const orgSvc = vi.hoisted(() => ({ listOrgTree: vi.fn(() => Promise.resolve([])) }));
vi.mock("@/services/org-node-service", () => orgSvc);

function kpi(key: string, value: number, previous: number, available = true): AnalyticsKpiValue {
  return { key, value, previous_value: previous, available, quality_badge: null, catalog_id: null };
}

const DATA: AnalyticsExecutiveDashboardWithSmrp = {
  horizon_days: 30,
  org_node_id: null,
  open_dis: kpi("open_dis", 12, 4),
  open_wos: kpi("open_wos", 7, 7),
  open_wos_rule: "not_yet_closed",
  total_assets: kpi("total_assets", 140, 140),
  overdue_items: kpi("overdue_items", 3, 1),
  pm_overdue: kpi("pm_overdue", 2, 2),
  pm_due_today: kpi("pm_due_today", 1, 1),
  pm_due_next_7d: kpi("pm_due_next_7d", 5, 5),
  avg_mtbf_hours: kpi("avg_mtbf_hours", 410, 400),
  avg_mttr_hours: kpi("avg_mttr_hours", 3.5, 4),
  avg_availability: kpi("avg_availability", 0.962, 0.95),
  avg_data_quality: kpi("avg_data_quality", 0.875, 0.8),
  maintenance_cost: kpi("maintenance_cost", 12000, 10000),
  maintenance_cost_pending: 0,
  reliability_snapshot_count: 30,
  reliability_total_events: 9,
  workload_days: [],
  workload_quality_badge: null,
  di_status_segments: [{ status: "in_review", count: 2 }],
  reliability_series: [],
  bad_actor_assets: [],
  smrp: null,
};

beforeEach(() => {
  vi.clearAllMocks();
  rights.can = () => true;
  useSessionStore.setState({
    info: { is_authenticated: true, is_locked: false, is_admin: true } as never,
  });
  svc.getAnalyticsExecutiveDashboard.mockResolvedValue(DATA);
  svc.getAnalyticsDashboardLayout.mockRejectedValue(new Error("no layout"));
});

function renderPage() {
  return render(
    <MemoryRouter>
      <AnalyticsDashboardsPage />
    </MemoryRouter>,
  );
}

function ListLocation() {
  const location = useLocation();
  return <p data-testid="list-location">{`${location.pathname}${location.search}`}</p>;
}

describe("AnalyticsDashboardsPage", () => {
  it("shows only the tiles whose figure the user may read (no bare dash)", async () => {
    rights.can = (p) => p !== P.DI_VIEW;
    renderPage();
    expect(await screen.findByText("kpi.openWos")).toBeInTheDocument();
    expect(screen.queryByText("kpi.openDis")).not.toBeInTheDocument();
  });

  it("names the open-OT rule on the tile and opens the list on the same rule (not yet closed)", async () => {
    render(
      <MemoryRouter initialEntries={["/analytics"]}>
        <Routes>
          <Route path="/analytics" element={<AnalyticsDashboardsPage />} />
          <Route path="/work-orders" element={<ListLocation />} />
        </Routes>
      </MemoryRouter>,
    );
    const context = await screen.findByText(/woOpenRule\.not_yet_closed/);
    fireEvent.click(context.closest("button") as HTMLElement);
    expect(await screen.findByTestId("list-location")).toHaveTextContent(
      "/work-orders?scope=not_closed",
    );
  });

  it("hides the organisation filter and does not read the tree without the organisation view right", async () => {
    const can = (p: PermissionName) => p !== P.ORG_VIEW;
    const perms: PermissionContextValue = {
      permissions: [],
      isLoading: false,
      can,
      canAny: (...ps) => ps.some(can),
      canAll: (...ps) => ps.every(can),
      refresh: vi.fn(),
    };
    render(
      <MemoryRouter>
        <PermissionContext.Provider value={perms}>
          <AnalyticsDashboardsPage />
        </PermissionContext.Provider>
      </MemoryRouter>,
    );
    expect(await screen.findByText("hub.dashboards.kpi.dataQuality")).toBeInTheDocument();
    expect(orgSvc.listOrgTree).not.toHaveBeenCalled();
    expect(screen.queryByText("hub.dashboards.filters.orgScope")).not.toBeInTheDocument();
  });

  it("shows ratios as percentages and trends only where a previous period exists", async () => {
    renderPage();
    expect(await screen.findByText("hub.dashboards.kpi.dataQuality")).toBeInTheDocument();
    // 0.875 → 87.5 %, not "0.88"
    expect(screen.getByText(/87[.,]5\s?%/)).toBeInTheDocument();
    expect(screen.queryByText("0.88")).not.toBeInTheDocument();
    expect(screen.queryByText("kpi.noChange")).not.toBeInTheDocument();
    // Cost rose 20 % vs the previous window; open requests went from 4 to 12 (+200 %), rebuilt
    // by the backend at the start of the horizon.
    expect(screen.getByText(/hub\.dashboards\.trend\.vsPrevious:\+20\s?%$/)).toBeInTheDocument();
    expect(
      screen.getAllByText(/hub\.dashboards\.trend\.vsPrevious:\+200\s?%$/).length,
    ).toBeGreaterThan(0);
    // Unchanged count (open WOs 7 → 7) reads stable.
    expect(screen.getAllByText("hub.dashboards.trend.stable").length).toBeGreaterThan(0);
    // DI status chart in words
    expect(screen.getByText("hub.dashboards.diStatus.in_review")).toBeInTheDocument();
  });

  it("states the settled cost and counts closed work orders not yet costed", async () => {
    svc.getAnalyticsExecutiveDashboard.mockResolvedValue({ ...DATA, maintenance_cost_pending: 2 });
    renderPage();
    expect(await screen.findByText("hub.dashboards.kpi.maintenanceCost")).toBeInTheDocument();
    expect(screen.getByText("hub.dashboards.context.costPending:2")).toBeInTheDocument();
  });

  it("shows the settled cost as not available without the finance view", async () => {
    svc.getAnalyticsExecutiveDashboard.mockResolvedValue({
      ...DATA,
      maintenance_cost: kpi("maintenance_cost", 0, 0, false),
      maintenance_cost_pending: null,
    });
    renderPage();
    expect(await screen.findByText("hub.dashboards.kpi.maintenanceCost")).toBeInTheDocument();
    expect(screen.getByText("hub.dashboards.context.costSettled")).toBeInTheDocument();
    expect(screen.queryByText("hub.dashboards.context.costPending:0")).not.toBeInTheDocument();
  });

  it("shows an error state with retry instead of raw error text", async () => {
    svc.getAnalyticsExecutiveDashboard.mockRejectedValueOnce({
      code: "DATABASE_ERROR",
      message: "SQLITE_BUSY",
    });
    renderPage();
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("hub.dashboards.loadError")).toBeInTheDocument();
  });

  it("opens /reliability from the reliability tiles only with the reliability view right", async () => {
    const mtbf = "hub.dashboards.kpi.mtbf";
    // rep.view only: the widget shows, no tile or evidence button leads to /reliability.
    rights.can = (p) => p !== P.RAM_VIEW;
    const first = renderPage();
    expect(await screen.findByText(mtbf)).toBeInTheDocument();
    expect(screen.getByText(mtbf).closest("button")).toBeNull();
    expect(screen.getByText("hub.dashboards.kpi.failureEvents").closest("button")).toBeNull();
    expect(screen.queryByText("widgets.reliability.openEvidence")).not.toBeInTheDocument();
    first.unmount();
    // ram.view held: the same tile is a link.
    rights.can = () => true;
    renderPage();
    expect(await screen.findByText(mtbf)).toBeInTheDocument();
    expect(screen.getByText(mtbf).closest("button")).not.toBeNull();
    expect(screen.getByText("widgets.reliability.openEvidence")).toBeInTheDocument();
  });
});
