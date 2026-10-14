import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AnalyticsDashboardsPage } from "@/pages/analytics/AnalyticsDashboardsPage";
import { useSessionStore } from "@/store/session-store";
import type { AnalyticsExecutiveDashboard, AnalyticsKpiValue } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "change" in opts ? `${key}:${String(opts["change"])}` : key,
    i18n: { language: "en" },
  }),
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

vi.mock("@/lib/session-ready", () => ({ isSessionActiveForBackgroundWork: () => true }));

const svc = vi.hoisted(() => ({
  getAnalyticsExecutiveDashboard: vi.fn(),
  getAnalyticsDashboardLayout: vi.fn(),
  saveAnalyticsDashboardLayout: vi.fn(),
}));
vi.mock("@/services/analytics-dashboard-service", () => svc);
vi.mock("@/services/org-node-service", () => ({ listOrgTree: vi.fn(() => Promise.resolve([])) }));

function kpi(key: string, value: number, previous: number, available = true): AnalyticsKpiValue {
  return { key, value, previous_value: previous, available, quality_badge: null, catalog_id: null };
}

const DATA: AnalyticsExecutiveDashboard = {
  horizon_days: 30,
  org_node_id: null,
  open_dis: kpi("open_dis", 12, 4),
  open_wos: kpi("open_wos", 7, 7),
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
  reliability_snapshot_count: 30,
  reliability_total_events: 9,
  workload_days: [],
  workload_quality_badge: null,
  di_status_segments: [{ status: "in_review", count: 2 }],
  reliability_series: [],
  bad_actor_assets: [],
};

beforeEach(() => {
  vi.clearAllMocks();
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

describe("AnalyticsDashboardsPage", () => {
  it("shows ratios as percentages and trends only where a previous period exists", async () => {
    renderPage();
    expect(await screen.findByText("hub.dashboards.kpi.dataQuality")).toBeInTheDocument();
    // 0.875 → 87.5 %, not "0.88"
    expect(screen.getByText(/87[.,]5\s?%/)).toBeInTheDocument();
    expect(screen.queryByText("0.88")).not.toBeInTheDocument();
    // open counts are "as of now": no "no change" / trend badge
    expect(screen.queryByText("kpi.noChange")).not.toBeInTheDocument();
    // cost rose 20 % vs previous window
    expect(screen.getByText(/hub\.dashboards\.trend\.vsPrevious:\+20/)).toBeInTheDocument();
    // DI status chart in words
    expect(screen.getByText("hub.dashboards.diStatus.in_review")).toBeInTheDocument();
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
});
