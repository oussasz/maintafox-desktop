/**
 * Behaviour tests for planning-store (scheduling backlog, commitments, capacity, gantt).
 * Only the planning-service boundary is mocked (plus the browser Blob URL API,
 * which jsdom does not implement, for the PDF download path).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createCapacityRule,
  createPlanningWindow,
  createScheduleBreakIn,
  createScheduleCommitment,
  exportPlanningGanttPdf,
  freezeSchedulePeriod,
  getPlanningGanttSnapshot,
  getScheduleBacklogSnapshot,
  listCapacityRules,
  listPlanningWindows,
  listScheduleBreakIns,
  listScheduleChangeLog,
  listScheduleCommitments,
  listTeamCapacityLoad,
  notifyScheduleTeams,
  refreshScheduleCandidates,
  rescheduleScheduleCommitment,
} from "@/services/planning-service";
import type {
  CapacityRule,
  CreateCapacityRuleInput,
  CreatePlanningWindowInput,
  CreateScheduleBreakInInput,
  CreateScheduleCommitmentInput,
  PlanningGanttSnapshot,
  PlanningWindow,
  RefreshScheduleCandidatesResult,
  RescheduleCommitmentInput,
  ScheduleBacklogSnapshot,
  ScheduleBreakIn,
  ScheduleChangeLogEntry,
  ScheduleCommitment,
  TeamCapacityLoad,
} from "@shared/ipc-types";

import { usePlanningStore } from "../planning-store";

vi.mock("@/services/planning-service", () => ({
  createCapacityRule: vi.fn(),
  createPlanningWindow: vi.fn(),
  createScheduleBreakIn: vi.fn(),
  createScheduleCommitment: vi.fn(),
  exportPlanningGanttPdf: vi.fn(),
  freezeSchedulePeriod: vi.fn(),
  getPlanningGanttSnapshot: vi.fn(),
  getScheduleBacklogSnapshot: vi.fn(),
  listCapacityRules: vi.fn(),
  listPlanningWindows: vi.fn(),
  listScheduleBreakIns: vi.fn(),
  listScheduleChangeLog: vi.fn(),
  listScheduleCommitments: vi.fn(),
  listTeamCapacityLoad: vi.fn(),
  notifyScheduleTeams: vi.fn(),
  refreshScheduleCandidates: vi.fn(),
  rescheduleScheduleCommitment: vi.fn(),
}));

const TS = "2026-10-01T08:00:00Z";

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeBacklog(overrides: Partial<ScheduleBacklogSnapshot> = {}): ScheduleBacklogSnapshot {
  return {
    as_of: TS,
    candidate_count: 0,
    ready_count: 0,
    blocked_count: 0,
    candidates: [],
    conflict_summary: [],
    derivation_rules: [],
    ...overrides,
  };
}

function makeCommitment(overrides: Partial<ScheduleCommitment> = {}): ScheduleCommitment {
  return {
    id: 1,
    schedule_candidate_id: 5,
    source_type: "work_order",
    source_id: 77,
    schedule_period_start: "2026-10-01",
    schedule_period_end: "2026-10-07",
    committed_start: "2026-10-02T08:00:00Z",
    committed_end: "2026-10-02T12:00:00Z",
    assigned_team_id: 3,
    assigned_personnel_id: null,
    committed_by_id: 1,
    frozen_at: null,
    estimated_labor_cost: null,
    budget_threshold: null,
    cost_variance_warning: 0,
    has_blocking_conflict: 0,
    nearest_feasible_window: null,
    planned_labor_hours: 2,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    source_code: "WO-0077",
    source_title: "Replace pump seal",
    assigned_team_label: "MECH",
    assigned_personnel_name: null,
    ...overrides,
  };
}

function makeCapacityRule(overrides: Partial<CapacityRule> = {}): CapacityRule {
  return {
    id: 2,
    entity_id: null,
    team_id: 3,
    effective_start: "2026-10-01",
    effective_end: null,
    available_hours_per_day: 8,
    max_overtime_hours_per_day: 2,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    ...overrides,
  };
}

function makeWindow(overrides: Partial<PlanningWindow> = {}): PlanningWindow {
  return {
    id: 4,
    entity_id: null,
    window_type: "shutdown",
    start_datetime: "2026-10-10T00:00:00Z",
    end_datetime: "2026-10-12T00:00:00Z",
    is_locked: 1,
    lock_reason: "Plant shutdown",
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    ...overrides,
  };
}

function makeLoad(overrides: Partial<TeamCapacityLoad> = {}): TeamCapacityLoad {
  return {
    team_id: 3,
    work_date: "2026-10-02",
    available_hours: 8,
    overtime_hours: 0,
    committed_hours: 4,
    utilization_ratio: 0.5,
    basis: "roster",
    work_windows: [{ start: "2026-10-02T08:00:00", end: "2026-10-02T16:00:00", hours: 8 }],
    unwindowed_hours: 0,
    ...overrides,
  };
}

function makeBreakIn(overrides: Partial<ScheduleBreakIn> = {}): ScheduleBreakIn {
  return {
    id: 6,
    schedule_commitment_id: 1,
    break_in_reason: "emergency",
    approved_by_user_id: 1,
    approved_by_personnel_id: null,
    override_reason: null,
    old_slot_start: "2026-10-02T08:00:00Z",
    old_slot_end: "2026-10-02T12:00:00Z",
    new_slot_start: "2026-10-03T08:00:00Z",
    new_slot_end: "2026-10-03T12:00:00Z",
    old_assignee_id: null,
    new_assignee_id: null,
    cost_impact_delta: null,
    notification_dedupe_key: null,
    row_version: 1,
    created_by_id: 1,
    created_at: TS,
    commitment_source_code: "WO-0077",
    approved_by_display_name: "Admin",
    old_assignee_name: null,
    new_assignee_name: null,
    ...overrides,
  };
}

function makeChangeLog(overrides: Partial<ScheduleChangeLogEntry> = {}): ScheduleChangeLogEntry {
  return {
    id: 8,
    commitment_id: 1,
    action_type: "reschedule",
    actor_id: 1,
    field_changed: "committed_start",
    old_value: "a",
    new_value: "b",
    reason_code: null,
    reason_note: null,
    reason: null,
    details_json: null,
    created_at: TS,
    actor_display_name: "Admin",
    ...overrides,
  };
}

function makeGantt(overrides: Partial<PlanningGanttSnapshot> = {}): PlanningGanttSnapshot {
  return {
    period_start: "2026-10-01",
    period_end: "2026-10-07",
    commitments: [makeCommitment()],
    locked_windows: [makeWindow()],
    capacity: [makeLoad()],
    assignee_lanes: [],
    crew_lanes: [],
    ...overrides,
  };
}

const capacityRuleInput: CreateCapacityRuleInput = {
  team_id: 3,
  effective_start: "2026-10-01",
  available_hours_per_day: 8,
  max_overtime_hours_per_day: 2,
};

const windowInput: CreatePlanningWindowInput = {
  window_type: "shutdown",
  start_datetime: "2026-10-10T00:00:00Z",
  end_datetime: "2026-10-12T00:00:00Z",
  is_locked: true,
  lock_reason: "Plant shutdown",
};

const commitmentInput: CreateScheduleCommitmentInput = {
  schedule_candidate_id: 5,
  expected_candidate_row_version: 1,
  committed_start: "2026-10-02T08:00:00Z",
  committed_end: "2026-10-02T12:00:00Z",
  assigned_team_id: 3,
};

const rescheduleInput: RescheduleCommitmentInput = {
  commitment_id: 1,
  expected_row_version: 1,
  committed_start: "2026-10-03T08:00:00Z",
  committed_end: "2026-10-03T12:00:00Z",
  assigned_team_id: 3,
};

const breakInInput: CreateScheduleBreakInInput = {
  schedule_commitment_id: 1,
  expected_commitment_row_version: 1,
  break_in_reason: "emergency",
  new_slot_start: "2026-10-03T08:00:00Z",
  new_slot_end: "2026-10-03T12:00:00Z",
};

function deferred<T>() {
  let resolve: (v: T) => void = () => undefined;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

// ── Setup ─────────────────────────────────────────────────────────────────────

const initial = usePlanningStore.getState();

beforeEach(() => {
  vi.resetAllMocks();
  usePlanningStore.setState(initial, true);
});

describe("planning-store", () => {
  it("starts empty", () => {
    const s = usePlanningStore.getState();
    expect(s.backlog).toBeNull();
    expect(s.commitments).toEqual([]);
    expect(s.capacityRules).toEqual([]);
    expect(s.planningWindows).toEqual([]);
    expect(s.breakIns).toEqual([]);
    expect(s.capacityLoad).toEqual([]);
    expect(s.ganttSnapshot).toBeNull();
    expect(s.changeLog).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.saving).toBe(false);
    expect(s.error).toBeNull();
  });

  // ── Backlog ───────────────────────────────────────────────────────────────

  describe("loadBacklog", () => {
    it("requests 300 candidates including resolved conflicts and stores the snapshot", async () => {
      const d = deferred<ScheduleBacklogSnapshot>();
      vi.mocked(getScheduleBacklogSnapshot).mockReturnValueOnce(d.promise);
      usePlanningStore.setState({ error: "old" });

      const p = usePlanningStore.getState().loadBacklog();
      expect(usePlanningStore.getState().loading).toBe(true);
      expect(usePlanningStore.getState().error).toBeNull();

      const backlog = makeBacklog({ candidate_count: 3, ready_count: 2, blocked_count: 1 });
      d.resolve(backlog);
      await p;

      expect(getScheduleBacklogSnapshot).toHaveBeenCalledWith({
        limit: 300,
        include_resolved_conflicts: true,
      });
      expect(usePlanningStore.getState().backlog).toEqual(backlog);
      expect(usePlanningStore.getState().loading).toBe(false);
    });

    it("swallows errors and sets the message", async () => {
      vi.mocked(getScheduleBacklogSnapshot).mockRejectedValueOnce("backlog boom");

      await expect(usePlanningStore.getState().loadBacklog()).resolves.toBeUndefined();

      expect(usePlanningStore.getState().error).toBe("backlog boom");
      expect(usePlanningStore.getState().backlog).toBeNull();
      expect(usePlanningStore.getState().loading).toBe(false);
    });
  });

  describe("refreshBacklog", () => {
    it("refreshes candidates from all sources then reloads the backlog", async () => {
      const d = deferred<RefreshScheduleCandidatesResult>();
      vi.mocked(refreshScheduleCandidates).mockReturnValueOnce(d.promise);
      const backlog = makeBacklog({ candidate_count: 1 });
      vi.mocked(getScheduleBacklogSnapshot).mockResolvedValueOnce(backlog);

      const p = usePlanningStore.getState().refreshBacklog();
      expect(usePlanningStore.getState().saving).toBe(true);
      expect(getScheduleBacklogSnapshot).not.toHaveBeenCalled();

      d.resolve({
        inserted_count: 1,
        updated_count: 0,
        evaluated_count: 1,
        ready_count: 1,
        blocked_count: 0,
      });
      await p;

      expect(refreshScheduleCandidates).toHaveBeenCalledWith({
        include_work_orders: true,
        include_pm_occurrences: true,
        include_approved_di: true,
        limit_per_source: 300,
      });
      expect(getScheduleBacklogSnapshot).toHaveBeenCalledTimes(1);
      expect(usePlanningStore.getState().backlog).toEqual(backlog);
      expect(usePlanningStore.getState().saving).toBe(false);
    });

    it("sets error, rethrows and does not reload when the refresh fails", async () => {
      const err = new Error("refresh failed");
      vi.mocked(refreshScheduleCandidates).mockRejectedValueOnce(err);

      await expect(usePlanningStore.getState().refreshBacklog()).rejects.toBe(err);

      expect(getScheduleBacklogSnapshot).not.toHaveBeenCalled();
      expect(usePlanningStore.getState().error).toBe("refresh failed");
      expect(usePlanningStore.getState().saving).toBe(false);
    });
  });

  // ── List loaders ──────────────────────────────────────────────────────────

  describe("loadCommitments", () => {
    it("passes the filter and stores the commitments", async () => {
      const rows = [makeCommitment()];
      vi.mocked(listScheduleCommitments).mockResolvedValueOnce(rows);
      const filter = { period_start: "2026-10-01", period_end: "2026-10-07", team_id: 3 };

      await usePlanningStore.getState().loadCommitments(filter);

      expect(listScheduleCommitments).toHaveBeenCalledWith(filter);
      expect(usePlanningStore.getState().commitments).toEqual(rows);
      expect(usePlanningStore.getState().loading).toBe(false);
    });

    it("swallows errors", async () => {
      vi.mocked(listScheduleCommitments).mockRejectedValueOnce(new Error("nope"));
      await usePlanningStore.getState().loadCommitments({});
      expect(usePlanningStore.getState().error).toBe("nope");
      expect(usePlanningStore.getState().loading).toBe(false);
    });
  });

  describe("loadCapacityRules", () => {
    it("defaults the filter to {}", async () => {
      vi.mocked(listCapacityRules).mockResolvedValueOnce([makeCapacityRule()]);
      await usePlanningStore.getState().loadCapacityRules();
      expect(listCapacityRules).toHaveBeenCalledWith({});
      expect(usePlanningStore.getState().capacityRules).toEqual([makeCapacityRule()]);
    });

    it("forwards an explicit filter", async () => {
      vi.mocked(listCapacityRules).mockResolvedValueOnce([]);
      await usePlanningStore.getState().loadCapacityRules({ team_id: 3 });
      expect(listCapacityRules).toHaveBeenCalledWith({ team_id: 3 });
    });

    it("swallows errors", async () => {
      vi.mocked(listCapacityRules).mockRejectedValueOnce(new Error("rules failed"));
      await usePlanningStore.getState().loadCapacityRules();
      expect(usePlanningStore.getState().error).toBe("rules failed");
      expect(usePlanningStore.getState().loading).toBe(false);
    });
  });

  describe("loadPlanningWindows", () => {
    it("defaults the filter to {} and stores windows", async () => {
      vi.mocked(listPlanningWindows).mockResolvedValueOnce([makeWindow()]);
      await usePlanningStore.getState().loadPlanningWindows();
      expect(listPlanningWindows).toHaveBeenCalledWith({});
      expect(usePlanningStore.getState().planningWindows).toEqual([makeWindow()]);
    });

    it("swallows errors", async () => {
      vi.mocked(listPlanningWindows).mockRejectedValueOnce(new Error("windows failed"));
      await usePlanningStore.getState().loadPlanningWindows({ include_locked: true });
      expect(listPlanningWindows).toHaveBeenCalledWith({ include_locked: true });
      expect(usePlanningStore.getState().error).toBe("windows failed");
    });
  });

  describe("loadCapacityLoad", () => {
    it("passes period and team id and stores the load", async () => {
      vi.mocked(listTeamCapacityLoad).mockResolvedValueOnce([makeLoad()]);
      await usePlanningStore.getState().loadCapacityLoad("2026-10-01", "2026-10-07", 3);
      expect(listTeamCapacityLoad).toHaveBeenCalledWith("2026-10-01", "2026-10-07", 3);
      expect(usePlanningStore.getState().capacityLoad).toEqual([makeLoad()]);
    });

    it("passes undefined team id through when omitted", async () => {
      vi.mocked(listTeamCapacityLoad).mockResolvedValueOnce([]);
      await usePlanningStore.getState().loadCapacityLoad("2026-10-01", "2026-10-07");
      expect(listTeamCapacityLoad).toHaveBeenCalledWith("2026-10-01", "2026-10-07", undefined);
    });

    it("swallows errors", async () => {
      vi.mocked(listTeamCapacityLoad).mockRejectedValueOnce(new Error("load failed"));
      await usePlanningStore.getState().loadCapacityLoad("a", "b", null);
      expect(usePlanningStore.getState().error).toBe("load failed");
      expect(usePlanningStore.getState().loading).toBe(false);
    });
  });

  describe("loadGantt", () => {
    it("stores the snapshot and mirrors commitments and capacity from it", async () => {
      const snap = makeGantt();
      const d = deferred<PlanningGanttSnapshot>();
      vi.mocked(getPlanningGanttSnapshot).mockReturnValueOnce(d.promise);
      const filter = { period_start: "2026-10-01", period_end: "2026-10-07", team_id: null };

      const p = usePlanningStore.getState().loadGantt(filter);
      expect(usePlanningStore.getState().loading).toBe(true);
      d.resolve(snap);
      await p;

      expect(getPlanningGanttSnapshot).toHaveBeenCalledWith(filter);
      const s = usePlanningStore.getState();
      expect(s.ganttSnapshot).toEqual(snap);
      expect(s.commitments).toBe(snap.commitments);
      expect(s.capacityLoad).toBe(snap.capacity);
      expect(s.loading).toBe(false);
    });

    it("swallows errors and leaves previous data", async () => {
      const previous = [makeCommitment({ id: 42 })];
      usePlanningStore.setState({ commitments: previous });
      vi.mocked(getPlanningGanttSnapshot).mockRejectedValueOnce(new Error("gantt failed"));

      await usePlanningStore.getState().loadGantt({ period_start: "a", period_end: "b" });

      const s = usePlanningStore.getState();
      expect(s.error).toBe("gantt failed");
      expect(s.commitments).toBe(previous);
      expect(s.ganttSnapshot).toBeNull();
    });
  });

  describe("loadChangeLog", () => {
    it("passes the commitment id and stores entries", async () => {
      vi.mocked(listScheduleChangeLog).mockResolvedValueOnce([makeChangeLog()]);
      await usePlanningStore.getState().loadChangeLog(1);
      expect(listScheduleChangeLog).toHaveBeenCalledWith(1);
      expect(usePlanningStore.getState().changeLog).toEqual([makeChangeLog()]);
    });

    it("swallows errors", async () => {
      vi.mocked(listScheduleChangeLog).mockRejectedValueOnce(new Error("log failed"));
      await usePlanningStore.getState().loadChangeLog();
      expect(listScheduleChangeLog).toHaveBeenCalledWith(undefined);
      expect(usePlanningStore.getState().error).toBe("log failed");
    });
  });

  describe("loadBreakIns", () => {
    it("defaults the filter to {} and stores break-ins", async () => {
      vi.mocked(listScheduleBreakIns).mockResolvedValueOnce([makeBreakIn()]);
      await usePlanningStore.getState().loadBreakIns();
      expect(listScheduleBreakIns).toHaveBeenCalledWith({});
      expect(usePlanningStore.getState().breakIns).toEqual([makeBreakIn()]);
    });

    it("swallows errors", async () => {
      vi.mocked(listScheduleBreakIns).mockRejectedValueOnce(new Error("breakins failed"));
      await usePlanningStore.getState().loadBreakIns({ break_in_reason: "emergency" });
      expect(listScheduleBreakIns).toHaveBeenCalledWith({ break_in_reason: "emergency" });
      expect(usePlanningStore.getState().error).toBe("breakins failed");
      expect(usePlanningStore.getState().loading).toBe(false);
    });
  });

  // ── Mutations ─────────────────────────────────────────────────────────────

  describe("createCapacityRule", () => {
    it("creates, reloads all rules with {} and returns the created rule", async () => {
      const created = makeCapacityRule({ id: 50 });
      vi.mocked(createCapacityRule).mockResolvedValueOnce(created);
      vi.mocked(listCapacityRules).mockResolvedValueOnce([created]);

      const result = await usePlanningStore.getState().createCapacityRule(capacityRuleInput);

      expect(result).toBe(created);
      expect(createCapacityRule).toHaveBeenCalledWith(capacityRuleInput);
      expect(listCapacityRules).toHaveBeenCalledWith({});
      expect(usePlanningStore.getState().capacityRules).toEqual([created]);
      expect(usePlanningStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure", async () => {
      const err = { message: "overlapping rule" };
      vi.mocked(createCapacityRule).mockRejectedValueOnce(err);

      await expect(usePlanningStore.getState().createCapacityRule(capacityRuleInput)).rejects.toBe(
        err,
      );

      expect(listCapacityRules).not.toHaveBeenCalled();
      expect(usePlanningStore.getState().error).toBe("overlapping rule");
      expect(usePlanningStore.getState().saving).toBe(false);
    });
  });

  describe("createPlanningWindow", () => {
    it("creates, reloads windows with {} and returns the created window", async () => {
      const created = makeWindow({ id: 60 });
      vi.mocked(createPlanningWindow).mockResolvedValueOnce(created);
      vi.mocked(listPlanningWindows).mockResolvedValueOnce([created]);

      const result = await usePlanningStore.getState().createPlanningWindow(windowInput);

      expect(result).toBe(created);
      expect(createPlanningWindow).toHaveBeenCalledWith(windowInput);
      expect(listPlanningWindows).toHaveBeenCalledWith({});
      expect(usePlanningStore.getState().planningWindows).toEqual([created]);
    });

    it("sets error and rethrows on failure", async () => {
      vi.mocked(createPlanningWindow).mockRejectedValueOnce(new Error("bad window"));
      await expect(usePlanningStore.getState().createPlanningWindow(windowInput)).rejects.toThrow(
        "bad window",
      );
      expect(usePlanningStore.getState().error).toBe("bad window");
      expect(usePlanningStore.getState().saving).toBe(false);
    });
  });

  describe.each([
    {
      name: "createCommitment",
      svc: createScheduleCommitment,
      run: () => usePlanningStore.getState().createCommitment(commitmentInput),
      input: commitmentInput,
      result: makeCommitment({ id: 70 }),
    },
    {
      name: "rescheduleCommitment",
      svc: rescheduleScheduleCommitment,
      run: () => usePlanningStore.getState().rescheduleCommitment(rescheduleInput),
      input: rescheduleInput,
      result: makeCommitment({ id: 1, row_version: 2 }),
    },
    {
      name: "createBreakIn",
      svc: createScheduleBreakIn,
      run: () => usePlanningStore.getState().createBreakIn(breakInInput),
      input: breakInInput,
      result: makeBreakIn({ id: 80 }),
    },
  ])("$name", ({ svc, run, input, result }) => {
    it("toggles saving, forwards the input and returns the service result without touching lists", async () => {
      const mocked = vi.mocked(svc as (i: unknown) => Promise<unknown>);
      const d = deferred<unknown>();
      mocked.mockReturnValueOnce(d.promise);

      const p = run();
      expect(usePlanningStore.getState().saving).toBe(true);
      d.resolve(result);

      await expect(p).resolves.toBe(result);
      expect(mocked).toHaveBeenCalledWith(input);
      expect(usePlanningStore.getState().saving).toBe(false);
      expect(usePlanningStore.getState().commitments).toEqual([]);
      expect(usePlanningStore.getState().breakIns).toEqual([]);
    });

    it("sets error and rethrows on failure", async () => {
      const mocked = vi.mocked(svc as (i: unknown) => Promise<unknown>);
      mocked.mockRejectedValueOnce(new Error("conflict"));

      await expect(run()).rejects.toThrow("conflict");
      expect(usePlanningStore.getState().error).toBe("conflict");
      expect(usePlanningStore.getState().saving).toBe(false);
    });
  });

  describe("freezeSchedule", () => {
    it("returns the frozen count", async () => {
      vi.mocked(freezeSchedulePeriod).mockResolvedValueOnce(4);
      const input = { period_start: "2026-10-01", period_end: "2026-10-07", reason: "lock" };

      await expect(usePlanningStore.getState().freezeSchedule(input)).resolves.toBe(4);
      expect(freezeSchedulePeriod).toHaveBeenCalledWith(input);
      expect(usePlanningStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure", async () => {
      vi.mocked(freezeSchedulePeriod).mockRejectedValueOnce(new Error("freeze failed"));
      await expect(
        usePlanningStore.getState().freezeSchedule({ period_start: "a", period_end: "b" }),
      ).rejects.toThrow("freeze failed");
      expect(usePlanningStore.getState().error).toBe("freeze failed");
    });
  });

  describe("notifyTeams", () => {
    it("returns the notification result", async () => {
      const res = { emitted_count: 3, skipped_count: 1 };
      vi.mocked(notifyScheduleTeams).mockResolvedValueOnce(res);
      const input = { period_start: "a", period_end: "b", team_id: 3, include_break_ins: true };

      await expect(usePlanningStore.getState().notifyTeams(input)).resolves.toEqual(res);
      expect(notifyScheduleTeams).toHaveBeenCalledWith(input);
      expect(usePlanningStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure", async () => {
      vi.mocked(notifyScheduleTeams).mockRejectedValueOnce(new Error("notify failed"));
      await expect(
        usePlanningStore.getState().notifyTeams({ period_start: "a", period_end: "b" }),
      ).rejects.toThrow("notify failed");
      expect(usePlanningStore.getState().error).toBe("notify failed");
      expect(usePlanningStore.getState().saving).toBe(false);
    });
  });

  describe("exportGanttPdf", () => {
    const originalCreate = URL.createObjectURL;
    const originalRevoke = URL.revokeObjectURL;
    const createObjectURL = vi.fn<(b: Blob) => string>();
    const revokeObjectURL = vi.fn<(u: string) => void>();

    beforeEach(() => {
      // The top-level beforeEach resets every mock, so re-arm the stubs here.
      createObjectURL.mockReset().mockReturnValue("blob:gantt");
      revokeObjectURL.mockReset();
      URL.createObjectURL = createObjectURL;
      URL.revokeObjectURL = revokeObjectURL;
    });

    afterEach(() => {
      URL.createObjectURL = originalCreate;
      URL.revokeObjectURL = originalRevoke;
      vi.restoreAllMocks();
    });

    it("requests an A3 PDF (team id null when omitted) and downloads it via a temporary link", async () => {
      vi.mocked(exportPlanningGanttPdf).mockResolvedValueOnce({
        file_name: "gantt.pdf",
        mime_type: "application/pdf",
        bytes: [37, 80, 68, 70],
      });
      const clickSpy = vi
        .spyOn(HTMLAnchorElement.prototype, "click")
        .mockImplementation(() => undefined);

      await usePlanningStore.getState().exportGanttPdf("2026-10-01", "2026-10-07");

      expect(exportPlanningGanttPdf).toHaveBeenCalledWith({
        period_start: "2026-10-01",
        period_end: "2026-10-07",
        team_id: null,
        paper_size: "A3",
      });
      expect(createObjectURL).toHaveBeenCalledTimes(1);
      const blob = createObjectURL.mock.calls[0]?.[0];
      expect(blob).toBeInstanceOf(Blob);
      expect(blob?.type).toBe("application/pdf");
      expect(blob?.size).toBe(4);
      expect(clickSpy).toHaveBeenCalledTimes(1);
      const anchor = clickSpy.mock.contexts[0];
      expect(anchor).toBeInstanceOf(HTMLAnchorElement);
      if (anchor instanceof HTMLAnchorElement) {
        expect(anchor.download).toBe("gantt.pdf");
        expect(anchor.href).toBe("blob:gantt");
        // Temporary link removed from the DOM afterwards.
        expect(anchor.isConnected).toBe(false);
      }
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:gantt");
      expect(usePlanningStore.getState().saving).toBe(false);
      expect(usePlanningStore.getState().error).toBeNull();
    });

    it("forwards an explicit team id", async () => {
      vi.mocked(exportPlanningGanttPdf).mockResolvedValueOnce({
        file_name: "g.pdf",
        mime_type: "application/pdf",
        bytes: [],
      });
      vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);

      await usePlanningStore.getState().exportGanttPdf("a", "b", 3);

      expect(exportPlanningGanttPdf).toHaveBeenCalledWith({
        period_start: "a",
        period_end: "b",
        team_id: 3,
        paper_size: "A3",
      });
    });

    it("sets error, rethrows and does not download on failure", async () => {
      vi.mocked(exportPlanningGanttPdf).mockRejectedValueOnce(new Error("pdf failed"));

      await expect(usePlanningStore.getState().exportGanttPdf("a", "b")).rejects.toThrow(
        "pdf failed",
      );

      expect(createObjectURL).not.toHaveBeenCalled();
      expect(usePlanningStore.getState().error).toBe("pdf failed");
      expect(usePlanningStore.getState().saving).toBe(false);
    });
  });
});
