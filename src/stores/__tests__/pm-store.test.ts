/**
 * pm-store — behaviour tests for the preventive-maintenance workspace store.
 *
 * Covers workspace loading, plan detail open/close, every list loader, the
 * mutations (with their refresh cascades), loading/saving flags, error capture
 * and rethrow semantics.
 */

import { describe, it, expect, beforeEach, vi } from "vitest";

import {
  createPmPlan,
  createPmPlanVersion,
  deletePmPlan,
  deletePmPlanVersion,
  executePmOccurrence,
  generatePmOccurrences,
  getPmDueMetrics,
  getPmGenerationStatus,
  getPmGovernanceKpiReport,
  listPmExecutions,
  listPmFindings,
  listPmOccurrences,
  listPmPlanVersions,
  listPmPlans,
  listPmPlanningReadiness,
  listPmRecurringFindings,
  publishPmPlanVersion,
  transitionPmOccurrence,
  transitionPmPlanLifecycle,
  updatePmGenerationSettings,
  updatePmPlan,
  updatePmPlanVersion,
} from "@/services/pm-service";
import type {
  CreatePmPlanInput,
  CreatePmPlanVersionInput,
  ExecutePmOccurrenceResult,
  GeneratePmOccurrencesResult,
  PmDueMetrics,
  PmExecution,
  PmFinding,
  PmGenerationStatus,
  PmGovernanceKpiReport,
  PmOccurrence,
  PmPlan,
  PmPlanVersion,
  PmPlanningReadinessProjection,
  PmRecurringFinding,
} from "@shared/ipc-types";

import { usePmStore } from "../pm-store";

vi.mock("@/services/pm-service", () => ({
  createPmPlan: vi.fn(),
  createPmPlanVersion: vi.fn(),
  deletePmPlan: vi.fn(),
  deletePmPlanVersion: vi.fn(),
  executePmOccurrence: vi.fn(),
  generatePmOccurrences: vi.fn(),
  getPmDueMetrics: vi.fn(),
  getPmGenerationStatus: vi.fn(),
  getPmGovernanceKpiReport: vi.fn(),
  listPmExecutions: vi.fn(),
  listPmFindings: vi.fn(),
  listPmOccurrences: vi.fn(),
  listPmPlanVersions: vi.fn(),
  listPmPlans: vi.fn(),
  listPmPlanningReadiness: vi.fn(),
  listPmRecurringFindings: vi.fn(),
  publishPmPlanVersion: vi.fn(),
  transitionPmOccurrence: vi.fn(),
  transitionPmPlanLifecycle: vi.fn(),
  updatePmGenerationSettings: vi.fn(),
  updatePmPlan: vi.fn(),
  updatePmPlanVersion: vi.fn(),
}));

const m = {
  createPmPlan: vi.mocked(createPmPlan),
  createPmPlanVersion: vi.mocked(createPmPlanVersion),
  deletePmPlan: vi.mocked(deletePmPlan),
  deletePmPlanVersion: vi.mocked(deletePmPlanVersion),
  executePmOccurrence: vi.mocked(executePmOccurrence),
  generatePmOccurrences: vi.mocked(generatePmOccurrences),
  getPmDueMetrics: vi.mocked(getPmDueMetrics),
  getPmGenerationStatus: vi.mocked(getPmGenerationStatus),
  getPmGovernanceKpiReport: vi.mocked(getPmGovernanceKpiReport),
  listPmExecutions: vi.mocked(listPmExecutions),
  listPmFindings: vi.mocked(listPmFindings),
  listPmOccurrences: vi.mocked(listPmOccurrences),
  listPmPlanVersions: vi.mocked(listPmPlanVersions),
  listPmPlans: vi.mocked(listPmPlans),
  listPmPlanningReadiness: vi.mocked(listPmPlanningReadiness),
  listPmRecurringFindings: vi.mocked(listPmRecurringFindings),
  publishPmPlanVersion: vi.mocked(publishPmPlanVersion),
  transitionPmOccurrence: vi.mocked(transitionPmOccurrence),
  transitionPmPlanLifecycle: vi.mocked(transitionPmPlanLifecycle),
  updatePmGenerationSettings: vi.mocked(updatePmGenerationSettings),
  updatePmPlan: vi.mocked(updatePmPlan),
  updatePmPlanVersion: vi.mocked(updatePmPlanVersion),
};

// ── Fixtures ──────────────────────────────────────────────────────────────────

const TS = "2026-10-01T10:00:00Z";

function makePlan(overrides: Partial<PmPlan> = {}): PmPlan {
  return {
    id: 1,
    code: "PM-001",
    title: "Pump quarterly",
    description: null,
    asset_scope_type: "equipment",
    asset_scope_id: 10,
    strategy_type: "fixed",
    criticality_value_id: null,
    criticality_code: null,
    criticality_label: null,
    assigned_team_id: null,
    assigned_team_code: null,
    assigned_team_name: null,
    assigned_team_label: null,
    requires_shutdown: 0,
    requires_permit: 0,
    is_active: 1,
    lifecycle_status: "draft",
    current_version_id: null,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    ...overrides,
  };
}

