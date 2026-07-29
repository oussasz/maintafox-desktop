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
  license_edition: z.string().nullable().optional(),
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

const EntitlementEnvelopeInputSchema = z.object({
  envelope_id: z.string(),
  previous_envelope_id: z.string().nullable().optional(),
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
  signature: z.string(),
});

const ActivationOrgFieldsSchema = {
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
  tenant_slug: z.string().optional(),
  license_plan: z.string().optional(),
  edition: z.string().optional(),
  license_status: z.string().optional(),
  activated_device_count: z.number().optional(),
  machine_label: z.string().nullable().optional(),
  has_demo_data: z.boolean().optional(),
  is_initialized: z.boolean().optional(),
  capabilities_digest: z.string().nullable().optional(),
  feature_flags_digest: z.string().nullable().optional(),
  entitlement_envelope: EntitlementEnvelopeInputSchema.optional(),
};

/** Preview validates a key without consuming a device slot (no activation_token). */
const ActivationPreviewSchema = z.object({
  tenant_id: z.string(),
  license_id: z.string(),
  machine_fingerprint: z.string(),
  ...ActivationOrgFieldsSchema,
});

const ActivationClaimSchema = z.object({
  tenant_id: z.string(),
  license_id: z.string(),
  machine_fingerprint: z.string(),
  activation_token: z.string(),
  ...ActivationOrgFieldsSchema,
});

export type ProductActivationPreview = z.infer<typeof ActivationPreviewSchema>;
export type ProductActivationClaim = z.infer<typeof ActivationClaimSchema>;
const ActivationTenantStatusSchema = z.object({
  tenant_id: z.string(),
  is_initialized: z.boolean(),
});

export type ActivationTenantStatus = z.infer<typeof ActivationTenantStatusSchema>;

export type ProductLicenseOnboardingState = z.infer<typeof ProductLicenseOnboardingStateSchema>;
export type ProductLicenseDiagnostics = z.infer<typeof ProductLicenseDiagnosticsSchema>;

export type UserFacingActivationError = {
  title: string;
  message: string;
  referenceCode: string;
  technicalDetails: string;
  status?: number;
  code?: string;
};

function readNestedNumber(obj: Record<string, unknown>, path: string[]): number | undefined {
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
}

function readNestedString(obj: Record<string, unknown>, path: string[]): string | undefined {
  let cur: unknown = obj;
  for (const segment of path) {
    if (typeof cur !== "object" || cur === null || !(segment in cur)) return undefined;
    cur = (cur as Record<string, unknown>)[segment];
  }
  return typeof cur === "string" ? cur : undefined;
}

function normalizeActivationPayload(raw: Record<string, unknown>): Record<string, unknown> {
  return {
    ...raw,
    device_limit:
      (raw["device_limit"] as number | undefined) ??
      (raw["slot_limit"] as number | undefined) ??
      (raw["seat_limit"] as number | undefined) ??
      (raw["deviceLimit"] as number | undefined) ??
      (raw["slotLimit"] as number | undefined) ??
      readNestedNumber(raw, ["claim", "slot_limit"]) ??
      readNestedNumber(raw, ["entitlement", "slot_limit"]) ??
      readNestedNumber(raw, ["license", "slot_limit"]),
    license_tier:
      (raw["license_tier"] as string | undefined) ??
      (raw["edition"] as string | undefined) ??
      (raw["license_plan"] as string | undefined) ??
      (raw["tier"] as string | undefined) ??
      (raw["plan"] as string | undefined) ??
      readNestedString(raw, ["claim", "tier"]) ??
      readNestedString(raw, ["entitlement", "tier"]),
    license_plan:
      (raw["license_plan"] as string | undefined) ??
      (raw["edition"] as string | undefined) ??
      (raw["license_tier"] as string | undefined) ??
      (raw["tier"] as string | undefined) ??
      (raw["plan"] as string | undefined),
    edition:
      (raw["edition"] as string | undefined) ??
      (raw["license_tier"] as string | undefined) ??
      (raw["license_plan"] as string | undefined) ??
      (raw["tier"] as string | undefined),
    tenant_display_name:
      (raw["tenant_display_name"] as string | undefined) ??
      (raw["company_display_name"] as string | undefined) ??
      (raw["company_name"] as string | undefined) ??
      (raw["tenant_name"] as string | undefined) ??
      readNestedString(raw, ["tenant", "display_name"]) ??
      readNestedString(raw, ["tenant", "name"]) ??
      readNestedString(raw, ["company", "display_name"]),
    tenant_slug:
      (raw["tenant_slug"] as string | undefined) ??
      (raw["slug"] as string | undefined) ??
      readNestedString(raw, ["tenant", "slug"]),
    license_status: (raw["license_status"] as string | undefined) ?? "active",
    activated_device_count:
      (raw["activated_device_count"] as number | undefined) ??
      (raw["active_device_count"] as number | undefined) ??
      readNestedNumber(raw, ["limits", "activated"]),
    has_demo_data:
      (raw["has_demo_data"] as boolean | undefined) ??
      (raw["demo_data"] as boolean | undefined) ??
      (raw["is_demo"] as boolean | undefined),
    is_initialized:
      (raw["is_initialized"] as boolean | undefined) ??
      ((raw["tenant"] as Record<string, unknown> | undefined)?.["is_initialized"] as
        | boolean
        | undefined),
  };
}

