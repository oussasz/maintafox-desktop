import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

/** Session flag: show one-time login notice after activation (LoginPage reads and clears). */
export const POST_ACTIVATION_LOGIN_HINT_KEY = "maintafox:post-activation-login-hint:v1";

export const PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY = "maintafox:product-license:key-cache:v1";
export const PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY =
  "maintafox:product-license:device-fingerprint:v1";

/**
 * Keys cleared on activation reset / logout-from-tenant.
 * IMPORTANT: Do not include {@link PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY} here â€” that ID must stay
 * stable for the lifetime of this app install so the control plane does not treat each reset as a new
 * machine and burn through slot_limit.
 */
export const PRODUCT_LICENSE_LOCAL_STORAGE_KEYS = [PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY] as const;

/** Removes browser-side license cache (localStorage + post-activation hint). Call after `resetProductLicenseActivation`. */
export function clearProductLicenseBrowserState(): void {
  if (typeof localStorage === "undefined") return;
  for (const k of PRODUCT_LICENSE_LOCAL_STORAGE_KEYS) {
    localStorage.removeItem(k);
  }
  if (typeof sessionStorage !== "undefined") {
    sessionStorage.removeItem(POST_ACTIVATION_LOGIN_HINT_KEY);
  }
}

/** Deletes device-scoped product license row in SQLite (`app_settings`). */
export async function resetProductLicenseActivation(): Promise<void> {
  await invoke<void>("reset_product_license_activation");
}

/** Wipes tenant runtime data (operational tables) while preserving global settings/bootstrap data. */
export async function resetLocalTenantRuntimeData(): Promise<number> {
  return await invoke<number>("reset_local_tenant_runtime_data");
}

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
  tenant_id: z.string().nullable().optional(),
  company_display_name: z.string().nullable().optional(),
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
  device_limit: z.number().optional(),
  license_tier: z.string().optional(),
  /** Company / tenant display name from control plane (may be omitted). */
  tenant_display_name: z.string().optional(),
  has_demo_data: z.boolean().optional(),
  is_initialized: z.boolean().optional(),
});

export type ProductActivationClaim = z.infer<typeof ActivationClaimSchema>;
const ActivationTenantStatusSchema = z.object({
  tenant_id: z.string(),
  is_initialized: z.boolean(),
});

export type ActivationTenantStatus = z.infer<typeof ActivationTenantStatusSchema>;

export type ProductLicenseOnboardingState = z.infer<typeof ProductLicenseOnboardingStateSchema>;
export type ProductLicenseDiagnostics = z.infer<typeof ProductLicenseDiagnosticsSchema>;

export async function getProductLicenseOnboardingState(): Promise<ProductLicenseOnboardingState> {
  const raw = await invoke<unknown>("get_product_license_onboarding_state");
  return ProductLicenseOnboardingStateSchema.parse(raw);
}

