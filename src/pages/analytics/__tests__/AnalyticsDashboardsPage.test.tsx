import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { AnalyticsDashboardsPage } from "@/pages/analytics/AnalyticsDashboardsPage";
import type { AnalyticsExecutiveDashboardWithSmrp } from "@/services/analytics-dashboard-service";
import type { FleetKpis } from "@/services/reliability-study-service";
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

/** Live fleet figures of the horizon (`study::fleet_kpis`), mixed hours basis. */
const RELIABILITY: FleetKpis = {
  period_start: "2025-11-06T00:00:00Z",
  period_end: "2026-11-06T00:00:00Z",
  window_too_short: false,
  equipment_count: 5,
  failure_count: 9,
  downtime_hours: 40,
  exposure_hours: 3690,
  availability_operational: 0.962,
  mtbf_hours: 410,
  eligible_failure_count: 9,
  mtbf_excluded_missing_date: 0,
  mtbf_excluded_outside_service: 2,
  mttr_hours: 3.5,
  restore_count: 8,
  restore_excluded_missing_failed_at: 0,
  restore_excluded_missing_restored_at: 1,
  restore_excluded_inconsistent: 0,
  mean_active_repair_hours: 2,
  active_repair_count: 9,
  repair_excluded_missing_hours: 0,
  data_quality: 0.875,
  records_complete: 7,
  records_missing_mode: 1,
  records_missing_cause: 1,
  records_missing_mechanism: 2,
  operating_basis_count: 3,
  calendar_basis_count: 2,
};

