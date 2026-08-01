import { create } from "zustand";

import {
  createPmPlan,
  createPmPlanVersion,
  executePmOccurrence,
  generatePmOccurrences,
  getPmDueMetrics,
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
  PmDueMetrics,
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
  UpdatePmPlanInput,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

interface PmStoreState {
  plans: PmPlan[];
  versions: PmPlanVersion[];
  occurrences: PmOccurrence[];
  executions: PmExecution[];
  findingsByExecutionId: Record<number, PmFinding[]>;
  recurringFindings: PmRecurringFinding[];
  planningReadiness: PmPlanningReadinessProjection | null;
  governanceKpis: PmGovernanceKpiReport | null;
  metrics: PmDueMetrics | null;
  selectedPlanId: number | null;
  loading: boolean;
  saving: boolean;
  error: string | null;
  loadPlans: () => Promise<void>;
  selectPlan: (planId: number | null) => Promise<void>;
  loadOccurrences: (filter?: PmOccurrenceFilter) => Promise<void>;
  loadDueMetrics: () => Promise<void>;
  loadExecutions: (pmPlanId?: number | null, occurrenceId?: number | null) => Promise<void>;
  loadFindings: (executionId: number) => Promise<void>;
  loadRecurringFindings: (pmPlanId?: number | null) => Promise<void>;
  loadPlanningReadiness: (pmPlanId?: number | null) => Promise<void>;
  loadGovernanceKpis: (input?: PmGovernanceKpiInput) => Promise<void>;
  generateOccurrences: (input: GeneratePmOccurrencesInput) => Promise<void>;
  transitionOccurrence: (input: TransitionPmOccurrenceInput) => Promise<PmOccurrence>;
  executeOccurrence: (input: ExecutePmOccurrenceInput) => Promise<ExecutePmOccurrenceResult>;
  createPlan: (input: CreatePmPlanInput) => Promise<PmPlan>;
  updatePlan: (planId: number, expectedRowVersion: number, input: UpdatePmPlanInput) => Promise<PmPlan>;
  transitionPlanLifecycle: (input: TransitionPmPlanLifecycleInput) => Promise<PmPlan>;
  createVersion: (pmPlanId: number, input: CreatePmPlanVersionInput) => Promise<PmPlanVersion>;
  updateVersion: (versionId: number, expectedRowVersion: number, input: UpdatePmPlanVersionInput) => Promise<PmPlanVersion>;
  publishVersion: (versionId: number, expectedRowVersion: number) => Promise<PmPlanVersion>;
}

export const usePmStore = create<PmStoreState>()((set, get) => ({
  plans: [],
  versions: [],
  occurrences: [],
  executions: [],
  findingsByExecutionId: {},
  recurringFindings: [],
  planningReadiness: null,
  governanceKpis: null,
  metrics: null,
  selectedPlanId: null,
  loading: false,
  saving: false,
  error: null,

  loadPlans: async () => {
    set({ loading: true, error: null });
    try {
      const plans = await listPmPlans({});
      const selectedPlanId = get().selectedPlanId;
      const nextSelected =
        selectedPlanId && plans.some((p) => p.id === selectedPlanId) ? selectedPlanId : (plans[0]?.id ?? null);
      set({ plans, selectedPlanId: nextSelected });
      if (nextSelected !== null) {
        const versions = await listPmPlanVersions(nextSelected);
        set({ versions });
      } else {
        set({ versions: [] });
      }
      await get().loadOccurrences(nextSelected !== null ? { pm_plan_id: nextSelected } : {});
      await get().loadDueMetrics();
      await get().loadExecutions(nextSelected, null);
      await get().loadRecurringFindings(nextSelected);
      await get().loadPlanningReadiness(nextSelected);
      await get().loadGovernanceKpis({ pm_plan_id: nextSelected ?? null });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  selectPlan: async (planId) => {
    set({ selectedPlanId: planId, versions: [], error: null, findingsByExecutionId: {} });
    try {
      if (planId !== null) {
        const versions = await listPmPlanVersions(planId);
        set({ versions });
      }
      await get().loadOccurrences(planId !== null ? { pm_plan_id: planId } : {});
      await get().loadExecutions(planId, null);
      await get().loadRecurringFindings(planId);
      await get().loadPlanningReadiness(planId);
      await get().loadGovernanceKpis({ pm_plan_id: planId ?? null });
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
      const recurringFindings = await listPmRecurringFindings({ pm_plan_id: pmPlanId ?? null, days_window: 90 });
      set({ recurringFindings });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    }
  },
  loadPlanningReadiness: async (pmPlanId) => {
    try {
      const planningReadiness = await listPmPlanningReadiness({ pm_plan_id: pmPlanId ?? null, limit: 200 });
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
      await generatePmOccurrences(input);
      const planId = get().selectedPlanId;
      await get().loadOccurrences(planId ? { pm_plan_id: planId } : {});
      await get().loadDueMetrics();
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
      const planId = get().selectedPlanId;
      await get().loadOccurrences(planId ? { pm_plan_id: planId } : {});
      await get().loadDueMetrics();
      await get().loadExecutions(planId, null);
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
      const planId = get().selectedPlanId;
      await get().loadOccurrences(planId ? { pm_plan_id: planId } : {});
      await get().loadDueMetrics();
      await get().loadExecutions(planId, null);
      await get().loadFindings(result.execution.id);
      await get().loadRecurringFindings(planId);
      await get().loadPlanningReadiness(planId);
      await get().loadGovernanceKpis({ pm_plan_id: planId ?? null });
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
      await get().loadPlans();
      await get().selectPlan(plan.id);
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
      await get().loadPlans();
      await get().selectPlan(plan.id);
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
      await get().loadPlans();
      await get().selectPlan(plan.id);
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
      await get().loadPlans();
      await get().selectPlan(pmPlanId);
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
      await get().loadPlans();
      await get().selectPlan(version.pm_plan_id);
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
      const version = await publishPmPlanVersion({ version_id: versionId, expected_row_version: expectedRowVersion });
      await get().loadPlans();
      await get().selectPlan(version.pm_plan_id);
      return version;
    } catch (err) {
      set({ error: toErrorMessage(err) });
      throw err;
    } finally {
      set({ saving: false });
    }
  },
}));
