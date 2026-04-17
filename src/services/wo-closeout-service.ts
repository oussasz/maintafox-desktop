/**
 * wo-closeout-service.ts
 *
 * IPC wrappers for WO close-out, verification, cost, attachment, and analytics commands.
 * Phase 2 – Sub-phase 05 – File 03 – Sprint S3.
 */

import { invoke } from "@tauri-apps/api/core";

import { VersionConflictError, WorkOrderSchema } from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";

// ── Types ─────────────────────────────────────────────────────────────────────

// -- Failure details --

export interface SaveFailureDetailInput {
  wo_id: number;
  symptom_id: number | null;
  failure_mode_id: number | null;
  failure_cause_id: number | null;
  failure_effect_id: number | null;
  is_temporary_repair: boolean;
  is_permanent_repair: boolean;
  cause_not_determined: boolean;
  notes: string | null;
}

export interface WoFailureDetail {
  id: number;
  work_order_id: number;
  symptom_id: number | null;
  failure_mode_id: number | null;
  failure_cause_id: number | null;
  failure_effect_id: number | null;
  is_temporary_repair: boolean;
  is_permanent_repair: boolean;
  cause_not_determined: boolean;
  notes: string | null;
}

// -- Verification --

export interface SaveVerificationInput {
  wo_id: number;
  verified_by_id: number;
  result: string;
  return_to_service_confirmed: boolean;
  recurrence_risk_level: string | null;
  notes: string | null;
  expected_row_version: number;
}

export interface WoVerification {
  id: number;
  work_order_id: number;
  verified_by_id: number;
  verified_at: string;
  result: string;
  return_to_service_confirmed: boolean;
  recurrence_risk_level: string | null;
  notes: string | null;
}

// -- Close / Reopen --

export interface WoCloseInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
}

export interface WoReopenInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  reason: string;
}

export interface UpdateWoRcaInput {
  wo_id: number;
  root_cause_summary?: string | null;
  corrective_action_summary?: string | null;
}

// -- Costs --

export interface WoCostSummary {
  wo_id: number;
  labor_cost: number;
  parts_cost: number;
  service_cost: number;
  total_cost: number;
  expected_duration_hours: number | null;
  actual_duration_hours: number | null;
  active_labor_hours: number;
  total_waiting_hours: number;
  duration_variance_hours: number | null;
}

export interface CostPostingHook {
  wo_id: number;
  wo_code: string;
  entity_id: number | null;
  asset_id: number | null;
  type_code: string;
  urgency_level: number | null;
  total_cost: number;
  labor_cost: number;
  parts_cost: number;
  service_cost: number;
  closed_at: string | null;
}

// -- Attachments --

export interface WoAttachment {
  id: number;
  work_order_id: number;
  file_name: string;
  relative_path: string;
  mime_type: string;
  size_bytes: number;
  uploaded_by_id: number | null;
  uploaded_at: string;
  notes: string | null;
}

export interface WoAttachmentUploadInput {
  woId: number;
  fileName: string;
  fileBytes: number[];
  mimeType: string;
  notes?: string | null;
}

// -- Analytics --

export interface WoAnalyticsSnapshot {
  wo_id: number;
  wo_code: string;
  type_code: string;
  asset_id: number | null;
  asset_code: string | null;
  entity_id: number | null;
  urgency_level: number | null;
  source_di_id: number | null;
  submitted_at: string | null;
  actual_start: string | null;
  actual_end: string | null;
  mechanically_completed_at: string | null;
  technically_verified_at: string | null;
  closed_at: string | null;
  expected_duration_hours: number | null;
  actual_duration_hours: number | null;
  active_labor_hours: number;
  total_waiting_hours: number;
  downtime_hours: number;
  schedule_deviation_hours: number | null;
  labor_cost: number;
  parts_cost: number;
  service_cost: number;
  total_cost: number;
  recurrence_risk_level: string | null;
  root_cause_summary: string | null;
  corrective_action_summary: string | null;
  failure_details: WoFailureDetail[];
  verifications: WoVerification[];
  reopen_count: number;
  labor_entries_count: number;
  parts_entries_count: number;
  attachment_count: number;
  task_count: number;
  mandatory_task_count: number;
  completed_task_count: number;
  delay_segment_count: number;
  downtime_segment_count: number;
  was_planned: boolean;
  parts_actuals_confirmed: boolean;
  pm_occurrence_id: number | null;
  permit_ids: number[];
}

