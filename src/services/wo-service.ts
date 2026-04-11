/**
 * wo-service.ts
 *
 * IPC wrappers for work order (WO/OT) commands.
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S3 (updated from S4 scaffold).
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  WoAssignInput,
  WoCancelInput,
  WoCloseInput,
  WoCostSummary,
  WoCreateInput,
  WoDraftUpdateInput,
  WoGetResponse,
  WoListFilter,
  WoListPage,
  WoMechCompleteInput,
  WoMechCompleteResponse,
  WoPauseInput,
  WoPlanInput,
  WoPreflightError,
  WoResumeInput,
  WoStartInput,
  WoStatsPayload,
  WorkOrder,
} from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

export const WoStatusSchema = z.enum([
  "draft",
  "awaiting_approval",
  "planned",
  "ready_to_schedule",
  "assigned",
  "waiting_for_prerequisite",
  "in_progress",
  "paused",
  "mechanically_complete",
  "technically_verified",
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
  entity_id: z.number().nullable(),
  planner_id: z.number().nullable(),
  approver_id: z.number().nullable(),
  assigned_group_id: z.number().nullable(),
  primary_responsible_id: z.number().nullable(),
  urgency_id: z.number().nullable(),
  title: z.string(),
  description: z.string().nullable(),
  planned_start: z.string().nullable(),
  planned_end: z.string().nullable(),
  shift: z.enum(["morning", "afternoon", "night", "full_day"]).nullable(),
  scheduled_at: z.string().nullable(),
  actual_start: z.string().nullable(),
  actual_end: z.string().nullable(),
  mechanically_completed_at: z.string().nullable(),
  technically_verified_at: z.string().nullable(),
  closed_at: z.string().nullable(),
  cancelled_at: z.string().nullable(),
  expected_duration_hours: z.number().nullable(),
  actual_duration_hours: z.number().nullable(),
  active_labor_hours: z.number().nullable(),
  total_waiting_hours: z.number().nullable(),
  downtime_hours: z.number().nullable(),
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
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  // Join fields (optional)
  status_code: z.string().nullable().optional(),
  status_label: z.string().nullable().optional(),
  status_color: z.string().nullable().optional(),
  type_code: z.string().nullable().optional(),
  type_label: z.string().nullable().optional(),
  urgency_level: z.number().nullable().optional(),
  urgency_label: z.string().nullable().optional(),
  urgency_color: z.string().nullable().optional(),
  asset_code: z.string().nullable().optional(),
  asset_label: z.string().nullable().optional(),
  planner_username: z.string().nullable().optional(),
  responsible_username: z.string().nullable().optional(),
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
});

const WoListPageSchema = z.object({
  items: z.array(WorkOrderSchema),
  total: z.number(),
});

const WoGetResponseSchema = z.object({
  wo: WorkOrderSchema,
  transitions: z.array(WoTransitionRowSchema),
});

// ── Error helpers ─────────────────────────────────────────────────────────────

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

// ── Commands ──────────────────────────────────────────────────────────────────

export async function listWos(filter: WoListFilter): Promise<WoListPage> {
  const raw = await invoke<unknown>("list_wo", { filter });
  return WoListPageSchema.parse(raw) as WoListPage;
}

export async function getWo(id: number): Promise<WoGetResponse> {
  const raw = await invoke<unknown>("get_wo", { id });
  return WoGetResponseSchema.parse(raw) as WoGetResponse;
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

// ── Planning & scheduling ─────────────────────────────────────────────────────

export async function planWo(input: WoPlanInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("plan_wo", { input });
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

// ── Execution lifecycle ───────────────────────────────────────────────────────

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

export async function closeWo(input: WoCloseInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("close_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function cancelWo(input: WoCancelInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("cancel_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// ── Cost summary ──────────────────────────────────────────────────────────────

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

// ── Stats / analytics ─────────────────────────────────────────────────────────

const WoStatsSchema = z.object({
  total: z.number(),
  in_progress: z.number(),
  completed: z.number(),
  overdue: z.number(),
  by_status: z.array(z.object({ status: z.string(), count: z.number() })),
  by_urgency: z.array(z.object({ urgency: z.string(), count: z.number() })),
  daily_completed: z.array(z.object({ date: z.string(), count: z.number() })),
  by_entity: z.array(z.object({ entity: z.string(), count: z.number() })),
});

export async function getWoStats(): Promise<WoStatsPayload> {
  const raw = await invoke<unknown>("get_wo_stats", {});
  return WoStatsSchema.parse(raw) as WoStatsPayload;
}
