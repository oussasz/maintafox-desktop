import { invoke } from "@tauri-apps/api/core";
import { z, ZodError } from "zod";

import type {
  ApplyAdminLicenseActionInput,
  ApplyAdminLicenseActionResult,
  ApplyLicensingCompromiseResponseInput,
  ApplyLicensingCompromiseResponseResult,
  LicenseTraceEvent,
  LicenseStatusView,
} from "@shared/ipc-types";

const LicenseStatusViewSchema = z.object({
  entitlement_state: z.string(),
  activation_state: z.string(),
  trust_state: z.string(),
  policy_sync_pending: z.boolean(),
  pending_local_writes: z.number(),
  last_admin_action: z.string().nullable(),
  last_admin_action_at: z.string().nullable(),
  actionable_message: z.string(),
  recovery_paths: z.array(z.string()),
});

const ApplyAdminLicenseActionResultSchema = z.object({
  action_id: z.string(),
  action: z.string(),
  applied_at: z.string(),
  entitlement_state_after: z.string(),
  activation_state_after: z.string(),
  pending_local_writes: z.number(),
  queued_local_writes: z.boolean(),
});

const LicenseTraceEventSchema = z.object({
  id: z.string(),
  correlation_id: z.string(),
  event_type: z.string(),
  source: z.string(),
  subject_type: z.string(),
  subject_id: z.string().nullable(),
  reason_code: z.string().nullable(),
  outcome: z.string(),
  occurred_at: z.string(),
  payload_hash: z.string(),
  previous_hash: z.string().nullable(),
  event_hash: z.string(),
});

const ApplyLicensingCompromiseResponseResultSchema = z.object({
  issuer: z.string(),
  key_id: z.string(),
  policy_sync_pending: z.boolean(),
  forced_revocation: z.boolean(),
  applied_at: z.string(),
});

function normalizeDecodeError(scope: string, err: unknown): Error {
  if (err instanceof ZodError) {
    return new Error(`${scope} response validation failed: ${err.message}`);
  }
  return err instanceof Error ? err : new Error(String(err));
}

export async function getLicenseEnforcementStatus(): Promise<LicenseStatusView> {
  try {
    const raw = await invoke<unknown>("get_license_enforcement_status");
    return LicenseStatusViewSchema.parse(raw) as LicenseStatusView;
  } catch (err) {
    throw normalizeDecodeError("get_license_enforcement_status", err);
  }
}

export async function applyAdminLicenseAction(
  input: ApplyAdminLicenseActionInput,
): Promise<ApplyAdminLicenseActionResult> {
  try {
    const raw = await invoke<unknown>("apply_admin_license_action", { input });
    return ApplyAdminLicenseActionResultSchema.parse(raw) as ApplyAdminLicenseActionResult;
  } catch (err) {
    throw normalizeDecodeError("apply_admin_license_action", err);
  }
}

export async function listLicenseTraceEvents(
  limit?: number,
  correlationId?: string | null,
): Promise<LicenseTraceEvent[]> {
  try {
    const raw = await invoke<unknown>("list_license_trace_events", {
      limit: limit ?? null,
      correlation_id: correlationId ?? null,
    });
    return z.array(LicenseTraceEventSchema).parse(raw) as LicenseTraceEvent[];
  } catch (err) {
    throw normalizeDecodeError("list_license_trace_events", err);
  }
}

export async function applyLicensingCompromiseResponse(
  input: ApplyLicensingCompromiseResponseInput,
): Promise<ApplyLicensingCompromiseResponseResult> {
  try {
    const raw = await invoke<unknown>("apply_licensing_compromise_response", { input });
    return ApplyLicensingCompromiseResponseResultSchema.parse(raw) as ApplyLicensingCompromiseResponseResult;
  } catch (err) {
    throw normalizeDecodeError("apply_licensing_compromise_response", err);
  }
}
