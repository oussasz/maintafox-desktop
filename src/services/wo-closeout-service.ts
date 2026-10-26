/**
 * wo-closeout-service.ts
 *
 * IPC wrappers for WO close-out, verification, cost, attachment, and analytics commands.
 * Phase 2 â€“ Sub-phase 05 â€“ File 03 â€“ Sprint S3.
 */

import { z } from "zod";

import { appErrorLines } from "@/lib/app-error";
import { invoke } from "@/lib/ipc-invoke";
import { VersionConflictError, WorkOrderSchema } from "@/services/wo-service";
import { extractIpcValidationDetails, toErrorMessage } from "@/utils/errors";
import type { WorkOrder } from "@shared/ipc-types";

// â”€â”€ Types â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
  /** ISO 14224 B.2 mechanism (WORK.FAILURE_MECHANISMS). */
  failure_mechanism_id?: number | null;
  /** ISO 14224 B.4 detection method (WORK.DETECTION_METHODS). */
  detection_method_id?: number | null;
  /** Version of the work order the screen loaded; refused when changed since (bumped on save). */
  expected_row_version: number;
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
  failure_mode_label?: string | null;
  failure_cause_label?: string | null;
  failure_effect_label?: string | null;
  failure_mechanism_id?: number | null;
  failure_mechanism_label?: string | null;
  detection_method_id?: number | null;
  detection_method_label?: string | null;
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
  no_downtime_attestation?: boolean | null;
  no_downtime_attestation_reason?: string | null;
  fmeca_parts_override_reason?: string | null;
  fmeca_parts_override_signed_by_id?: number | null;
  fmeca_parts_override_signer_password?: string | null;
}

export interface WoReopenInput {
  wo_id: number;
  actor_id: number;
  expected_row_version: number;
  reason: string;
  target_status?: string | null;
  /** Reopening to in progress: estimated remaining time (hours, > 0) for the permit gate. */
  estimated_remaining_hours?: number | null;
}

export interface UpdateWoRcaInput {
  wo_id: number;
  root_cause_summary?: string | null;
  corrective_action_summary?: string | null;
  /** Version of the work order the screen loaded; refused when changed since (bumped on save). */
  expected_row_version: number;
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

export type WoAttachmentPhase = "before" | "during" | "after" | "evidence";

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
  phase: WoAttachmentPhase | null;
}

export interface WoAttachmentUploadInput {
  woId: number;
  fileName: string;
  fileBytes: number[];
  mimeType: string;
  phase?: WoAttachmentPhase | null;
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
  /** PM occurrence that generated this WO. */
  pm_occurrence_id: number | null;
  pm_plan_code: string | null;
  pm_plan_title: string | null;
  pm_due_at: string | null;
  permit_ids: number[];
  permits: WoLinkedPermit[];
}

export interface WoLinkedPermit {
  id: number;
  code: string;
  status: string;
  permit_type_code: string | null;
}

// â”€â”€ Blocking error helper â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export class CloseoutBlockingError extends Error {
  blockingErrors: string[];

  constructor(message: string, blockingErrors: string[]) {
    super(message);
    this.name = "CloseoutBlockingError";
    this.blockingErrors = blockingErrors;
  }
}

/** The reasons of a refused close-out, one translated line each (read from the error's `details`). */
function extractBlockingErrors(error: unknown): string[] {
  return appErrorLines(error);
}

function maybeWrapBlockingError(error: unknown): never {
  const ipc = error as { code?: string };
  if (ipc?.code === "VALIDATION_FAILED") {
    const blockingErrors = extractBlockingErrors(error);
    throw new CloseoutBlockingError("Close-out blocked.", blockingErrors);
  }
  throw error;
}

// â”€â”€ Commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

// -- Failure detail --

const WoFailureDetailSchema = z.object({
  id: z.number(),
  work_order_id: z.number(),
  symptom_id: z.number().nullable(),
  failure_mode_id: z.number().nullable(),
  failure_cause_id: z.number().nullable(),
  failure_effect_id: z.number().nullable(),
  is_temporary_repair: z.boolean(),
  is_permanent_repair: z.boolean(),
  cause_not_determined: z.boolean(),
  notes: z.string().nullable(),
  failure_mode_label: z.string().nullable().optional(),
  failure_cause_label: z.string().nullable().optional(),
  failure_effect_label: z.string().nullable().optional(),
  failure_mechanism_id: z.number().nullable().optional(),
  failure_mechanism_label: z.string().nullable().optional(),
  detection_method_id: z.number().nullable().optional(),
  detection_method_label: z.string().nullable().optional(),
});

const WoForCloseOutSchema = z.object({
  // Lazy: read when a response is parsed, not when this module loads.
  wo: z.lazy(() => WorkOrderSchema),
  failure_detail: WoFailureDetailSchema.nullable(),
});

/** The work order with its stored failure coding, read together for the close-out form. */
export interface WoForCloseOut {
  wo: WorkOrder;
  failure_detail: WoFailureDetail | null;
}

/**
 * The work order and its failure coding (`get_wo`), whatever its status: the close-out form is
 * loaded from it, at the version it carries (AU-18).
 */
export async function getWoForCloseOut(woId: number): Promise<WoForCloseOut> {
  const raw = await invoke<unknown>("get_wo", { id: woId });
  const parsed = WoForCloseOutSchema.parse(raw);
  return {
    wo: parsed.wo as WorkOrder,
    failure_detail: parsed.failure_detail as WoFailureDetail | null,
  };
}

/** A saved failure detail with the work order's new version (for the next chained save). */
export type WoFailureDetailSaved = WoFailureDetail & { wo_row_version: number };

export async function saveFailureDetail(
  input: SaveFailureDetailInput,
): Promise<WoFailureDetailSaved> {
  return invoke<WoFailureDetailSaved>("save_failure_detail", { input });
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

/** Saves the root cause / corrective action; returns the work order's new version. */
export async function updateWoRca(input: UpdateWoRcaInput): Promise<number> {
  return invoke<number>("update_wo_rca", { input });
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
    phase: input.phase ?? null,
    notes: input.notes ?? null,
  });
}

export async function deleteWoAttachment(attachmentId: number): Promise<void> {
  await invoke<void>("delete_wo_attachment", { attachmentId });
}

// -- Costs --

/** Saves the service cost at the version the screen loaded; returns the new version. */
export async function updateServiceCost(
  woId: number,
  serviceCost: number,
  expectedRowVersion: number,
): Promise<number> {
  return invoke<number>("update_service_cost", { woId, serviceCost, expectedRowVersion });
}

/** True when the backend refused a save because the record changed since it was loaded. */
export function isRecordChangedElsewhere(error: unknown): boolean {
  const code = "GATE_RECORD_CHANGED_ELSEWHERE";
  return (
    toErrorMessage(error).includes(code) ||
    extractIpcValidationDetails(error).some((d) => d.code === code || d.message.includes(code))
  );
}

// -- Analytics --

export async function getWoAnalyticsSnapshot(woId: number): Promise<WoAnalyticsSnapshot> {
  return invoke<WoAnalyticsSnapshot>("get_wo_analytics_snapshot", { woId });
}

// â”€â”€ Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/** Maximum file size for WO attachments (25 MB). */
export const MAX_WO_ATTACHMENT_SIZE_BYTES = 25 * 1024 * 1024;

/** Convert a File to number[] for Tauri IPC transport. */
export async function fileToNumberArray(file: File): Promise<number[]> {
  const buffer = await file.arrayBuffer();
  return Array.from(new Uint8Array(buffer));
}
