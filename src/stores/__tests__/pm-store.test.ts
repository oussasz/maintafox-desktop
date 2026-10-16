/**
 * pm-store — behaviour tests for the preventive-maintenance workspace store.
 *
 * Covers workspace loading, plan detail open/close, every list loader, the
 * mutations (with their refresh cascades), loading/saving flags and the error contract
 * (workspace `error`, `detailError`, mutations rethrown and never stored).
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

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
  listPmPlanningReadiness,
  listPmPlans,
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
    planned_crew_size: 1,
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
    failures_found_by_pm: 0,
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
    expect(s.detailLoading).toBe(false);
    expect(s.saving).toBe(false);
    expect(s.error).toBeNull();
    expect(s.detailError).toBeNull();
  });

  // ── loadWorkspace ─────────────────────────────────────────────────────────

  describe("loadWorkspace", () => {
    it("loads plans, due + calendar occurrences, metrics and generation status", async () => {
      const plans = [makePlan(), makePlan({ id: 2, code: "PM-002" })];
      m.listPmPlans.mockResolvedValueOnce(plans);
      const due = [makeOccurrence({ id: 1 })];
      const calendar = [makeOccurrence({ id: 2, status: "completed" })];
      m.listPmOccurrences.mockResolvedValueOnce(due).mockResolvedValueOnce(calendar);

      await usePmStore.getState().loadWorkspace();

      const s = usePmStore.getState();
      expect(m.listPmPlans).toHaveBeenCalledWith({});
      expect(m.listPmOccurrences).toHaveBeenNthCalledWith(1, { include_completed: false });
      expect(m.listPmOccurrences).toHaveBeenNthCalledWith(2, { include_completed: true });
      expect(s.plans).toEqual(plans);
      expect(s.dueOccurrences).toEqual(due);
      expect(s.calendarOccurrences).toEqual(calendar);
      expect(s.metrics).toEqual(makeMetrics());
      expect(s.generationStatus).toEqual(makeGenStatus());
      expect(s.loading).toBe(false);
      expect(s.error).toBeNull();
    });

    it("sets loading while in flight and clears a stale error", async () => {
      const gate = deferred<PmPlan[]>();
      m.listPmPlans.mockReturnValueOnce(gate.promise);
      usePmStore.setState({ error: new Error("stale") });
      const run = usePmStore.getState().loadWorkspace();
      expect(usePmStore.getState().loading).toBe(true);
      expect(usePmStore.getState().error).toBeNull();
      gate.resolve([]);
      await run;
      expect(usePmStore.getState().loading).toBe(false);
    });

    it("refreshes the active plan detail when the active plan still exists", async () => {
      usePmStore.setState({ activePlanId: 1 });
      await usePmStore.getState().loadWorkspace();
      expectDetailRefreshed(1);
    });

    it("skips the detail refresh when the active plan is no longer listed", async () => {
      usePmStore.setState({ activePlanId: 99 });
      await usePmStore.getState().loadWorkspace();
      expect(m.listPmPlanVersions).not.toHaveBeenCalled();
    });

    it("keeps the raw failure for the ErrorState and does not rethrow", async () => {
      const err = { code: "DB_ERROR", message: "plans down" };
      m.listPmPlans.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().loadWorkspace()).resolves.toBeUndefined();
      expect(usePmStore.getState().error).toBe(err);
      expect(m.listPmOccurrences).not.toHaveBeenCalled();
      expect(usePmStore.getState().loading).toBe(false);
    });

    it("a failing list loader fails the workspace load (no silently partial page)", async () => {
      const err = new Error("due failed");
      m.listPmOccurrences.mockRejectedValueOnce(err);
      await usePmStore.getState().loadWorkspace();
      expect(usePmStore.getState().error).toBe(err);
      expect(m.getPmDueMetrics).not.toHaveBeenCalled();
    });

    it("a failing detail refresh lands in detailError, not in the workspace error", async () => {
      usePmStore.setState({ activePlanId: 1 });
      const err = new Error("versions failed");
      m.listPmPlanVersions.mockRejectedValueOnce(err);
      await usePmStore.getState().loadWorkspace();
      expect(usePmStore.getState().error).toBeNull();
      expect(usePmStore.getState().detailError).toBe(err);
    });
  });

  // ── openPlan / closePlan ──────────────────────────────────────────────────

  describe("openPlan", () => {
    it("sets activePlanId, resets detail and loads all plan detail", async () => {
      usePmStore.setState({
        versions: [makeVersion({ id: 999 })],
        findingsByExecutionId: { 5: [makeFinding()] },
        detailError: new Error("old"),
      });
      const gate = deferred<PmPlanVersion[]>();
      m.listPmPlanVersions.mockReturnValueOnce(gate.promise);
      const run = usePmStore.getState().openPlan(1);
      const mid = usePmStore.getState();
      expect(mid.activePlanId).toBe(1);
      expect(mid.versions).toEqual([]);
      expect(mid.findingsByExecutionId).toEqual({});
      expect(mid.detailError).toBeNull();
      expect(mid.detailLoading).toBe(true);
      gate.resolve([makeVersion()]);
      await run;
      expectDetailRefreshed(1);
      const s = usePmStore.getState();
      expect(s.versions).toEqual([makeVersion()]);
      expect(s.executions).toEqual([makeExecution()]);
      expect(s.recurringFindings).toEqual([makeRecurring()]);
      expect(s.planningReadiness).toEqual(makeReadiness());
      expect(s.governanceKpis).toEqual(makeKpis());
      expect(s.detailLoading).toBe(false);
    });

    it("captures a detail failure in detailError without rethrowing", async () => {
      const err = new Error("no access");
      m.listPmPlanVersions.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().openPlan(1)).resolves.toBeUndefined();
      expect(usePmStore.getState().detailError).toBe(err);
      expect(usePmStore.getState().error).toBeNull();
      expect(usePmStore.getState().activePlanId).toBe(1);
    });
  });

  it("closePlan clears the active plan and all plan-detail state but not workspace lists", () => {
    usePmStore.setState({
      activePlanId: 1,
      versions: [makeVersion()],
      occurrences: [makeOccurrence()],
      executions: [makeExecution()],
      findingsByExecutionId: { 1: [makeFinding()] },
      recurringFindings: [makeRecurring()],
      planningReadiness: makeReadiness(),
      governanceKpis: makeKpis(),
      detailError: new Error("x"),
      plans: [makePlan()],
      dueOccurrences: [makeOccurrence()],
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
    expect(s.detailError).toBeNull();
    expect(s.plans).toHaveLength(1);
    expect(s.dueOccurrences).toHaveLength(1);
  });

  // ── individual loaders ────────────────────────────────────────────────────

  describe("individual loaders", () => {
    it("loadOccurrences forwards the filter, defaulting to {}", async () => {
      await usePmStore.getState().loadOccurrences();
      expect(m.listPmOccurrences).toHaveBeenCalledWith({});
      await usePmStore.getState().loadOccurrences({ pm_plan_id: 3 });
      expect(m.listPmOccurrences).toHaveBeenLastCalledWith({ pm_plan_id: 3 });
    });

    it("loadExecutions normalises undefined ids to null", async () => {
      await usePmStore.getState().loadExecutions();
      expect(m.listPmExecutions).toHaveBeenCalledWith({ pm_plan_id: null, occurrence_id: null });
    });

    it("loadFindings merges findings per execution id", async () => {
      usePmStore.setState({ findingsByExecutionId: { 1: [makeFinding({ id: 1 })] } });
      m.listPmFindings.mockResolvedValueOnce([makeFinding({ id: 2, pm_execution_id: 2 })]);
      await usePmStore.getState().loadFindings(2);
      const map = usePmStore.getState().findingsByExecutionId;
      expect(Object.keys(map)).toEqual(["1", "2"]);
    });

    it("loadFindings records a failure as detailError (called from the dialog)", async () => {
      const err = new Error("findings down");
      m.listPmFindings.mockRejectedValueOnce(err);
      await expect(usePmStore.getState().loadFindings(2)).resolves.toBeUndefined();
      expect(usePmStore.getState().detailError).toBe(err);
    });

    it("plain loaders propagate failures to their caller", async () => {
      m.getPmDueMetrics.mockRejectedValueOnce(new Error("metrics down"));
      await expect(usePmStore.getState().loadDueMetrics()).rejects.toThrow("metrics down");
    });
  });

  // ── mutations ─────────────────────────────────────────────────────────────

  describe("mutations", () => {
    it("a failed mutation rethrows, clears saving and is not stored (the IPC toast reports it)", async () => {
      const err = { code: "VALIDATION_FAILED", message: "row version mismatch" };
      m.transitionPmOccurrence.mockRejectedValueOnce(err);
      await expect(
        usePmStore.getState().transitionOccurrence({
          occurrence_id: 1,
          expected_row_version: 1,
          next_status: "scheduled",
        }),
      ).rejects.toBe(err);
      const s = usePmStore.getState();
      expect(s.saving).toBe(false);
      expect(s.error).toBeNull();
      expect(s.detailError).toBeNull();
    });

    it("sets saving while a mutation is in flight", async () => {
      const gate = deferred<GeneratePmOccurrencesResult>();
      m.generatePmOccurrences.mockReturnValueOnce(gate.promise);
      const run = usePmStore.getState().generateOccurrences({});
      expect(usePmStore.getState().saving).toBe(true);
      gate.resolve(makeGenResult());
      await run;
      expect(usePmStore.getState().saving).toBe(false);
    });

    it("generateOccurrences refreshes lists, generation status and the open plan", async () => {
      usePmStore.setState({ activePlanId: 1 });
      m.generatePmOccurrences.mockResolvedValueOnce(makeGenResult({ generated_count: 2 }));
      const result = await usePmStore.getState().generateOccurrences({});
      expect(result.generated_count).toBe(2);
      expect(m.getPmDueMetrics).toHaveBeenCalled();
      expect(m.getPmGenerationStatus).toHaveBeenCalled();
      expectDetailRefreshed(1);
    });

    it("a refresh failing after a successful mutation does not reject it", async () => {
      usePmStore.setState({ activePlanId: 1 });
      m.transitionPmOccurrence.mockResolvedValueOnce(makeOccurrence({ status: "scheduled" }));
      const err = new Error("versions failed");
      m.listPmPlanVersions.mockRejectedValueOnce(err);
      const occ = await usePmStore.getState().transitionOccurrence({
        occurrence_id: 1,
        expected_row_version: 1,
        next_status: "scheduled",
      });
      expect(occ.status).toBe("scheduled");
      expect(usePmStore.getState().detailError).toBe(err);
    });

    it("transitionOccurrence with no open plan only refreshes the workspace lists", async () => {
      m.transitionPmOccurrence.mockResolvedValueOnce(makeOccurrence({ pm_plan_id: 7 }));
      await usePmStore.getState().transitionOccurrence({
        occurrence_id: 1,
        expected_row_version: 1,
        next_status: "scheduled",
      });
      expect(m.listPmPlanVersions).not.toHaveBeenCalled();
      expect(usePmStore.getState().activePlanId).toBeNull();
      expect(m.listPmOccurrences).toHaveBeenCalledWith({ include_completed: false });
    });

    it("executeOccurrence refreshes the open plan and loads the new execution's findings", async () => {
      usePmStore.setState({ activePlanId: 1 });
      const result: ExecutePmOccurrenceResult = {
        occurrence: makeOccurrence({ status: "completed" }),
        execution: makeExecution({ id: 42 }),
        findings: [],
      };
      m.executePmOccurrence.mockResolvedValueOnce(result);
      await expect(
        usePmStore.getState().executeOccurrence({
          occurrence_id: 1,
          expected_occurrence_row_version: 1,
          execution_result: "completed_no_findings",
        }),
      ).resolves.toEqual(result);
      expectDetailRefreshed(1);
      expect(m.listPmFindings).toHaveBeenCalledWith(42);
    });

    it("createPlan reloads the workspace and opens the new plan", async () => {
      m.createPmPlan.mockResolvedValueOnce(makePlan({ id: 5 }));
      const plan = await usePmStore.getState().createPlan(createPlanInput);
      expect(plan.id).toBe(5);
      expect(m.listPmPlans).toHaveBeenCalled();
      expect(usePmStore.getState().activePlanId).toBe(5);
      expectDetailRefreshed(5);
    });

    it("updatePlan forwards id/row version/input and reopens the plan", async () => {
      m.updatePmPlan.mockResolvedValueOnce(makePlan({ id: 1, row_version: 2 }));
      await usePmStore.getState().updatePlan(1, 1, { title: "New" });
      expect(m.updatePmPlan).toHaveBeenCalledWith(1, 1, { title: "New" });
      expect(usePmStore.getState().activePlanId).toBe(1);
    });

    it("transitionPlanLifecycle reloads and reopens the plan", async () => {
      m.transitionPmPlanLifecycle.mockResolvedValueOnce(makePlan({ lifecycle_status: "proposed" }));
      await usePmStore.getState().transitionPlanLifecycle({
        plan_id: 1,
        expected_row_version: 1,
        next_status: "proposed",
      });
      expect(m.listPmPlans).toHaveBeenCalled();
      expectDetailRefreshed(1);
    });

    it("deletePlan closes the deleted active plan and reloads the workspace", async () => {
      usePmStore.setState({ activePlanId: 1, versions: [makeVersion()] });
      m.deletePmPlan.mockResolvedValueOnce(undefined);
      await usePmStore.getState().deletePlan(1, 3);
      expect(m.deletePmPlan).toHaveBeenCalledWith(1, 3);
      expect(usePmStore.getState().activePlanId).toBeNull();
      expect(m.listPmPlans).toHaveBeenCalled();
    });

    it("createVersion / updateVersion / publishVersion / deleteVersion reopen the plan", async () => {
      m.createPmPlanVersion.mockResolvedValueOnce(makeVersion());
      await usePmStore.getState().createVersion(1, createVersionInput);
      expect(m.createPmPlanVersion).toHaveBeenCalledWith(1, createVersionInput);

      m.updatePmPlanVersion.mockResolvedValueOnce(makeVersion({ row_version: 2 }));
      await usePmStore.getState().updateVersion(100, 1, {});
      expect(m.updatePmPlanVersion).toHaveBeenCalledWith(100, 1, {});

      m.publishPmPlanVersion.mockResolvedValueOnce(makeVersion({ status: "published" }));
      await usePmStore.getState().publishVersion(100, 2);
      expect(m.publishPmPlanVersion).toHaveBeenCalledWith({
        version_id: 100,
        expected_row_version: 2,
      });

      m.deletePmPlanVersion.mockResolvedValueOnce(undefined);
      await usePmStore.getState().deleteVersion(100, 3);
      expect(m.deletePmPlanVersion).toHaveBeenCalledWith(100, 3);
      expect(usePmStore.getState().activePlanId).toBe(1);
    });

    it("updateGenerationSettings stores the new status and rethrows failures", async () => {
      const status = makeGenStatus();
      m.updatePmGenerationSettings.mockResolvedValueOnce(status);
      await expect(
        usePmStore.getState().updateGenerationSettings({ enabled: false }),
      ).resolves.toEqual(status);
      expect(usePmStore.getState().generationStatus).toEqual(status);

      m.updatePmGenerationSettings.mockRejectedValueOnce(new Error("bad cron"));
      await expect(
        usePmStore.getState().updateGenerationSettings({ cron_expr: "x" }),
      ).rejects.toThrow("bad cron");
      expect(usePmStore.getState().error).toBeNull();
      expect(usePmStore.getState().saving).toBe(false);
    });
  });
});
