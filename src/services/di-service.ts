/**
 * di-service.ts
 *
 * IPC wrappers for intervention request (DI) commands.
 * Phase 2 â€“ Sub-phase 04 â€“ File 01 â€“ Sprint S3.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  DiCreateInput,
  DiDraftUpdateInput,
  DiGetResponse,
  DiListFilter,
  DiListPage,
  DiQueueCounts,
  DiResubmitInput,
  DiTriageSubmittedInput,
  InterventionRequest,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * One request as the backend sends it (`di::domain::InterventionRequest`). Shared by every DI
 * service so no field is silently dropped by a local copy of the schema.
 */
export const InterventionRequestSchema = z.object({
  id: z.number(),
  code: z.string(),
  equipment_id: z.number(),
  sub_asset_ref: z.string().nullable(),
  org_node_id: z.number(),
  status: z.string(),
  title: z.string(),
  description: z.string(),
  origin_type: z.string(),
  request_type: z.string(),
  symptom_code_id: z.number().nullable(),
  impact_level: z.string(),
  production_impact: z.boolean(),
  safety_flag: z.boolean(),
  environmental_flag: z.boolean(),
  quality_flag: z.boolean(),
  reported_urgency: z.string(),
  validated_urgency: z.string().nullable(),
  observed_at: z.string().nullable(),
  submitted_at: z.string(),
  reviewer_id: z.number().nullable(),
  screened_at: z.string().nullable(),
  approved_at: z.string().nullable(),
  deferred_until: z.string().nullable(),
  declined_at: z.string().nullable(),
  closed_at: z.string().nullable(),
  archived_at: z.string().nullable(),
  converted_to_wo_id: z.number().nullable(),
  converted_at: z.string().nullable(),
  sla_rule_id: z.number().nullable().optional(),
  sla_target_response_hours: z.number().nullable().optional(),
  sla_target_resolution_hours: z.number().nullable().optional(),
  sla_escalation_threshold_hours: z.number().nullable().optional(),
  sla_response_deadline: z.string().nullable().optional(),
  sla_resolution_deadline: z.string().nullable().optional(),
  sla_response_breach_notified_at: z.string().nullable().optional(),
  sla_resolution_breach_notified_at: z.string().nullable().optional(),
  reviewer_note: z.string().nullable(),
  classification_code_id: z.number().nullable(),
  is_recurrence_flag: z.boolean(),
  recurrence_di_id: z.number().nullable(),
  row_version: z.number(),
  submitter_id: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  asset_code: z.string().nullable().optional(),
  asset_label: z.string().nullable().optional(),
  org_node_code: z.string().nullable().optional(),
  org_node_label: z.string().nullable().optional(),
  submitter_display_name: z.string().nullable().optional(),
  reviewer_display_name: z.string().nullable().optional(),
  converted_to_wo_code: z.string().nullable().optional(),
  converted_to_wo_title: z.string().nullable().optional(),
  disposition_code: z.string().nullable().optional(),
  disposition_notes: z.string().nullable().optional(),
  related_di_id: z.number().nullable().optional(),
  closed_by_id: z.number().nullable().optional(),
  deferred_from_status: z.string().nullable().optional(),
  related_di_code: z.string().nullable().optional(),
  source_inspection_anomaly_id: z.number().nullable().optional(),
  source_inspection_round_id: z.number().nullable().optional(),
  source_inspection_round_code: z.string().nullable().optional(),
  source_inspection_round_title: z.string().nullable().optional(),
  source_inspection_round_scheduled_at: z.string().nullable().optional(),
  sla_state: z
    .enum(["on_track", "at_risk", "breached", "completed", "paused"])
    .nullable()
    .optional(),
  sla_paused_hours: z.number().optional(),
  sla_paused_response_hours: z.number().optional(),
});

const DiTransitionRowSchema = z.object({
  id: z.number(),
  from_status: z.string(),
  to_status: z.string(),
  action: z.string(),
  actor_id: z.number().nullable(),
  actor_display_name: z.string().nullable().optional(),
  reason_code: z.string().nullable(),
  notes: z.string().nullable(),
  acted_at: z.string(),
});

const DiSummaryRowSchema = z.object({
  id: z.number(),
  code: z.string(),
  title: z.string(),
  status: z.string(),
  submitted_at: z.string(),
});

const DiListPageSchema = z.object({
  items: z.array(InterventionRequestSchema),
  total: z.number(),
});

const DiGetResponseSchema = z.object({
  di: InterventionRequestSchema,
  transitions: z.array(DiTransitionRowSchema),
  similar: z.array(DiSummaryRowSchema),
});

// â”€â”€ Error helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listDis(filter: DiListFilter): Promise<DiListPage> {
  const raw = await invoke<unknown>("list_di", { filter });
  return DiListPageSchema.parse(raw) as DiListPage;
}

export async function getDi(id: number): Promise<DiGetResponse> {
  const raw = await invoke<unknown>("get_di", { id });
  return DiGetResponseSchema.parse(raw) as DiGetResponse;
}

export async function createDi(input: DiCreateInput): Promise<InterventionRequest> {
  const raw = await invoke<unknown>("create_di", { input });
  return InterventionRequestSchema.parse(raw) as InterventionRequest;
}

export async function updateDiDraft(input: DiDraftUpdateInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("update_di_draft", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function triageSubmittedDi(
  input: DiTriageSubmittedInput,
): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("triage_submitted_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

const DiQueueCountsSchema = z.object({
  to_screen: z.number(),
  to_review: z.number(),
  awaiting_info: z.number(),
  to_approve: z.number(),
  approved_not_converted: z.number(),
  deferred_due: z.number(),
  sla_at_risk: z.number(),
  mine: z.number(),
  mine_awaiting_info: z.number(),
});

/** Size of every work queue of the requests screen (`mine` = the signed-in user's). */
export async function getDiQueueCounts(): Promise<DiQueueCounts> {
  const raw = await invoke<unknown>("get_di_queue_counts");
  return DiQueueCountsSchema.parse(raw) as DiQueueCounts;
}

/** The requester answers the reviewer's question; the request goes back to review. */
export async function resubmitDi(input: DiResubmitInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("resubmit_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

/** Statuses of a request still being handled (not closed). */
export const DI_OPEN_STATUSES = [
  "submitted",
  "in_review",
  "returned_for_clarification",
  "awaiting_approval",
  "approved",
  "deferred",
] as const;

export interface PossibleDuplicate {
  di: InterventionRequest;
  /** Same failure symptom as the request being written. */
  sameSymptom: boolean;
}

/**
 * Open requests already raised on the same asset (local database, works offline), those with the
 * same symptom first. `excludeId` leaves out the request being edited.
 */
export async function findPossibleDuplicateDis(params: {
  equipmentId: number;
  symptomCodeId: number | null;
  excludeId?: number | null;
}): Promise<PossibleDuplicate[]> {
  const page = await listDis({
    equipment_id: params.equipmentId,
    status: [...DI_OPEN_STATUSES],
    limit: 20,
    offset: 0,
  });
  return page.items
    .filter((di) => di.id !== params.excludeId)
    .map((di) => ({
      di,
      sameSymptom: params.symptomCodeId != null && di.symptom_code_id === params.symptomCodeId,
    }))
    .sort((a, b) => Number(b.sameSymptom) - Number(a.sameSymptom));
}