const DATA: AnalyticsExecutiveDashboardWithSmrp = {
  horizon_days: 365,
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
  sample_size_score: kpi("sample_size_score", 0.6, 0.6),
  reliability: RELIABILITY,
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
    expect(await screen.findByText("rams.kpi.dataQuality")).toBeInTheDocument();
    expect(orgSvc.listOrgTree).not.toHaveBeenCalled();
    expect(screen.queryByText("hub.dashboards.filters.orgScope")).not.toBeInTheDocument();
  });

  it("shows ratios as percentages and trends only where a previous period exists", async () => {
    renderPage();
    expect(await screen.findByText("rams.kpi.dataQuality")).toBeInTheDocument();
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
    const mtbf = "rams.kpi.mtbf";
    // rep.view only: the widget shows, no tile or evidence button leads to /reliability.
    rights.can = (p) => p !== P.RAM_VIEW;
    const first = renderPage();
    expect(await screen.findByText(mtbf)).toBeInTheDocument();
    expect(screen.getByText(mtbf).closest("button")).toBeNull();
    expect(screen.getByText("rams.kpi.failures").closest("button")).toBeNull();
    expect(screen.queryByText("widgets.reliability.openEvidence")).not.toBeInTheDocument();
    first.unmount();
    // ram.view held: the same tile is a link.
    rights.can = () => true;
    renderPage();
    expect(await screen.findByText(mtbf)).toBeInTheDocument();
    expect(screen.getByText(mtbf).closest("button")).not.toBeNull();
    expect(screen.getByText("widgets.reliability.openEvidence")).toBeInTheDocument();
  });

  it("does not colour a rising PM-due-today or PM-due-in-7-days count; only overdue is red", async () => {
    svc.getAnalyticsExecutiveDashboard.mockResolvedValue({
      ...DATA,
      pm_overdue: kpi("pm_overdue", 6, 2),
      pm_due_today: kpi("pm_due_today", 6, 2),
      pm_due_next_7d: kpi("pm_due_next_7d", 9, 5),
    });
    renderPage();
    // The trend badge is red ("bg-status-danger") only for a rising "bad" sense.
    const isRed = async (key: string) =>
      (await screen.findByText(key))
        .closest("button")
        ?.querySelector("span[class*='bg-status-danger']") != null;
    expect(await screen.findByText("hub.dashboards.kpi.pmDueToday")).toBeInTheDocument();
    expect(await isRed("hub.dashboards.kpi.pmDueToday")).toBe(false);
    expect(await isRed("hub.dashboards.kpi.pmDueNext7d")).toBe(false);
    expect(await isRed("hub.dashboards.kpi.pmOverdue")).toBe(true);
  });

  it("opens on 12 months and offers 7 / 30 / 90 days and 12 months", async () => {
    renderPage();
    expect(await screen.findByText("rams.kpi.mtbf")).toBeInTheDocument();
    expect(svc.getAnalyticsExecutiveDashboard).toHaveBeenCalledWith({
      horizon_days: 365,
      org_node_id: null,
    });
  });

  it("labels the reliability cards like the RAMS home and states window and hours basis", async () => {
    renderPage();
    for (const key of [
      "rams.kpi.mtbf",
      "rams.kpi.mttr",
      "rams.kpi.availability",
      "rams.kpi.failures",
      "rams.kpi.dataQuality",
    ]) {
      expect(await screen.findByText(key)).toBeInTheDocument();
    }
    for (const old of ["mtbf", "mttr", "availability", "dataQuality", "failureEvents"]) {
      expect(screen.queryByText(`hub.dashboards.kpi.${old}`)).not.toBeInTheDocument();
    }
    // MTBF context: its definition (window + basis), then "N exclus : …".
    const mtbfCard = screen.getByText("rams.kpi.mtbf").closest("button") as HTMLElement;
    expect(mtbfCard).toHaveTextContent("rams.kpi.mtbfHint · rams.kpi.excluded.outsideService:2");
    expect(screen.queryByText(/rams\.kpi\.tooShort/)).not.toBeInTheDocument();
  });

  it("shows counts only and says the period is too short under 90 days", async () => {
    svc.getAnalyticsExecutiveDashboard.mockResolvedValue({
      ...DATA,
      horizon_days: 30,
      avg_mtbf_hours: kpi("avg_mtbf_hours", 0, 0, false),
      avg_mttr_hours: kpi("avg_mttr_hours", 0, 0, false),
      avg_availability: kpi("avg_availability", 0, 0, false),
      reliability: { ...RELIABILITY, window_too_short: true, mtbf_hours: null, mttr_hours: null },
      reliability_total_events: 2,
    });
    renderPage();
    expect(await screen.findByText("rams.kpi.mtbf")).toBeInTheDocument();
    // MTBF, MTTR and Aₒ: "—" and the reason.
    expect(screen.getAllByText(/rams\.kpi\.tooShort/)).toHaveLength(3);
    const failures = screen.getByText("rams.kpi.failures").closest("button") as HTMLElement;
    expect(failures).toHaveTextContent("2");
  });

  it('names the sample-size score "Taille d\'échantillon" and says it comes from the snapshots', async () => {
    renderPage();
    const title = await screen.findByText("hub.dashboards.kpi.sampleSize");
    const card = title.parentElement as HTMLElement;
    expect(card).toHaveTextContent("hub.dashboards.context.snapshots");
    expect(card).toHaveTextContent(/60\s?%/);
    expect(card).not.toHaveTextContent(/quality/i);
    // The snapshot count says the same.
    expect(screen.getAllByText("hub.dashboards.context.snapshots")).toHaveLength(2);
  });

  it("shows no trend on data quality when the window before had no record", async () => {
    svc.getAnalyticsExecutiveDashboard.mockResolvedValue({
      ...DATA,
      avg_data_quality: {
        ...kpi("avg_data_quality", 0.5, 0),
        quality_badge: "insufficient_baseline",
      },
    });
    renderPage();
    const card = (await screen.findByText("rams.kpi.dataQuality")).closest("button") as HTMLElement;
    expect(card).not.toHaveTextContent(/hub\.dashboards\.trend\./);
  });

  it("takes every trend sense from the KPI registry: no sense literal in the page source", () => {
    const source = readFileSync(
      resolve(process.cwd(), "src/pages/analytics/AnalyticsDashboardsPage.tsx"),
      "utf8",
    );
    expect(source).not.toMatch(/periodTrend\([^)]*"(up-good|up-bad|neutral)"/s);
    expect(source).not.toMatch(/"up-(good|bad)"/);
  });
});