function makeVersion(overrides: Partial<PmPlanVersion> = {}): PmPlanVersion {
  return {
    id: 100,
    pm_plan_id: 1,
    version_no: 1,
    status: "draft",
    effective_from: "2026-10-01",
    effective_to: null,
    trigger_definition_json: '{"interval_days":90}',
    task_package_json: null,
    required_parts_json: null,
    required_skills_json: null,
    required_tools_json: null,
    estimated_duration_hours: 2,
    estimated_labor_cost: null,
    estimated_parts_cost: null,
    estimated_service_cost: null,
    change_reason: null,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    ...overrides,
  };
}

function makeOccurrence(overrides: Partial<PmOccurrence> = {}): PmOccurrence {
  return {
    id: 500,
    pm_plan_id: 1,
    plan_version_id: 100,
    due_basis: "calendar",
    due_at: "2026-10-15T00:00:00Z",
    due_meter_value: null,
    generated_at: TS,
    status: "forecast",
    linked_work_order_id: null,
    linked_work_order_code: null,
    deferral_reason: null,
    missed_reason: null,
    row_version: 1,
    created_at: TS,
    updated_at: TS,
    plan_code: "PM-001",
    plan_title: "Pump quarterly",
    strategy_type: "fixed",
    ...overrides,
  };
}

function makeExecution(overrides: Partial<PmExecution> = {}): PmExecution {
  return {
    id: 900,
    pm_occurrence_id: 500,
    work_order_id: null,
    work_order_code: null,
    execution_result: "completed",
    executed_at: TS,
    notes: null,
    actor_id: 3,
    actual_duration_hours: 1.5,
    actual_labor_hours: 1.5,
    created_at: TS,
    ...overrides,
  };
}

function makeFinding(overrides: Partial<PmFinding> = {}): PmFinding {
  return {
    id: 1,
    pm_execution_id: 900,
    finding_type: "leak",
    severity: "minor",
    description: "Small leak",
    follow_up_di_id: null,
    follow_up_work_order_id: null,
    follow_up_di_code: null,
    follow_up_work_order_code: null,
    created_at: TS,
    ...overrides,
  };
}

function makeRecurring(overrides: Partial<PmRecurringFinding> = {}): PmRecurringFinding {
  return {
    pm_plan_id: 1,
    plan_code: "PM-001",
    finding_type: "leak",
    occurrence_count: 3,
    first_seen_at: TS,
    last_seen_at: TS,
    latest_severity: "minor",
    ...overrides,
  };
}

function makeReadiness(
  overrides: Partial<PmPlanningReadinessProjection> = {},
): PmPlanningReadinessProjection {
  return {
    as_of: TS,
    candidate_count: 1,
    ready_count: 1,
    blocked_count: 0,
    derivation_rules: ["rule"],
    candidates: [{ occurrence: makeOccurrence(), ready_for_scheduling: true, blockers: [] }],
    ...overrides,
  };
}

function makeRate() {
  return { numerator: 1, denominator: 2, value_pct: 50, derivation: "n/d" };
}

function makeKpis(overrides: Partial<PmGovernanceKpiReport> = {}): PmGovernanceKpiReport {
  return {
    as_of: TS,
    from: "2026-07-01",
    to: "2026-10-01",
    pm_plan_id: null,
    criticality_code: null,
    compliance: makeRate(),
    overdue_risk: makeRate(),
    first_pass_completion: makeRate(),
    follow_up_ratio: makeRate(),
    effort_variance: {
      sample_size: 1,
      estimated_hours: 2,
      actual_hours: 1.5,
      variance_hours: -0.5,
      variance_pct: -25,
      derivation: "a-e",
    },
    derivation_rules: [],
    ...overrides,
  };
}

function makeMetrics(overrides: Partial<PmDueMetrics> = {}): PmDueMetrics {
  return {
    as_of: TS,
    overdue_count: 1,
    due_today_count: 2,
    due_next_7d_count: 3,
    ready_for_scheduling_count: 4,
    ...overrides,
  };
}

function makeGenStatus(overrides: Partial<PmGenerationStatus> = {}): PmGenerationStatus {
  return {
    control: {
      id: 1,
      enabled: true,
      cron_expr: "0 2 * * *",
      horizon_days: 30,
      last_run_at: null,
      next_run_at: null,
      last_status: null,
      last_generated_count: null,
      last_skipped_count: null,
      last_error: null,
      updated_at: TS,
    },
    latest_run: null,
    ...overrides,
  };
}

function makeGenResult(
  overrides: Partial<GeneratePmOccurrencesResult> = {},
): GeneratePmOccurrencesResult {
  return {
    generated_count: 2,
    skipped_count: 0,
    trigger_events_recorded: 0,
    occurrence_ids: [501, 502],
    ...overrides,
  };
}

const createPlanInput: CreatePmPlanInput = {
  code: "PM-002",
  title: "Motor yearly",
  asset_scope_type: "equipment",
  strategy_type: "fixed",
  requires_shutdown: false,
  requires_permit: false,
};

const createVersionInput: CreatePmPlanVersionInput = {
  effective_from: "2026-11-01",
  trigger_definition_json: '{"interval_days":365}',
};

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Default happy-path responses for every read loader. */
function primeLoaders() {
  m.listPmPlans.mockResolvedValue([makePlan()]);
  m.listPmOccurrences.mockResolvedValue([makeOccurrence()]);
  m.getPmDueMetrics.mockResolvedValue(makeMetrics());
  m.getPmGenerationStatus.mockResolvedValue(makeGenStatus());
  m.listPmPlanVersions.mockResolvedValue([makeVersion()]);
  m.listPmExecutions.mockResolvedValue([makeExecution()]);
  m.listPmFindings.mockResolvedValue([makeFinding()]);
  m.listPmRecurringFindings.mockResolvedValue([makeRecurring()]);
  m.listPmPlanningReadiness.mockResolvedValue(makeReadiness());
  m.getPmGovernanceKpiReport.mockResolvedValue(makeKpis());
}

