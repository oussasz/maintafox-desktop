import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

/** Session flag: show one-time login notice after activation (LoginPage reads and clears). */
export const POST_ACTIVATION_LOGIN_HINT_KEY = "maintafox:post-activation-login-hint:v1";

/**
 * Where earlier versions kept the licence key in the webview, to claim again with it. The key now
 * stays in Rust (`reclaim_product_activation`); a value left by an earlier version is removed
 * ({@link removeLegacyProductLicenseKeyCache}), never read.
 */
export const LEGACY_PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY =
  "maintafox:product-license:key-cache:v1";
/**
 * IMPORTANT: never cleared on activation reset or logout-from-tenant: this ID must stay stable for
 * the lifetime of this app install so the control plane does not treat each reset as a new machine
 * and burn through slot_limit.
 */
export const PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY =
  "maintafox:product-license:device-fingerprint:v1";

/**
 * Removes the licence key an earlier version cached in the webview. Never throws: storage that
 * cannot be reached holds nothing to remove.
 */
export function removeLegacyProductLicenseKeyCache(): void {
  try {
    localStorage.removeItem(LEGACY_PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY);
  } catch {
    // No reachable storage (private mode, no window): nothing cached there.
  }
}

/** Removes browser-side licence state (legacy key cache + post-activation hint). Call after `resetProductLicenseActivation`. */
export function clearProductLicenseBrowserState(): void {
  removeLegacyProductLicenseKeyCache();
  if (typeof sessionStorage !== "undefined") {
    sessionStorage.removeItem(POST_ACTIVATION_LOGIN_HINT_KEY);
  }
}

/** Deletes device-scoped product license row in SQLite (`app_settings`). */
export async function resetProductLicenseActivation(): Promise<void> {
  await invoke<void>("reset_product_license_activation");
}

/**
 * Whether activating `claimTenantId` must first wipe local tenant data.
 * Re-activating the tenant already bound on this device keeps its data (e.g. after a licence
 * reset or app reinstall); a first activation or a switch to another tenant wipes it so no data
 * leaks across tenants. The backend still requires an admin session to wipe once an
 * administrator exists.
 */
export function activationRequiresLocalDataReset(
  boundTenantId: string | null | undefined,
  claimTenantId: string,
): boolean {
  const bound = boundTenantId?.trim();
  return !bound || bound !== claimTenantId.trim();
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
  "denied_offline_grace_expired",
]);

/** Fired when a control-plane call reveals the licence may have changed (e.g. sync got 401/403). */
export const LICENSE_RECHECK_REQUESTED_EVENT = "maintafox:license-recheck-requested";

export function requestLicenseRecheck(reason: string): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(LICENSE_RECHECK_REQUESTED_EVENT, { detail: { reason } }));
}

/**
 * Policy-refresh failures after which claiming again with the licence key stored in Rust
 * (`reclaimProductActivation`) is allowed: the device token is merely unusable (expired,
 * malformed, or replaced by a newer claim). Revocations and other denials must never fall back to
 * a claim.
 */
