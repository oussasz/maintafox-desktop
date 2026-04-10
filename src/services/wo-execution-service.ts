import { invoke } from "@tauri-apps/api/core";

import type { WorkOrder, WoShift } from "@shared/ipc-types";

export type TaskResultCode = "ok" | "nok" | "na" | "deferred";

export type DowntimeType = "full" | "partial" | "standby" | "quality_loss";

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

export interface WoIntervener {
  id: number;
  work_order_id: number;
  intervener_id: number;
  skill_id: number | null;
  started_at: string | null;
  ended_at: string | null;
  hours_worked: number | null;
  hourly_rate: number | null;
  notes: string | null;
}

export interface AddPartInput {
  wo_id: number;
  article_id?: number | null;
  article_ref?: string | null;
  quantity_planned: number;
  unit_cost?: number | null;
  notes?: string | null;
}

export interface WoPart {
  id: number;
  work_order_id: number;
  article_id: number | null;
  article_ref: string | null;
  quantity_planned: number;
  quantity_used: number | null;
  unit_cost: number | null;
  stock_location_id: number | null;
  notes: string | null;
}

export interface AddTaskInput {
  wo_id: number;
  task_description: string;
  sequence_order: number;
  is_mandatory: boolean;
  estimated_minutes?: number | null;
}

export interface WoTask {
  id: number;
  work_order_id: number;
  task_description: string;
  sequence_order: number;
  estimated_minutes: number | null;
  is_mandatory: boolean;
  is_completed: boolean;
  completed_by_id: number | null;
  completed_at: string | null;
  result_code: TaskResultCode | null;
  notes: string | null;
}

export interface WoDelaySegment {
  id: number;
  work_order_id: number;
  started_at: string;
  ended_at: string | null;
  delay_reason_id: number | null;
  comment: string | null;
  entered_by_id: number | null;
}

export interface WoDowntimeSegment {
  id: number;
  work_order_id: number;
  started_at: string;
  ended_at: string | null;
  downtime_type: DowntimeType;
  comment: string | null;
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
  intervener_id: number,
  ended_at: string,
  actor_id: number,
): Promise<WoIntervener> {
  return invoke<WoIntervener>("close_labor", { intervener_id, ended_at, actor_id });
}

export async function listLabor(wo_id: number): Promise<WoIntervener[]> {
  return invoke<WoIntervener[]>("list_labor", { wo_id });
}

export async function addPart(input: AddPartInput): Promise<WoPart> {
  return invoke<WoPart>("add_part", { input });
}

export async function recordPartUsage(
  wo_part_id: number,
  quantity_used: number,
  unit_cost?: number | null,
): Promise<WoPart> {
  return invoke<WoPart>("record_part_usage", {
    wo_part_id,
    quantity_used,
    unit_cost: unit_cost ?? null,
  });
}

export async function confirmNoParts(wo_id: number): Promise<void> {
  return invoke<void>("confirm_no_parts", { wo_id });
}

export async function listParts(wo_id: number): Promise<WoPart[]> {
  return invoke<WoPart[]>("list_wo_parts", { wo_id });
}

export async function addTask(input: AddTaskInput): Promise<WoTask> {
  return invoke<WoTask>("add_task", { input });
}

export async function completeTask(
  task_id: number,
  actor_id: number,
  result_code: TaskResultCode,
  notes?: string | null,
): Promise<WoTask> {
  return invoke<WoTask>("complete_task", { task_id, actor_id, result_code, notes: notes ?? null });
}

export async function listTasks(wo_id: number): Promise<WoTask[]> {
  return invoke<WoTask[]>("list_tasks", { wo_id });
}

export async function openDowntime(
  wo_id: number,
  downtime_type: DowntimeType,
  actor_id: number,
  comment?: string | null,
): Promise<WoDowntimeSegment> {
  return invoke<WoDowntimeSegment>("open_downtime", {
    wo_id,
    downtime_type,
    actor_id,
    comment: comment ?? null,
  });
}

export async function closeDowntime(
  segment_id: number,
  ended_at?: string | null,
): Promise<WoDowntimeSegment> {
  return invoke<WoDowntimeSegment>("close_downtime", { segment_id, ended_at: ended_at ?? null });
}

export async function listDelaySegments(wo_id: number): Promise<WoDelaySegment[]> {
  return invoke<WoDelaySegment[]>("list_delay_segments", { wo_id });
}

export async function listDowntimeSegments(wo_id: number): Promise<WoDowntimeSegment[]> {
  return invoke<WoDowntimeSegment[]>("list_downtime_segments", { wo_id });
}