// ── Blocking error helper ─────────────────────────────────────────────────────

export class CloseoutBlockingError extends Error {
  blockingErrors: string[];

  constructor(message: string, blockingErrors: string[]) {
    super(message);
    this.name = "CloseoutBlockingError";
    this.blockingErrors = blockingErrors;
  }
}

function extractBlockingErrors(error: unknown): string[] {
  if (typeof error === "string") return [error];
  if (error && typeof error === "object") {
    const obj = error as Record<string, unknown>;
    if (Array.isArray(obj["errors"])) {
      return obj["errors"].filter((x): x is string => typeof x === "string");
    }
    if (Array.isArray(obj["blockingErrors"])) {
      return obj["blockingErrors"].filter((x): x is string => typeof x === "string");
    }
    if (typeof obj["message"] === "string") return [obj["message"]];
  }
  return ["Unknown error."];
}

function maybeWrapBlockingError(error: unknown): never {
  const ipc = error as { code?: string };
  if (ipc?.code === "VALIDATION_FAILED") {
    const blockingErrors = extractBlockingErrors(error);
    throw new CloseoutBlockingError("Close-out blocked.", blockingErrors);
  }
  throw error;
}

// ── Commands ──────────────────────────────────────────────────────────────────

// -- Failure detail --

export async function saveFailureDetail(input: SaveFailureDetailInput): Promise<WoFailureDetail> {
  return invoke<WoFailureDetail>("save_failure_detail", { input });
}

// -- Verification --

export async function saveVerification(
  input: SaveVerificationInput,
): Promise<[WoVerification, WorkOrder]> {
  return invoke<[WoVerification, WorkOrder]>("save_verification", { input });
}

// -- Close / Reopen --

export async function closeWo(input: WoCloseInput): Promise<WorkOrder> {
  try {
    const raw = await invoke<unknown>("close_wo", { input });
    return WorkOrderSchema.parse(raw) as WorkOrder;
  } catch (error) {
    const ipc = error as { code?: string; message?: string };
    if (ipc?.code === "VALIDATION_FAILED" && ipc.message?.includes("version")) {
      throw new VersionConflictError(ipc.message);
    }
    maybeWrapBlockingError(error);
  }
}

export async function reopenWo(input: WoReopenInput): Promise<WorkOrder> {
  return invoke<WorkOrder>("reopen_wo", { input });
}

export async function updateWoRca(input: UpdateWoRcaInput): Promise<void> {
  await invoke<void>("update_wo_rca", { input });
}

// -- Attachments --

export async function listWoAttachments(woId: number): Promise<WoAttachment[]> {
  return invoke<WoAttachment[]>("list_wo_attachments", { woId });
}

export async function uploadWoAttachment(input: WoAttachmentUploadInput): Promise<WoAttachment> {
  return invoke<WoAttachment>("upload_wo_attachment", {
    woId: input.woId,
    fileName: input.fileName,
    fileBytes: input.fileBytes,
    mimeType: input.mimeType,
    notes: input.notes ?? null,
  });
}

export async function deleteWoAttachment(attachmentId: number): Promise<void> {
  await invoke<void>("delete_wo_attachment", { attachmentId });
}

// -- Costs --

export async function updateServiceCost(woId: number, serviceCost: number): Promise<void> {
  await invoke<void>("update_service_cost", { woId, serviceCost });
}

// -- Analytics --

export async function getWoAnalyticsSnapshot(woId: number): Promise<WoAnalyticsSnapshot> {
  return invoke<WoAnalyticsSnapshot>("get_wo_analytics_snapshot", { woId });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Maximum file size for WO attachments (25 MB). */
export const MAX_WO_ATTACHMENT_SIZE_BYTES = 25 * 1024 * 1024;

/** Convert a File to number[] for Tauri IPC transport. */
export async function fileToNumberArray(file: File): Promise<number[]> {
  const buffer = await file.arrayBuffer();
  return Array.from(new Uint8Array(buffer));
}