/** Control-plane API origin (activation, sync exchange). Matches VPS deployment URL. */
export function controlPlaneApiBase(): string {
  const fromEnv = (import.meta.env["VITE_ADMIN_API_BASE_URL"] as string | undefined)?.trim();
  return (fromEnv && fromEnv.length > 0 ? fromEnv : "https://api.maintafox.systems").replace(
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
    let rawErrorText: string | undefined;
    try {
      const text = await res.text();
      rawErrorText = text;
      if (text.trim().length > 0) {
        const body = JSON.parse(text) as Record<string, unknown>;
        const nestedError =
          typeof body["error"] === "object" && body["error"] !== null
            ? (body["error"] as Record<string, unknown>)
            : null;
        code =
          (typeof body["code"] === "string" ? body["code"] : undefined) ??
          (typeof body["error"] === "string" ? body["error"] : undefined) ??
          (nestedError && typeof nestedError["code"] === "string"
            ? nestedError["code"]
            : undefined);
        message =
          (typeof body["message"] === "string" ? body["message"] : undefined) ??
          (typeof body["detail"] === "string" ? body["detail"] : undefined) ??
          (typeof body["error_description"] === "string" ? body["error_description"] : undefined) ??
          (nestedError && typeof nestedError["message"] === "string"
            ? nestedError["message"]
            : undefined);
      }
    } catch {
      // Keep fallback message.
    }
    if ((!message || message.trim().length === 0) && rawErrorText?.trim()) {
      message = rawErrorText.trim();
    }
    if (res.status === 409) {
      code = code ?? "activation_conflict";
      message =
        message ??
        "Activation conflict: this key is already claimed or no slot is currently available for this device. Ask your Vendor Admin to revoke/reassign the entitlement key, then try again.";
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
  const raw = (await res.json()) as Record<string, unknown>;
  const readNestedNumber = (obj: Record<string, unknown>, path: string[]): number | undefined => {
    let cur: unknown = obj;
    for (const segment of path) {
      if (typeof cur !== "object" || cur === null || !(segment in cur)) return undefined;
      cur = (cur as Record<string, unknown>)[segment];
    }
    if (typeof cur === "number") return cur;
    if (typeof cur === "string") {
      const parsed = Number(cur);
      return Number.isFinite(parsed) ? parsed : undefined;
    }
    return undefined;
  };
  const readNestedString = (obj: Record<string, unknown>, path: string[]): string | undefined => {
    let cur: unknown = obj;
    for (const segment of path) {
      if (typeof cur !== "object" || cur === null || !(segment in cur)) return undefined;
      cur = (cur as Record<string, unknown>)[segment];
    }
    return typeof cur === "string" ? cur : undefined;
  };
  const normalized = {
    ...raw,
    device_limit:
      (raw["device_limit"] as number | undefined) ??
      (raw["slot_limit"] as number | undefined) ??
      (raw["seat_limit"] as number | undefined) ??
      (raw["deviceLimit"] as number | undefined) ??
      (raw["slotLimit"] as number | undefined) ??
      readNestedNumber(raw, ["claim", "slot_limit"]) ??
      readNestedNumber(raw, ["claim", "slotLimit"]) ??
      readNestedNumber(raw, ["entitlement", "slot_limit"]) ??
      readNestedNumber(raw, ["entitlement", "slotLimit"]) ??
      readNestedNumber(raw, ["entitlement_key", "slot_limit"]) ??
      readNestedNumber(raw, ["license", "slot_limit"]) ??
      readNestedNumber(raw, ["license_metadata", "slot_limit"]) ??
      readNestedNumber(raw, ["limits", "slot_limit"]) ??
      readNestedNumber(raw, ["entitlements", "slot_limit"]),
    license_tier:
      (raw["license_tier"] as string | undefined) ??
      (raw["tier"] as string | undefined) ??
      (raw["plan"] as string | undefined) ??
      (raw["licenseTier"] as string | undefined) ??
      readNestedString(raw, ["claim", "tier"]) ??
      readNestedString(raw, ["claim", "plan"]) ??
      readNestedString(raw, ["entitlement", "tier"]) ??
      readNestedString(raw, ["entitlement", "plan"]) ??
      readNestedString(raw, ["entitlement_key", "tier"]) ??
      readNestedString(raw, ["entitlement_key", "plan"]) ??
      readNestedString(raw, ["license", "tier"]) ??
      readNestedString(raw, ["license_metadata", "tier"]) ??
      readNestedString(raw, ["license", "plan"]) ??
      readNestedString(raw, ["license_metadata", "plan"]) ??
      readNestedString(raw, ["entitlements", "tier"]) ??
      readNestedString(raw, ["entitlements", "plan"]),
    tenant_display_name:
      (raw["tenant_display_name"] as string | undefined) ??
      (raw["company_display_name"] as string | undefined) ??
      (raw["company_name"] as string | undefined) ??
      (raw["tenant_name"] as string | undefined) ??
      readNestedString(raw, ["tenant", "display_name"]) ??
      readNestedString(raw, ["tenant", "name"]) ??
      readNestedString(raw, ["company", "display_name"]) ??
      readNestedString(raw, ["company", "name"]) ??
      readNestedString(raw, ["claim", "tenant_display_name"]) ??
      readNestedString(raw, ["claim", "company_display_name"]),
    has_demo_data:
      (raw["has_demo_data"] as boolean | undefined) ??
      (raw["demo_data"] as boolean | undefined) ??
      (raw["is_demo"] as boolean | undefined) ??
      ((raw["claim"] as Record<string, unknown> | undefined)?.["has_demo_data"] as
        | boolean
        | undefined),
    is_initialized:
      (raw["is_initialized"] as boolean | undefined) ??
      ((raw["tenant"] as Record<string, unknown> | undefined)?.["is_initialized"] as
        | boolean
        | undefined) ??
      ((raw["claim"] as Record<string, unknown> | undefined)?.["is_initialized"] as
        | boolean
        | undefined),
  };
  return ActivationClaimSchema.parse(normalized);
}

export async function getActivationTenantStatus(
  activationToken: string,
): Promise<ActivationTenantStatus> {
  const token = activationToken.trim();
  if (!token) throw new Error("Activation token is required.");
  const res = await fetch(`${controlPlaneApiBase()}/api/v1/activation/tenant-status`, {
    method: "GET",
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
    },
  });
  if (!res.ok) {
    throw new Error(`Failed to fetch tenant status (${res.status})`);
  }
  const raw = (await res.json()) as unknown;
  return ActivationTenantStatusSchema.parse(raw);
}

export async function markActivationTenantInitialized(
  activationToken: string,
): Promise<ActivationTenantStatus> {
  const token = activationToken.trim();
  if (!token) throw new Error("Activation token is required.");
  const res = await fetch(`${controlPlaneApiBase()}/api/v1/activation/tenant-status/initialize`, {
    method: "POST",
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: "{}",
  });
  if (!res.ok) {
    throw new Error(`Failed to mark tenant initialized (${res.status})`);
  }
  const raw = (await res.json()) as unknown;
  return ActivationTenantStatusSchema.parse(raw);
}

const ActivationBootstrapStateSchema = z.object({
  tenant_id: z.string().nullable().optional(),
  company_display_name: z.string().nullable().optional(),
  has_tenant_admin: z.boolean(),
});

const ActivationLicenseMetadataSchema = z.object({
  tenant_id: z.string(),
  company_display_name: z.string().nullable().optional(),
  license_id: z.string(),
  expires_at: z.string().nullable().optional(),
  device_limit: z.number().nullable().optional(),
  license_tier: z.string().nullable().optional(),
  machine_fingerprint: z.string(),
});

export type ActivationBootstrapState = z.infer<typeof ActivationBootstrapStateSchema>;
export type ActivationLicenseMetadata = z.infer<typeof ActivationLicenseMetadataSchema>;

export async function getActivationBootstrapState(): Promise<ActivationBootstrapState> {
  const raw = await invoke<unknown>("get_activation_bootstrap_state");
  return ActivationBootstrapStateSchema.parse(raw);
}

export async function getActivationLicenseMetadata(): Promise<ActivationLicenseMetadata | null> {
  const raw = await invoke<unknown>("get_activation_license_metadata");
  if (raw == null) return null;
  return ActivationLicenseMetadataSchema.parse(raw);
}

export async function bootstrapInitialTenantAdmin(input: {
  username: string;
  email: string;
  password: string;
  display_name?: string;
}): Promise<void> {
  await invoke<void>("bootstrap_initial_tenant_admin", { input });
}

export async function submitProductLicenseKey(input: {
  key: string;
  claim?: ProductActivationClaim | null;
  machine_fingerprint?: string;
  app_version?: string;
}): Promise<void> {
  await invoke<void>("submit_product_license_key", {
    key: input.key,
    // Tauri command arg mapping is camelCase on the JS side.
    claimJson: input.claim ? JSON.stringify(input.claim) : null,
    machineFingerprint: input.machine_fingerprint ?? null,
    appVersion: input.app_version ?? null,
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
    outcomeJson: JSON.stringify(payload),
  });
  return ProductLicenseOnboardingStateSchema.parse(raw);
}

export async function getProductLicenseDiagnostics(): Promise<ProductLicenseDiagnostics | null> {
  const raw = await invoke<unknown>("get_product_license_diagnostics");
  if (raw == null) return null;
  return ProductLicenseDiagnosticsSchema.parse(raw);
}
