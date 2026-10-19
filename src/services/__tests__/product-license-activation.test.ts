/**
 * Activation against the control plane: the desktop accepts the field spellings older and newer
 * servers send, turns HTTP errors into coded errors, and maps every code to user-facing copy
 * (never raw backend text as the main message).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  type ProductActivationClaim,
  assertClaimSerializableForDesktop,
  claimProductActivation,
  getActivationLicenseMetadata,
  getActivationTenantStatus,
  getProductActivationToken,
  getProductLicenseDiagnostics,
  markActivationTenantInitialized,
  previewProductActivation,
  refreshProductActivationPolicy,
  toDesktopActivationClaimRecord,
  toUserFacingActivationError,
} from "@/services/product-license-service";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: ipc.invoke,
}));

const INPUT = { license_key: "MFX-TEST", machine_fingerprint: "fp-1" };
const BASE = { tenant_id: "t-1", license_id: "l-1", machine_fingerprint: "fp-1" };

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
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
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
    fetchMock.mockResolvedValue(respond(200, { ...BASE, ...extra }));
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
    fetchMock.mockResolvedValue(respond(200, { ...BASE, ...extra }));
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
    fetchMock.mockResolvedValue(respond(200, { ...BASE, ...extra }));
    expect((await previewProductActivation(INPUT)).tenant_display_name).toBe("Acme");
  });

  it("reads slug, counts, demo and initialisation flags from their alternative fields", async () => {
    fetchMock.mockResolvedValue(
      respond(200, {
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

    fetchMock.mockResolvedValue(
      respond(200, {
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

  it("claims with the activation token and the same normalisation", async () => {
    fetchMock.mockResolvedValue(
      respond(200, { ...BASE, activation_token: "tok", slot_limit: 2, plan: "p" }),
    );
    const claim = await claimProductActivation(INPUT);
    expect(claim).toMatchObject({ activation_token: "tok", device_limit: 2, license_tier: "p" });
    expect(String(fetchMock.mock.calls[0]?.[0])).toContain("/api/v1/activation/claim");
  });
});

describe("activation HTTP errors", () => {
  async function failure(status: number, body: unknown): Promise<Error & Record<string, unknown>> {
    fetchMock.mockResolvedValue(respond(status, body));
    try {
      await previewProductActivation(INPUT);
    } catch (err) {
      return err as Error & Record<string, unknown>;
    }
    throw new Error("expected a failure");
  }

  it("a 409 without a body means the device limit is reached", async () => {
    const err = await failure(409, "");
    expect(err["code"]).toBe("slot_limit_reached");
    expect(err.message).toMatch(/device slot/i);
    expect(err["technicalDetails"]).toBe("License validation HTTP 409");
  });

  it.each([
    [
      "top-level code and message",
      { code: "license_expired", message: "Expired" },
      "license_expired",
      "Expired",
    ],
    [
      "error string and detail",
      { error: "license_revoked", detail: "Revoked" },
      "license_revoked",
      "Revoked",
    ],
    ["error_description", { error: "x", error_description: "Desc" }, "x", "Desc"],
    [
      "nested error object",
      { error: { code: "tenant_inactive", message: "Off" } },
      "tenant_inactive",
      "Off",
    ],
  ])("reads %s", async (_label, body, code, message) => {
    const err = await failure(400, body);
    expect([err["code"], err.message, err["status"]]).toEqual([code, message, 400]);
  });

  it("keeps a non-JSON body as the message and falls back to the status otherwise", async () => {
    expect((await failure(502, "Bad gateway")).message).toBe("Bad gateway");
    const empty = await failure(503, "   ");
    expect(empty.message).toBe("License validation failed (503)");
    expect(empty["code"]).toBeUndefined();
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

describe("tenant status, initialisation and policy refresh", () => {
  it("refuses an empty activation token before calling the server", async () => {
    await expect(getActivationTenantStatus("  ")).rejects.toThrow(/token is required/);
    await expect(markActivationTenantInitialized("")).rejects.toThrow(/token is required/);
    await expect(refreshProductActivationPolicy(" ")).rejects.toThrow(/token is required/);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("reads the tenant status with the bearer token, and reports server errors with their code", async () => {
    fetchMock.mockResolvedValue(respond(200, { tenant_id: "t-1", is_initialized: false }));
    expect(await getActivationTenantStatus(" tok ")).toMatchObject({ is_initialized: false });
    const init = fetchMock.mock.calls[0]?.[1] as RequestInit;
    expect((init.headers as Record<string, string>)["Authorization"]).toBe("Bearer tok");

    fetchMock.mockResolvedValue(respond(401, { code: "invalid_token", message: "Bad token" }));
    await expect(getActivationTenantStatus("tok")).rejects.toMatchObject({
      code: "invalid_token",
      status: 401,
    });
  });

  it("tells an unavailable initialisation (404) from a failed one", async () => {
    fetchMock.mockResolvedValue(respond(200, { tenant_id: "t-1", is_initialized: true }));
    expect(await markActivationTenantInitialized("tok")).toMatchObject({ is_initialized: true });

    fetchMock.mockResolvedValue(respond(404, ""));
    await expect(markActivationTenantInitialized("tok")).rejects.toMatchObject({
      code: "tenant_init_unavailable",
      message: "Tenant initialization failed (404)",
    });
    fetchMock.mockResolvedValue(respond(500, "database down"));
    await expect(markActivationTenantInitialized("tok")).rejects.toMatchObject({
      code: "tenant_init_failed",
      message: "database down",
      technicalDetails: "database down",
    });
  });

  it("keeps the current token when the policy refresh does not return one", async () => {
    fetchMock.mockResolvedValue(respond(200, { ...BASE, plan: "pro" }));
    expect(await refreshProductActivationPolicy("tok-1")).toMatchObject({
      activation_token: "tok-1",
      license_tier: "pro",
    });
    fetchMock.mockResolvedValue(respond(200, { ...BASE, activation_token: "tok-2" }));
    expect((await refreshProductActivationPolicy("tok-1")).activation_token).toBe("tok-2");
    fetchMock.mockResolvedValue(respond(403, { error: "license_revoked" }));
    await expect(refreshProductActivationPolicy("tok-1")).rejects.toMatchObject({
      code: "license_revoked",
    });
  });

  it("sends the running app version with the policy refresh only when it is well formed", async () => {
    const sentBody = (call: number): unknown =>
      JSON.parse(String((fetchMock.mock.calls[call]?.[1] as RequestInit).body)) as unknown;
    fetchMock.mockImplementation(async () => respond(200, { ...BASE }));

    await refreshProductActivationPolicy("tok-1", "0.4.0-dev");
    expect(sentBody(0)).toEqual({ app_version: "0.4.0-dev" });
    await refreshProductActivationPolicy("tok-1", " 1.2.3 ");
    expect(sentBody(1)).toEqual({ app_version: "1.2.3" });

    // Unknown or malformed: nothing is sent, so the server applies a forced update as it would
    // to a PC that does not say its version.
    await refreshProductActivationPolicy("tok-1");
    expect(sentBody(2)).toEqual({});
    await refreshProductActivationPolicy("tok-1", "1.2");
    expect(sentBody(3)).toEqual({});
    await refreshProductActivationPolicy("tok-1", "v1.2.3");
    expect(sentBody(4)).toEqual({});
    await refreshProductActivationPolicy("tok-1", `1.2.3-${"a".repeat(30)}`);
    expect(sentBody(5)).toEqual({});
  });
});

describe("claim stored on the device", () => {
  const claim = (overrides: Partial<ProductActivationClaim> = {}): ProductActivationClaim => ({
    ...BASE,
    activation_token: "tok",
    ...overrides,
  });

  it("uses the edition, else the tier, else the plan, and nulls what the server omitted", () => {
    expect(toDesktopActivationClaimRecord(claim({ license_plan: "plan" }))).toMatchObject({
      license_tier: "plan",
      license_plan: "plan",
      expires_at: null,
      device_limit: null,
    });
    expect(toDesktopActivationClaimRecord(claim())["license_tier"]).toBeNull();
  });

  it("refuses a claim without its required identifiers", () => {
    expect(() => assertClaimSerializableForDesktop(claim({ activation_token: " " }))).toThrow(
      /missing required field: activation_token/,
    );
    expect(() => assertClaimSerializableForDesktop(claim())).not.toThrow();
  });

  it("returns nothing, not an empty object, when the device holds no activation", async () => {
    ipc.invoke.mockResolvedValue(null);
    expect(await getActivationLicenseMetadata()).toBeNull();
    expect(await getProductLicenseDiagnostics()).toBeNull();
    expect(await getProductActivationToken()).toBeNull();
    ipc.invoke.mockResolvedValue("   ");
    expect(await getProductActivationToken()).toBeNull();
    ipc.invoke.mockResolvedValue("tok");
    expect(await getProductActivationToken()).toBe("tok");
  });
});
