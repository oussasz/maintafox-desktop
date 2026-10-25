/**
 * Activation against the control plane, sent from Rust: the desktop accepts the field spellings
 * older and newer servers send, turns error answers into coded errors, and maps every code to
 * user-facing copy (never raw backend text as the main message). No call carries a token.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  claimProductActivation,
  getActivationLicenseMetadata,
  getProductLicenseDiagnostics,
  markActivationTenantInitialized,
  previewProductActivation,
  reclaimProductActivation,
  refreshProductActivationPolicy,
  toUserFacingActivationError,
} from "@/services/product-license-service";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: ipc.invoke,
}));

const INPUT = { license_key: "MFX-TEST", machine_fingerprint: "fp-1" };
const BASE = { tenant_id: "t-1", license_id: "l-1", machine_fingerprint: "fp-1" };

/** Rust's answer to an activation call that got `answer` from the server. */
function answered(answer: unknown): { kind: "answered"; answer: unknown } {
  return { kind: "answered", answer };
}

beforeEach(() => {
  ipc.invoke.mockReset();
});

describe("activation payload from different server versions", () => {
  it.each([
    ["device_limit", { device_limit: 3 }, 3],
    ["slot_limit", { slot_limit: 4 }, 4],
    ["seat_limit", { seat_limit: 5 }, 5],
    ["deviceLimit", { deviceLimit: 6 }, 6],
    ["slotLimit", { slotLimit: 7 }, 7],
    ["claim.slot_limit", { claim: { slot_limit: 8 } }, 8],
    ["entitlement.slot_limit as text", { entitlement: { slot_limit: "9" } }, 9],
    ["license.slot_limit", { license: { slot_limit: 10 } }, 10],
    ["unparsable nested text", { license: { slot_limit: "many" } }, undefined],
    ["nothing", {}, undefined],
  ])("reads the device limit from %s", async (_label, extra, expected) => {
    ipc.invoke.mockResolvedValue(answered({ ...BASE, ...extra }));
    const preview = await previewProductActivation(INPUT);
    expect(preview.device_limit).toBe(expected);
  });

  it.each([
    ["license_tier", { license_tier: "pro" }, "pro", "pro", "pro"],
    ["edition", { edition: "core" }, "core", "core", "core"],
    ["license_plan", { license_plan: "plan-x" }, "plan-x", "plan-x", "plan-x"],
    ["tier", { tier: "t1" }, "t1", "t1", "t1"],
    ["plan", { plan: "p1" }, "p1", "p1", undefined],
    ["claim.tier", { claim: { tier: "nested" } }, "nested", undefined, undefined],
    ["entitlement.tier", { entitlement: { tier: "ent" } }, "ent", undefined, undefined],
  ])("reads tier, plan and edition from %s", async (_label, extra, tier, plan, edition) => {
    ipc.invoke.mockResolvedValue(answered({ ...BASE, ...extra }));
    const preview = await previewProductActivation(INPUT);
    expect([preview.license_tier, preview.license_plan, preview.edition]).toEqual([
      tier,
      plan,
      edition,
    ]);
  });

  it.each([
    ["tenant_display_name", { tenant_display_name: "Acme" }],
    ["company_display_name", { company_display_name: "Acme" }],
    ["company_name", { company_name: "Acme" }],
    ["tenant_name", { tenant_name: "Acme" }],
    ["tenant.display_name", { tenant: { display_name: "Acme" } }],
    ["tenant.name", { tenant: { name: "Acme" } }],
    ["company.display_name", { company: { display_name: "Acme" } }],
  ])("reads the company name from %s", async (_label, extra) => {
    ipc.invoke.mockResolvedValue(answered({ ...BASE, ...extra }));
    expect((await previewProductActivation(INPUT)).tenant_display_name).toBe("Acme");
  });

  it("reads slug, counts, demo and initialisation flags from their alternative fields", async () => {
    ipc.invoke.mockResolvedValue(
      answered({
        ...BASE,
        tenant: { slug: "acme", is_initialized: true },
        limits: { activated: 2 },
        is_demo: true,
      }),
    );
    const preview = await previewProductActivation(INPUT);
    expect(preview).toMatchObject({
      tenant_slug: "acme",
      activated_device_count: 2,
      has_demo_data: true,
      is_initialized: true,
      license_status: "active",
    });

    ipc.invoke.mockResolvedValue(
      answered({
        ...BASE,
        slug: "acme-2",
        active_device_count: 1,
        demo_data: false,
        license_status: "suspended",
      }),
    );
    expect(await previewProductActivation(INPUT)).toMatchObject({
      tenant_slug: "acme-2",
      activated_device_count: 1,
      has_demo_data: false,
      license_status: "suspended",
    });
  });

  it("asks Rust for the preview with the key, fingerprint, label and version", async () => {
    ipc.invoke.mockResolvedValue(answered(BASE));
    await previewProductActivation(INPUT);
    expect(ipc.invoke).toHaveBeenCalledWith("preview_product_activation", {
      licenseKey: "MFX-TEST",
      machineFingerprint: "fp-1",
      machineLabel: null,
      appVersion: null,
    });
  });

  it("claims through Rust with the same normalisation; the answer carries no token", async () => {
    ipc.invoke.mockResolvedValue(answered({ ...BASE, slot_limit: 2, plan: "p" }));
    const claim = await claimProductActivation({
      ...INPUT,
      machine_label: "Bench",
      app_version: "1.2.3",
    });
    expect(claim).toMatchObject({ device_limit: 2, license_tier: "p" });
    expect(claim).not.toHaveProperty("activation_token");
    expect(ipc.invoke).toHaveBeenCalledWith("claim_product_activation", {
      licenseKey: "MFX-TEST",
      machineFingerprint: "fp-1",
      machineLabel: "Bench",
      appVersion: "1.2.3",
    });
  });

  it("refuses an answer that names no organisation", async () => {
    ipc.invoke.mockResolvedValue(answered({ license_id: "l-1", machine_fingerprint: "fp-1" }));
    await expect(previewProductActivation(INPUT)).rejects.toThrow(/tenant_id/);
  });

  it("throws when Rust sent nothing for a preview or claim", async () => {
    ipc.invoke.mockResolvedValue({ kind: "not_activated" });
    await expect(claimProductActivation(INPUT)).rejects.toThrow("Activation: nothing was sent.");
  });
});

