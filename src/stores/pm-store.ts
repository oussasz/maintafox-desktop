/**
 * pm-store — preventive-maintenance workspace (plans, due queue, calendar, plan detail).
 *
 * Error contract (design system §4):
 *  - `error`: the workspace lists could not be loaded or refreshed → the page shows `ErrorState`
 *    with a retry in place of the content.
 *  - `detailError`: the open plan's detail could not be loaded → shown inside the detail dialog.
 *  - A failed mutation (save, transition, delete…) is NOT stored: the global IPC toast already
 *    reports it, and the action rejects so the calling dialog stays open for a retry.
 *  - When a mutation succeeded but the refresh after it failed, the mutation still resolves
 *    (retrying it would duplicate the change); the refresh failure lands in `error`/`detailError`.
 */
import { create } from "zustand";

import {
  type PmComplianceReport,
  type PmExecutionRow,
  type PmOccurrenceRow,
  type PmPlanRow,
  createPmPlan,
  createPmRevisionFromFinding,
  createPmPlanVersion,
  deletePmPlan,
  deletePmPlanVersion,
  executePmOccurrence,
  generatePmOccurrences,
  getPmCompliance,
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
  ExecutePmOccurrenceInput,
  ExecutePmOccurrenceResult,
  GeneratePmOccurrencesInput,
  GeneratePmOccurrencesResult,
  PmDueMetrics,
  PmFinding,
  PmGenerationStatus,
  PmGovernanceKpiInput,
  PmGovernanceKpiReport,
  PmOccurrenceFilter,
  PmPlanVersion,
  PmPlanningReadinessProjection,
  PmRecurringFinding,
  TransitionPmOccurrenceInput,
  TransitionPmPlanLifecycleInput,
  UpdatePmGenerationSettingsInput,
  UpdatePmPlanInput,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

/** Recurring findings look back over this many days (shown in the section title). */
export const PM_RECURRING_FINDINGS_WINDOW_DAYS = 90;
/** Compliance period offered first on the PM page: the home tile's 30 days, so both agree. */
export const PM_COMPLIANCE_DEFAULT_DAYS = 30;
/** Upper bound of readiness candidates loaded for one plan (backend maximum is 1000). */
const PM_READINESS_LIMIT = 200;

interface PmStoreState {
  plans: PmPlanRow[];
  versions: PmPlanVersion[];
  occurrences: PmOccurrenceRow[];
  dueOccurrences: PmOccurrenceRow[];
  calendarOccurrences: PmOccurrenceRow[];
  executions: PmExecutionRow[];
  findingsByExecutionId: Record<number, PmFinding[]>;
  recurringFindings: PmRecurringFinding[];
  planningReadiness: PmPlanningReadinessProjection | null;
  governanceKpis: PmGovernanceKpiReport | null;
  metrics: PmDueMetrics | null;
  generationStatus: PmGenerationStatus | null;
  /** PM compliance over the last `complianceDays` days (KPI tile). */
  compliance: PmComplianceReport | null;
  complianceDays: number;
  activePlanId: number | null;
  loading: boolean;
  detailLoading: boolean;
  saving: boolean;
  /** Workspace lists failed to load (raw failure, rendered through `describeAppError`). */
  error: unknown;
  /** Open plan detail failed to load. */
  detailError: unknown;
  loadWorkspace: () => Promise<void>;
  openPlan: (planId: number) => Promise<void>;
  closePlan: () => void;
  loadDueOccurrences: () => Promise<void>;
  loadCalendarOccurrences: () => Promise<void>;
  loadOccurrences: (filter?: PmOccurrenceFilter) => Promise<void>;
  loadDueMetrics: () => Promise<void>;
  loadGenerationStatus: () => Promise<void>;
  /** Loads compliance for the last `days` days (remembered for refreshes). */
  loadCompliance: (days?: number) => Promise<void>;
  /** Draft version of the finding's plan with the finding as change reason; opens that plan. */
  reviseFromFinding: (findingId: number, planId: number) => Promise<PmPlanVersion>;
  updateGenerationSettings: (input: UpdatePmGenerationSettingsInput) => Promise<PmGenerationStatus>;
  loadExecutions: (pmPlanId?: number | null, occurrenceId?: number | null) => Promise<void>;
  loadFindings: (executionId: number) => Promise<void>;
  loadRecurringFindings: (pmPlanId?: number | null) => Promise<void>;
  loadPlanningReadiness: (pmPlanId?: number | null) => Promise<void>;
  loadGovernanceKpis: (input?: PmGovernanceKpiInput) => Promise<void>;
  generateOccurrences: (input: GeneratePmOccurrencesInput) => Promise<GeneratePmOccurrencesResult>;
  transitionOccurrence: (input: TransitionPmOccurrenceInput) => Promise<PmOccurrenceRow>;
  executeOccurrence: (input: ExecutePmOccurrenceInput) => Promise<ExecutePmOccurrenceResult>;
  createPlan: (input: CreatePmPlanInput) => Promise<PmPlanRow>;
  updatePlan: (
    planId: number,
    expectedRowVersion: number,
    input: UpdatePmPlanInput,
  ) => Promise<PmPlanRow>;
  transitionPlanLifecycle: (input: TransitionPmPlanLifecycleInput) => Promise<PmPlanRow>;
  createVersion: (pmPlanId: number, input: CreatePmPlanVersionInput) => Promise<PmPlanVersion>;
  updateVersion: (
    versionId: number,
    expectedRowVersion: number,
    input: UpdatePmPlanVersionInput,
  ) => Promise<PmPlanVersion>;
  publishVersion: (versionId: number, expectedRowVersion: number) => Promise<PmPlanVersion>;
  deletePlan: (planId: number, expectedRowVersion: number) => Promise<void>;
  deleteVersion: (versionId: number, expectedRowVersion: number) => Promise<void>;
}

type Get = () => PmStoreState;

/** Loads everything the plan detail dialog shows (throws on the first failure). */
async function loadPlanDetail(get: Get, planId: number): Promise<void> {
  const versions = await listPmPlanVersions(planId);
  usePmStore.setState({ versions });
  await get().loadOccurrences({ pm_plan_id: planId, include_completed: true });
  await get().loadExecutions(planId, null);
  await get().loadRecurringFindings(planId);
  await get().loadPlanningReadiness(planId);
  await get().loadGovernanceKpis({ pm_plan_id: planId });
}

/** Refreshes the due queue, calendar and due KPIs (throws on the first failure). */
async function loadOperationalLists(get: Get): Promise<void> {
  await get().loadDueOccurrences();
  await get().loadCalendarOccurrences();
  await get().loadDueMetrics();
}

/** Refresh after a successful mutation: a failure is recorded, never thrown (see header). */
async function refreshAfterMutation(
  step: () => Promise<void>,
  target: "error" | "detailError",
): Promise<void> {
  try {
    await step();
  } catch (err) {
    usePmStore.setState(target === "error" ? { error: err } : { detailError: err });
  }
}

async function refreshPlanAfterMutation(get: Get, planId: number | null): Promise<void> {
  if (planId == null) return;
  usePmStore.setState({ detailError: null });
  await refreshAfterMutation(() => loadPlanDetail(get, planId), "detailError");
}

/** Mutation wrapper: `saving` while in flight; failures propagate to the caller. */
async function withSaving<T>(run: () => Promise<T>): Promise<T> {
  usePmStore.setState({ saving: true });
  try {
    return await run();
  } finally {
    usePmStore.setState({ saving: false });
  }
}

export const usePmStore = create<PmStoreState>()((set, get) => ({
  plans: [],
  versions: [],
  occurrences: [],
  dueOccurrences: [],
  calendarOccurrences: [],
  executions: [],
  findingsByExecutionId: {},
  recurringFindings: [],
  planningReadiness: null,
  governanceKpis: null,
  metrics: null,
  generationStatus: null,
  compliance: null,
  complianceDays: PM_COMPLIANCE_DEFAULT_DAYS,
  activePlanId: null,
  loading: false,
  detailLoading: false,
  saving: false,
  error: null,
  detailError: null,

  loadWorkspace: async () => {
    set({ loading: true, error: null });
    try {
      const plans = await listPmPlans({});
      set({ plans });
      await loadOperationalLists(get);
      await get().loadGenerationStatus();
    } catch (err) {
      set({ error: err });
    } finally {
      set({ loading: false });
    }
    const activePlanId = get().activePlanId;
    if (activePlanId != null && get().plans.some((p) => p.id === activePlanId)) {
      await refreshPlanAfterMutation(get, activePlanId);
    }
  },

  openPlan: async (planId) => {
    set({
      activePlanId: planId,
      versions: [],
      occurrences: [],
      executions: [],
      recurringFindings: [],
      planningReadiness: null,
      governanceKpis: null,
      findingsByExecutionId: {},
      detailError: null,
      detailLoading: true,
    });
    try {
      await loadPlanDetail(get, planId);
    } catch (err) {
      set({ detailError: err });
    } finally {
      set({ detailLoading: false });
    }
  },

  closePlan: () => {
    set({
      activePlanId: null,
      versions: [],
      occurrences: [],
      executions: [],
      findingsByExecutionId: {},
      recurringFindings: [],
      planningReadiness: null,
      governanceKpis: null,
      detailError: null,
    });
  },

  loadDueOccurrences: async () => {
    const dueOccurrences = await listPmOccurrences({ include_completed: false });
    set({ dueOccurrences });
  },

  loadCalendarOccurrences: async () => {
    const calendarOccurrences = await listPmOccurrences({ include_completed: true });
    set({ calendarOccurrences });
  },

  loadOccurrences: async (filter = {}) => {
    const occurrences = await listPmOccurrences(filter);
    set({ occurrences });
  },

  loadDueMetrics: async () => {
    const metrics = await getPmDueMetrics();
    set({ metrics });
  },

  loadGenerationStatus: async () => {
    const generationStatus = await getPmGenerationStatus();
    set({ generationStatus });
  },

  loadCompliance: async (days) => {
    const span = days ?? get().complianceDays;
    const to = new Date();
    const from = new Date(to.getTime() - span * 86_400_000);
    set({ complianceDays: span });
    const compliance = await getPmCompliance({
      from: from.toISOString(),
      to: to.toISOString(),
      pm_plan_id: null,
    });
    set({ compliance });
  },

  reviseFromFinding: (findingId, planId) =>
    withSaving(async () => {
      const version = await createPmRevisionFromFinding(findingId);
      await refreshPlanAfterMutation(get, planId);
      return version;
    }),

  updateGenerationSettings: (input) =>
    withSaving(async () => {
      const generationStatus = await updatePmGenerationSettings(input);
      set({ generationStatus });
      return generationStatus;
    }),

  loadExecutions: async (pmPlanId, occurrenceId) => {
    const executions = await listPmExecutions({
      pm_plan_id: pmPlanId ?? null,
      occurrence_id: occurrenceId ?? null,
    });
    set({ executions });
  },

  loadFindings: async (executionId) => {
    try {
      const findings = await listPmFindings(executionId);
      set((state) => ({
        findingsByExecutionId: {
          ...state.findingsByExecutionId,
          [executionId]: findings,
        },
      }));
    } catch (err) {
      set({ detailError: err });
    }
  },

  loadRecurringFindings: async (pmPlanId) => {
    const recurringFindings = await listPmRecurringFindings({
      pm_plan_id: pmPlanId ?? null,
      days_window: PM_RECURRING_FINDINGS_WINDOW_DAYS,
    });
    set({ recurringFindings });
  },

  loadPlanningReadiness: async (pmPlanId) => {
    const planningReadiness = await listPmPlanningReadiness({
      pm_plan_id: pmPlanId ?? null,
      limit: PM_READINESS_LIMIT,
    });
    set({ planningReadiness });
  },

  loadGovernanceKpis: async (input = {}) => {
    const governanceKpis = await getPmGovernanceKpiReport(input);
    set({ governanceKpis });
  },

  generateOccurrences: (input) =>
    withSaving(async () => {
      const result = await generatePmOccurrences(input);
      await refreshAfterMutation(async () => {
        await loadOperationalLists(get);
        await get().loadGenerationStatus();
      }, "error");
      await refreshPlanAfterMutation(get, get().activePlanId);
      return result;
    }),

  transitionOccurrence: (input) =>
    withSaving(async () => {
      const occurrence = await transitionPmOccurrence(input);
      await refreshAfterMutation(() => loadOperationalLists(get), "error");
      await refreshPlanAfterMutation(get, get().activePlanId);
      return occurrence;
    }),

  executeOccurrence: (input) =>
    withSaving(async () => {
      const result = await executePmOccurrence(input);
      await refreshAfterMutation(() => loadOperationalLists(get), "error");
      const planId = get().activePlanId;
      await refreshPlanAfterMutation(get, planId);
      if (planId != null) await get().loadFindings(result.execution.id);
      return result;
    }),

  createPlan: (input) =>
    withSaving(async () => {
      const plan = await createPmPlan(input);
      await get().loadWorkspace();
      await get().openPlan(plan.id);
      return plan;
    }),

  updatePlan: (planId, expectedRowVersion, input) =>
    withSaving(async () => {
      const plan = await updatePmPlan(planId, expectedRowVersion, input);
      await get().loadWorkspace();
      await get().openPlan(plan.id);
      return plan;
    }),

  transitionPlanLifecycle: (input) =>
    withSaving(async () => {
      const plan = await transitionPmPlanLifecycle(input);
      await get().loadWorkspace();
      await get().openPlan(plan.id);
      return plan;
    }),

  createVersion: (pmPlanId, input) =>
    withSaving(async () => {
      const version = await createPmPlanVersion(pmPlanId, input);
      await get().openPlan(pmPlanId);
      return version;
    }),

  updateVersion: (versionId, expectedRowVersion, input) =>
    withSaving(async () => {
      const version = await updatePmPlanVersion(versionId, expectedRowVersion, input);
      await get().openPlan(version.pm_plan_id);
      return version;
    }),

  publishVersion: (versionId, expectedRowVersion) =>
    withSaving(async () => {
      const version = await publishPmPlanVersion({
        version_id: versionId,
        expected_row_version: expectedRowVersion,
      });
      await get().loadWorkspace();
      await get().openPlan(version.pm_plan_id);
      return version;
    }),

  deletePlan: (planId, expectedRowVersion) =>
    withSaving(async () => {
      await deletePmPlan(planId, expectedRowVersion);
      if (get().activePlanId === planId) {
        get().closePlan();
      }
      await get().loadWorkspace();
    }),

  deleteVersion: (versionId, expectedRowVersion) =>
    withSaving(async () => {
      await deletePmPlanVersion(versionId, expectedRowVersion);
      const planId = get().activePlanId;
      if (planId != null) {
        await get().openPlan(planId);
      }
    }),
}));
