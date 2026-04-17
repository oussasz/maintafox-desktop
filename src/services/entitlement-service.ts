import { invoke } from "@tauri-apps/api/core";
import { z, ZodError } from "zod";

import type {
  EntitlementCapabilityCheck,
  EntitlementDiagnostics,
  EntitlementEnvelopeInput,
  EntitlementRefreshResult,
  EntitlementSummary,
} from "@shared/ipc-types";

const EntitlementSummarySchema = z.object({
  envelope_id: z.string().nullable(),
  state: z.string(),
  effective_state: z.string(),
  tier: z.string().nullable(),
  channel: z.string().nullable(),
  lineage_version: z.number().nullable(),
  valid_until: z.string().nullable(),
  offline_grace_until: z.string().nullable(),
  last_verified_at: z.string().nullable(),
  capability_map_json: z.string(),
  feature_flag_map_json: z.string(),
});

const EntitlementRefreshResultSchema = z.object({
  envelope_id: z.string(),
  verified: z.boolean(),
  verification_result: z.string(),
  effective_state: z.string(),
  active_lineage_version: z.number(),
});

const EntitlementCapabilityCheckSchema = z.object({
  capability: z.string(),
  allowed: z.boolean(),
  reason: z.string(),
  effective_state: z.string(),
  envelope_id: z.string().nullable(),
});

const EntitlementEnvelopeSchema = z.object({
  id: z.number(),
  envelope_id: z.string(),
  previous_envelope_id: z.string().nullable(),
  lineage_version: z.number(),
  issuer: z.string(),
  key_id: z.string(),
  signature_alg: z.string(),
  tier: z.string(),
  state: z.string(),
  channel: z.string(),
  machine_slots: z.number(),
  feature_flags_json: z.string(),
  capabilities_json: z.string(),
  policy_json: z.string(),
  issued_at: z.string(),
  valid_from: z.string(),
  valid_until: z.string(),
  offline_grace_until: z.string(),
  payload_hash: z.string(),
  signature: z.string(),
  verified_at: z.string().nullable(),
  verification_result: z.string(),
  created_at: z.string(),
});

const EntitlementDiagnosticsSchema = z.object({
  summary: EntitlementSummarySchema,
  last_refresh_at: z.string().nullable(),
  last_refresh_error: z.string().nullable(),
  lineage: z.array(EntitlementEnvelopeSchema),
  runbook_links: z.array(z.string()),
});

function formatDecodeError(scope: string, err: unknown): Error {
  if (err instanceof ZodError) {
    return new Error(`${scope} response validation failed: ${err.message}`);
  }
  return err instanceof Error ? err : new Error(String(err));
}

export async function applyEntitlementEnvelope(
  input: EntitlementEnvelopeInput,
): Promise<EntitlementRefreshResult> {
  try {
    const result = await invoke<unknown>("apply_entitlement_envelope", { input });
    return EntitlementRefreshResultSchema.parse(result) as EntitlementRefreshResult;
  } catch (err) {
    throw formatDecodeError("apply_entitlement_envelope", err);
  }
}

export async function getEntitlementSummary(): Promise<EntitlementSummary> {
  try {
    const result = await invoke<unknown>("get_entitlement_summary");
    return EntitlementSummarySchema.parse(result) as EntitlementSummary;
  } catch (err) {
    throw formatDecodeError("get_entitlement_summary", err);
  }
}

export async function checkEntitlementCapability(capability: string): Promise<EntitlementCapabilityCheck> {
  try {
    const result = await invoke<unknown>("check_entitlement_capability", { capability });
    return EntitlementCapabilityCheckSchema.parse(result) as EntitlementCapabilityCheck;
  } catch (err) {
    throw formatDecodeError("check_entitlement_capability", err);
  }
}

export async function getEntitlementDiagnostics(limit?: number): Promise<EntitlementDiagnostics> {
  try {
    const result = await invoke<unknown>("get_entitlement_diagnostics", {
      limit: limit ?? null,
    });
    return EntitlementDiagnosticsSchema.parse(result) as EntitlementDiagnostics;
  } catch (err) {
    throw formatDecodeError("get_entitlement_diagnostics", err);
  }
}