describe("activation error answers (read from the body in Rust)", () => {
  async function failure(result: unknown): Promise<Error & Record<string, unknown>> {
    ipc.invoke.mockResolvedValue(result);
    try {
      await previewProductActivation(INPUT);
    } catch (err) {
      return err as Error & Record<string, unknown>;
    }
    throw new Error("expected a failure");
  }

  it("a 409 without a code means the device limit is reached", async () => {
    const err = await failure({
      kind: "http_error",
      status: 409,
      code: null,
      message: null,
      details: null,
    });
    expect(err["code"]).toBe("slot_limit_reached");
    expect(err.message).toMatch(/device slot/i);
    expect(err["technicalDetails"]).toBe("License validation HTTP 409");
  });

  it("carries the server's code, message, status and body", async () => {
    const err = await failure({
      kind: "http_error",
      status: 400,
      code: "license_expired",
      message: "Expired",
      details: '{"code":"license_expired"}',
    });
    expect([err["code"], err.message, err["status"], err["technicalDetails"]]).toEqual([
      "license_expired",
      "Expired",
      400,
      '{"code":"license_expired"}',
    ]);
  });

  it("falls back to the status without a message", async () => {
    const empty = await failure({ kind: "http_error", status: 503 });
    expect(empty.message).toBe("License validation failed (503)");
    expect(empty["code"]).toBeUndefined();
  });

  it("throws a plain error when no usable answer came", async () => {
    const err = await failure({ kind: "network_error", message: "Cannot reach the control API" });
    expect(err.message).toBe("Cannot reach the control API");
    expect(err["status"]).toBeUndefined();
  });
});

