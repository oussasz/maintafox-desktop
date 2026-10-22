/**
 * Product licence service: the paths the activation/claim suites do not reach. Error copy when the
 * server gave no status or no code, the configurable control-plane origin, the local licence IPC
 * commands (argument shaping + payload validation), reconciliation, browser-state clearing and the
 * defensive guards on the claim stored on the device.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  CLAIM_FALLBACK_TOKEN_ERRORS,
  LICENSE_RECHECK_REQUESTED_EVENT,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY,
  PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY,
  type ProductActivationClaim,
  activationRequiresLocalDataReset,
  applyProductLicenseReconciliation,
  assertClaimSerializableForDesktop,
  bootstrapInitialTenantAdmin,
  claimProductActivation,
  clearProductLicenseBrowserState,
  controlPlaneApiBase,
  getActivationBootstrapState,
  getActivationLicenseMetadata,
  getProductLicenseDiagnostics,
  getProductLicenseOnboardingState,
  previewProductActivation,
  requestLicenseRecheck,
  requestPlantJoin,
  resetLocalTenantRuntimeData,
  resetProductLicenseActivation,
  serializeClaimJsonForDesktop,
  submitProductLicenseKey,
  toUserFacingActivationError,
} from "@/services/product-license-service";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: ipc.invoke,
}));

const FAKE_KEY = "TEST-0000-0000-0000";
const INPUT = { license_key: FAKE_KEY, machine_fingerprint: "fp-test" };
const BASE = { tenant_id: "t-test", license_id: "l-test", machine_fingerprint: "fp-test" };

function respond(status: number, body: unknown): Response {
  const text = typeof body === "string" ? body : JSON.stringify(body);
  return {
    ok: status >= 200 && status < 300,
    status,
    text: () => Promise.resolve(text),
    json: () => Promise.resolve(typeof body === "string" ? JSON.parse(body) : body),
  } as unknown as Response;
}

const fetchMock = vi.fn();

beforeEach(() => {
  fetchMock.mockReset();
  ipc.invoke.mockReset();
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
  localStorage.clear();
  sessionStorage.clear();
});

describe("control-plane origin", () => {
  it("uses the configured origin and drops one trailing slash", async () => {
    vi.stubEnv("VITE_ADMIN_API_BASE_URL", "  https://cp.example.test/  ");
    expect(controlPlaneApiBase()).toBe("https://cp.example.test");

    fetchMock.mockResolvedValue(respond(200, BASE));
    await previewProductActivation(INPUT);
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe(
      "https://cp.example.test/api/v1/activation/preview",
    );
  });

  it("falls back to the shared default origin when the override is blank or absent", () => {
    vi.stubEnv("VITE_ADMIN_API_BASE_URL", "   ");
    const blank = controlPlaneApiBase();
    vi.stubEnv("VITE_ADMIN_API_BASE_URL", "");
    const empty = controlPlaneApiBase();
    expect(blank).toMatch(/^https?:\/\//);
    expect(blank).not.toMatch(/\/$/);
    expect(empty).toBe(blank);
  });
});

describe("claim request and its errors", () => {
  it("posts the claim as JSON to the claim endpoint", async () => {
    fetchMock.mockResolvedValue(respond(200, { ...BASE, activation_token: "tok-test" }));
    await claimProductActivation({ ...INPUT, machine_label: "Bench PC", app_version: "1.2.3" });
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toMatch(/\/api\/v1\/activation\/claim$/);
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual({
      ...INPUT,
      machine_label: "Bench PC",
      app_version: "1.2.3",
    });
  });

  it("labels a failed claim as an activation failure, not a licence validation", async () => {
    fetchMock.mockResolvedValue(respond(500, ""));
    await expect(claimProductActivation(INPUT)).rejects.toMatchObject({
      message: "Activation failed (500)",
      status: 500,
      technicalDetails: "Activation HTTP 500",
    });
  });

  it("rejects a claim response that has no activation token", async () => {
    fetchMock.mockResolvedValue(respond(200, BASE));
    await expect(claimProductActivation(INPUT)).rejects.toThrow(/activation_token/);
  });

  it("rejects a preview response missing the tenant identifier", async () => {
    fetchMock.mockResolvedValue(respond(200, { license_id: "l", machine_fingerprint: "f" }));
    await expect(previewProductActivation(INPUT)).rejects.toThrow(/tenant_id/);
  });

  it("an error object without a code leaves the code unset but keeps its message", async () => {
    fetchMock.mockResolvedValue(respond(400, { error: { message: "Nope" } }));
    const err = await previewProductActivation(INPUT).catch((e: unknown) => e);
    expect(err).toMatchObject({ message: "Nope", status: 400 });
    expect((err as { code?: string }).code).toBeUndefined();
  });

  it("an unreadable response body still yields a coded error for a 409", async () => {
    const res = respond(409, "");
    (res as unknown as { text: () => Promise<string> }).text = (): Promise<string> =>
      Promise.reject(new Error("stream closed"));
    fetchMock.mockResolvedValue(res);
    await expect(previewProductActivation(INPUT)).rejects.toMatchObject({
      code: "slot_limit_reached",
      technicalDetails: "License validation HTTP 409",
    });
  });

  it("ignores nested values of the wrong type when normalising the payload", async () => {
    fetchMock.mockResolvedValue(
      respond(200, {
        ...BASE,
        claim: { slot_limit: true, tier: 7 },
        entitlement: { slot_limit: {}, tier: ["x"] },
        tenant: { display_name: 12 },
        limits: { activated: "many" },
      }),
    );
    const preview = await previewProductActivation(INPUT);
    expect(preview.device_limit).toBeUndefined();
    expect(preview.license_tier).toBeUndefined();
    expect(preview.tenant_display_name).toBeUndefined();
    expect(preview.activated_device_count).toBeUndefined();
  });
});

describe("user-facing activation errors without a status", () => {
  function codeOnly(code: string): Error & { code: string } {
    return Object.assign(new Error("raw backend text"), { code });
  }

  it.each([
    ["tenant_init_failed", "Activation could not be completed"],
    ["license_not_found", "License not found"],
    ["activation_revoked", "Device revoked"],
    ["tenant_inactive", "Account deactivated"],
    ["license_revoked", "License revoked"],
    ["license_expired", "License expired"],
    ["slot_limit_reached", "Device limit reached"],
    ["force_update_required", "Update required"],
  ])("%s without a status carries the code but no status", (code, title) => {
    const out = toUserFacingActivationError(codeOnly(code));
    expect(out.title).toBe(title);
    expect(out.code).toBe(code);
    expect(out.referenceCode).toBe(code);
    expect(out).not.toHaveProperty("status");
    expect(out.technicalDetails).toBe(`code=${code} · raw backend text`);
  });

  it("keeps the status next to the code when both are present", () => {
    const err = Object.assign(new Error("boom"), { code: "license_expired", status: 410 });
    expect(toUserFacingActivationError(err)).toMatchObject({
      title: "License expired",
      status: 410,
      code: "license_expired",
      technicalDetails: "code=license_expired · status=410 · boom",
    });
  });

  it("an unknown code is the reference, with or without a status", () => {
    const withoutStatus = toUserFacingActivationError(codeOnly("something_new"));
    expect(withoutStatus).toMatchObject({
      title: "Activation could not be completed",
      referenceCode: "something_new",
      code: "something_new",
    });
    expect(withoutStatus).not.toHaveProperty("status");

    const withStatus = toUserFacingActivationError(
      Object.assign(new Error("x"), { code: "something_new", status: 422 }),
    );
    expect(withStatus).toMatchObject({ status: 422, code: "something_new" });
  });

  it("a server error keeps its code as well as its status", () => {
    const out = toUserFacingActivationError(
      Object.assign(new Error("x"), { code: "upstream_down", status: 503 }),
    );
    expect(out).toMatchObject({
      title: "Activation server unavailable",
      status: 503,
      code: "upstream_down",
      referenceCode: "upstream_down",
    });
  });

  it("a storage validation failure without a code is referenced as VALIDATION_FAILED", () => {
    const out = toUserFacingActivationError(new Error("bad claimJson payload"));
    expect(out).toMatchObject({ referenceCode: "VALIDATION_FAILED" });
    expect(out).not.toHaveProperty("code");
    expect(out).not.toHaveProperty("status");
  });

  it("a validation failure keeps the status when the server gave one", () => {
    const out = toUserFacingActivationError(
      Object.assign(new Error("x"), { code: "VALIDATION_FAILED", status: 400 }),
    );
    expect(out).toMatchObject({ status: 400, code: "VALIDATION_FAILED" });
  });

  it("a 404 with a code keeps both, and a 404 without a code stays unset", () => {
    const withCode = toUserFacingActivationError(
      Object.assign(new Error("x"), { code: "license_not_found", status: 404 }),
    );
    expect(withCode).toMatchObject({ status: 404, code: "license_not_found" });
    const bare = toUserFacingActivationError(Object.assign(new Error("x"), { status: 404 }));
    expect(bare).toMatchObject({ title: "License not found", referenceCode: "404" });
    expect(bare).not.toHaveProperty("code");
  });
});

describe("local licence commands", () => {
  it("resets the device licence and the tenant runtime data through their commands", async () => {
    ipc.invoke.mockResolvedValueOnce(undefined).mockResolvedValueOnce(42);
    await resetProductLicenseActivation();
    expect(await resetLocalTenantRuntimeData()).toBe(42);
    expect(ipc.invoke.mock.calls).toEqual([
      ["reset_product_license_activation"],
      ["reset_local_tenant_runtime_data"],
    ]);
  });

  it("passes the bootstrap administrator under the input key", async () => {
    ipc.invoke.mockResolvedValue(undefined);
    const input = {
      username: "admin-test",
      email: "a@example.test",
      password: "x",
      display_name: "A",
    };
    await bootstrapInitialTenantAdmin(input);
    expect(ipc.invoke).toHaveBeenCalledWith("bootstrap_initial_tenant_admin", { input });
  });

  it("submits the key with the canonical claim JSON and camelCase argument names", async () => {
    ipc.invoke.mockResolvedValue(undefined);
    const claim: ProductActivationClaim = {
      ...BASE,
      activation_token: "tok-test",
      edition: "core",
      license_tier: "core",
      license_plan: "core",
    };
    await submitProductLicenseKey({
      key: FAKE_KEY,
      claim,
      machine_fingerprint: "fp-test",
      app_version: "9.9.9",
    });
    const [command, args] = ipc.invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("submit_product_license_key");
    expect(Object.keys(args).sort()).toEqual([
      "appVersion",
      "claimJson",
      "key",
      "machineFingerprint",
    ]);
    expect(args["key"]).toBe(FAKE_KEY);
    expect(args["machineFingerprint"]).toBe("fp-test");
    expect(args["appVersion"]).toBe("9.9.9");
    const stored = JSON.parse(String(args["claimJson"])) as Record<string, unknown>;
    expect(stored).not.toHaveProperty("edition");
    expect(stored["license_tier"]).toBe("core");
  });

  it("submits nulls when no claim, fingerprint or version is supplied", async () => {
    ipc.invoke.mockResolvedValue(undefined);
    await submitProductLicenseKey({ key: FAKE_KEY });
    expect(ipc.invoke).toHaveBeenCalledWith("submit_product_license_key", {
      key: FAKE_KEY,
      claimJson: null,
      machineFingerprint: null,
      appVersion: null,
    });
    await submitProductLicenseKey({ key: FAKE_KEY, claim: null });
    expect(ipc.invoke.mock.calls[1]?.[1]).toMatchObject({ claimJson: null });
  });

  it("propagates a rejected submit so the caller can map it", async () => {
    ipc.invoke.mockRejectedValue(new Error("disk full"));
    await expect(submitProductLicenseKey({ key: FAKE_KEY })).rejects.toThrow("disk full");
  });
});

describe("onboarding state payload", () => {
  const state = {
    complete: false,
    status: "pending_online_validation",
    pending_online_validation: true,
  };

  it("accepts the minimal state and keeps optional fields that are present", async () => {
    ipc.invoke.mockResolvedValueOnce(state);
    expect(await getProductLicenseOnboardingState()).toEqual(state);

    const full = {
      ...state,
      status: "denied_expired",
      deny_reason_code: "license_expired",
      deny_message: null,
      retry_attempt: 3,
      next_retry_at: "2026-01-01T00:00:00Z",
      tenant_id: "t-test",
      company_display_name: "Acme",
      license_edition: null,
    };
    ipc.invoke.mockResolvedValueOnce(full);
    expect(await getProductLicenseOnboardingState()).toEqual(full);
    expect(ipc.invoke).toHaveBeenCalledWith("get_product_license_onboarding_state");
  });

  it.each([
    ["an unknown status", { ...state, status: "banana" }],
    ["a negative retry count", { ...state, retry_attempt: -1 }],
    ["a fractional retry count", { ...state, retry_attempt: 1.5 }],
    ["a missing flag", { complete: true, status: "active" }],
  ])("rejects %s", async (_label, payload) => {
    ipc.invoke.mockResolvedValue(payload);
    await expect(getProductLicenseOnboardingState()).rejects.toThrow();
  });
});

describe("bootstrap state and licence metadata", () => {
  it("reads the bootstrap state, with the tenant fields optional", async () => {
    ipc.invoke.mockResolvedValueOnce({ has_tenant_admin: false, plant_join: null });
    expect(await getActivationBootstrapState()).toEqual({
      has_tenant_admin: false,
      plant_join: null,
    });
    ipc.invoke.mockResolvedValueOnce({
      tenant_id: "t-test",
      company_display_name: null,
      has_tenant_admin: true,
      plant_join: "sign_in",
    });
    expect(await getActivationBootstrapState()).toMatchObject({
      tenant_id: "t-test",
      has_tenant_admin: true,
    });
    expect(ipc.invoke).toHaveBeenCalledWith("get_activation_bootstrap_state");
  });

  it("reads where the PC stands in joining its plant, and asks the worker to join", async () => {
    for (const plant_join of ["sign_in", "create_first_admin", "joining", "connection_needed"]) {
      ipc.invoke.mockResolvedValueOnce({
        tenant_id: "t-test",
        has_tenant_admin: false,
        plant_join,
      });
      expect(await getActivationBootstrapState()).toMatchObject({ plant_join });
    }
    ipc.invoke.mockResolvedValueOnce({ tenant_id: "t-test", has_tenant_admin: false });
    await expect(getActivationBootstrapState()).rejects.toThrow(/plant_join/);
    ipc.invoke.mockResolvedValueOnce(null);
    await requestPlantJoin();
    expect(ipc.invoke).toHaveBeenCalledWith("request_plant_join");
  });

  it("rejects a bootstrap state without the administrator flag", async () => {
    ipc.invoke.mockResolvedValue({ tenant_id: "t-test" });
    await expect(getActivationBootstrapState()).rejects.toThrow(/has_tenant_admin/);
  });

  it("reads the stored licence metadata when the device has one", async () => {
    const metadata = {
      tenant_id: "t-test",
      license_id: "l-test",
      machine_fingerprint: "fp-test",
      expires_at: null,
      device_limit: 5,
      license_tier: "core",
    };
    ipc.invoke.mockResolvedValue(metadata);
    expect(await getActivationLicenseMetadata()).toEqual(metadata);
    expect(ipc.invoke).toHaveBeenCalledWith("get_activation_license_metadata");
  });

  it("rejects stored licence metadata that lacks the licence identifier", async () => {
    ipc.invoke.mockResolvedValue({ tenant_id: "t-test", machine_fingerprint: "fp-test" });
    await expect(getActivationLicenseMetadata()).rejects.toThrow(/license_id/);
  });
});

describe("licence diagnostics", () => {
  const diagnostics = {
    status: "degraded_api_unavailable",
    pending_online_validation: false,
    reconciliation: { retry_attempt: 2, last_error_code: "network_error" },
    diagnostics: [{ at: "2026-01-01T00:00:00Z", kind: "reconcile", message: "retry", code: null }],
    has_activation_claim: true,
  };

  it("returns the diagnostics with their optional fields left absent", async () => {
    ipc.invoke.mockResolvedValue(diagnostics);
    const out = await getProductLicenseDiagnostics();
    expect(out).toEqual(diagnostics);
    expect(out?.reconciliation.next_retry_at).toBeUndefined();
    expect(ipc.invoke).toHaveBeenCalledWith("get_product_license_diagnostics");
  });

  it.each([
    ["an unknown status", { ...diagnostics, status: "mystery" }],
    ["a missing reconciliation block", { ...diagnostics, reconciliation: undefined }],
    ["an event without a message", { ...diagnostics, diagnostics: [{ at: "x", kind: "k" }] }],
  ])("rejects diagnostics with %s", async (_label, payload) => {
    ipc.invoke.mockResolvedValue(payload);
    await expect(getProductLicenseDiagnostics()).rejects.toThrow();
  });
});

describe("licence reconciliation", () => {
  const onboarding = { complete: true, status: "active", pending_online_validation: false };
  const envelope = {
    envelope_id: "env-test",
    lineage_version: 1,
    issuer: "test-issuer",
    key_id: "k-test",
    signature_alg: "test-alg",
    tier: "core",
    state: "active",
    channel: "stable",
    machine_slots: 2,
    feature_flags_json: "{}",
    capabilities_json: "{}",
    policy_json: "{}",
    issued_at: "2026-01-01T00:00:00Z",
    valid_from: "2026-01-01T00:00:00Z",
    valid_until: "2027-01-01T00:00:00Z",
    offline_grace_until: "2026-02-01T00:00:00Z",
    signature: "sig-test",
  };

  function sentOutcome(): Record<string, unknown> {
    const [command, args] = ipc.invoke.mock.calls[0] as [string, { outcomeJson: string }];
    expect(command).toBe("apply_product_license_reconciliation");
    expect(Object.keys(args)).toEqual(["outcomeJson"]);
    return JSON.parse(args.outcomeJson) as Record<string, unknown>;
  }

  it("sends a failure outcome as JSON and returns the new onboarding state", async () => {
    ipc.invoke.mockResolvedValue({ ...onboarding, status: "degraded_api_unavailable" });
    const state = await applyProductLicenseReconciliation({
      kind: "network_error",
      error_code: "ECONNRESET",
      error_message: "connection reset",
      app_version: "1.0.0",
    });
    expect(state.status).toBe("degraded_api_unavailable");
    expect(sentOutcome()).toEqual({
      kind: "network_error",
      error_code: "ECONNRESET",
      error_message: "connection reset",
      app_version: "1.0.0",
    });
  });

  it("sends a successful claim in desktop-canonical form, with its envelope at the top level", async () => {
    ipc.invoke.mockResolvedValue(onboarding);
    await applyProductLicenseReconciliation({
      kind: "success",
      claim: {
        ...BASE,
        activation_token: "tok-test",
        edition: "core",
        slot_limit: 3,
        entitlement_envelope: envelope,
      } as never,
    });
    const sent = sentOutcome();
    expect(sent["entitlement_envelope"]).toEqual(envelope);
    const claim = sent["claim"] as Record<string, unknown>;
    expect(claim["license_tier"]).toBe("core");
    expect(claim).not.toHaveProperty("edition");
    expect(claim).not.toHaveProperty("entitlement_envelope");
    expect(claim["tenant_id"]).toBe("t-test");
  });

  it("prefers an explicit envelope over the one carried by the claim", async () => {
    ipc.invoke.mockResolvedValue(onboarding);
    await applyProductLicenseReconciliation({
      kind: "success",
      claim: {
        ...BASE,
        activation_token: "tok-test",
        entitlement_envelope: { ...envelope, envelope_id: "from-claim" },
      },
      entitlement_envelope: { ...envelope, envelope_id: "explicit" },
    });
    expect((sentOutcome()["entitlement_envelope"] as { envelope_id: string }).envelope_id).toBe(
      "explicit",
    );
  });

  it("refuses an outcome of an unknown kind before calling the backend", async () => {
    await expect(
      applyProductLicenseReconciliation({ kind: "exploded" } as never),
    ).rejects.toThrow();
    expect(ipc.invoke).not.toHaveBeenCalled();
  });

  it("refuses a returned state with an unknown status", async () => {
    ipc.invoke.mockResolvedValue({ ...onboarding, status: "nope" });
    await expect(applyProductLicenseReconciliation({ kind: "denied" })).rejects.toThrow();
  });
});

describe("browser-side licence state", () => {
  it("clears the cached key and the login hint but keeps the device fingerprint", () => {
    localStorage.setItem(PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY, FAKE_KEY);
    localStorage.setItem(PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY, "fp-test");
    sessionStorage.setItem(POST_ACTIVATION_LOGIN_HINT_KEY, "1");
    clearProductLicenseBrowserState();
    expect(localStorage.getItem(PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY)).toBeNull();
    expect(sessionStorage.getItem(POST_ACTIVATION_LOGIN_HINT_KEY)).toBeNull();
    expect(localStorage.getItem(PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY)).toBe("fp-test");
  });

  it("still clears the key when session storage is unavailable", () => {
    localStorage.setItem(PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY, FAKE_KEY);
    vi.stubGlobal("sessionStorage", undefined);
    expect(() => clearProductLicenseBrowserState()).not.toThrow();
    expect(localStorage.getItem(PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY)).toBeNull();
  });

  it("does nothing, and does not throw, without any storage", () => {
    const removed = vi.spyOn(Storage.prototype, "removeItem");
    vi.stubGlobal("localStorage", undefined);
    expect(() => clearProductLicenseBrowserState()).not.toThrow();
    expect(removed).not.toHaveBeenCalled();
  });

  it("announces a licence re-check with its reason", () => {
    const seen: string[] = [];
    const listener = (e: Event): void => {
      seen.push((e as CustomEvent<{ reason: string }>).detail.reason);
    };
    window.addEventListener(LICENSE_RECHECK_REQUESTED_EVENT, listener);
    try {
      requestLicenseRecheck("sync-401");
    } finally {
      window.removeEventListener(LICENSE_RECHECK_REQUESTED_EVENT, listener);
    }
    expect(seen).toEqual(["sync-401"]);
  });

  it("does not throw when there is no window to announce on", () => {
    vi.stubGlobal("window", undefined);
    expect(() => requestLicenseRecheck("sync-403")).not.toThrow();
  });
});

describe("rules for re-claiming and wiping local data", () => {
  it.each([
    [null, "t-1", true],
    [undefined, "t-1", true],
    ["   ", "t-1", true],
    ["t-1", "t-1", false],
    [" t-1 ", "t-1 ", false],
    ["t-1", "t-2", true],
  ])("bound %j, claim %j -> wipe required: %s", (bound, claim, expected) => {
    expect(activationRequiresLocalDataReset(bound, claim)).toBe(expected);
  });

  it("allows a claim fallback only for an unusable token", () => {
    expect([...CLAIM_FALLBACK_TOKEN_ERRORS].sort()).toEqual([
      "invalid_token",
      "missing_token",
      "token_superseded",
    ]);
    expect(CLAIM_FALLBACK_TOKEN_ERRORS.has("license_revoked")).toBe(false);
  });
});

describe("guards on the claim stored on the device", () => {
  const claim: ProductActivationClaim = {
    ...BASE,
    activation_token: "tok-test",
  };

  /** Makes the next serialisation produce `output`, as a broken serializer would. */
  function serializerReturns(output: string): void {
    vi.spyOn(JSON, "stringify").mockReturnValueOnce(output);
  }

  it("rejects output that is not a JSON object literal", () => {
    serializerReturns('"just a string"');
    expect(() => assertClaimSerializableForDesktop(claim)).toThrow(/produced invalid JSON/);
  });

  it("reports the parse error when the output is malformed JSON", () => {
    serializerReturns("{not json");
    expect(() => assertClaimSerializableForDesktop(claim)).toThrow(/produced invalid JSON: .+/);
  });

  it("rejects a record carrying the edition alias", () => {
    serializerReturns(
      '{"tenant_id":"t","license_id":"l","machine_fingerprint":"f","activation_token":"a","edition":"core"}',
    );
    expect(() => assertClaimSerializableForDesktop(claim)).toThrow(/alias field: edition/);
  });

  it.each(["slot_limit", "seat_limit"])("rejects a record carrying %s", (alias) => {
    serializerReturns(
      `{"tenant_id":"t","license_id":"l","machine_fingerprint":"f","activation_token":"a","${alias}":2}`,
    );
    expect(() => assertClaimSerializableForDesktop(claim)).toThrow(/slot_limit\/seat_limit/);
  });

  it.each(["company_display_name", "company_name", "tenant_name"])(
    "rejects a record carrying %s",
    (alias) => {
      serializerReturns(
        `{"tenant_id":"t","license_id":"l","machine_fingerprint":"f","activation_token":"a","${alias}":"Acme"}`,
      );
      expect(() => assertClaimSerializableForDesktop(claim)).toThrow(/company_display_name/);
    },
  );

  it.each(["tenant_id", "license_id", "machine_fingerprint"] as const)(
    "refuses a claim whose %s is blank",
    (field) => {
      expect(() => assertClaimSerializableForDesktop({ ...claim, [field]: "  " })).toThrow(
        `missing required field: ${field}`,
      );
    },
  );

  it("serialises a claim without an envelope as the bare record", () => {
    const json = JSON.parse(serializeClaimJsonForDesktop(claim)) as Record<string, unknown>;
    expect(json).not.toHaveProperty("entitlement_envelope");
    expect(json["activation_token"]).toBe("tok-test");
  });
});