function throwActivationHttpError(
  res: Response,
  scope: string,
  bodyText: string | undefined,
): never {
  let code: string | undefined;
  let message: string | undefined;
  if (bodyText?.trim()) {
    try {
      const body = JSON.parse(bodyText) as Record<string, unknown>;
      const nestedError =
        typeof body["error"] === "object" && body["error"] !== null
          ? (body["error"] as Record<string, unknown>)
          : null;
      code =
        (typeof body["code"] === "string" ? body["code"] : undefined) ??
        (typeof body["error"] === "string" ? body["error"] : undefined) ??
        (nestedError && typeof nestedError["code"] === "string" ? nestedError["code"] : undefined);
      message =
        (typeof body["message"] === "string" ? body["message"] : undefined) ??
        (typeof body["detail"] === "string" ? body["detail"] : undefined) ??
        (typeof body["error_description"] === "string" ? body["error_description"] : undefined) ??
        (nestedError && typeof nestedError["message"] === "string"
          ? nestedError["message"]
          : undefined);
    } catch {
      message = bodyText.trim();
    }
  }
  if (res.status === 409) {
    code = code ?? "slot_limit_reached";
    message =
      message ??
      "No device slot is available for this license. Ask your administrator to free a seat, then try again.";
  }
  const err = new Error(message ?? `${scope} failed (${res.status})`) as Error & {
    status?: number;
    code?: string;
    technicalDetails?: string;
  };
  err.status = res.status;
  if (code !== undefined) err.code = code;
  err.technicalDetails = bodyText?.trim() || `${scope} HTTP ${res.status}`;
  throw err;
}

/** Map raw activation/control-plane errors to user-facing copy (never show raw backend text as primary). */
export function toUserFacingActivationError(err: unknown): UserFacingActivationError {
  const e = err as Error & { status?: number; code?: string; technicalDetails?: string };
  const code = e.code;
  const status = e.status;
  const technicalDetails =
    e.technicalDetails ??
    [code ? `code=${code}` : null, status != null ? `status=${status}` : null, e.message]
      .filter(Boolean)
      .join(" · ");

  const referenceCode = code ?? (status != null ? String(status) : "activation_error");

  if (code === "tenant_init_unavailable" || code === "tenant_init_failed") {
    return {
      title: "Activation could not be completed",
      message:
        "The activation server could not finish preparing your organization. Please try again.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      ...(code ? { code } : {}),
    };
  }
  if (code === "license_not_found" || status === 404) {
    return {
      title: "License not found",
      message: "We could not find a valid license for this key. Check the key and try again.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      ...(code ? { code } : {}),
    };
  }
  if (code === "license_revoked") {
    return {
      title: "License revoked",
      message: "This license has been revoked. Contact your administrator for a new key.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      code,
    };
  }
  if (code === "license_expired") {
    return {
      title: "License expired",
      message: "This license is no longer valid. Renewal is required before you can activate.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      code,
    };
  }
  if (code === "slot_limit_reached" || code === "activation_conflict") {
    return {
      title: "Device limit reached",
      message:
        "All allowed devices for this license are already activated. Ask your administrator to free a seat.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      ...(code ? { code } : {}),
    };
  }
  if (code === "force_update_required") {
    return {
      title: "Update required",
      message: "This device must be updated before activation can continue.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      code,
    };
  }
  if (
    code === "VALIDATION_FAILED" ||
    (typeof e.message === "string" &&
      (e.message.includes("ProductActivationClaimRecord") || e.message.includes("claimJson")))
  ) {
    return {
      title: "Activation could not be completed",
      message:
        "The activation response could not be saved on this device. Please try again. If it continues, contact support with the reference code.",
      referenceCode: code ?? "VALIDATION_FAILED",
      technicalDetails,
      ...(status != null ? { status } : {}),
      ...(code ? { code } : {}),
    };
  }
  if (typeof status === "number" && status >= 500) {
    return {
      title: "Activation server unavailable",
      message: "We could not reach the activation service. Check your connection and try again.",
      referenceCode,
      technicalDetails,
      status,
      ...(code ? { code } : {}),
    };
  }
  if (status == null && !code) {
    return {
      title: "Unable to connect",
      message:
        "We could not reach the activation service. Check your network connection and try again.",
      referenceCode: "network_unreachable",
      technicalDetails,
    };
  }
  return {
    title: "Activation could not be completed",
    message: "Something went wrong while activating this device. Please try again.",
    referenceCode,
    technicalDetails,
    ...(status != null ? { status } : {}),
    ...(code ? { code } : {}),
  };
}

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

