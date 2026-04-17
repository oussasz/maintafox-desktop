import { invoke } from "@tauri-apps/api/core";

import type {
  DowntimeType,
  TaskResultCode,
  WoDelaySegment,
  WoDowntimeSegment,
  WoExecPart,
  WoExecTask,
  WoIntervener,
  WoMechCompleteInput,
  WorkOrder,
} from "@shared/ipc-types";

// Re-export canonical types for backward compatibility
export type { TaskResultCode, DowntimeType, WoIntervener, WoDelaySegment, WoDowntimeSegment };

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
  stock_location_id?: number | null;
  auto_reserve?: boolean | null;
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

export async function addPart(input: AddPartInput): Promise<WoExecPart> {
  return invoke<WoExecPart>("add_part", { input });
}

export async function recordPartUsage(
  woPartId: number,
  quantityUsed: number,
  unitCost?: number | null,
): Promise<WoExecPart> {
  return invoke<WoExecPart>("record_part_usage", {
    woPartId,
    quantityUsed,
    unitCost: unitCost ?? null,
  });
}

export async function confirmNoParts(woId: number): Promise<void> {
  return invoke<void>("confirm_no_parts", { woId });
}

export async function listParts(woId: number): Promise<WoExecPart[]> {
  return invoke<WoExecPart[]>("list_wo_parts", { woId });
}

export async function addTask(input: AddTaskInput): Promise<WoExecTask> {
  return invoke<WoExecTask>("add_task", { input });
}

export async function completeTask(
  taskId: number,
  actorId: number,
  resultCode: TaskResultCode,
  notes?: string | null,
): Promise<WoExecTask> {
  return invoke<WoExecTask>("complete_task", { taskId, actorId, resultCode, notes: notes ?? null });
}

export async function listTasks(woId: number): Promise<WoExecTask[]> {
  return invoke<WoExecTask[]>("list_tasks", { woId });
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
