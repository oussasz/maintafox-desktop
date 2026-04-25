/**
 * di-review-service.ts
 *
 * IPC wrappers for DI triage / review commands.
 * Phase 2 â€“ Sub-phase 04 â€“ File 02 â€“ Sprint S3.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  DiApproveInput,
  DiDeferInput,
  DiReactivateInput,
  DiRejectInput,
  DiReturnInput,
  DiReviewEvent,
  DiScreenInput,
  InterventionRequest,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const InterventionRequestSchema = z.object({
  id: z.number(),
  code: z.string(),
  asset_id: z.number(),
  sub_asset_ref: z.string().nullable(),
  org_node_id: z.number(),
  status: z.string(),
  title: z.string(),
  description: z.string(),
  origin_type: z.string(),
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
  review_team_id: z.number().nullable(),
  reviewer_id: z.number().nullable(),
  screened_at: z.string().nullable(),
  approved_at: z.string().nullable(),
  deferred_until: z.string().nullable(),
  declined_at: z.string().nullable(),
  closed_at: z.string().nullable(),
  archived_at: z.string().nullable(),
  converted_to_wo_id: z.number().nullable(),
  converted_at: z.string().nullable(),
  reviewer_note: z.string().nullable(),
  classification_code_id: z.number().nullable(),
  is_recurrence_flag: z.boolean(),
  recurrence_di_id: z.number().nullable(),
  is_modified: z.boolean().default(false),
  row_version: z.number(),
  submitter_id: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const DiReviewEventSchema = z.object({
  id: z.number(),
  di_id: z.number(),
  event_type: z.string(),
  actor_id: z.number().nullable(),
  acted_at: z.string(),
  from_status: z.string(),
  to_status: z.string(),
  reason_code: z.string().nullable(),
  notes: z.string().nullable(),
  sla_target_hours: z.number().nullable(),
  sla_deadline: z.string().nullable(),
  step_up_used: z.boolean(),
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

export async function screenDi(input: DiScreenInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("screen_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function returnDi(input: DiReturnInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("return_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function rejectDi(input: DiRejectInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("reject_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function approveDi(input: DiApproveInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("approve_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function deferDi(input: DiDeferInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("defer_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function reactivateDi(input: DiReactivateInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("reactivate_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function getDiReviewEvents(diId: number): Promise<DiReviewEvent[]> {
  const raw = await invoke<unknown>("get_di_review_events", { diId });
  return z.array(DiReviewEventSchema).parse(raw) as DiReviewEvent[];
}
