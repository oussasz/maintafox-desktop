/**
 * Home page services: each panel reaches its one backend command with the right arguments,
 * keeps the module sections the user may not open as `null`, and refuses a malformed answer
 * (the panel then shows its error with a retry — never a 0).
 */
import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import {
  getDashboardDiStatusChart,
  getDashboardLayout,
  getDashboardWorkloadChart,
  getOverviewKpiLayout,
  getOverviewKpis,
  saveDashboardLayout,
  saveOverviewKpiLayout,
} from "../dashboard-service";
import {
  HOME_PERIODS,
  getHomeMyWork,
  getHomePlanning,
  getHomeSupervision,
  isHomePeriod,
} from "../home-service";

const queues = {
  to_screen: 2,
  to_review: 1,
  awaiting_info: 0,
  to_approve: 0,
  approved_not_converted: 1,
  deferred_due: 0,
  sla_at_risk: 1,
  mine: 3,
  mine_awaiting_info: 1,
};

const myWork = {
  as_of: "2026-10-16T08:00:00Z",
  work_orders: {
    open: 3,
    overdue: 1,
    today: 1,
    by_urgency: [
      { urgency_code: "emergency", count: 1 },
      { urgency_code: null, count: 2 },
    ],
    items: [
      {
        id: 4,
        code: "OT-2026-0004",
        title: "Pump seal",
        status_code: "in_progress",
        urgency_code: "emergency",
        planned_start: null,
        planned_end: "2026-10-15T12:00:00Z",
        asset_label: "P-101 — Feed pump",
        overdue: true,
      },
    ],
  },
  requests: { open: 3, awaiting_my_answer: 1 },
  rounds: {
    personnel_linked: true,
    overdue: 1,
    today: 0,
    next_7_days: 2,
    todo: 3,
    items: [
      {
        id: 9,
        template_code: "RND-1",
        template_name: "Pump house",
        status: "scheduled",
        scheduled_at: "2026-10-15T06:00:00Z",
        overdue: true,
      },
    ],
  },
  pm: {
    overdue: 0,
    due_next_7_days: 1,
    pending: 1,
    items: [
      {
        id: 12,
        plan_code: "PM-1",
        plan_title: "Lubrication",
        due_at: "2026-10-18T06:00:00Z",
        work_order_code: "OT-2026-0010",
        overdue: false,
      },
    ],
  },
  notifications: { unread: 4, to_acknowledge: 1 },
};

const planning = {
  as_of: "2026-10-16T08:00:00Z",
  backlog: {
    candidates: 12,
    ready: 8,
    blocked: 4,
    blocked_by: [{ dimension: "parts", count: 3 }],
    truncated: false,
  },
  week: {
    week_start: "2026-10-12",
    week_end: "2026-10-18",
    basis: { rest_days: [6, 7], scope: "all_sites", site_name: null },
    teams: 2,
    capacity_hours: 160,
    committed_hours: 120,
    load_pct: 75,
    overloaded_team_days: 1,
  },
  week_unavailable: null,
  schedule_compliance: { period_days: 7, scheduled_hours: 0, done_hours: 0, value_pct: null },
};

const supervision = {
  as_of: "2026-10-16T08:00:00Z",
  requests: {
    sla_open: { on_track: 4, at_risk: 1, breached: 1, paused: 1, no_target: 0 },
    period_days: 30,
    sla_closing: { met: 9, breached: 1, total: 10 },
    queues,
  },
  work_orders: {
    open: 6,
    overdue: 2,
    planned: 3,
    reactive: 3,
    emergency: 1,
    without_nature: 0,
    cells: [{ nature_code: "corrective_immediate", urgency_code: "emergency", count: 1 }],
  },
  pm: {
    overdue: 1,
    due_today: 0,
    due_next_7_days: 2,
    period_days: 30,
    compliance_done_on_time: 3,
    compliance_due: 4,
    compliance_pct: 75,
  },
  stock: {
    lines_to_reorder: 2,
    articles_to_reorder: 1,
    lines_out_of_stock: 1,
    items: [
      {
        article_id: 5,
        article_code: "BRG-6204",
        article_name: "Bearing",
        warehouse_code: "MAIN",
        available_qty: 0,
        reorder_point: 4,
        min_stock: 2,
        trigger_type: "reorder_point",
      },
    ],
  },
  safety: {
    open_permits: 2,
    expiring_rule: "disabled",
    expiring_lead_minutes: null,
    expiring: { total: 0, items: [], without_record: 0 },
    expired_open: {
      total: 1,
      items: [
        {
          permit_id: 3,
          code: "PTW-3",
          permit_type: "Hot work",
          asset_code: "P-101",
          expires_at: "2026-10-15T18:00:00Z",
        },
      ],
      without_record: 0,
    },
    locks_in_place: 4,
    permits_with_locks: 1,
  },
  assets: {
    critical_down: 1,
    criticality_label: "Criticité A",
    down_status_labels: ["Hors service", "En maintenance"],
    items: [{ id: 2, asset_code: "P-101", name: "Feed pump", status_label: "Hors service" }],
  },
};

