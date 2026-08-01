import { z, ZodError } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  CreatePmPlanInput,
  CreatePmPlanVersionInput,
  ExecutePmOccurrenceInput,
  ExecutePmOccurrenceResult,
  GeneratePmOccurrencesInput,
  GeneratePmOccurrencesResult,
  PmDueMetrics,
  PmGovernanceKpiInput,
  PmGovernanceKpiReport,
  PmPlanningReadinessInput,
  PmPlanningReadinessProjection,
  PmExecution,
  PmExecutionFilter,
  PmFinding,
  PmOccurrence,
  PmOccurrenceFilter,
  PmPlan,
  PmPlanFilter,
  PmPlanVersion,
  PmRecurringFinding,
  PmRecurringFindingsInput,
  PublishPmPlanVersionInput,
  TransitionPmOccurrenceInput,
  TransitionPmPlanLifecycleInput,
  UpdatePmPlanInput,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

const PmPlanSchema = z.object({
  id: z.number(),
  code: z.string(),
  title: z.string(),
  description: z.string().nullable(),
  asset_scope_type: z.string(),
  asset_scope_id: z.number().nullable(),
  strategy_type: z.string(),
  criticality_value_id: z.number().nullable(),
  criticality_code: z.string().nullable(),
  criticality_label: z.string().nullable(),
  assigned_group_id: z.number().nullable(),
  requires_shutdown: z.number(),
  requires_permit: z.number(),
  is_active: z.number(),
  lifecycle_status: z.string(),
  current_version_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const PmPlanVersionSchema = z.object({
  id: z.number(),
  pm_plan_id: z.number(),
  version_no: z.number(),
  status: z.string(),
  effective_from: z.string(),
  effective_to: z.string().nullable(),
  trigger_definition_json: z.string(),
  task_package_json: z.string().nullable(),
  required_parts_json: z.string().nullable(),
  required_skills_json: z.string().nullable(),
  required_tools_json: z.string().nullable(),
  estimated_duration_hours: z.number().nullable(),
  estimated_labor_cost: z.number().nullable(),
  estimated_parts_cost: z.number().nullable(),
  estimated_service_cost: z.number().nullable(),
  change_reason: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const PmOccurrenceSchema = z.object({
  id: z.number(),
  pm_plan_id: z.number(),
  plan_version_id: z.number(),
  due_basis: z.string(),
  due_at: z.string().nullable(),
  due_meter_value: z.number().nullable(),
  generated_at: z.string(),
  status: z.string(),
  linked_work_order_id: z.number().nullable(),
  linked_work_order_code: z.string().nullable(),
  deferral_reason: z.string().nullable(),
  missed_reason: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  plan_code: z.string().nullable(),
  plan_title: z.string().nullable(),
  strategy_type: z.string().nullable(),
});

const PmDueMetricsSchema = z.object({
  as_of: z.string(),
  overdue_count: z.number(),
  due_today_count: z.number(),
  due_next_7d_count: z.number(),
  ready_for_scheduling_count: z.number(),
});
const PmPlanningReadinessBlockerSchema = z.object({
  code: z.string(),
  message: z.string(),
  source: z.string(),
});

const PmPlanningCandidateSchema = z.object({
  occurrence: PmOccurrenceSchema,
  ready_for_scheduling: z.boolean(),
  blockers: z.array(PmPlanningReadinessBlockerSchema),
});

const PmPlanningReadinessProjectionSchema = z.object({
  as_of: z.string(),
  candidate_count: z.number(),
  ready_count: z.number(),
  blocked_count: z.number(),
  derivation_rules: z.array(z.string()),
  candidates: z.array(PmPlanningCandidateSchema),
});

const PmRateKpiSchema = z.object({
  numerator: z.number(),
  denominator: z.number(),
  value_pct: z.number().nullable(),
  derivation: z.string(),
});

const PmEffortVarianceKpiSchema = z.object({
  sample_size: z.number(),
  estimated_hours: z.number(),
  actual_hours: z.number(),
  variance_hours: z.number(),
  variance_pct: z.number().nullable(),
  derivation: z.string(),
});

const PmGovernanceKpiReportSchema = z.object({
  as_of: z.string(),
  from: z.string(),
  to: z.string(),
  pm_plan_id: z.number().nullable(),
  criticality_code: z.string().nullable(),
  compliance: PmRateKpiSchema,
  overdue_risk: PmRateKpiSchema,
  first_pass_completion: PmRateKpiSchema,
  follow_up_ratio: PmRateKpiSchema,
  effort_variance: PmEffortVarianceKpiSchema,
  derivation_rules: z.array(z.string()),
});

const GeneratePmOccurrencesResultSchema = z.object({
  generated_count: z.number(),
  skipped_count: z.number(),
  trigger_events_recorded: z.number(),
  occurrence_ids: z.array(z.number()),
});

const PmExecutionSchema = z.object({
  id: z.number(),
  pm_occurrence_id: z.number(),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  execution_result: z.string(),
  executed_at: z.string(),
  notes: z.string().nullable(),
  actor_id: z.number().nullable(),
  actual_duration_hours: z.number().nullable(),
  actual_labor_hours: z.number().nullable(),
  created_at: z.string().nullable(),
});

const PmFindingSchema = z.object({
  id: z.number(),
  pm_execution_id: z.number(),
  finding_type: z.string(),
  severity: z.string().nullable(),
  description: z.string(),
  follow_up_di_id: z.number().nullable(),
  follow_up_work_order_id: z.number().nullable(),
  follow_up_di_code: z.string().nullable(),
  follow_up_work_order_code: z.string().nullable(),
  created_at: z.string(),
});

const ExecutePmOccurrenceResultSchema = z.object({
  occurrence: PmOccurrenceSchema,
  execution: PmExecutionSchema,
  findings: z.array(PmFindingSchema),
});

const PmRecurringFindingSchema = z.object({
  pm_plan_id: z.number(),
  plan_code: z.string().nullable(),
  finding_type: z.string(),
  occurrence_count: z.number(),
  first_seen_at: z.string(),
  last_seen_at: z.string(),
  latest_severity: z.string().nullable(),
});

interface IpcErrorShape {
  code: string;
  message: string;
}

function isIpcError(err: unknown): err is IpcErrorShape {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

export class PmIpcError extends Error {
  readonly code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = "PmIpcError";
    this.code = code;
  }
}

function mapInvokeError(err: unknown): never {
  if (isIpcError(err)) throw new PmIpcError(err.code, err.message);
  if (err instanceof Error) throw err;
  throw new Error(String(err));
}

async function invokeParsed<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  schema: z.ZodType<T>,
): Promise<T> {
  try {
    const raw = await invoke<unknown>(command, args);
    return schema.parse(raw);
  } catch (err) {
    if (err instanceof ZodError) throw new PmIpcError("VALIDATION_FAILED", err.message);
    mapInvokeError(err);
  }
}

export function listPmPlans(filter: PmPlanFilter): Promise<PmPlan[]> {
  return invokeParsed("list_pm_plans", { filter }, z.array(PmPlanSchema));
}

export function getPmPlan(planId: number): Promise<PmPlan> {
  return invokeParsed("get_pm_plan", { planId }, PmPlanSchema);
}

export function createPmPlan(input: CreatePmPlanInput): Promise<PmPlan> {
  return invokeParsed("create_pm_plan", { input }, PmPlanSchema);
}

export function updatePmPlan(
  planId: number,
  expectedRowVersion: number,
  input: UpdatePmPlanInput,
): Promise<PmPlan> {
  return invokeParsed("update_pm_plan", { planId, expectedRowVersion, input }, PmPlanSchema);
}

export function transitionPmPlanLifecycle(input: TransitionPmPlanLifecycleInput): Promise<PmPlan> {
  return invokeParsed("transition_pm_plan_lifecycle", { input }, PmPlanSchema);
}

export function listPmPlanVersions(pmPlanId: number): Promise<PmPlanVersion[]> {
  return invokeParsed("list_pm_plan_versions", { pmPlanId }, z.array(PmPlanVersionSchema));
}

export function createPmPlanVersion(
  pmPlanId: number,
  input: CreatePmPlanVersionInput,
): Promise<PmPlanVersion> {
  return invokeParsed("create_pm_plan_version", { pmPlanId, input }, PmPlanVersionSchema);
}

export function updatePmPlanVersion(
  versionId: number,
  expectedRowVersion: number,
  input: UpdatePmPlanVersionInput,
): Promise<PmPlanVersion> {
  return invokeParsed(
    "update_pm_plan_version",
    { versionId, expectedRowVersion, input },
    PmPlanVersionSchema,
  );
}

export function publishPmPlanVersion(input: PublishPmPlanVersionInput): Promise<PmPlanVersion> {
  return invokeParsed("publish_pm_plan_version", { input }, PmPlanVersionSchema);
}

export function listPmOccurrences(filter: PmOccurrenceFilter): Promise<PmOccurrence[]> {
  return invokeParsed("list_pm_occurrences", { filter }, z.array(PmOccurrenceSchema));
}

export function generatePmOccurrences(
  input: GeneratePmOccurrencesInput,
): Promise<GeneratePmOccurrencesResult> {
  return invokeParsed("generate_pm_occurrences", { input }, GeneratePmOccurrencesResultSchema);
}

export function transitionPmOccurrence(input: TransitionPmOccurrenceInput): Promise<PmOccurrence> {
  return invokeParsed("transition_pm_occurrence", { input }, PmOccurrenceSchema);
}

export function listPmPlanningReadiness(
  input: PmPlanningReadinessInput,
): Promise<PmPlanningReadinessProjection> {
  return invokeParsed("list_pm_planning_readiness", { input }, PmPlanningReadinessProjectionSchema);
}

export function getPmGovernanceKpiReport(
  input: PmGovernanceKpiInput,
): Promise<PmGovernanceKpiReport> {
  return invokeParsed("get_pm_governance_kpi_report", { input }, PmGovernanceKpiReportSchema);
}
export function getPmDueMetrics(): Promise<PmDueMetrics> {
  return invokeParsed("get_pm_due_metrics", undefined, PmDueMetricsSchema);
}

export function executePmOccurrence(
  input: ExecutePmOccurrenceInput,
): Promise<ExecutePmOccurrenceResult> {
  return invokeParsed("execute_pm_occurrence", { input }, ExecutePmOccurrenceResultSchema);
}

export function listPmExecutions(filter: PmExecutionFilter): Promise<PmExecution[]> {
  return invokeParsed("list_pm_executions", { filter }, z.array(PmExecutionSchema));
}

export function listPmFindings(executionId: number): Promise<PmFinding[]> {
  return invokeParsed("list_pm_findings", { executionId }, z.array(PmFindingSchema));
}

export function listPmRecurringFindings(
  input: PmRecurringFindingsInput,
): Promise<PmRecurringFinding[]> {
  return invokeParsed("list_pm_recurring_findings", { input }, z.array(PmRecurringFindingSchema));
}