/** Assert the plan-detail refresh cascade ran for `planId`. */
function expectDetailRefreshed(planId: number) {
  expect(m.listPmPlanVersions).toHaveBeenCalledWith(planId);
  expect(m.listPmOccurrences).toHaveBeenCalledWith({ pm_plan_id: planId, include_completed: true });
  expect(m.listPmExecutions).toHaveBeenCalledWith({ pm_plan_id: planId, occurrence_id: null });
  expect(m.listPmRecurringFindings).toHaveBeenCalledWith({ pm_plan_id: planId, days_window: 90 });
  expect(m.listPmPlanningReadiness).toHaveBeenCalledWith({ pm_plan_id: planId, limit: 200 });
  expect(m.getPmGovernanceKpiReport).toHaveBeenCalledWith({ pm_plan_id: planId });
}

const initial = usePmStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  Object.values(m).forEach((fn) => fn.mockReset());
  usePmStore.setState(initial, true);
  primeLoaders();
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("pm-store", () => {
  it("starts empty and idle", () => {
    const s = usePmStore.getState();
    expect(s.plans).toEqual([]);
    expect(s.versions).toEqual([]);
    expect(s.occurrences).toEqual([]);
    expect(s.dueOccurrences).toEqual([]);
    expect(s.calendarOccurrences).toEqual([]);
    expect(s.executions).toEqual([]);
    expect(s.findingsByExecutionId).toEqual({});
    expect(s.recurringFindings).toEqual([]);
    expect(s.planningReadiness).toBeNull();
    expect(s.governanceKpis).toBeNull();
    expect(s.metrics).toBeNull();
    expect(s.generationStatus).toBeNull();
    expect(s.activePlanId).toBeNull();
    expect(s.loading).toBe(false);
    expect(s.saving).toBe(false);
    expect(s.error).toBeNull();
  });

  // ── loadWorkspace ─────────────────────────────────────────────────────────

  describe("loadWorkspace", () => {
    it("loads plans, due + calendar occurrences, metrics and generation status", async () => {
      const plans = [makePlan(), makePlan({ id: 2, code: "PM-002" })];
      m.listPmPlans.mockResolvedValueOnce(plans);
      const due = [makeOccurrence({ id: 1 })];
      const cal = [makeOccurrence({ id: 2, status: "completed" })];
      m.listPmOccurrences.mockResolvedValueOnce(due).mockResolvedValueOnce(cal);
      const metrics = makeMetrics({ overdue_count: 9 });
      m.getPmDueMetrics.mockResolvedValueOnce(metrics);
      const gen = makeGenStatus();
      m.getPmGenerationStatus.mockResolvedValueOnce(gen);

      await usePmStore.getState().loadWorkspace();

      expect(m.listPmPlans).toHaveBeenCalledWith({});
      expect(m.listPmOccurrences).toHaveBeenNthCalledWith(1, { include_completed: false });
      expect(m.listPmOccurrences).toHaveBeenNthCalledWith(2, { include_completed: true });
      const s = usePmStore.getState();
      expect(s.plans).toEqual(plans);
      expect(s.dueOccurrences).toEqual(due);
      expect(s.calendarOccurrences).toEqual(cal);
      expect(s.metrics).toEqual(metrics);
      expect(s.generationStatus).toEqual(gen);
      expect(s.loading).toBe(false);
      expect(s.error).toBeNull();
      // No active plan → no detail refresh.
      expect(m.listPmPlanVersions).not.toHaveBeenCalled();
    });

    it("sets loading while in flight and clears a stale error", async () => {
      const d = deferred<PmPlan[]>();
      m.listPmPlans.mockReturnValueOnce(d.promise);
      usePmStore.setState({ error: "stale" });

      const p = usePmStore.getState().loadWorkspace();
      expect(usePmStore.getState().loading).toBe(true);
      expect(usePmStore.getState().error).toBeNull();

      d.resolve([]);
      await p;
      expect(usePmStore.getState().loading).toBe(false);
    });

    it("refreshes the active plan detail when the active plan still exists", async () => {
      usePmStore.setState({ activePlanId: 1 });
      const versions = [makeVersion({ id: 101 })];
      m.listPmPlanVersions.mockResolvedValueOnce(versions);

      await usePmStore.getState().loadWorkspace();

      expectDetailRefreshed(1);
      expect(usePmStore.getState().versions).toEqual(versions);
    });

    it("skips the detail refresh when the active plan is no longer listed", async () => {
      usePmStore.setState({ activePlanId: 42 });
      await usePmStore.getState().loadWorkspace();
      expect(m.listPmPlanVersions).not.toHaveBeenCalled();
      expect(usePmStore.getState().activePlanId).toBe(42);
    });

    it("on listPmPlans failure sets error, skips sub-loads and does not rethrow", async () => {
      m.listPmPlans.mockRejectedValueOnce({ code: "DB", message: "plans down" });

      await expect(usePmStore.getState().loadWorkspace()).resolves.toBeUndefined();

      expect(usePmStore.getState().error).toBe("plans down");
      expect(usePmStore.getState().loading).toBe(false);
      expect(m.listPmOccurrences).not.toHaveBeenCalled();
    });

    it("a failing sub-loader records its error but the remaining loaders still run", async () => {
      m.listPmOccurrences.mockRejectedValueOnce(new Error("due failed"));

      await usePmStore.getState().loadWorkspace();

      expect(usePmStore.getState().error).toBe("due failed");
      expect(m.getPmDueMetrics).toHaveBeenCalled();
      expect(m.getPmGenerationStatus).toHaveBeenCalled();
      expect(usePmStore.getState().metrics).toEqual(makeMetrics());
    });

    it("a failing listPmPlanVersions during active detail refresh is captured as error", async () => {
      usePmStore.setState({ activePlanId: 1 });
      m.listPmPlanVersions.mockRejectedValueOnce(new Error("versions failed"));

      await usePmStore.getState().loadWorkspace();

      expect(usePmStore.getState().error).toBe("versions failed");
      expect(usePmStore.getState().loading).toBe(false);
    });
  });

  // ── openPlan / closePlan ──────────────────────────────────────────────────

  describe("openPlan", () => {
    it("sets activePlanId, resets detail and loads all plan detail", async () => {
      usePmStore.setState({
        versions: [makeVersion({ id: 999 })],
        findingsByExecutionId: { 1: [makeFinding()] },
        error: "old",
      });
      const versions = [makeVersion({ id: 200, pm_plan_id: 3 })];
      m.listPmPlanVersions.mockResolvedValueOnce(versions);
      const occ = [makeOccurrence({ pm_plan_id: 3 })];
      m.listPmOccurrences.mockResolvedValueOnce(occ);
      const exec = [makeExecution({ id: 7 })];
      m.listPmExecutions.mockResolvedValueOnce(exec);
      const rec = [makeRecurring({ pm_plan_id: 3 })];
      m.listPmRecurringFindings.mockResolvedValueOnce(rec);
      const ready = makeReadiness({ ready_count: 5 });
      m.listPmPlanningReadiness.mockResolvedValueOnce(ready);
      const kpis = makeKpis({ pm_plan_id: 3 });
      m.getPmGovernanceKpiReport.mockResolvedValueOnce(kpis);

      const p = usePmStore.getState().openPlan(3);
      // Synchronous reset before any IPC resolves.
      expect(usePmStore.getState().activePlanId).toBe(3);
      expect(usePmStore.getState().versions).toEqual([]);
      expect(usePmStore.getState().findingsByExecutionId).toEqual({});
      expect(usePmStore.getState().error).toBeNull();
      await p;

      expectDetailRefreshed(3);
      const s = usePmStore.getState();
      expect(s.versions).toEqual(versions);
      expect(s.occurrences).toEqual(occ);
      expect(s.executions).toEqual(exec);
      expect(s.recurringFindings).toEqual(rec);
      expect(s.planningReadiness).toEqual(ready);
      expect(s.governanceKpis).toEqual(kpis);
    });

    it("captures listPmPlanVersions failure as error without rethrowing", async () => {
      m.listPmPlanVersions.mockRejectedValueOnce({ message: "no access" });

      await expect(usePmStore.getState().openPlan(3)).resolves.toBeUndefined();

      expect(usePmStore.getState().error).toBe("no access");
      expect(usePmStore.getState().activePlanId).toBe(3);
      expect(m.listPmOccurrences).not.toHaveBeenCalled();
    });
  });

  it("closePlan clears the active plan and all plan-detail state but not workspace lists", () => {
    const plans = [makePlan()];
    const due = [makeOccurrence()];
    usePmStore.setState({
      plans,
      dueOccurrences: due,
      activePlanId: 1,
      versions: [makeVersion()],
      occurrences: [makeOccurrence()],
      executions: [makeExecution()],
      findingsByExecutionId: { 900: [makeFinding()] },
      recurringFindings: [makeRecurring()],
      planningReadiness: makeReadiness(),
      governanceKpis: makeKpis(),
    });

    usePmStore.getState().closePlan();

    const s = usePmStore.getState();
    expect(s.activePlanId).toBeNull();
    expect(s.versions).toEqual([]);
    expect(s.occurrences).toEqual([]);
    expect(s.executions).toEqual([]);
    expect(s.findingsByExecutionId).toEqual({});
    expect(s.recurringFindings).toEqual([]);
    expect(s.planningReadiness).toBeNull();
    expect(s.governanceKpis).toBeNull();
    expect(s.plans).toBe(plans);
    expect(s.dueOccurrences).toBe(due);
  });

  // ── Individual loaders ────────────────────────────────────────────────────

  describe("individual loaders", () => {
    it("loadDueOccurrences requests non-completed occurrences", async () => {
      const rows = [makeOccurrence({ id: 3 })];
      m.listPmOccurrences.mockResolvedValueOnce(rows);
      await usePmStore.getState().loadDueOccurrences();
      expect(m.listPmOccurrences).toHaveBeenCalledWith({ include_completed: false });
      expect(usePmStore.getState().dueOccurrences).toEqual(rows);
    });

    it("loadCalendarOccurrences requests completed occurrences too", async () => {
      const rows = [makeOccurrence({ id: 4 })];
      m.listPmOccurrences.mockResolvedValueOnce(rows);
      await usePmStore.getState().loadCalendarOccurrences();
      expect(m.listPmOccurrences).toHaveBeenCalledWith({ include_completed: true });
      expect(usePmStore.getState().calendarOccurrences).toEqual(rows);
    });

    it("loadOccurrences forwards the filter, defaulting to {}", async () => {
      await usePmStore.getState().loadOccurrences({ pm_plan_id: 2, status: "due" });
      expect(m.listPmOccurrences).toHaveBeenLastCalledWith({ pm_plan_id: 2, status: "due" });
      await usePmStore.getState().loadOccurrences();
      expect(m.listPmOccurrences).toHaveBeenLastCalledWith({});
      expect(usePmStore.getState().occurrences).toEqual([makeOccurrence()]);
    });

    it("loadDueMetrics and loadGenerationStatus store their results", async () => {
      await usePmStore.getState().loadDueMetrics();
      await usePmStore.getState().loadGenerationStatus();
      expect(usePmStore.getState().metrics).toEqual(makeMetrics());
      expect(usePmStore.getState().generationStatus).toEqual(makeGenStatus());
    });

    it("loadExecutions normalises undefined ids to null", async () => {
      await usePmStore.getState().loadExecutions();
      expect(m.listPmExecutions).toHaveBeenLastCalledWith({
        pm_plan_id: null,
        occurrence_id: null,
      });
      await usePmStore.getState().loadExecutions(1, 500);
      expect(m.listPmExecutions).toHaveBeenLastCalledWith({ pm_plan_id: 1, occurrence_id: 500 });
      expect(usePmStore.getState().executions).toEqual([makeExecution()]);
    });

    it("loadFindings merges findings per execution id", async () => {
      const a = [makeFinding({ id: 1, pm_execution_id: 900 })];
      const b = [makeFinding({ id: 2, pm_execution_id: 901 })];
      m.listPmFindings.mockResolvedValueOnce(a).mockResolvedValueOnce(b);

      await usePmStore.getState().loadFindings(900);
      await usePmStore.getState().loadFindings(901);

      expect(m.listPmFindings).toHaveBeenNthCalledWith(1, 900);
      expect(m.listPmFindings).toHaveBeenNthCalledWith(2, 901);
      expect(usePmStore.getState().findingsByExecutionId).toEqual({ 900: a, 901: b });
    });

    it("loadRecurringFindings uses a 90 day window and null plan by default", async () => {
      await usePmStore.getState().loadRecurringFindings();
      expect(m.listPmRecurringFindings).toHaveBeenCalledWith({ pm_plan_id: null, days_window: 90 });
      expect(usePmStore.getState().recurringFindings).toEqual([makeRecurring()]);
    });

    it("loadPlanningReadiness uses limit 200 and null plan by default", async () => {
      await usePmStore.getState().loadPlanningReadiness();
      expect(m.listPmPlanningReadiness).toHaveBeenCalledWith({ pm_plan_id: null, limit: 200 });
      expect(usePmStore.getState().planningReadiness).toEqual(makeReadiness());
    });

    it("loadGovernanceKpis forwards input and defaults to {}", async () => {
      await usePmStore.getState().loadGovernanceKpis({ from: "2026-01-01", to: "2026-02-01" });
      expect(m.getPmGovernanceKpiReport).toHaveBeenLastCalledWith({
        from: "2026-01-01",
        to: "2026-02-01",
      });
      await usePmStore.getState().loadGovernanceKpis();
      expect(m.getPmGovernanceKpiReport).toHaveBeenLastCalledWith({});
      expect(usePmStore.getState().governanceKpis).toEqual(makeKpis());
    });

    it.each([
      [
        "loadDueOccurrences",
        () => m.listPmOccurrences,
        () => usePmStore.getState().loadDueOccurrences(),
      ],
      [
        "loadCalendarOccurrences",
        () => m.listPmOccurrences,
        () => usePmStore.getState().loadCalendarOccurrences(),
      ],
      ["loadOccurrences", () => m.listPmOccurrences, () => usePmStore.getState().loadOccurrences()],
      ["loadDueMetrics", () => m.getPmDueMetrics, () => usePmStore.getState().loadDueMetrics()],
      [
        "loadGenerationStatus",
        () => m.getPmGenerationStatus,
        () => usePmStore.getState().loadGenerationStatus(),
      ],
      ["loadExecutions", () => m.listPmExecutions, () => usePmStore.getState().loadExecutions()],
      ["loadFindings", () => m.listPmFindings, () => usePmStore.getState().loadFindings(1)],
      [
        "loadRecurringFindings",
        () => m.listPmRecurringFindings,
        () => usePmStore.getState().loadRecurringFindings(),
      ],
      [
        "loadPlanningReadiness",
        () => m.listPmPlanningReadiness,
        () => usePmStore.getState().loadPlanningReadiness(),
      ],
      [
        "loadGovernanceKpis",
        () => m.getPmGovernanceKpiReport,
        () => usePmStore.getState().loadGovernanceKpis(),
      ],
    ] as const)("%s captures service errors without rethrowing", async (_name, getMock, run) => {
      getMock().mockRejectedValueOnce({ code: "DB", message: "loader failed" });
      await expect(run()).resolves.toBeUndefined();
      expect(usePmStore.getState().error).toBe("loader failed");
    });

    it("loaders do not toggle loading or saving", async () => {
      const d = deferred<PmDueMetrics>();
      m.getPmDueMetrics.mockReturnValueOnce(d.promise);
      const p = usePmStore.getState().loadDueMetrics();
      expect(usePmStore.getState().loading).toBe(false);
      expect(usePmStore.getState().saving).toBe(false);
      d.resolve(makeMetrics());
      await p;
    });
  });

  // ── updateGenerationSettings ──────────────────────────────────────────────

  describe("updateGenerationSettings", () => {
    it("saves, stores and returns the new status", async () => {
      const status = makeGenStatus();
      const d = deferred<PmGenerationStatus>();
      m.updatePmGenerationSettings.mockReturnValueOnce(d.promise);

      const p = usePmStore
        .getState()
        .updateGenerationSettings({ enabled: false, horizon_days: 60 });
      expect(usePmStore.getState().saving).toBe(true);
      d.resolve(status);

      await expect(p).resolves.toEqual(status);
      expect(m.updatePmGenerationSettings).toHaveBeenCalledWith({
        enabled: false,
        horizon_days: 60,
      });
      expect(usePmStore.getState().generationStatus).toEqual(status);
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure", async () => {
      const err = { code: "VALIDATION", message: "bad cron" };
      m.updatePmGenerationSettings.mockRejectedValueOnce(err);

      await expect(usePmStore.getState().updateGenerationSettings({ cron_expr: "x" })).rejects.toBe(
        err,
      );
      expect(usePmStore.getState().error).toBe("bad cron");
      expect(usePmStore.getState().saving).toBe(false);
    });
  });

  // ── generateOccurrences ───────────────────────────────────────────────────

  describe("generateOccurrences", () => {
    it("generates, refreshes workspace lists and returns the result (no active plan)", async () => {
      const result = makeGenResult();
      m.generatePmOccurrences.mockResolvedValueOnce(result);

      await expect(
        usePmStore.getState().generateOccurrences({ horizon_days: 30 }),
      ).resolves.toEqual(result);

      expect(m.generatePmOccurrences).toHaveBeenCalledWith({ horizon_days: 30 });
      expect(m.listPmOccurrences).toHaveBeenCalledWith({ include_completed: false });
      expect(m.listPmOccurrences).toHaveBeenCalledWith({ include_completed: true });
      expect(m.getPmDueMetrics).toHaveBeenCalled();
      expect(m.getPmGenerationStatus).toHaveBeenCalled();
      expect(m.listPmPlanVersions).not.toHaveBeenCalled();
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("refreshes active plan detail when a plan is open", async () => {
      usePmStore.setState({ activePlanId: 1 });
      m.generatePmOccurrences.mockResolvedValueOnce(makeGenResult());
      await usePmStore.getState().generateOccurrences({ pm_plan_id: 1 });
      expectDetailRefreshed(1);
    });

    it("sets saving while in flight", async () => {
      const d = deferred<GeneratePmOccurrencesResult>();
      m.generatePmOccurrences.mockReturnValueOnce(d.promise);
      const p = usePmStore.getState().generateOccurrences({});
      expect(usePmStore.getState().saving).toBe(true);
      d.resolve(makeGenResult());
      await p;
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure without refreshing", async () => {
      const err = new Error("generation locked");
      m.generatePmOccurrences.mockRejectedValueOnce(err);

      await expect(usePmStore.getState().generateOccurrences({})).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("generation locked");
      expect(usePmStore.getState().saving).toBe(false);
      expect(m.listPmOccurrences).not.toHaveBeenCalled();
    });
  });

  // ── transitionOccurrence ──────────────────────────────────────────────────

  describe("transitionOccurrence", () => {
    const input = { occurrence_id: 500, expected_row_version: 1, next_status: "ready" };

    it("transitions, refreshes lists and the active plan detail, returns the occurrence", async () => {
      usePmStore.setState({ activePlanId: 1 });
      const occ = makeOccurrence({ status: "ready", row_version: 2 });
      m.transitionPmOccurrence.mockResolvedValueOnce(occ);

      await expect(usePmStore.getState().transitionOccurrence(input)).resolves.toEqual(occ);

      expect(m.transitionPmOccurrence).toHaveBeenCalledWith(input);
      expect(m.getPmDueMetrics).toHaveBeenCalled();
      expect(m.getPmGenerationStatus).not.toHaveBeenCalled();
      expectDetailRefreshed(1);
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("falls back to the occurrence's plan when no plan is open (without activating it)", async () => {
      m.transitionPmOccurrence.mockResolvedValueOnce(makeOccurrence({ pm_plan_id: 8 }));

      await usePmStore.getState().transitionOccurrence(input);

      expectDetailRefreshed(8);
      // Detail state is loaded for plan 8 while activePlanId stays null.
      expect(usePmStore.getState().activePlanId).toBeNull();
      expect(usePmStore.getState().versions).toEqual([makeVersion()]);
    });

    it("sets error and rethrows on failure", async () => {
      const err = { code: "CONFLICT", message: "row version mismatch" };
      m.transitionPmOccurrence.mockRejectedValueOnce(err);

      await expect(usePmStore.getState().transitionOccurrence(input)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("row version mismatch");
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("rethrows when the post-transition detail refresh fails", async () => {
      m.transitionPmOccurrence.mockResolvedValueOnce(makeOccurrence());
      const err = new Error("versions failed");
      m.listPmPlanVersions.mockRejectedValueOnce(err);

      await expect(usePmStore.getState().transitionOccurrence(input)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("versions failed");
    });
  });

  // ── executeOccurrence ─────────────────────────────────────────────────────

  describe("executeOccurrence", () => {
    const input = {
      occurrence_id: 500,
      expected_occurrence_row_version: 1,
      execution_result: "completed",
    };

    function makeExecResult(planId = 1): ExecutePmOccurrenceResult {
      return {
        occurrence: makeOccurrence({ pm_plan_id: planId, status: "completed" }),
        execution: makeExecution({ id: 901 }),
        findings: [makeFinding({ pm_execution_id: 901 })],
      };
    }

    it("executes, refreshes, loads findings for the new execution and returns the result", async () => {
      usePmStore.setState({ activePlanId: 1 });
      const result = makeExecResult();
      m.executePmOccurrence.mockResolvedValueOnce(result);
      const findings = [makeFinding({ id: 77, pm_execution_id: 901 })];
      m.listPmFindings.mockResolvedValueOnce(findings);

      await expect(usePmStore.getState().executeOccurrence(input)).resolves.toEqual(result);

      expect(m.executePmOccurrence).toHaveBeenCalledWith(input);
      expectDetailRefreshed(1);
      expect(m.listPmFindings).toHaveBeenCalledWith(901);
      expect(usePmStore.getState().findingsByExecutionId[901]).toEqual(findings);
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("falls back to the result occurrence's plan when no plan is open", async () => {
      m.executePmOccurrence.mockResolvedValueOnce(makeExecResult(6));
      await usePmStore.getState().executeOccurrence(input);
      expectDetailRefreshed(6);
    });

    it("sets saving during execution", async () => {
      const d = deferred<ExecutePmOccurrenceResult>();
      m.executePmOccurrence.mockReturnValueOnce(d.promise);
      const p = usePmStore.getState().executeOccurrence(input);
      expect(usePmStore.getState().saving).toBe(true);
      d.resolve(makeExecResult());
      await p;
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure", async () => {
      const err = new Error("not executable");
      m.executePmOccurrence.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().executeOccurrence(input)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("not executable");
      expect(m.listPmFindings).not.toHaveBeenCalled();
    });
  });

  // ── Plan mutations ────────────────────────────────────────────────────────

  describe("createPlan", () => {
    it("creates, reloads the workspace, opens the new plan and returns it", async () => {
      const plan = makePlan({ id: 2, code: "PM-002" });
      m.createPmPlan.mockResolvedValueOnce(plan);

      await expect(usePmStore.getState().createPlan(createPlanInput)).resolves.toEqual(plan);

      expect(m.createPmPlan).toHaveBeenCalledWith(createPlanInput);
      expect(m.listPmPlans).toHaveBeenCalledWith({});
      expect(usePmStore.getState().activePlanId).toBe(2);
      expectDetailRefreshed(2);
      expect(usePmStore.getState().saving).toBe(false);
      expect(usePmStore.getState().loading).toBe(false);
    });

    it("sets saving while in flight", async () => {
      const d = deferred<PmPlan>();
      m.createPmPlan.mockReturnValueOnce(d.promise);
      const p = usePmStore.getState().createPlan(createPlanInput);
      expect(usePmStore.getState().saving).toBe(true);
      d.resolve(makePlan());
      await p;
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure without touching the workspace", async () => {
      const err = { code: "DUPLICATE", message: "code exists" };
      m.createPmPlan.mockRejectedValueOnce(err);

      await expect(usePmStore.getState().createPlan(createPlanInput)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("code exists");
      expect(usePmStore.getState().saving).toBe(false);
      expect(m.listPmPlans).not.toHaveBeenCalled();
    });
  });

  describe("updatePlan", () => {
    it("forwards id/row version/input, reloads and reopens the plan", async () => {
      const plan = makePlan({ id: 1, title: "Renamed", row_version: 2 });
      m.updatePmPlan.mockResolvedValueOnce(plan);

      await expect(usePmStore.getState().updatePlan(1, 1, { title: "Renamed" })).resolves.toEqual(
        plan,
      );

      expect(m.updatePmPlan).toHaveBeenCalledWith(1, 1, { title: "Renamed" });
      expect(m.listPmPlans).toHaveBeenCalled();
      expect(usePmStore.getState().activePlanId).toBe(1);
      expectDetailRefreshed(1);
    });

    it("sets error and rethrows on failure", async () => {
      const err = new Error("stale row version");
      m.updatePmPlan.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().updatePlan(1, 1, {})).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("stale row version");
      expect(usePmStore.getState().saving).toBe(false);
    });
  });

  describe("transitionPlanLifecycle", () => {
    const input = { plan_id: 1, expected_row_version: 1, next_status: "active" };

    it("transitions, reloads and reopens the plan", async () => {
      const plan = makePlan({ lifecycle_status: "active", row_version: 2 });
      m.transitionPmPlanLifecycle.mockResolvedValueOnce(plan);

      await expect(usePmStore.getState().transitionPlanLifecycle(input)).resolves.toEqual(plan);

      expect(m.transitionPmPlanLifecycle).toHaveBeenCalledWith(input);
      expect(m.listPmPlans).toHaveBeenCalled();
      expect(usePmStore.getState().activePlanId).toBe(1);
    });

    it("sets error and rethrows on failure", async () => {
      const err = { message: "no published version" };
      m.transitionPmPlanLifecycle.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().transitionPlanLifecycle(input)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("no published version");
    });
  });

  describe("deletePlan", () => {
    it("closes the plan when deleting the active one, then reloads the workspace", async () => {
      usePmStore.setState({
        activePlanId: 1,
        versions: [makeVersion()],
        governanceKpis: makeKpis(),
      });
      m.deletePmPlan.mockResolvedValueOnce(undefined);
      m.listPmPlans.mockResolvedValueOnce([]);

      await expect(usePmStore.getState().deletePlan(1, 3)).resolves.toBeUndefined();

      expect(m.deletePmPlan).toHaveBeenCalledWith(1, 3);
      const s = usePmStore.getState();
      expect(s.activePlanId).toBeNull();
      expect(s.versions).toEqual([]);
      expect(s.governanceKpis).toBeNull();
      expect(s.plans).toEqual([]);
      expect(s.saving).toBe(false);
    });

    it("keeps a different active plan open", async () => {
      usePmStore.setState({ activePlanId: 1 });
      m.deletePmPlan.mockResolvedValueOnce(undefined);

      await usePmStore.getState().deletePlan(2, 1);

      expect(usePmStore.getState().activePlanId).toBe(1);
      expect(m.listPmPlans).toHaveBeenCalled();
    });

    it("sets error, rethrows and keeps the active plan on failure", async () => {
      usePmStore.setState({ activePlanId: 1 });
      const err = { code: "IN_USE", message: "plan has occurrences" };
      m.deletePmPlan.mockRejectedValueOnce(err);

      await expect(usePmStore.getState().deletePlan(1, 1)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("plan has occurrences");
      expect(usePmStore.getState().activePlanId).toBe(1);
      expect(usePmStore.getState().saving).toBe(false);
    });
  });

  // ── Version mutations ─────────────────────────────────────────────────────

  describe("createVersion", () => {
    it("creates for the plan, reopens that plan and returns the version", async () => {
      const version = makeVersion({ id: 101, pm_plan_id: 1, version_no: 2 });
      m.createPmPlanVersion.mockResolvedValueOnce(version);

      await expect(usePmStore.getState().createVersion(1, createVersionInput)).resolves.toEqual(
        version,
      );

      expect(m.createPmPlanVersion).toHaveBeenCalledWith(1, createVersionInput);
      expect(usePmStore.getState().activePlanId).toBe(1);
      expectDetailRefreshed(1);
      // Does not reload the workspace.
      expect(m.listPmPlans).not.toHaveBeenCalled();
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure", async () => {
      const err = new Error("overlapping effective dates");
      m.createPmPlanVersion.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().createVersion(1, createVersionInput)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("overlapping effective dates");
      expect(usePmStore.getState().saving).toBe(false);
    });
  });

  describe("updateVersion", () => {
    it("updates and reopens the version's plan", async () => {
      const version = makeVersion({ id: 100, pm_plan_id: 4, row_version: 2 });
      m.updatePmPlanVersion.mockResolvedValueOnce(version);

      await expect(
        usePmStore.getState().updateVersion(100, 1, { change_reason: "fix" }),
      ).resolves.toEqual(version);

      expect(m.updatePmPlanVersion).toHaveBeenCalledWith(100, 1, { change_reason: "fix" });
      expect(usePmStore.getState().activePlanId).toBe(4);
      expectDetailRefreshed(4);
    });

    it("sets error and rethrows on failure", async () => {
      const err = "version is published";
      m.updatePmPlanVersion.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().updateVersion(100, 1, {})).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("version is published");
    });
  });

  describe("publishVersion", () => {
    it("publishes with the IPC input shape, reloads workspace and reopens the plan", async () => {
      const version = makeVersion({ status: "published", pm_plan_id: 1 });
      m.publishPmPlanVersion.mockResolvedValueOnce(version);

      await expect(usePmStore.getState().publishVersion(100, 2)).resolves.toEqual(version);

      expect(m.publishPmPlanVersion).toHaveBeenCalledWith({
        version_id: 100,
        expected_row_version: 2,
      });
      expect(m.listPmPlans).toHaveBeenCalled();
      expect(usePmStore.getState().activePlanId).toBe(1);
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("sets error and rethrows on failure", async () => {
      const err = { code: "VALIDATION", message: "trigger invalid" };
      m.publishPmPlanVersion.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().publishVersion(100, 2)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("trigger invalid");
      expect(usePmStore.getState().saving).toBe(false);
    });
  });

  describe("deleteVersion", () => {
    it("deletes and reopens the active plan", async () => {
      usePmStore.setState({ activePlanId: 1 });
      m.deletePmPlanVersion.mockResolvedValueOnce(undefined);

      await expect(usePmStore.getState().deleteVersion(100, 1)).resolves.toBeUndefined();

      expect(m.deletePmPlanVersion).toHaveBeenCalledWith(100, 1);
      expectDetailRefreshed(1);
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("does not refresh any detail when no plan is open", async () => {
      m.deletePmPlanVersion.mockResolvedValueOnce(undefined);
      await usePmStore.getState().deleteVersion(100, 1);
      expect(m.listPmPlanVersions).not.toHaveBeenCalled();
    });

    it("sets error and rethrows on failure", async () => {
      const err = new Error("cannot delete published version");
      m.deletePmPlanVersion.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().deleteVersion(100, 1)).rejects.toBe(err);
      expect(usePmStore.getState().error).toBe("cannot delete published version");
      expect(usePmStore.getState().saving).toBe(false);
    });
  });
});