describe("home service", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("offers only the periods the backend accepts", () => {
    expect([...HOME_PERIODS]).toEqual([7, 30]);
    expect(isHomePeriod(7)).toBe(true);
    expect(isHomePeriod(30)).toBe(true);
    expect(isHomePeriod(14)).toBe(false);
  });

  it("reads my work, every section present", async () => {
    mockInvoke.mockResolvedValueOnce(myWork);
    await expect(getHomeMyWork()).resolves.toEqual(myWork);
    expect(mockInvoke).toHaveBeenCalledWith("get_home_my_work", undefined);
  });

  it("keeps the sections of modules the user may not open as null", async () => {
    const restricted = { ...myWork, work_orders: null, requests: null, rounds: null, pm: null };
    mockInvoke.mockResolvedValueOnce(restricted);
    const w = await getHomeMyWork();
    expect(w.work_orders).toBeNull();
    expect(w.pm).toBeNull();
    expect(w.notifications.to_acknowledge).toBe(1);
  });

  it("refuses a malformed my-work answer instead of showing 0", async () => {
    mockInvoke.mockResolvedValueOnce({ ...myWork, notifications: { unread: "4" } });
    await expect(getHomeMyWork()).rejects.toThrow();
  });

  it("keeps a missing work week as its coded reason, never a guessed week", async () => {
    const noWeek = { ...planning, week: null, week_unavailable: "WORK_WEEK_NOT_CONFIGURED" };
    mockInvoke.mockResolvedValueOnce(noWeek);
    await expect(getHomePlanning(7)).resolves.toEqual(noWeek);
    mockInvoke.mockResolvedValueOnce({ ...planning, week: null, week_unavailable: "MONDAY" });
    await expect(getHomePlanning(7)).rejects.toThrow();
  });

  it("reads the planner panel for the chosen period", async () => {
    mockInvoke.mockResolvedValueOnce(planning);
    await expect(getHomePlanning(7)).resolves.toEqual(planning);
    expect(mockInvoke).toHaveBeenCalledWith("get_home_planning", { periodDays: 7 });
    mockInvoke.mockResolvedValueOnce({ ...planning, week: { ...planning.week, load_pct: "75" } });
    await expect(getHomePlanning(30)).rejects.toThrow();
  });

  it("reads the supervision panel and refuses a malformed one", async () => {
    mockInvoke.mockResolvedValueOnce(supervision);
    await expect(getHomeSupervision(30)).resolves.toEqual(supervision);
    expect(mockInvoke).toHaveBeenCalledWith("get_home_supervision", { periodDays: 30 });

    const empty = {
      as_of: supervision.as_of,
      requests: null,
      work_orders: null,
      pm: null,
      stock: null,
      safety: null,
      assets: null,
    };
    mockInvoke.mockResolvedValueOnce(empty);
    await expect(getHomeSupervision(7)).resolves.toEqual(empty);

    mockInvoke.mockResolvedValueOnce({ ...supervision, assets: { critical_down: 1 } });
    await expect(getHomeSupervision(7)).rejects.toThrow();
  });
});

describe("dashboard service", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("reads the activity chart with hidden series as null", async () => {
    const chart = {
      days: [{ date: "2026-10-16", di_created: 2, wo_completed: null, pm_due: 0 }],
      period_days: 7,
      quality_badge: null,
    };
    mockInvoke.mockResolvedValueOnce(chart);
    await expect(getDashboardWorkloadChart(7)).resolves.toEqual(chart);
    expect(mockInvoke).toHaveBeenCalledWith("get_dashboard_workload_chart", { periodDays: 7 });
    mockInvoke.mockResolvedValueOnce({ ...chart, days: [{ date: "2026-10-16" }] });
    await expect(getDashboardWorkloadChart(7)).rejects.toThrow();
  });

  it("reads open requests by status", async () => {
    const chart = {
      segments: [{ status: "submitted", count: 3, avg_age_days: 1.5 }],
      available: true,
    };
    mockInvoke.mockResolvedValueOnce(chart);
    await expect(getDashboardDiStatusChart()).resolves.toEqual(chart);
    mockInvoke.mockResolvedValueOnce({
      segments: [{ status: "submitted", count: 3 }],
      available: true,
    });
    await expect(getDashboardDiStatusChart()).rejects.toThrow();
  });

  it("reads and saves the layouts", async () => {
    mockInvoke.mockResolvedValueOnce({ layout_json: "{}" });
    await expect(getDashboardLayout()).resolves.toEqual({ layout_json: "{}" });
    mockInvoke.mockResolvedValueOnce({ layout_json: '{"version":1}' });
    await expect(saveDashboardLayout('{"version":1}')).resolves.toEqual({
      layout_json: '{"version":1}',
    });
    expect(mockInvoke).toHaveBeenCalledWith("save_dashboard_layout", {
      input: { layout_json: '{"version":1}' },
    });
    mockInvoke.mockResolvedValueOnce({ layout_json: "{}" });
    await expect(getOverviewKpiLayout()).resolves.toEqual({ layout_json: "{}" });
    mockInvoke.mockResolvedValueOnce(undefined);
    await expect(saveOverviewKpiLayout({ overview_kpis_json: "[]" })).resolves.toBeUndefined();
    expect(mockInvoke).toHaveBeenCalledWith("save_overview_kpi_layout", {
      input: { overview_kpis_json: "[]" },
    });
  });

  it("reads the indicator strip and refuses a malformed cell", async () => {
    const payload = {
      horizon_days: 30,
      cells: [
        {
          semantic_key: "DI_OPEN",
          catalog_id: "KPI-038",
          value: 3,
          previous_value: 1,
          available: true,
          baseline_available: true,
          quality_badge: null,
          value_kind: "count",
          trend_sense: "up-bad",
        },
      ],
    };
    mockInvoke.mockResolvedValueOnce(payload);
    await expect(getOverviewKpis()).resolves.toEqual(payload);
    mockInvoke.mockResolvedValueOnce({ horizon_days: 30, cells: [{ semantic_key: "DI_OPEN" }] });
    await expect(getOverviewKpis()).rejects.toThrow();
  });
});