export const CLAIM_FALLBACK_TOKEN_ERRORS: ReadonlySet<string> = new Set([
  "missing_token",
  "invalid_token",
  "token_superseded",
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

/**
 * A claim as the webview sees it: the organisation and licence terms of the answer. Its activation
 * token stays in Rust (`claim_product_activation`), held there for the key entry.
 */
const ActivationClaimSchema = ActivationPreviewSchema;

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

/**
 * The field spellings servers send for a claim or preview. The policy refresh answer is read in
 * Rust with the same rules (`policy_refresh_claim` in product_license.rs): change both together.
 */
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

type ActivationHttpError = Error & {
  status?: number;
  code?: string;
  technicalDetails?: string;
};

/**
 * The error an activation call throws for an HTTP error answer: the server's `code` and
 * `message` (a 409 is a slot limit unless the server says otherwise), and the body as details.
 */
function activationHttpError(
  status: number,
  scope: string,
  fromBody: { code: string | undefined; message: string | undefined },
  bodyText: string | undefined,
): ActivationHttpError {
  let { code, message } = fromBody;
  if (status === 409) {
    code = code ?? "slot_limit_reached";
    message =
      message ??
      "No device slot is available for this license. Ask your administrator to free a seat, then try again.";
  }
  const err = new Error(message ?? `${scope} failed (${status})`) as ActivationHttpError;
  err.status = status;
  if (code !== undefined) err.code = code;
  err.technicalDetails = bodyText?.trim() || `${scope} HTTP ${status}`;
  return err;
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
  if (code === "activation_revoked") {
    return {
      title: "Device revoked",
      message:
        "This device was revoked by the vendor. Ask your administrator to have it reinstated.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      code,
    };
  }
  if (code === "tenant_inactive") {
    return {
      title: "Account deactivated",
      message: "Your organization's account is deactivated. Contact your vendor to restore access.",
      referenceCode,
      technicalDetails,
      ...(status != null ? { status } : {}),
      code,
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

const ActivationCallFailureSchemas = [
  z.object({
    kind: z.literal("http_error"),
    status: z.number().int(),
    code: z.string().nullable().optional(),
    message: z.string().nullable().optional(),
    details: z.string().nullable().optional(),
  }),
  z.object({ kind: z.literal("invalid_response"), message: z.string() }),
  z.object({ kind: z.literal("network_error"), message: z.string() }),
] as const;

type ActivationCallFailure = z.infer<(typeof ActivationCallFailureSchemas)[number]>;

/**
 * What an activation call sent from Rust returns (`ActivationCallResult` in product_license.rs):
 * the answer, nothing sent (`not_activated`), or why there is no answer.
 */
function activationCallResultSchema<T extends z.ZodTypeAny>(
  answer: T,
): z.ZodDiscriminatedUnion<
  "kind",
  [
    z.ZodObject<{ kind: z.ZodLiteral<"answered">; answer: T }>,
    z.ZodObject<{ kind: z.ZodLiteral<"not_activated"> }>,
    ...typeof ActivationCallFailureSchemas,
  ]
> {
  return z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("answered"), answer }),
    z.object({ kind: z.literal("not_activated") }),
    ...ActivationCallFailureSchemas,
  ]);
}

/**
 * Throws an activation call's failure as every activation call throws it: the server's error
 * answer (`status`, `code`, `technicalDetails`), else a plain error (no usable answer).
 */
function throwActivationCallFailure(failure: ActivationCallFailure, scope: string): never {
  if (failure.kind === "http_error") {
    throw activationHttpError(
      failure.status,
      scope,
      { code: failure.code ?? undefined, message: failure.message ?? undefined },
      failure.details ?? undefined,
    );
  }
  throw new Error(failure.message);
}

const ActivationAnswerResultSchema = activationCallResultSchema(z.record(z.unknown()));

type ActivationRequest = {
  license_key: string;
  machine_fingerprint: string;
  machine_label?: string;
  app_version?: string;
};

/** The answer of a preview or claim sent from Rust, or its failure thrown. */
function activationAnswer(raw: unknown, scope: string): Record<string, unknown> {
  const result = ActivationAnswerResultSchema.parse(raw);
  switch (result.kind) {
    case "answered":
      return result.answer;
    case "not_activated":
      // A preview or claim always sends the key it is given: Rust never answers this for them.
      throw new Error(`${scope}: nothing was sent.`);
    default:
      throwActivationCallFailure(result, scope);
  }
}

/** Validate license + return org summary without consuming a device slot (sent from Rust). */
export async function previewProductActivation(
  input: ActivationRequest,
): Promise<ProductActivationPreview> {
  const raw = await invoke<unknown>("preview_product_activation", {
    licenseKey: input.license_key,
    machineFingerprint: input.machine_fingerprint,
    machineLabel: input.machine_label ?? null,
    appVersion: input.app_version ?? null,
  });
  return ActivationPreviewSchema.parse(
    normalizeActivationPayload(activationAnswer(raw, "License validation")),
  );
}

/**
 * Takes a device slot for the key (sent from Rust). Rust keeps the claim and its activation token
 * for {@link submitProductLicenseKey}; this returns the organisation and licence terms only.
 */
export async function claimProductActivation(
  input: ActivationRequest,
): Promise<ProductActivationClaim> {
  const raw = await invoke<unknown>("claim_product_activation", {
    licenseKey: input.license_key,
    machineFingerprint: input.machine_fingerprint,
    machineLabel: input.machine_label ?? null,
    appVersion: input.app_version ?? null,
  });
  return ActivationClaimSchema.parse(
    normalizeActivationPayload(activationAnswer(raw, "Activation")),
  );
}

const TenantInitializeResultSchema = activationCallResultSchema(ActivationTenantStatusSchema);

/**
 * Marks the activated organisation as initialised on the control API, sent from Rust with the
 * activation token held there (the claim just taken, else the stored activation). Throws with
 * code `tenant_init_unavailable` (404) or `tenant_init_failed`.
 */
export async function markActivationTenantInitialized(): Promise<ActivationTenantStatus> {
  const result = TenantInitializeResultSchema.parse(
    await invoke<unknown>("mark_activation_tenant_initialized"),
  );
  switch (result.kind) {
    case "answered":
      return result.answer;
    case "not_activated": {
      const err = new Error(
        "This device holds no activation to initialize.",
      ) as ActivationHttpError;
      err.code = "tenant_init_failed";
      throw err;
    }
    case "http_error": {
      const details = result.details?.trim();
      const err = new Error(
        details || `Tenant initialization failed (${result.status})`,
      ) as ActivationHttpError;
      err.status = result.status;
      err.code = result.status === 404 ? "tenant_init_unavailable" : "tenant_init_failed";
      err.technicalDetails =
        details || `POST /api/v1/activation/tenant-status/initialize → ${result.status}`;
      throw err;
    }
    default:
      throwActivationCallFailure(result, "Tenant initialization");
  }
}

/**
 * What the first-start screen of an activated PC shows (`sync::plant_join`): sign in, create the
 * first administrator (a complete pull confirmed the organisation has no account), joining the
 * plant (the pull is running), or a connection is needed (the pull cannot run or failed).
 */
export const PlantJoinStateSchema = z.enum([
  "sign_in",
  "create_first_admin",
  "joining",
  "connection_needed",
]);
export type PlantJoinState = z.infer<typeof PlantJoinStateSchema>;

const ActivationBootstrapStateSchema = z.object({
  tenant_id: z.string().nullable().optional(),
  company_display_name: z.string().nullable().optional(),
  has_tenant_admin: z.boolean(),
  /** `null` when this PC is not activated. */
  plant_join: PlantJoinStateSchema.nullable(),
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
 * The first-start screen joins the plant: the sync worker runs now (and again on "Retry") instead
 * of waiting for its next check. Refused once this PC has an administrator.
 */
export async function requestPlantJoin(): Promise<void> {
  await invoke<void>("request_plant_join");
}

/**
 * Asks this PC whether entering the previewed licence would be accepted (same rule as
 * `submitProductLicenseKey`), before the licence is taken on the server or local data is wiped.
 * Resolves when accepted; rejects with the same coded refusal the submit would give. Changes nothing.
 */
export async function checkProductLicenseKeyEntry(
  preview: ProductActivationPreview,
): Promise<void> {
  await invoke<void>("check_product_license_key_entry", {
    previewJson: JSON.stringify({
      tenant_id: preview.tenant_id,
      license_id: preview.license_id,
      machine_fingerprint: preview.machine_fingerprint,
      has_demo_data: preview.has_demo_data ?? null,
      is_initialized: preview.is_initialized ?? null,
      // The licence terms, mapped as Rust maps the claim's (`policy_refresh_claim`).
      expires_at: preview.expires_at ?? null,
      device_limit: preview.device_limit ?? null,
      license_tier: preview.edition ?? preview.license_tier ?? preview.license_plan ?? null,
      offline_grace_hours: preview.offline_grace_hours ?? null,
      force_update_required: preview.force_update_required ?? null,
      force_min_app_version: preview.force_min_app_version ?? null,
      update_channel: preview.update_channel ?? null,
      trust_revocation_disconnects_immediately:
        preview.trust_revocation_disconnects_immediately ?? null,
      reconnect_requires_fresh_heartbeat: preview.reconnect_requires_fresh_heartbeat ?? null,
      entitlement_envelope: preview.entitlement_envelope ?? null,
    }),
  });
}

/**
 * Records the key entry of the claim {@link claimProductActivation} just took: Rust records the
 * claim it holds for this key and organisation (`tenant_id`, the one shown and checked for the
 * local data reset); the webview never passes the claim or its token.
 */
export async function submitProductLicenseKey(input: {
  key: string;
  tenant_id: string;
  app_version?: string;
}): Promise<void> {
  await invoke<void>("submit_product_license_key", {
    key: input.key,
    tenantId: input.tenant_id,
    appVersion: input.app_version ?? null,
  });
}

/**
 * The licence server's denial codes, the ones `map_denied_status` (product_license.rs) records as a
 * licence denial; once recorded, only an administrator with step-up lifts it. Only these are
 * reported as `denied`: any other 4xx (a malformed request, a licence not found, a proxy's rate
 * limit) is reported as `http_error`, which never needs an administrator to clear.
 */
export const LICENCE_DENIAL_CODES: ReadonlySet<string> = new Set([
  "activation_revoked",
  "tenant_inactive",
  "license_revoked",
  "license_expired",
  "slot_limit_reached",
  "force_update_required",
]);

/**
 * A licence check that got no licence answer. A success is never recorded from here: Rust records
 * the claims it receives (policy refresh, re-claim, key entry) and refuses one from the webview.
 */
const ReconciliationOutcomeInputSchema = z.object({
  kind: z.enum(["network_error", "http_error", "denied"]),
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

const PolicyRefreshResultSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("no_token") }),
  z.object({ kind: z.literal("refreshed"), state: ProductLicenseOnboardingStateSchema }),
  ...ActivationCallFailureSchemas,
]);

/** A licence policy refresh that got an answer it could use. */
export type PolicyRefreshOutcome =
  | { kind: "no_token" }
  | { kind: "refreshed"; state: ProductLicenseOnboardingState };

/**
 * Refresh the licence without consuming a new device slot (`refresh_product_activation_policy`).
 * The request is sent from Rust with the activation token stored there, which never reaches the
 * webview; Rust also records a renewed licence as a successful reconciliation.
 *
 * `appVersion` is the version this PC is running, so a forced update releases a PC that is
 * already up to date. It is sent only when well formed; without it the server applies the forced
 * update as if the PC were out of date.
 *
 * Resolves `no_token` when this PC holds no activation (nothing was sent), or `refreshed` with the
 * recorded state. Throws, as the other activation calls do: the server's error answer (`status`,
 * `code`, `technicalDetails`), a plain error when no usable answer came, or this PC's refusal of
 * the licence change (an administrator or a step-up needed).
 */
export async function refreshProductActivationPolicy(
  appVersion?: string,
): Promise<PolicyRefreshOutcome> {
  const raw = await invoke<unknown>("refresh_product_activation_policy", {
    appVersion: appVersion ?? null,
  });
  const result = PolicyRefreshResultSchema.parse(raw);
  switch (result.kind) {
    case "no_token":
      return { kind: "no_token" };
    case "refreshed":
      return { kind: "refreshed", state: result.state };
    default:
      throwActivationCallFailure(result, "Policy refresh");
  }
}

const ReclaimResultSchema = activationCallResultSchema(ProductLicenseOnboardingStateSchema);

/** A re-claim that got an answer it could use. */
export type ReclaimOutcome =
  | { kind: "not_activated" }
  | { kind: "reclaimed"; state: ProductLicenseOnboardingState };

/**
 * Claims this PC's activation again when its activation token is unusable or absent
 * (`reclaim_product_activation`): sent from Rust with the licence key stored at the key entry and
 * the stored machine fingerprint (neither reaches the webview); Rust records the new claim as a
 * successful reconciliation. Resolves `not_activated` when no key is stored here (nothing sent).
 * Throws as {@link refreshProductActivationPolicy} does.
 */
export async function reclaimProductActivation(input: {
  machine_label?: string;
  app_version?: string;
}): Promise<ReclaimOutcome> {
  const result = ReclaimResultSchema.parse(
    await invoke<unknown>("reclaim_product_activation", {
      machineLabel: input.machine_label ?? null,
      appVersion: input.app_version ?? null,
    }),
  );
  switch (result.kind) {
    case "answered":
      return { kind: "reclaimed", state: result.answer };
    case "not_activated":
      return { kind: "not_activated" };
    default:
      throwActivationCallFailure(result, "Activation");
  }
}

export async function getProductLicenseDiagnostics(): Promise<ProductLicenseDiagnostics | null> {
  const raw = await invoke<unknown>("get_product_license_diagnostics");
  if (raw == null) return null;
  return ProductLicenseDiagnosticsSchema.parse(raw);
}
