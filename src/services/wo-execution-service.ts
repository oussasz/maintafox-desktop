import { invoke } from "@tauri-apps/api/core";

import type {
  DowntimeType,
  TaskResultCode,
  WoDelaySegment,
  WoDowntimeSegment,
  WoExecPart,
  WoExecTask,
  WoIntervener,
  WoShift,
  WorkOrder,
} from "@shared/ipc-types";

// Re-export canonical types for backward compatibility
export type { TaskResultCode, DowntimeType, WoIntervener, WoDelaySegment, WoDowntimeSegment };

/** @deprecated Use WoExecTask from @shared/ipc-types */
export type WoTask = WoExecTask;

/** @deprecated Use WoExecPart from @shared/ipc-types */
export type WoPart = WoExecPart;

export interface WoPlanInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  planner_id: number;
  planned_start: string;
  planned_end: string;
  shift?: WoShift | null;
  expected_duration_hours?: number | null;
  urgency_id?: number | null;
}

export interface WoAssignInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  assigned_group_id?: number | null;
  primary_responsible_id?: number | null;
  scheduled_at?: string | null;
}

export interface WoStartInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
}

export interface WoPauseInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  delay_reason_id: number;
  comment?: string | null;
}

export interface WoResumeInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
}

export interface WoHoldInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  delay_reason_id: number;
  comment?: string | null;
}

export interface WoMechCompleteInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  actual_end?: string | null;
  actual_duration_hours?: number | null;
  conclusion?: string | null;
}

export interface AddLaborInput {
  wo_id: number;
  intervener_id: number;
  skill_id?: number | null;
  started_at?: string | null;
  ended_at?: string | null;
  hours_worked?: number | null;
  hourly_rate?: number | null;
  notes?: string | null;
}

export interface AddPartInput {
  wo_id: number;
  article_id?: number | null;
  article_ref?: string | null;
  quantity_planned: number;
  unit_cost?: number | null;
  notes?: string | null;
}

export interface AddTaskInput {
  wo_id: number;
  task_description: string;
  sequence_order: number;
  is_mandatory: boolean;
  estimated_minutes?: number | null;
}

interface IpcErrorLike {
  code?: string;
  message?: string;
}

export class WoBlockingError extends Error {
  blockingErrors: string[];

  constructor(message: string, blockingErrors: string[]) {
    super(message);
    this.name = "WoBlockingError";
    this.blockingErrors = blockingErrors;
  }
}

function toBlockingErrors(error: unknown): string[] {
  if (typeof error === "string") {
    return [error];
  }

  if (error && typeof error === "object") {
    const maybe = error as Record<string, unknown>;

    if (Array.isArray(maybe["blockingErrors"])) {
      return maybe["blockingErrors"].filter((x): x is string => typeof x === "string");
    }

    if (Array.isArray(maybe["errors"])) {
      return maybe["errors"].filter((x): x is string => typeof x === "string");
    }

    if (typeof maybe["message"] === "string") {
      return [maybe["message"]];
    }
  }

  return ["Unknown completion blocking error."];
}

function maybeWrapBlockingError(error: unknown): never {
  const ipc = error as IpcErrorLike;
  if (ipc?.code === "VALIDATION_FAILED") {
    const blockingErrors = toBlockingErrors(error);
    throw new WoBlockingError("Mechanical completion blocked.", blockingErrors);
  }
  throw error;
}

export async function planWo(input: WoPlanInput): Promise<WorkOrder> {
  return invoke<WorkOrder>("plan_wo", { input });
}

export async function assignWo(input: WoAssignInput): Promise<WorkOrder> {
  return invoke<WorkOrder>("assign_wo", { input });
}

export async function startWo(input: WoStartInput): Promise<WorkOrder> {
  return invoke<WorkOrder>("start_wo", { input });
}

export async function pauseWo(input: WoPauseInput): Promise<WorkOrder> {
  return invoke<WorkOrder>("pause_wo", { input });
}

export async function resumeWo(input: WoResumeInput): Promise<WorkOrder> {
  return invoke<WorkOrder>("resume_wo", { input });
}

export async function holdWo(input: WoHoldInput): Promise<WorkOrder> {
  return invoke<WorkOrder>("hold_wo", { input });
}

export async function completeMechanically(input: WoMechCompleteInput): Promise<WorkOrder> {
  try {
    return await invoke<WorkOrder>("complete_wo_mechanically", { input });
  } catch (error) {
    maybeWrapBlockingError(error);
  }
}

export async function addLabor(input: AddLaborInput): Promise<WoIntervener> {
  return invoke<WoIntervener>("add_labor", { input });
}

export async function closeLabor(
  intervenerId: number,
  endedAt: string,
  actorId: number,
): Promise<WoIntervener> {
  return invoke<WoIntervener>("close_labor", { intervenerId, endedAt, actorId });
}

export async function listLabor(woId: number): Promise<WoIntervener[]> {
  return invoke<WoIntervener[]>("list_labor", { woId });
}

export async function addPart(input: AddPartInput): Promise<WoPart> {
  return invoke<WoPart>("add_part", { input });
}

export async function recordPartUsage(
  woPartId: number,
  quantityUsed: number,
  unitCost?: number | null,
): Promise<WoPart> {
  return invoke<WoPart>("record_part_usage", {
    woPartId,
    quantityUsed,
    unitCost: unitCost ?? null,
  });
}

export async function confirmNoParts(woId: number): Promise<void> {
  return invoke<void>("confirm_no_parts", { woId });
}

export async function listParts(woId: number): Promise<WoPart[]> {
  return invoke<WoPart[]>("list_wo_parts", { woId });
}

export async function addTask(input: AddTaskInput): Promise<WoTask> {
  return invoke<WoTask>("add_task", { input });
}

export async function completeTask(
  taskId: number,
  actorId: number,
  resultCode: TaskResultCode,
  notes?: string | null,
): Promise<WoTask> {
  return invoke<WoTask>("complete_task", { taskId, actorId, resultCode, notes: notes ?? null });
}

export async function listTasks(woId: number): Promise<WoTask[]> {
  return invoke<WoTask[]>("list_tasks", { woId });
}

export async function openDowntime(
  woId: number,
  downtimeType: DowntimeType,
  actorId: number,
  comment?: string | null,
): Promise<WoDowntimeSegment> {
  return invoke<WoDowntimeSegment>("open_downtime", {
    woId,
    downtimeType,
    actorId,
    comment: comment ?? null,
  });
}

export async function closeDowntime(
  segmentId: number,
  endedAt?: string | null,
): Promise<WoDowntimeSegment> {
  return invoke<WoDowntimeSegment>("close_downtime", { segmentId, endedAt: endedAt ?? null });
}

export async function listDelaySegments(woId: number): Promise<WoDelaySegment[]> {
  return invoke<WoDelaySegment[]>("list_delay_segments", { woId });
}

export async function listDowntimeSegments(woId: number): Promise<WoDowntimeSegment[]> {
  return invoke<WoDowntimeSegment[]>("list_downtime_segments", { woId });
}