async function postActivationEndpoint(
  path: "/api/v1/activation/preview" | "/api/v1/activation/claim",
  input: {
    license_key: string;
    machine_fingerprint: string;
    machine_label?: string;
    app_version?: string;
  },
): Promise<Record<string, unknown>> {
  const res = await fetch(`${controlPlaneApiBase()}${path}`, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(input),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => undefined);
    throwActivationHttpError(
      res,
      path.includes("preview") ? "License validation" : "Activation",
      text,
    );
  }
  return (await res.json()) as Record<string, unknown>;
}

/** Validate license + return org summary without consuming a device slot. */
export async function previewProductActivation(input: {
  license_key: string;
  machine_fingerprint: string;
  machine_label?: string;
  app_version?: string;
}): Promise<ProductActivationPreview> {
  const raw = await postActivationEndpoint("/api/v1/activation/preview", input);
  return ActivationPreviewSchema.parse(normalizeActivationPayload(raw));
}

export async function claimProductActivation(input: {
  license_key: string;
  machine_fingerprint: string;
  machine_label?: string;
  app_version?: string;
}): Promise<ProductActivationClaim> {
  const raw = await postActivationEndpoint("/api/v1/activation/claim", input);
  return ActivationClaimSchema.parse(normalizeActivationPayload(raw));
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
    const text = await res.text().catch(() => undefined);
    throwActivationHttpError(res, "Tenant status", text);
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
    const text = await res.text().catch(() => undefined);
    const err = new Error(
      text?.trim() || `Tenant initialization failed (${res.status})`,
    ) as Error & { status?: number; code?: string; technicalDetails?: string };
    err.status = res.status;
    err.code = res.status === 404 ? "tenant_init_unavailable" : "tenant_init_failed";
    err.technicalDetails =
      text?.trim() || `POST /api/v1/activation/tenant-status/initialize → ${res.status}`;
    throw err;
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

/**
 * Canonical claim fields for desktop Rust `ProductActivationClaimRecord`.
 * Omits serde aliases and FE-only fields so submit/reconcile never hit duplicate-field errors.
 */
export function toDesktopActivationClaimRecord(
  claim: ProductActivationClaim,
): Record<string, unknown> {
  const edition = claim.edition ?? claim.license_tier ?? claim.license_plan ?? null;
  return {
    tenant_id: claim.tenant_id,
    license_id: claim.license_id,
    machine_fingerprint: claim.machine_fingerprint,
    activation_token: claim.activation_token,
    expires_at: claim.expires_at ?? null,
    force_min_app_version: claim.force_min_app_version ?? null,
    force_update_required: claim.force_update_required ?? null,
    update_channel: claim.update_channel ?? null,
    offline_grace_hours: claim.offline_grace_hours ?? null,
    trust_revocation_disconnects_immediately:
      claim.trust_revocation_disconnects_immediately ?? null,
    reconnect_requires_fresh_heartbeat: claim.reconnect_requires_fresh_heartbeat ?? null,
    tenant_display_name: claim.tenant_display_name ?? null,
    tenant_slug: claim.tenant_slug ?? null,
    device_limit: claim.device_limit ?? null,
    license_tier: edition,
    license_plan: edition,
    license_status: claim.license_status ?? null,
    activated_device_count: claim.activated_device_count ?? null,
    has_demo_data: claim.has_demo_data ?? null,
    is_initialized: claim.is_initialized ?? null,
  };
}

/** claimJson for `submit_product_license_key` — record + optional entitlement_envelope. */
export function serializeClaimJsonForDesktop(claim: ProductActivationClaim): string {
  const record = toDesktopActivationClaimRecord(claim);
  if (claim.entitlement_envelope) {
    return JSON.stringify({
      ...record,
      entitlement_envelope: claim.entitlement_envelope,
    });
  }
  return JSON.stringify(record);
}

/** Preflight: ensure claim can be persisted before destructive local tenant reset. */
export function assertClaimSerializableForDesktop(claim: ProductActivationClaim): void {
  const json = serializeClaimJsonForDesktop(claim);
  if (!json.trim().startsWith("{")) {
    throw new Error("Activation claim serialization produced invalid JSON.");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (e) {
    const detail = e instanceof Error ? e.message : String(e);
    throw new Error(`Activation claim serialization produced invalid JSON: ${detail}`);
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new Error("Activation claim serialization must produce a JSON object.");
  }
  const obj = parsed as Record<string, unknown>;
  for (const key of [
    "tenant_id",
    "license_id",
    "machine_fingerprint",
    "activation_token",
  ] as const) {
    if (typeof obj[key] !== "string" || !(obj[key] as string).trim()) {
      throw new Error(`Activation claim is missing required field: ${key}`);
    }
  }
  // Guard against alias co-presence that Rust serde rejects.
  if ("edition" in obj) {
    throw new Error("Desktop claim payload must not include alias field: edition");
  }
  if ("slot_limit" in obj || "seat_limit" in obj) {
    throw new Error("Desktop claim payload must not include alias fields: slot_limit/seat_limit");
  }
  if ("company_display_name" in obj || "company_name" in obj || "tenant_name" in obj) {
    throw new Error(
      "Desktop claim payload must not include alias fields: company_display_name/company_name/tenant_name",
    );
  }
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
    // Must use desktop-canonical claim JSON — raw claim includes serde aliases
    // (edition + license_tier, etc.) that Rust rejects as duplicate fields.
    claimJson: input.claim ? serializeClaimJsonForDesktop(input.claim) : null,
    machineFingerprint: input.machine_fingerprint ?? null,
    appVersion: input.app_version ?? null,
  });
}

