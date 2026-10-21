/**
 * di-conversion-service.ts
 *
 * IPC wrappers for DI-to-WO conversion and SLA commands.
 * Phase 2 â€“ Sub-phase 04 â€“ File 03 â€“ Sprint S3.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { InterventionRequestSchema } from "@/services/di-service";
import type {
  DiSlaRule,
  DiSlaStatus,
  SlaRuleUpdateInput,
  WoConversionResult,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const PermitFlagOutcomeSchema = z.object({
  requires_permit: z.boolean(),
  reason_code: z.string(),
  rule_code: z.string().nullable(),
  permit_type_code: z.string().nullable(),
  safety_flag: z.boolean(),
  unreadable_rule_codes: z.array(z.string()),
});

const WoConversionResultSchema = z.object({
  di: InterventionRequestSchema,
  wo_id: z.number(),
  wo_code: z.string(),
  permit_flag: PermitFlagOutcomeSchema,
});

const DiSlaStatusSchema = z.object({
  rule_id: z.number().nullable(),
  target_response_hours: z.number().nullable(),
  target_resolution_hours: z.number().nullable(),
  sla_deadline: z.string().nullable(),
  resolution_deadline: z.string().nullable(),
  response_elapsed_hours: z.number().nullable(),
  resolution_elapsed_hours: z.number().nullable(),
  response_remaining_hours: z.number().nullable(),
  resolution_remaining_hours: z.number().nullable(),
  is_response_breached: z.boolean(),
  is_resolution_breached: z.boolean(),
  paused_hours: z.number().default(0),
  status: z.enum(["on_track", "at_risk", "breached", "completed", "paused"]).nullable(),
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

// â”€â”€ Conversion commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * Convert a DI in `approved` to a work order shell.
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

// â”€â”€ SLA commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
