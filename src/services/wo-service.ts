/**
 * wo-service.ts
 *
 * IPC wrappers for work order (WO/OT) commands.
 * Phase 2 â€“ Sub-phase 05 â€“ File 01 â€“ Sprint S3 (updated from S4 scaffold).
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  UpdateWorkOrderStatusInput,
  WoApprovePlanningInput,
  WoAssignInput,
  WoBacklogHeatmap,
  WoBacklogHeatmapInput,
  WoCancelInput,
  WoCompletionGate,
  WoCostSummary,
  WoCreateInput,
  WoDraftUpdateInput,
  WoEmergencyReassignInput,
  WoEvaluateReadinessInput,
  WoGetResponse,
  WoHoldInput,
  WoListFilter,
  WoListPage,
  WoMarkReadyInput,
  WoMechCompleteInput,
  WoMechCompleteResponse,
  WoPauseInput,
  WoPlanInput,
  WoPreflightError,
  WoReadinessResult,
  WoResumeInput,
  WoReturnToPlanningInput,
  WoStartInput,
  WoStatsPayload,
  WoSubmitInput,
  WorkOrder,
  WorkOrderStatusOption,
  WorkOrderSubtypeOption,
  WorkOrderTypeOption,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export const WoStatusSchema = z.enum([
  "draft",
  "planning",
  "ready",
  "in_progress",
  "on_hold",
  "completed",
  "closed",
  "cancelled",
]);

export const WoMacroStateSchema = z.enum(["open", "executing", "completed", "closed", "cancelled"]);

export const WorkOrderSchema = z.object({
  id: z.number(),
  code: z.string(),
  type_id: z.number(),
  status_id: z.number(),
  equipment_id: z.number().nullable(),
  component_id: z.number().nullable(),
  location_id: z.number().nullable(),
  requester_id: z.number().nullable(),
  source_di_id: z.number().nullable(),
  source_inspection_anomaly_id: z.number().nullable().optional(),
  source_ram_ishikawa_diagram_id: z.number().nullable().optional(),
  source_ishikawa_flow_node_id: z.string().nullable().optional(),
  source_rca_cause_text: z.string().nullable().optional(),
  entity_id: z.number().nullable(),
  planner_id: z.number().nullable(),
  approver_id: z.number().nullable(),
  assigned_team_id: z.number().nullable(),
  primary_responsible_id: z.number().nullable(),
  urgency_id: z.number().nullable(),
  title: z.string(),
  description: z.string().nullable(),
  planned_start: z.string().nullable(),
  planned_end: z.string().nullable(),
  shift: z.enum(["morning", "afternoon", "night", "full_day"]).nullable(),
  shift_class_version_id: z.number().nullable().default(null),
  shift_class_label: z.string().nullable().default(null),
  shift_start: z.string().nullable().default(null),
  shift_end: z.string().nullable().default(null),
  scheduled_at: z.string().nullable(),
  actual_start: z.string().nullable(),
  actual_end: z.string().nullable(),
  mechanically_completed_at: z.string().nullable(),
  technically_verified_at: z.string().nullable(),
  closed_at: z.string().nullable(),
  cancelled_at: z.string().nullable(),
  expected_duration_hours: z.number().nullable(),
  planned_crew_size: z.number().nullable(),
  actual_duration_hours: z.number().nullable(),
  active_labor_hours: z.number().nullable(),
  total_waiting_hours: z.number().nullable(),
  downtime_hours: z.number().nullable(),
  planned_downtime_hours: z.number().nullable().optional(),
  labor_cost: z.number().nullable(),
  parts_cost: z.number().nullable(),
  service_cost: z.number().nullable(),
  total_cost: z.number().nullable(),
  recurrence_risk_level: z.string().nullable(),
  production_impact_id: z.number().nullable(),
  root_cause_summary: z.string().nullable(),
  corrective_action_summary: z.string().nullable(),
  verification_method: z.string().nullable(),
  notes: z.string().nullable(),
  cancel_reason: z.string().nullable(),
  parts_actuals_confirmed: z.boolean(),
  service_cost_input: z.number().nullable(),
  reopen_count: z.number(),
  last_closed_at: z.string().nullable(),
  requires_permit: z.boolean(),
  entity_sync_id: z.string().nullable().optional(),
  closeout_validation_profile_id: z.number().nullable().optional(),
  closeout_validation_passed: z.boolean().optional(),
  no_downtime_attestation: z.boolean().optional(),
  no_downtime_attestation_reason: z.string().nullable().optional(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  // Join fields (optional)
  status_code: z.string().nullable().optional(),
  status_label: z.string().nullable().optional(),
  status_color: z.string().nullable().optional(),
  type_code: z.string().nullable().optional(),
  type_label: z.string().nullable().optional(),
  nature_code: z
    .enum([
      "corrective_immediate",
      "corrective_deferred",
      "preventive_predetermined",
      "preventive_condition_based",
      "preventive_predictive",
      "improvement",
    ])
    .nullable()
    .optional(),
  nature_label: z.string().nullable().optional(),
  subtype_id: z.number().nullable().optional(),
  subtype_code: z.string().nullable().optional(),
  subtype_label: z.string().nullable().optional(),
  urgency_code: z.enum(["emergency", "urgent", "high", "normal", "low"]).nullable().optional(),
  urgency_rank: z.number().nullable().optional(),
  urgency_label: z.string().nullable().optional(),
  urgency_color: z.string().nullable().optional(),
  asset_code: z.string().nullable().optional(),
  asset_label: z.string().nullable().optional(),
  planner_username: z.string().nullable().optional(),
  responsible_username: z.string().nullable().optional(),
  planner_display_name: z.string().nullable().optional(),
  responsible_display_name: z.string().nullable().optional(),
  source_di_code: z.string().nullable().optional(),
  source_di_title: z.string().nullable().optional(),
  source_di_status: z.string().nullable().optional(),
  assigned_team_code: z.string().nullable().optional(),
  assigned_team_name: z.string().nullable().optional(),
  assigned_team_label: z.string().nullable().optional(),
  schedule_commitment_id: z.number().nullable().optional(),
  schedule_frozen: z.boolean().nullable().optional(),
  lifecycle_state: z.string().nullable().optional(),
  source_inspection_round_id: z.number().nullable().optional(),
  source_inspection_round_code: z.string().nullable().optional(),
  source_inspection_round_title: z.string().nullable().optional(),
  source_inspection_round_scheduled_at: z.string().nullable().optional(),
  source_pm_plan_id: z.number().nullable().optional(),
  source_pm_plan_code: z.string().nullable().optional(),
  source_pm_plan_title: z.string().nullable().optional(),
  source_pm_occurrence_id: z.number().nullable().optional(),
});

export const WoTransitionRowSchema = z.object({
  id: z.number(),
  wo_id: z.number(),
  from_status: z.string(),
  to_status: z.string(),
  action: z.string(),
  actor_id: z.number().nullable(),
  reason_code: z.string().nullable(),
  notes: z.string().nullable(),
  acted_at: z.string(),
  lifecycle_state: z.string().nullable().optional(),
});

const WoListPageSchema = z.object({
  items: z.array(WorkOrderSchema),
  total: z.number(),
});

const WoGetResponseSchema = z.object({
  wo: WorkOrderSchema,
  transitions: z.array(WoTransitionRowSchema),
});

const NatureCodeSchema = z.enum([
  "corrective_immediate",
  "corrective_deferred",
  "preventive_predetermined",
  "preventive_condition_based",
  "preventive_predictive",
  "improvement",
]);

const WorkOrderTypeOptionSchema = z.object({
  id: z.number(),
  code: z.string(),
  label: z.string(),
  label_en: z.string(),
  nature: NatureCodeSchema.nullable(),
  nature_label: z.string().nullable(),
  nature_label_en: z.string().nullable(),
  is_system: z.boolean(),
  is_active: z.boolean(),
});

const WorkOrderSubtypeOptionSchema = z.object({
  id: z.number(),
  code: z.string(),
  label: z.string(),
  nature: NatureCodeSchema.nullable(),
  is_active: z.boolean(),
});

const WorkOrderStatusOptionSchema = z.object({
  id: z.number(),
  code: z.string(),
  label: z.string(),
  color: z.string(),
  macro_state: z.string(),
  is_terminal: z.boolean(),
  is_system: z.boolean(),
  sequence: z.number(),
});

// â”€â”€ Error helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

interface IpcError {
  code: string;
  message: string;
}

interface IpcErrorLike extends IpcError {
  errors?: unknown;
}

function isIpcError(err: unknown): err is IpcError {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

export class VersionConflictError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "VersionConflictError";
  }
}

export class CompletionBlockedError extends Error {
  errors: WoPreflightError[];

  constructor(errors: WoPreflightError[]) {
    super("Mechanical completion blocked.");
    this.name = "CompletionBlockedError";
    this.errors = errors;
  }
}

export function normalizePreflightCode(message: string): string {
  const lower = message.toLowerCase();

  if (lower.includes("open labor") || lower.includes("main-d'œuvre ouvertes")) {
    return "OPEN_LABOR";
  }
  if (lower.includes("mandatory tasks incomplete") || lower.includes("tâches obligatoires")) {
    return "INCOMPLETE_TASKS";
  }
  if (lower.includes("parts actuals") || lower.includes("pièces")) {
    return "MISSING_PARTS";
  }
  if (lower.includes("open downtime") || lower.includes("temps d'arrêt ouverts")) {
    return "OPEN_DOWNTIME";
  }

  return "BLOCKING_ERROR";
}

function extractPreflightErrors(err: unknown): WoPreflightError[] {
  const ipc = err as IpcErrorLike;

  if (Array.isArray(ipc?.errors)) {
    return ipc.errors
      .filter((value): value is string => typeof value === "string")
      .map((message) => ({ code: normalizePreflightCode(message), message }));
  }

  if (isIpcError(err) && err.message) {
    return [{ code: normalizePreflightCode(err.message), message: err.message }];
  }

  return [{ code: "BLOCKING_ERROR", message: "Mechanical completion blocked." }];
}

function rethrowIfVersionConflict(err: unknown): never {
  if (isIpcError(err) && err.code === "VALIDATION_FAILED" && err.message.includes("version")) {
    throw new VersionConflictError(err.message);
  }
  throw err;
}

// â”€â”€ Commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listWos(filter: WoListFilter): Promise<WoListPage> {
  const raw = await invoke<unknown>("list_wo", { filter });
  return WoListPageSchema.parse(raw) as WoListPage;
}

export async function getWo(id: number): Promise<WoGetResponse> {
  const raw = await invoke<unknown>("get_wo", { id });
  return WoGetResponseSchema.parse(raw) as WoGetResponse;
}

export async function listWorkOrderTypes(): Promise<WorkOrderTypeOption[]> {
  const raw = await invoke<unknown>("list_work_order_types");
  return z.array(WorkOrderTypeOptionSchema).parse(raw) as WorkOrderTypeOption[];
}

/** The plant's OT sub-types (`WORK.ORDER_SUBTYPES`) with the nature each belongs to. */
export async function listWorkOrderSubtypes(): Promise<WorkOrderSubtypeOption[]> {
  const raw = await invoke<unknown>("list_work_order_subtypes");
  return z.array(WorkOrderSubtypeOptionSchema).parse(raw) as WorkOrderSubtypeOption[];
}