const ReconciliationOutcomeInputSchema = z.object({
  kind: z.enum(["success", "network_error", "http_error", "denied"]),
  claim: ActivationClaimSchema.optional(),
  entitlement_envelope: EntitlementEnvelopeInputSchema.optional(),
  error_code: z.string().optional(),
  error_message: z.string().optional(),
  app_version: z.string().optional(),
});

export async function applyProductLicenseReconciliation(
  outcome: z.input<typeof ReconciliationOutcomeInputSchema>,
): Promise<ProductLicenseOnboardingState> {
  const payload = ReconciliationOutcomeInputSchema.parse(outcome);
  const envelope = payload.entitlement_envelope ?? payload.claim?.entitlement_envelope ?? undefined;
  // Same canonical claim shape as submit_product_license_key (no serde aliases).
  const claimForRust = payload.claim ? toDesktopActivationClaimRecord(payload.claim) : undefined;
  const forRust = {
    kind: payload.kind,
    claim: claimForRust,
    entitlement_envelope: envelope,
    error_code: payload.error_code,
    error_message: payload.error_message,
    app_version: payload.app_version,
  };
  const raw = await invoke<unknown>("apply_product_license_reconciliation", {
    outcomeJson: JSON.stringify(forRust),
  });
  return ProductLicenseOnboardingStateSchema.parse(raw);
}

/** Refresh edition capabilities without consuming a new device slot. */
export async function refreshProductActivationPolicy(
  activationToken: string,
): Promise<ProductActivationClaim> {
  const token = activationToken.trim();
  if (!token) throw new Error("Activation token is required.");
  const res = await fetch(`${controlPlaneApiBase()}/api/v1/activation/policy-refresh`, {
    method: "POST",
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: "{}",
  });
  if (!res.ok) {
    const text = await res.text().catch(() => undefined);
    throwActivationHttpError(res, "Policy refresh", text);
  }
  const raw = (await res.json()) as Record<string, unknown>;
  const normalized = normalizeActivationPayload(raw);
  // Control plane may omit activation_token on refresh; reuse the bearer we already hold.
  if (
    typeof normalized["activation_token"] !== "string" ||
    !String(normalized["activation_token"]).trim()
  ) {
    normalized["activation_token"] = token;
  }
  return ActivationClaimSchema.parse(normalized);
}

/** Local activation bearer token (no session required — device-scoped onboarding state). */
export async function getProductActivationToken(): Promise<string | null> {
  const raw = await invoke<unknown>("get_product_activation_token");
  return typeof raw === "string" && raw.trim() ? raw : null;
}

export async function getProductLicenseDiagnostics(): Promise<ProductLicenseDiagnostics | null> {
  const raw = await invoke<unknown>("get_product_license_diagnostics");
  if (raw == null) return null;
  return ProductLicenseDiagnosticsSchema.parse(raw);
}
