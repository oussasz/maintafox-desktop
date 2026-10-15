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
  PmGenerationStatus,
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
  UpdatePmGenerationSettingsInput,
  UpdatePmPlanInput,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

/**
 * Target asset of an equipment-scoped plan (read-only join done by the backend). Kept here until
 * `shared/ipc-types.ts` `PmPlan` / `PmOccurrence` carry these two optional fields.
 */
export interface PmAssetRef {
  asset_code?: string | null | undefined;
  asset_name?: string | null | undefined;
}
export type PmPlanRow = PmPlan &
  PmAssetRef & {
    /** Name of the family / location / criticality group of a fanned-out plan. */
    asset_scope_label?: string | null | undefined;
  };

/** `on_time` | `early` | `late` against the occurrence's tolerance window. */
export type PmTimingStatus = "on_time" | "early" | "late";

/** Occurrence read model: its asset (one per asset for fanned-out plans) and tolerance window. */
export type PmOccurrenceRow = PmOccurrence &
  PmAssetRef & {
    equipment_id?: number | null | undefined;
    window_start_at?: string | null | undefined;
    window_end_at?: string | null | undefined;
    window_start_meter?: number | null | undefined;
    window_end_meter?: number | null | undefined;
    timing_status?: string | null | undefined;
  };

/** Execution with the meter reading at completion and its timing. */
export type PmExecutionRow = PmExecution & {
  meter_reading?: number | null | undefined;
  timing_status?: string | null | undefined;
};

export interface PmComplianceInput {
  from: string;
  to: string;
  pm_plan_id?: number | null;
}

/** PM compliance (SMRP 5.4.1 style): work inside its window ÷ PM occurrences due in the period. */
export interface PmComplianceReport {
  from: string;
  to: string;
  pm_plan_id: number | null;
  due_count: number;
  on_time_count: number;
  early_count: number;
  late_count: number;
  not_done_count: number;
  /** Due, still inside its window and not done: not judged yet (not in `due_count`). */
  pending_count?: number | undefined;
  compliance_pct: number | null;
}

export interface PmScopeOption {
  id: number;
  code: string;
  label: string;
}

export interface PmScopeAsset {
  equipment_id: number;
  asset_code: string;
  asset_name: string;
}

/** A meter with its lifetime value (rollover and replacement keep counting). */
export interface PmAssetMeter {
  id: number;
  name: string;
  meter_type: string;
  unit: string | null;
  current_reading: number;
  lifetime_reading: number;
  rollover_value: number | null;
  last_read_at: string | null;
  is_primary: boolean;
}

export type PmMeterReadingKind = "normal" | "rollover" | "replacement";

export interface RecordPmMeterReadingInput {
  meter_id: number;
  reading_value: number;
  reading_at: string | null;
  kind: PmMeterReadingKind | null;
}

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
  assigned_team_id: z.number().nullable(),
  assigned_team_code: z.string().nullable(),
  assigned_team_name: z.string().nullable(),
  assigned_team_label: z.string().nullable(),
  requires_shutdown: z.number(),
  requires_permit: z.number(),
  is_active: z.number(),
  lifecycle_status: z.string(),
  current_version_id: z.number().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  asset_code: z.string().nullable().optional(),
  asset_name: z.string().nullable().optional(),
  asset_scope_label: z.string().nullable().optional(),
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
  asset_code: z.string().nullable().optional(),
  asset_name: z.string().nullable().optional(),
  equipment_id: z.number().nullable().optional(),
  window_start_at: z.string().nullable().optional(),
  window_end_at: z.string().nullable().optional(),
  window_start_meter: z.number().nullable().optional(),
  window_end_meter: z.number().nullable().optional(),
  timing_status: z.string().nullable().optional(),
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
  failures_found_by_pm: z.number(),
  effort_variance: PmEffortVarianceKpiSchema,
  derivation_rules: z.array(z.string()),
});

const GeneratePmOccurrencesResultSchema = z.object({
  generated_count: z.number(),
  skipped_count: z.number(),
  trigger_events_recorded: z.number(),
  occurrence_ids: z.array(z.number()),
});

const PmGenerationControlSchema = z.object({
  id: z.number(),
  enabled: z.boolean(),
  cron_expr: z.string(),
  horizon_days: z.number(),
  last_run_at: z.string().nullable(),
  next_run_at: z.string().nullable(),
  last_status: z.string().nullable(),
  last_generated_count: z.number().nullable(),
  last_skipped_count: z.number().nullable(),
  last_error: z.string().nullable(),
  updated_at: z.string(),
});

const PmGenerationRunSchema = z.object({
  id: z.number(),
  started_at: z.string(),
  finished_at: z.string().nullable(),
  trigger_source: z.string(),
  status: z.string(),
  generated_count: z.number(),
  skipped_count: z.number(),
  trigger_events_recorded: z.number(),
  error_message: z.string().nullable(),
  scope_json: z.string().nullable(),
});

const PmGenerationStatusSchema = z.object({
  control: PmGenerationControlSchema,
  latest_run: PmGenerationRunSchema.nullable(),
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
  meter_reading: z.number().nullable().optional(),
  timing_status: z.string().nullable().optional(),
});

const PmComplianceReportSchema = z.object({
  from: z.string(),
  to: z.string(),
  pm_plan_id: z.number().nullable(),
  due_count: z.number(),
  on_time_count: z.number(),
  early_count: z.number(),
  late_count: z.number(),
  not_done_count: z.number(),
  pending_count: z.number().optional(),
  compliance_pct: z.number().nullable(),
});