export async function listWorkOrderStatuses(): Promise<WorkOrderStatusOption[]> {
  const raw = await invoke<unknown>("list_work_order_statuses");
  return z.array(WorkOrderStatusOptionSchema).parse(raw) as WorkOrderStatusOption[];
}

export async function updateWorkOrderStatus(
  id: number,
  input: UpdateWorkOrderStatusInput,
): Promise<WorkOrderStatusOption> {
  const raw = await invoke<unknown>("update_work_order_status", { id, input });
  return WorkOrderStatusOptionSchema.parse(raw) as WorkOrderStatusOption;
}

export async function createWo(input: WoCreateInput): Promise<WorkOrder> {
  const raw = await invoke<unknown>("create_wo", { input });
  return WorkOrderSchema.parse(raw) as WorkOrder;
}

export async function updateWoDraft(input: WoDraftUpdateInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("update_wo_draft", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// â”€â”€ Planning & scheduling â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const WoShiftOptionsSchema = z.object({
  team_id: z.number().nullable(),
  day: z.string().nullable(),
  options: z.array(
    z.object({
      class_version_id: z.number(),
      schedule_class_id: z.number(),
      class_label: z.string(),
      shift_start: z.string(),
      shift_end: z.string(),
      break_minutes: z.number(),
      net_hours: z.number(),
      members: z.number(),
      members_available: z.number(),
      /** A night shift that began the day before and still runs at the planned start. */
      starts_previous_day: z.boolean().default(false),
    }),
  ),
  reason: z.enum(["no_team", "no_day", "no_shift"]).nullable(),
});
export type WoShiftOptionsResult = z.infer<typeof WoShiftOptionsSchema>;

