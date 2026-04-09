/**
 * wo-service.ts
 *
 * IPC wrappers for work order (WO/OT) commands.
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S4.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  WoAddLaborInput,
  WoAssignInput,
  WoCancelInput,
  WoCloseInput,
  WoCloseLaborInput,
  WoCostSummary,
  WoCreateInput,
  WoDraftUpdateInput,
  WoGetResponse,
  WoLaborEntry,
  WoListFilter,
  WoListPage,
  WoMechCompleteInput,
  WoMechCompleteResponse,
  WoPartUsage,
  WoPauseInput,
  WoPlanInput,
  WoResumeInput,
  WoStartInput,
  WoStatsPayload,
  WoTask,
  WorkOrder,
} from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

const WorkOrderSchema = z.object({
  id: z.number(),
  code: z.string(),
  title: z.string(),
  description: z.string().nullable(),
  type_id: z.number().nullable(),
  type_label: z.string().nullable(),
  equipment_id: z.number().nullable(),
  equipment_code: z.string().nullable(),
  equipment_name: z.string().nullable(),
  location_id: z.number().nullable(),
  entity_id: z.number().nullable(),
  urgency_id: z.number().nullable(),
  urgency_label: z.string().nullable(),
  status: z.string(),
  assigned_to_id: z.number().nullable(),
  assigned_to_name: z.string().nullable(),
  team_id: z.number().nullable(),
  source_di_id: z.number().nullable(),
  source_di_code: z.string().nullable(),
  planned_start: z.string().nullable(),
  planned_end: z.string().nullable(),
  actual_start: z.string().nullable(),
  actual_end: z.string().nullable(),
  expected_duration_hours: z.number().nullable(),
  actual_duration_hours: z.number().nullable(),
  notes: z.string().nullable(),
  conclusion: z.string().nullable(),
  shift: z.string().nullable().optional(),
  row_version: z.number(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  closed_at: z.string().nullable(),
});

const WoListPageSchema = z.object({
  items: z.array(WorkOrderSchema),
  total: z.number(),
});

const WoGetResponseSchema = z.object({
  wo: WorkOrderSchema,
});

const WoPreflightErrorSchema = z.object({
  code: z.string(),
  message: z.string(),
});

const WoMechCompleteResponseSchema = z.object({
  wo: WorkOrderSchema,
  errors: z.array(WoPreflightErrorSchema),
});

// ── Error helpers ─────────────────────────────────────────────────────────────

interface IpcError {
  code: string;
  message: string;
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
    return WoMechCompleteResponseSchema.parse(raw) as WoMechCompleteResponse;
  } catch (err) {
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

// ── Labor ─────────────────────────────────────────────────────────────────────

const LaborEntrySchema = z.object({
  id: z.number(),
  work_order_id: z.number(),
  intervener_id: z.number(),
  intervener_name: z.string().nullable(),
  skill: z.string().nullable(),
  started_at: z.string().nullable(),
  ended_at: z.string().nullable(),
  hours_worked: z.number().nullable(),
  hourly_rate: z.number().nullable(),
  notes: z.string().nullable(),
  created_at: z.string(),
});

export async function listLabor(workOrderId: number): Promise<WoLaborEntry[]> {
  const raw = await invoke<unknown>("list_wo_labor", { workOrderId });
  return z.array(LaborEntrySchema).parse(raw) as WoLaborEntry[];
}

export async function addLabor(input: WoAddLaborInput): Promise<WoLaborEntry> {
  const raw = await invoke<unknown>("add_wo_labor", { input });
  return LaborEntrySchema.parse(raw) as WoLaborEntry;
}

export async function closeLabor(input: WoCloseLaborInput): Promise<WoLaborEntry> {
  const raw = await invoke<unknown>("close_wo_labor", { input });
  return LaborEntrySchema.parse(raw) as WoLaborEntry;
}

// ── Tasks ─────────────────────────────────────────────────────────────────────

const TaskSchema = z.object({
  id: z.number(),
  work_order_id: z.number(),
  sequence: z.number(),
  description: z.string(),
  is_mandatory: z.boolean(),
  is_completed: z.boolean(),
  completed_at: z.string().nullable(),
  completed_by_id: z.number().nullable(),
});

export async function listTasks(workOrderId: number): Promise<WoTask[]> {
  const raw = await invoke<unknown>("list_wo_tasks", { workOrderId });
  return z.array(TaskSchema).parse(raw) as WoTask[];
}

export async function completeTask(taskId: number): Promise<WoTask> {
  const raw = await invoke<unknown>("complete_wo_task", { taskId });
  return TaskSchema.parse(raw) as WoTask;
}

// ── Parts ─────────────────────────────────────────────────────────────────────

const PartUsageSchema = z.object({
  id: z.number(),
  work_order_id: z.number(),
  part_id: z.number().nullable(),
  part_label: z.string().nullable(),
  quantity_planned: z.number().nullable(),
  quantity_actual: z.number().nullable(),
  unit_cost: z.number().nullable(),
  notes: z.string().nullable(),
});

export async function listParts(workOrderId: number): Promise<WoPartUsage[]> {
  const raw = await invoke<unknown>("list_wo_parts", { workOrderId });
  return z.array(PartUsageSchema).parse(raw) as WoPartUsage[];
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