describe("user-facing activation errors", () => {
  function coded(
    code: string | undefined,
    status?: number,
    message = "raw backend text",
  ): Error & { code: string | undefined; status: number | undefined } {
    return Object.assign(new Error(message), { code, status });
  }

  it.each([
    ["tenant_init_unavailable", "Activation could not be completed"],
    ["tenant_init_failed", "Activation could not be completed"],
    ["license_not_found", "License not found"],
    ["activation_revoked", "Device revoked"],
    ["tenant_inactive", "Account deactivated"],
    ["license_revoked", "License revoked"],
    ["license_expired", "License expired"],
    ["slot_limit_reached", "Device limit reached"],
    ["activation_conflict", "Device limit reached"],
    ["force_update_required", "Update required"],
    ["VALIDATION_FAILED", "Activation could not be completed"],
  ])("%s → %s, keeping the code as reference", (code, title) => {
    const out = toUserFacingActivationError(coded(code, 400));
    expect(out.title).toBe(title);
    expect(out.message).not.toContain("raw backend text");
    expect(out.referenceCode).toBe(code);
    expect(out.technicalDetails).toContain("raw backend text");
  });

  it("maps statuses when there is no code", () => {
    expect(toUserFacingActivationError(coded(undefined, 404)).title).toBe("License not found");
    expect(toUserFacingActivationError(coded(undefined, 503))).toMatchObject({
      title: "Activation server unavailable",
      referenceCode: "503",
      status: 503,
    });
    expect(toUserFacingActivationError(coded(undefined, 418)).referenceCode).toBe("418");
  });

  it("treats a failure with neither status nor code as a network problem", () => {
    expect(toUserFacingActivationError(new TypeError("Failed to fetch"))).toMatchObject({
      title: "Unable to connect",
      referenceCode: "network_unreachable",
    });
  });

  it("recognises a claim that could not be stored on the device", () => {
    const out = toUserFacingActivationError(new Error("invalid ProductActivationClaimRecord"));
    expect(out.referenceCode).toBe("VALIDATION_FAILED");
  });

  it("uses the stored technical details when the error carries them", () => {
    const err = Object.assign(new Error("x"), { status: 400, technicalDetails: "body" });
    expect(toUserFacingActivationError(err)).toMatchObject({
      technicalDetails: "body",
      title: "Activation could not be completed",
    });
  });
});

describe("tenant initialisation (sent from Rust with the token held there)", () => {
  it("asks Rust without any token and returns the tenant status", async () => {
    ipc.invoke.mockResolvedValue(answered({ tenant_id: "t-1", is_initialized: true }));
    expect(await markActivationTenantInitialized()).toMatchObject({ is_initialized: true });
    expect(ipc.invoke).toHaveBeenCalledWith("mark_activation_tenant_initialized");
  });

  it("fails as tenant_init_failed when this PC holds no activation", async () => {
    ipc.invoke.mockResolvedValue({ kind: "not_activated" });
    await expect(markActivationTenantInitialized()).rejects.toMatchObject({
      code: "tenant_init_failed",
    });
  });

  it("tells an unavailable initialisation (404) from a failed one", async () => {
    ipc.invoke.mockResolvedValue({ kind: "http_error", status: 404, details: null });
    await expect(markActivationTenantInitialized()).rejects.toMatchObject({
      code: "tenant_init_unavailable",
      message: "Tenant initialization failed (404)",
      technicalDetails: "POST /api/v1/activation/tenant-status/initialize → 404",
    });
    ipc.invoke.mockResolvedValue({ kind: "http_error", status: 500, details: "database down" });
    await expect(markActivationTenantInitialized()).rejects.toMatchObject({
      code: "tenant_init_failed",
      status: 500,
      message: "database down",
      technicalDetails: "database down",
    });
    ipc.invoke.mockResolvedValue({ kind: "network_error", message: "offline" });
    await expect(markActivationTenantInitialized()).rejects.toThrow("offline");
  });
});