const PmScopeOptionSchema = z.object({ id: z.number(), code: z.string(), label: z.string() });

const PmScopeAssetSchema = z.object({
  equipment_id: z.number(),
  asset_code: z.string(),
  asset_name: z.string(),
});

const PmAssetMeterSchema = z.object({
  id: z.number(),
  name: z.string(),
  meter_type: z.string(),
  unit: z.string().nullable(),
  current_reading: z.number(),
  lifetime_reading: z.number(),
  rollover_value: z.number().nullable(),
  last_read_at: z.string().nullable(),
  is_primary: z.boolean(),
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
  /**
   * True when the failure came back from the backend: the global IPC toast has already told the
   * user, so screens must not repeat it. False for a response the app could not read.
   */
  readonly reported: boolean;
  constructor(code: string, message: string, reported = true) {
    super(message);
    this.name = "PmIpcError";
    this.code = code;
    this.reported = reported;
  }
}

/** True when the global IPC toast already reported this failure. */
export function isReportedPmError(err: unknown): boolean {
  return err instanceof PmIpcError && err.reported;
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
  let raw: unknown;
  try {
    raw = await invoke<unknown>(command, args);
  } catch (err) {
    mapInvokeError(err);
  }
  try {
    return schema.parse(raw);
  } catch (err) {
    if (err instanceof ZodError) {
      throw new PmIpcError("RESPONSE_SHAPE_INVALID", err.message, false);
    }
    throw err;
  }
}

export function listPmPlans(filter: PmPlanFilter): Promise<PmPlanRow[]> {
  return invokeParsed("list_pm_plans", { filter }, z.array(PmPlanSchema));
}

export function getPmPlan(planId: number): Promise<PmPlanRow> {
  return invokeParsed("get_pm_plan", { planId }, PmPlanSchema);
}

export function createPmPlan(input: CreatePmPlanInput): Promise<PmPlanRow> {
  return invokeParsed("create_pm_plan", { input }, PmPlanSchema);
}

export function updatePmPlan(
  planId: number,
  expectedRowVersion: number,
  input: UpdatePmPlanInput,
): Promise<PmPlanRow> {
  return invokeParsed("update_pm_plan", { planId, expectedRowVersion, input }, PmPlanSchema);
}

export function transitionPmPlanLifecycle(
  input: TransitionPmPlanLifecycleInput,
): Promise<PmPlanRow> {
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

export function listPmOccurrences(filter: PmOccurrenceFilter): Promise<PmOccurrenceRow[]> {
  return invokeParsed("list_pm_occurrences", { filter }, z.array(PmOccurrenceSchema));
}

export function generatePmOccurrences(
  input: GeneratePmOccurrencesInput,
): Promise<GeneratePmOccurrencesResult> {
  return invokeParsed("generate_pm_occurrences", { input }, GeneratePmOccurrencesResultSchema);
}

export function getPmGenerationStatus(): Promise<PmGenerationStatus> {
  return invokeParsed("get_pm_generation_status", undefined, PmGenerationStatusSchema);
}

export function updatePmGenerationSettings(
  input: UpdatePmGenerationSettingsInput,
): Promise<PmGenerationStatus> {
  return invokeParsed("update_pm_generation_settings", { input }, PmGenerationStatusSchema);
}

export function transitionPmOccurrence(
  input: TransitionPmOccurrenceInput,
): Promise<PmOccurrenceRow> {
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

export function listPmExecutions(filter: PmExecutionFilter): Promise<PmExecutionRow[]> {
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

export function deletePmPlan(planId: number, expectedRowVersion: number): Promise<void> {
  return invoke("delete_pm_plan", { planId, expectedRowVersion });
}

export function deletePmPlanVersion(versionId: number, expectedRowVersion: number): Promise<void> {
  return invoke("delete_pm_plan_version", { versionId, expectedRowVersion });
}

export function getPmOccurrence(occurrenceId: number): Promise<PmOccurrenceRow> {
  return invokeParsed("get_pm_occurrence", { occurrenceId }, PmOccurrenceSchema);
}

export function getPmCompliance(input: PmComplianceInput): Promise<PmComplianceReport> {
  return invokeParsed("get_pm_compliance", { input }, PmComplianceReportSchema);
}

export function listPmScopeOptions(scopeType: string): Promise<PmScopeOption[]> {
  return invokeParsed("list_pm_scope_options", { scopeType }, z.array(PmScopeOptionSchema));
}

export function previewPmScopeAssets(scopeType: string, scopeId: number): Promise<PmScopeAsset[]> {
  return invokeParsed(
    "preview_pm_scope_assets",
    { scopeType, scopeId },
    z.array(PmScopeAssetSchema),
  );
}

export function listPmAssetMeters(equipmentId: number): Promise<PmAssetMeter[]> {
  return invokeParsed("list_pm_asset_meters", { equipmentId }, z.array(PmAssetMeterSchema));
}

export function recordPmMeterReading(input: RecordPmMeterReadingInput): Promise<PmAssetMeter> {
  return invokeParsed("record_pm_meter_reading", { input }, PmAssetMeterSchema);
}

export function createPmRevisionFromFinding(findingId: number): Promise<PmPlanVersion> {
  return invokeParsed("create_pm_revision_from_finding", { findingId }, PmPlanVersionSchema);
}
