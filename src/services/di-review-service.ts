/**
 * di-review-service.ts
 *
 * IPC wrappers for DI triage / review commands.
 * Phase 2 â€“ Sub-phase 04 â€“ File 02 â€“ Sprint S3.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { InterventionRequestSchema } from "@/services/di-service";
import type {
  DiArchiveInput,
  DiApproveInput,
  DiCancelOwnInput,
  DiCloseInput,
  DiCloseNonExecutableInput,
  DiDeferInput,
  DiReactivateInput,
  DiRejectInput,
  DiReturnInput,
  DiReviewEvent,
  DiScreenInput,
  InterventionRequest,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const DiReviewEventSchema = z.object({
  id: z.number(),
  di_id: z.number(),
  event_type: z.string(),
  actor_id: z.number().nullable(),
  actor_display_name: z.string().nullable(),
  acted_at: z.string(),
  from_status: z.string(),
  to_status: z.string(),
  reason_code: z.string().nullable(),
  notes: z.string().nullable(),
  sla_target_hours: z.number().nullable(),
  sla_deadline: z.string().nullable(),
  sla_resolution_target_hours: z.number().nullable().optional(),
  sla_resolution_deadline: z.string().nullable().optional(),
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

export async function closeDi(input: DiCloseInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("close_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function cancelOwnDi(input: DiCancelOwnInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("cancel_own_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function closeDiAsNonExecutable(
  input: DiCloseNonExecutableInput,
): Promise<InterventionRequest> {
  // Compat path → unified close with no_work_required
  return closeDi({
    di_id: input.di_id,
    actor_id: input.actor_id,
    expected_row_version: input.expected_row_version,
    disposition_code: "no_work_required",
    notes: input.notes ?? null,
    related_di_id: null,
  });
}

export async function archiveDi(input: DiArchiveInput): Promise<InterventionRequest> {
  try {
    const raw = await invoke<unknown>("archive_di", { input });
    return InterventionRequestSchema.parse(raw) as InterventionRequest;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function getDiReviewEvents(diId: number): Promise<DiReviewEvent[]> {
  const raw = await invoke<unknown>("get_di_review_events", { diId });
  return z.array(DiReviewEventSchema).parse(raw) as DiReviewEvent[];
}
