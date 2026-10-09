import { create } from "zustand";

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
import { toErrorMessage } from "@/utils/errors";
import type {
  CreatePmPlanInput,
  CreatePmPlanVersionInput,
  ExecutePmOccurrenceInput,
  ExecutePmOccurrenceResult,
  GeneratePmOccurrencesInput,
  GeneratePmOccurrencesResult,
  PmDueMetrics,
  PmGenerationStatus,
  PmGovernanceKpiInput,
  PmGovernanceKpiReport,
  PmExecution,
  PmFinding,
  PmOccurrence,
  PmOccurrenceFilter,
  PmPlan,
  PmPlanVersion,
  PmPlanningReadinessProjection,
  PmRecurringFinding,
  TransitionPmOccurrenceInput,
  TransitionPmPlanLifecycleInput,
  UpdatePmGenerationSettingsInput,
  UpdatePmPlanInput,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

interface PmStoreState {
  plans: PmPlan[];
  versions: PmPlanVersion[];
  occurrences: PmOccurrence[];
  dueOccurrences: PmOccurrence[];
  calendarOccurrences: PmOccurrence[];
  executions: PmExecution[];
  findingsByExecutionId: Record<number, PmFinding[]>;
  recurringFindings: PmRecurringFinding[];
  planningReadiness: PmPlanningReadinessProjection | null;
  governanceKpis: PmGovernanceKpiReport | null;
  metrics: PmDueMetrics | null;
  generationStatus: PmGenerationStatus | null;
  activePlanId: number | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  loadWorkspace: () => Promise<void>;
  openPlan: (planId: number) => Promise<void>;
  closePlan: () => void;
  loadDueOccurrences: () => Promise<void>;
  loadCalendarOccurrences: () => Promise<void>;
  loadOccurrences: (filter?: PmOccurrenceFilter) => Promise<void>;
  loadDueMetrics: () => Promise<void>;
  loadGenerationStatus: () => Promise<void>;
  updateGenerationSettings: (input: UpdatePmGenerationSettingsInput) => Promise<PmGenerationStatus>;
  loadExecutions: (pmPlanId?: number | null, occurrenceId?: number | null) => Promise<void>;
  loadFindings: (executionId: number) => Promise<void>;
  loadRecurringFindings: (pmPlanId?: number | null) => Promise<void>;
  loadPlanningReadiness: (pmPlanId?: number | null) => Promise<void>;
  loadGovernanceKpis: (input?: PmGovernanceKpiInput) => Promise<void>;
  generateOccurrences: (input: GeneratePmOccurrencesInput) => Promise<GeneratePmOccurrencesResult>;
  transitionOccurrence: (input: TransitionPmOccurrenceInput) => Promise<PmOccurrence>;
  executeOccurrence: (input: ExecutePmOccurrenceInput) => Promise<ExecutePmOccurrenceResult>;
  createPlan: (input: CreatePmPlanInput) => Promise<PmPlan>;
  updatePlan: (
    planId: number,
    expectedRowVersion: number,
    input: UpdatePmPlanInput,
  ) => Promise<PmPlan>;
  transitionPlanLifecycle: (input: TransitionPmPlanLifecycleInput) => Promise<PmPlan>;
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

async function refreshActivePlanDetail(get: () => PmStoreState, planId: number | null) {
  if (planId == null) {
    return;
  }
  const versions = await listPmPlanVersions(planId);
  usePmStore.setState({ versions });
  await get().loadOccurrences({ pm_plan_id: planId, include_completed: true });
  await get().loadExecutions(planId, null);
  await get().loadRecurringFindings(planId);
  await get().loadPlanningReadiness(planId);
  await get().loadGovernanceKpis({ pm_plan_id: planId });
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
  activePlanId: null,
  loading: false,
  saving: false,
  error: null,

  loadWorkspace: async () => {
    set({ loading: true, error: null });
    try {
      const plans = await listPmPlans({});
      set({ plans });
      await get().loadDueOccurrences();
      await get().loadCalendarOccurrences();
      await get().loadDueMetrics();
      await get().loadGenerationStatus();
      const activePlanId = get().activePlanId;
      if (activePlanId != null && plans.some((p) => p.id === activePlanId)) {
        await refreshActivePlanDetail(get, activePlanId);
      }
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  openPlan: async (planId) => {
    set({
      activePlanId: planId,
      versions: [],
      findingsByExecutionId: {},
      error: null,
    });
    try {
      await refreshActivePlanDetail(get, planId);
    } catch (err) {
      set({ error: toErrorMessage(err) });
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
    });
  },

  loadDueOccurrences: async () => {
    try {
      const dueOccurrences = await listPmOccurrences({
        include_completed: false,
      });
      set({ dueOccurrences });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  loadCalendarOccurrences: async () => {
    try {
      const calendarOccurrences = await listPmOccurrences({
        include_completed: true,
      });
      set({ calendarOccurrences });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  loadOccurrences: async (filter = {}) => {
    try {
      const occurrences = await listPmOccurrences(filter);
      set({ occurrences });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  loadDueMetrics: async () => {
    try {
      const metrics = await getPmDueMetrics();
      set({ metrics });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  loadGenerationStatus: async () => {
    try {
      const generationStatus = await getPmGenerationStatus();
      set({ generationStatus });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  updateGenerationSettings: async (input) => {
    set({ saving: true, error: null });
    try {
      const generationStatus = await updatePmGenerationSettings(input);
      set({ generationStatus });
      return generationStatus;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  loadExecutions: async (pmPlanId, occurrenceId) => {
    try {
      const executions = await listPmExecutions({
        pm_plan_id: pmPlanId ?? null,
        occurrence_id: occurrenceId ?? null,
      });
      set({ executions });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
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
      set({ error: toErrorMessage(err) });
    }
  },

  loadRecurringFindings: async (pmPlanId) => {
    try {
      const recurringFindings = await listPmRecurringFindings({
        pm_plan_id: pmPlanId ?? null,
        days_window: 90,
      });
      set({ recurringFindings });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  loadPlanningReadiness: async (pmPlanId) => {
    try {
      const planningReadiness = await listPmPlanningReadiness({
        pm_plan_id: pmPlanId ?? null,
        limit: 200,
      });
      set({ planningReadiness });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  loadGovernanceKpis: async (input = {}) => {
    try {
      const governanceKpis = await getPmGovernanceKpiReport(input);
      set({ governanceKpis });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },

  generateOccurrences: async (input) => {
    set({ saving: true, error: null });
    try {
      const result = await generatePmOccurrences(input);
      await get().loadDueOccurrences();
      await get().loadCalendarOccurrences();
      await get().loadDueMetrics();
      await get().loadGenerationStatus();
      const planId = get().activePlanId;
      if (planId != null) {
        await refreshActivePlanDetail(get, planId);
      }
      return result;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  transitionOccurrence: async (input) => {
    set({ saving: true, error: null });
    try {
      const occurrence = await transitionPmOccurrence(input);
      await get().loadDueOccurrences();
      await get().loadCalendarOccurrences();
      await get().loadDueMetrics();
      const planId = get().activePlanId ?? occurrence.pm_plan_id;
      await refreshActivePlanDetail(get, planId);
      return occurrence;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  executeOccurrence: async (input) => {
    set({ saving: true, error: null });
    try {
      const result = await executePmOccurrence(input);
      await get().loadDueOccurrences();
      await get().loadCalendarOccurrences();
      await get().loadDueMetrics();
      const planId = get().activePlanId ?? result.occurrence.pm_plan_id;
      await refreshActivePlanDetail(get, planId);
      await get().loadFindings(result.execution.id);
      return result;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  createPlan: async (input) => {
    set({ saving: true, error: null });
    try {
      const plan = await createPmPlan(input);
      await get().loadWorkspace();
      await get().openPlan(plan.id);
      return plan;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  updatePlan: async (planId, expectedRowVersion, input) => {
    set({ saving: true, error: null });
    try {
      const plan = await updatePmPlan(planId, expectedRowVersion, input);
      await get().loadWorkspace();
      await get().openPlan(plan.id);
      return plan;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  transitionPlanLifecycle: async (input) => {
    set({ saving: true, error: null });
    try {
      const plan = await transitionPmPlanLifecycle(input);
      await get().loadWorkspace();
      await get().openPlan(plan.id);
      return plan;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  createVersion: async (pmPlanId, input) => {
    set({ saving: true, error: null });
    try {
      const version = await createPmPlanVersion(pmPlanId, input);
      await get().openPlan(pmPlanId);
      return version;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  updateVersion: async (versionId, expectedRowVersion, input) => {
    set({ saving: true, error: null });
    try {
      const version = await updatePmPlanVersion(versionId, expectedRowVersion, input);
      await get().openPlan(version.pm_plan_id);
      return version;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  publishVersion: async (versionId, expectedRowVersion) => {
    set({ saving: true, error: null });
    try {
      const version = await publishPmPlanVersion({
        version_id: versionId,
        expected_row_version: expectedRowVersion,
      });
      await get().loadWorkspace();
      await get().openPlan(version.pm_plan_id);
      return version;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  deletePlan: async (planId, expectedRowVersion) => {
    set({ saving: true, error: null });
    try {
      await deletePmPlan(planId, expectedRowVersion);
      if (get().activePlanId === planId) {
        get().closePlan();
      }
      await get().loadWorkspace();
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },

  deleteVersion: async (versionId, expectedRowVersion) => {
    set({ saving: true, error: null });
    try {
      await deletePmPlanVersion(versionId, expectedRowVersion);
      const planId = get().activePlanId;
      if (planId != null) {
        await get().openPlan(planId);
      }
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },
}));