/**
 * The shift windows the work order's assigned team — or `teamId`, the team being assigned —
 * works at `plannedStart` (local `YYYY-MM-DDTHH:mm` as typed, or the stored instant; default: its
 * planned start), from the members' Classes horaires. An early-morning start also lists the
 * previous night's shift still running. Empty with a reason when there is no team, no date, or
 * nobody works then.
 */
export async function listWoShiftOptions(
  woId: number,
  plannedStart: string | null = null,
  teamId: number | null = null,
): Promise<WoShiftOptionsResult> {
  const raw = await invoke<unknown>("list_wo_shift_options", { woId, plannedStart, teamId });
  return WoShiftOptionsSchema.parse(raw);
}

export async function planWo(input: WoPlanInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("plan_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

/** Input for `set_wo_permit_requirement` (actor comes from the session). */
export interface WoPermitRequirementInput {
  wo_id: number;
  expected_row_version: number;
  requires_permit: boolean;
}

/** Statuses in which the permit / LOTO requirement can still change (backend mirror). */
export const PERMIT_REQUIREMENT_EDITABLE_STATUSES: ReadonlySet<string> = new Set([
  "draft",
  "planning",
  "ready",
]);

export async function setWoPermitRequirement(input: WoPermitRequirementInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("set_wo_permit_requirement", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function assignWo(input: WoAssignInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("assign_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function emergencyReassignWo(input: WoEmergencyReassignInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("emergency_reassign_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// ── Option B lifecycle transitions ───────────────────────────────────────────

/** draft → planning */
export async function submitWo(input: WoSubmitInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("submit_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

/** Evaluate planning readiness (non-mutating preflight) */
export async function evaluateWoReadiness(
  input: WoEvaluateReadinessInput,
): Promise<WoReadinessResult> {
  const raw = await invoke<unknown>("evaluate_wo_readiness", { woId: input.wo_id });
  return raw as WoReadinessResult;
}

/** planning → ready */
export async function markWoReady(input: WoMarkReadyInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("mark_wo_ready", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

/** ready → planning (rejection / rework) */
export async function returnToPlanning(input: WoReturnToPlanningInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("return_to_planning", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

/** planning → ready (supervisor approval path) */
export async function approvePlanning(input: WoApprovePlanningInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("approve_planning", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// â”€â”€ Execution lifecycle â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function startWo(input: WoStartInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("start_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function pauseWo(input: WoPauseInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("pause_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function resumeWo(input: WoResumeInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("resume_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function holdWo(input: WoHoldInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("hold_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function completeWoMechanically(
  input: WoMechCompleteInput,
): Promise<WoMechCompleteResponse> {
  try {
    const raw = await invoke<unknown>("complete_wo_mechanically", { input });
    return {
      wo: WorkOrderSchema.parse(raw) as WorkOrder,
      errors: [],
    };
  } catch (err) {
    if (isIpcError(err) && err.code === "VALIDATION_FAILED") {
      throw new CompletionBlockedError(extractPreflightErrors(err));
    }
    rethrowIfVersionConflict(err);
  }
}

export async function evaluateWoCompletionGates(woId: number): Promise<WoCompletionGate[]> {
  const raw = await invoke<unknown>("evaluate_wo_completion_gates", { woId });
  return z
    .array(
      z.object({
        code: z.string(),
        passed: z.boolean(),
        required: z.boolean(),
        detail: z.string().nullable().optional(),
      }),
    )
    .parse(raw) as WoCompletionGate[];
}

export async function cancelWo(input: WoCancelInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("cancel_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// â”€â”€ Cost summary â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const CostSummarySchema = z.object({
  labor_cost: z.number(),
  parts_cost: z.number(),
  service_cost: z.number(),
  total_cost: z.number(),
  expected_duration_hours: z.number().nullable(),
  actual_duration_hours: z.number().nullable(),
});

export async function getCostSummary(woId: number): Promise<WoCostSummary> {
  const raw = await invoke<unknown>("get_cost_summary", { woId });
  return CostSummarySchema.parse(raw) as WoCostSummary;
}

// â”€â”€ Stats / analytics â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const WoStatsSchema = z.object({
  total: z.number(),
  in_progress: z.number(),
  completed: z.number(),
  overdue: z.number(),
  by_status: z.array(z.object({ status: z.string(), count: z.number() })),
  by_lifecycle: z.array(z.object({ status: z.string(), count: z.number() })),
  by_urgency: z.array(z.object({ urgency: z.string(), count: z.number() })),
  daily_completed: z.array(z.object({ date: z.string(), count: z.number() })),
  by_entity: z.array(z.object({ entity: z.string(), count: z.number() })),
});

const WoBacklogBucketSchema = z.object({
  wo_count: z.number(),
  estimated_hours: z.number(),
  unestimated_count: z.number(),
});

const WoBacklogHeatmapSchema = z.object({
  start_date: z.string(),
  days: z.array(
    z.object({
      date: z.string(),
      planned_hours: z.number(),
      wo_count: z.number(),
      unestimated_count: z.number(),
      capacity_hours: z.number().nullable(),
      overloaded: z.boolean(),
    }),
  ),
  capacity_configured: z.boolean(),
  unscheduled: WoBacklogBucketSchema,
  overdue: WoBacklogBucketSchema,
  blocked: z.array(
    z.object({ category: z.string(), wo_count: z.number(), waiting_hours: z.number() }),
  ),
});

export async function getWoBacklogHeatmap(input: WoBacklogHeatmapInput): Promise<WoBacklogHeatmap> {
  const raw = await invoke<unknown>("get_wo_backlog_heatmap", { input });
  return WoBacklogHeatmapSchema.parse(raw);
}

export async function getWoStats(): Promise<WoStatsPayload> {
  const raw = await invoke<unknown>("get_wo_stats", {});
  return WoStatsSchema.parse(raw) as WoStatsPayload;
}
