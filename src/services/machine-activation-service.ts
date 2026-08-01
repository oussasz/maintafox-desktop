import { z, ZodError } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  ApplyMachineActivationInput,
  MachineActivationApplyResult,
  MachineActivationDiagnostics,
  MachineActivationStatus,
  OfflineActivationDecision,
  RebindMachineActivationInput,
  RebindMachineActivationResult,
  RotateActivationSecretInput,
  RotateActivationSecretResult,
} from "@shared/ipc-types";

const MachineActivationStatusSchema = z.object({
  contract_id: z.string().nullable(),
  machine_id: z.string().nullable(),
  slot_assignment_id: z.string().nullable(),
  slot_number: z.number().nullable(),
  slot_limit: z.number().nullable(),
  trust_score: z.number().nullable(),
  revocation_state: z.string(),
  issued_at: z.string().nullable(),
  expires_at: z.string().nullable(),
  offline_grace_until: z.string().nullable(),
  drift_score: z.number(),
  drift_within_tolerance: z.boolean(),
  denial_code: z.string().nullable(),
  denial_message: z.string().nullable(),
});

const MachineActivationApplyResultSchema = z.object({
  contract_id: z.string(),
  trusted_binding: z.boolean(),
  drift_score: z.number(),
  slot_assignment_consistent: z.boolean(),
  replay_rejected: z.boolean(),
});

const OfflineActivationDecisionSchema = z.object({
  allowed: z.boolean(),
  denial_code: z.string().nullable(),
  denial_message: z.string().nullable(),
  requires_online_reconnect: z.boolean(),
  grace_hours_remaining: z.number().nullable(),
});

const ActivationLineageRecordSchema = z.object({
  id: z.string(),
  event_code: z.string(),
  contract_id: z.string().nullable(),
  slot_assignment_id: z.string().nullable(),
  detail_json: z.string(),
  occurred_at: z.string(),
  actor_user_id: z.number().nullable(),
});

const MachineActivationDiagnosticsSchema = z.object({
  status: MachineActivationStatusSchema,
  last_reconnect_at: z.string().nullable(),
  last_revocation_applied_at: z.string().nullable(),
  lineage: z.array(ActivationLineageRecordSchema),
  runbook_links: z.array(z.string()),
});

const RotateActivationSecretResultSchema = z.object({
  rotated: z.boolean(),
  rotated_at: z.string(),
  reason: z.string(),
});

const RebindMachineActivationResultSchema = z.object({
  previous_contract_id: z.string().nullable(),
  rebind_required: z.boolean(),
  rebind_requested_at: z.string(),
  reason: z.string(),
});

function normalizeDecodeError(scope: string, err: unknown): Error {
  if (err instanceof ZodError) {
    return new Error(`${scope} response validation failed: ${err.message}`);
  }
  return err instanceof Error ? err : new Error(String(err));
}

export async function applyMachineActivationContract(
  input: ApplyMachineActivationInput,
): Promise<MachineActivationApplyResult> {
  try {
    const raw = await invoke<unknown>("apply_machine_activation_contract", { input });
    return MachineActivationApplyResultSchema.parse(raw) as MachineActivationApplyResult;
  } catch (err) {
    throw normalizeDecodeError("apply_machine_activation_contract", err);
  }
}

export async function getMachineActivationStatus(): Promise<MachineActivationStatus> {
  try {
    const raw = await invoke<unknown>("get_machine_activation_status");
    return MachineActivationStatusSchema.parse(raw) as MachineActivationStatus;
  } catch (err) {
    throw normalizeDecodeError("get_machine_activation_status", err);
  }
}

export async function evaluateOfflineActivationPolicy(): Promise<OfflineActivationDecision> {
  try {
    const raw = await invoke<unknown>("evaluate_offline_activation_policy");
    return OfflineActivationDecisionSchema.parse(raw) as OfflineActivationDecision;
  } catch (err) {
    throw normalizeDecodeError("evaluate_offline_activation_policy", err);
  }
}

export async function rotateActivationBindingSecret(
  input: RotateActivationSecretInput,
): Promise<RotateActivationSecretResult> {
  try {
    const raw = await invoke<unknown>("rotate_activation_binding_secret", { input });
    return RotateActivationSecretResultSchema.parse(raw) as RotateActivationSecretResult;
  } catch (err) {
    throw normalizeDecodeError("rotate_activation_binding_secret", err);
  }
}

export async function getMachineActivationDiagnostics(
  limit?: number,
): Promise<MachineActivationDiagnostics> {
  try {
    const raw = await invoke<unknown>("get_machine_activation_diagnostics", {
      limit: limit ?? null,
    });
    return MachineActivationDiagnosticsSchema.parse(raw) as MachineActivationDiagnostics;
  } catch (err) {
    throw normalizeDecodeError("get_machine_activation_diagnostics", err);
  }
}

export async function requestMachineActivationRebind(
  input: RebindMachineActivationInput,
): Promise<RebindMachineActivationResult> {
  try {
    const raw = await invoke<unknown>("request_machine_activation_rebind", { input });
    return RebindMachineActivationResultSchema.parse(raw) as RebindMachineActivationResult;
  } catch (err) {
    throw normalizeDecodeError("request_machine_activation_rebind", err);
  }
}