describe("re-claim with the key stored in Rust", () => {
  const STATE = {
    complete: true,
    status: "active",
    pending_online_validation: false,
    retry_attempt: 0,
  };

  it("asks Rust with the label and version only, and returns the recorded state", async () => {
    ipc.invoke.mockResolvedValue(answered(STATE));
    expect(
      await reclaimProductActivation({ machine_label: "Bench", app_version: "0.9.2" }),
    ).toEqual({ kind: "reclaimed", state: STATE });
    expect(ipc.invoke).toHaveBeenCalledWith("reclaim_product_activation", {
      machineLabel: "Bench",
      appVersion: "0.9.2",
    });
  });

  it("says when no key is stored here, and throws error answers as the refresh does", async () => {
    ipc.invoke.mockResolvedValue({ kind: "not_activated" });
    expect(await reclaimProductActivation({})).toEqual({ kind: "not_activated" });
    ipc.invoke.mockResolvedValue({ kind: "http_error", status: 403, code: "license_revoked" });
    await expect(reclaimProductActivation({})).rejects.toMatchObject({
      code: "license_revoked",
      status: 403,
    });
  });
});

describe("policy refresh (sent from Rust with the stored activation token)", () => {
  const STATE = {
    complete: true,
    status: "active",
    pending_online_validation: false,
    retry_attempt: 0,
  };

  it("asks Rust with the app version only, never with a token, and returns the recorded state", async () => {
    ipc.invoke.mockResolvedValue({ kind: "refreshed", state: STATE });
    expect(await refreshProductActivationPolicy("0.9.2")).toEqual({
      kind: "refreshed",
      state: STATE,
    });
    expect(ipc.invoke).toHaveBeenCalledWith("refresh_product_activation_policy", {
      appVersion: "0.9.2",
    });
    await refreshProductActivationPolicy();
    expect(ipc.invoke).toHaveBeenLastCalledWith("refresh_product_activation_policy", {
      appVersion: null,
    });
  });

  it("says when this PC holds no activation token", async () => {
    ipc.invoke.mockResolvedValue({ kind: "no_token" });
    expect(await refreshProductActivationPolicy("0.9.2")).toEqual({ kind: "no_token" });
  });

  it("throws the server's error answer as every activation call does", async () => {
    ipc.invoke.mockResolvedValue({
      kind: "http_error",
      status: 403,
      code: "license_revoked",
      message: "Revoked",
      details: '{"error":"license_revoked","message":"Revoked"}',
    });
    await expect(refreshProductActivationPolicy("0.9.2")).rejects.toMatchObject({
      code: "license_revoked",
      status: 403,
      message: "Revoked",
      technicalDetails: '{"error":"license_revoked","message":"Revoked"}',
    });

    ipc.invoke.mockResolvedValue({
      kind: "http_error",
      status: 409,
      code: null,
      message: null,
      details: null,
    });
    const slot = (await refreshProductActivationPolicy().catch((e: unknown) => e)) as Error &
      Record<string, unknown>;
    expect(slot["code"]).toBe("slot_limit_reached");
    expect(slot.message).toMatch(/device slot/i);
    expect(slot["technicalDetails"]).toBe("Policy refresh HTTP 409");

    ipc.invoke.mockResolvedValue({ kind: "http_error", status: 503 });
    const down = (await refreshProductActivationPolicy().catch((e: unknown) => e)) as Error &
      Record<string, unknown>;
    expect(down.message).toBe("Policy refresh failed (503)");
    expect(down["code"]).toBeUndefined();
    expect(down["status"]).toBe(503);
  });

  it("throws a plain error, with neither code nor status, when no usable answer came", async () => {
    for (const kind of ["network_error", "invalid_response"] as const) {
      ipc.invoke.mockResolvedValue({ kind, message: "Cannot reach the control API" });
      const err = (await refreshProductActivationPolicy().catch((e: unknown) => e)) as Error &
        Record<string, unknown>;
      expect(err.message).toBe("Cannot reach the control API");
      expect(err["code"]).toBeUndefined();
      expect(err["status"]).toBeUndefined();
      expect(toUserFacingActivationError(err).referenceCode).toBe("network_unreachable");
    }
  });
});

describe("activation stored on the device", () => {
  it("returns nothing, not an empty object, when the device holds no activation", async () => {
    ipc.invoke.mockResolvedValue(null);
    expect(await getActivationLicenseMetadata()).toBeNull();
    expect(await getProductLicenseDiagnostics()).toBeNull();
  });
});
