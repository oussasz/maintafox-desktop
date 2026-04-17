import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const ProductLicenseActivationStatusSchema = z.enum([
  "uninitialized",
  "pending_online_validation",
  "active",
  "degraded_api_unavailable",
  "denied_revoked",
  "denied_expired",
  "denied_slot_limit",
  "denied_force_update_required",
  "denied_invalid",
]);

const ProductLicenseOnboardingStateSchema = z.object({
  complete: z.boolean(),
  status: ProductLicenseActivationStatusSchema,
  pending_online_validation: z.boolean(),
  deny_reason_code: z.string().nullable().optional(),
  deny_message: z.string().nullable().optional(),
  degraded_reason: z.string().nullable().optional(),
  next_retry_at: z.string().nullable().optional(),
  retry_attempt: z.number().int().nonnegative().optional(),
  last_reconciled_at: z.string().nullable().optional(),
  last_error_code: z.string().nullable().optional(),
  last_error_message: z.string().nullable().optional(),
});

const ProductLicenseDiagnosticEventSchema = z.object({
  at: z.string(),
  kind: z.string(),
  message: z.string(),
  code: z.string().nullable().optional(),
});

const ProductLicenseDiagnosticsSchema = z.object({
  status: ProductLicenseActivationStatusSchema,
  deny_reason_code: z.string().nullable().optional(),
  deny_message: z.string().nullable().optional(),
  pending_online_validation: z.boolean(),
  last_reconciled_at: z.string().nullable().optional(),
  machine_fingerprint: z.string().nullable().optional(),
  app_version: z.string().nullable().optional(),
  reconciliation: z.object({
    retry_attempt: z.number().int().nonnegative(),
    next_retry_at: z.string().nullable().optional(),
    last_attempt_at: z.string().nullable().optional(),
    last_success_at: z.string().nullable().optional(),
    last_error_code: z.string().nullable().optional(),
    last_error_message: z.string().nullable().optional(),
  }),
  diagnostics: z.array(ProductLicenseDiagnosticEventSchema),
  has_activation_claim: z.boolean(),
});

const ActivationClaimSchema = z.object({
  tenant_id: z.string(),
  license_id: z.string(),
  machine_fingerprint: z.string(),
  activation_token: z.string(),
  expires_at: z.string().nullable().optional(),
  update_channel: z.string().optional(),
  offline_grace_hours: z.number().optional(),
  trust_revocation_disconnects_immediately: z.boolean().optional(),
  reconnect_requires_fresh_heartbeat: z.boolean().optional(),
  force_min_app_version: z.string().nullable().optional(),
  force_update_mode: z.enum(["off", "required", "emergency"]).optional(),
  force_update_reason: z.string().nullable().optional(),
  force_update_policy_source: z.enum(["tenant", "cohort"]).optional(),
  force_update_required: z.boolean().optional(),
});

export type ProductActivationClaim = z.infer<typeof ActivationClaimSchema>;
export type ProductLicenseOnboardingState = z.infer<typeof ProductLicenseOnboardingStateSchema>;
export type ProductLicenseDiagnostics = z.infer<typeof ProductLicenseDiagnosticsSchema>;

export async function getProductLicenseOnboardingState(): Promise<ProductLicenseOnboardingState> {
  const raw = await invoke<unknown>("get_product_license_onboarding_state");
  return ProductLicenseOnboardingStateSchema.parse(raw);
}

/** Control-plane API origin (activation, sync exchange). Matches VPS deployment URL. */
export function controlPlaneApiBase(): string {
  const fromEnv = (import.meta.env["VITE_ADMIN_API_BASE_URL"] as string | undefined)?.trim();
  return (fromEnv && fromEnv.length > 0 ? fromEnv : "http://api.maintafox.systems").replace(
    /\/$/,
    "",
  );
}

export async function claimProductActivation(input: {
  license_key: string;
  machine_fingerprint: string;
  machine_label?: string;
  app_version?: string;
}): Promise<ProductActivationClaim> {
  const res = await fetch(`${controlPlaneApiBase()}/api/v1/activation/claim`, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(input),
  });
  if (!res.ok) {
    let code: string | undefined;
    let message: string | undefined;
    try {
      const body = (await res.json()) as { error?: string; message?: string };
      code = body.error;
      message = body.message;
    } catch {
      // Keep fallback message.
    }
    const err = new Error(message ?? `Activation claim failed (${res.status})`) as Error & {
      status?: number;
      code?: string;
    };
    err.status = res.status;
    if (code !== undefined) {
      err.code = code;
    }
    throw err;
  }
  return ActivationClaimSchema.parse(await res.json());
}

export async function submitProductLicenseKey(input: {
  key: string;
  claim?: ProductActivationClaim | null;
  machine_fingerprint?: string;
  app_version?: string;
}): Promise<void> {
  await invoke<void>("submit_product_license_key", {
    key: input.key,
    claim_json: input.claim ? JSON.stringify(input.claim) : null,
    machine_fingerprint: input.machine_fingerprint ?? null,
    app_version: input.app_version ?? null,
  });
}

const ReconciliationOutcomeInputSchema = z.object({
  kind: z.enum(["success", "network_error", "http_error", "denied"]),
  claim: ActivationClaimSchema.optional(),
  error_code: z.string().optional(),
  error_message: z.string().optional(),
  app_version: z.string().optional(),
});

export async function applyProductLicenseReconciliation(
  outcome: z.input<typeof ReconciliationOutcomeInputSchema>,
): Promise<ProductLicenseOnboardingState> {
  const payload = ReconciliationOutcomeInputSchema.parse(outcome);
  const raw = await invoke<unknown>("apply_product_license_reconciliation", {
    outcome_json: JSON.stringify(payload),
  });
  return ProductLicenseOnboardingStateSchema.parse(raw);
}

export async function getProductLicenseDiagnostics(): Promise<ProductLicenseDiagnostics | null> {
  const raw = await invoke<unknown>("get_product_license_diagnostics");
  if (raw == null) return null;
  return ProductLicenseDiagnosticsSchema.parse(raw);
}
