/**
 * di-conversion-service.ts
 *
 * IPC wrappers for DI-to-WO conversion and SLA commands.
 * Phase 2 – Sub-phase 04 – File 03 – Sprint S3.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  WoConversionResult,
  DiSlaStatus,
  DiSlaRule,
  SlaRuleUpdateInput,
} from "@shared/ipc-types";

// ── Zod schemas ───────────────────────────────────────────────────────────────

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
  row_version: z.number(),
  submitter_id: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const WoConversionResultSchema = z.object({
  di: InterventionRequestSchema,
  wo_id: z.number(),
  wo_code: z.string(),
});

const DiSlaStatusSchema = z.object({
  rule_id: z.number().nullable(),
  target_response_hours: z.number().nullable(),
  target_resolution_hours: z.number().nullable(),
  sla_deadline: z.string().nullable(),
  response_elapsed_hours: z.number().nullable(),
  resolution_elapsed_hours: z.number().nullable(),
  is_response_breached: z.boolean(),
  is_resolution_breached: z.boolean(),
});

const DiSlaRuleSchema = z.object({
  id: z.number(),
  name: z.string(),
  urgency_level: z.string(),
  origin_type: z.string().nullable(),
  asset_criticality_class: z.string().nullable(),
  target_response_hours: z.number(),
  target_resolution_hours: z.number(),
  escalation_threshold_hours: z.number(),
  is_active: z.boolean(),
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

// ── Conversion commands ───────────────────────────────────────────────────────

/**
 * Convert a DI in `approved_for_planning` to a work order shell.
 *
 * Step-up reauthentication is validated at the backend command layer via
 * `require_step_up!`. The frontend must ensure step-up has been performed
 * in the current session before calling this function.
 */
export async function convertDiToWo(input: {
  diId: number;
  expectedRowVersion: number;
  conversionNotes?: string;
}): Promise<WoConversionResult> {
  try {
    const raw = await invoke<unknown>("convert_di_to_wo", {
      input: {
        di_id: input.diId,
        actor_id: 0, // overridden by backend from session
        expected_row_version: input.expectedRowVersion,
        conversion_notes: input.conversionNotes ?? null,
      },
    });
    return WoConversionResultSchema.parse(raw) as WoConversionResult;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// ── SLA commands ──────────────────────────────────────────────────────────────

export async function getSlaStatus(diId: number): Promise<DiSlaStatus> {
  const raw = await invoke<unknown>("get_sla_status", { diId });
  return DiSlaStatusSchema.parse(raw) as DiSlaStatus;
}

export async function listSlaRules(): Promise<DiSlaRule[]> {
  const raw = await invoke<unknown>("list_sla_rules");
  return z.array(DiSlaRuleSchema).parse(raw) as DiSlaRule[];
}

export async function updateSlaRule(input: SlaRuleUpdateInput): Promise<DiSlaRule> {
  const raw = await invoke<unknown>("update_sla_rule", { input });
  return DiSlaRuleSchema.parse(raw) as DiSlaRule;
}
