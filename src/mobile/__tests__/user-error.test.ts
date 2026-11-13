import { describe, expect, it } from "vitest";

import { classifyError, licenceDenial } from "../lib/user-error";

describe("user errors (plain words, never the engine text)", () => {
  it("reads the activation failure seen on the phone as an unreachable server", () => {
    const seen = new Error(
      "Control API request interrupted: error sending request for url (http://127.0.0.1:47831/api/v1/activation/preview)",
    );
    expect(classifyError(seen, "activation")).toEqual({ kind: "serverUnreachable" });
    expect(classifyError(new Error("Cannot reach the control API: dns error"))).toEqual({
      kind: "serverUnreachable",
    });
  });

  it("uses the engine code first", () => {
    expect(classifyError({ code: "AUTH_ERROR", message: "Invalid credentials" }, "signin")).toEqual(
      {
        kind: "badCredentials",
      },
    );
    expect(classifyError({ code: "AUTH_ERROR", message: "x" })).toEqual({ kind: "sessionExpired" });
    expect(
      classifyError({ code: "PERMISSION_DENIED", message: "Permission denied: di.create" }),
    ).toEqual({ kind: "permissionDenied" });
    expect(classifyError({ code: "TENANT_SCOPE_VIOLATION", message: "x" })).toEqual({
      kind: "accessScope",
    });
  });

  it("keeps the end of an account lockout", () => {
    const err = {
      code: "ACCOUNT_LOCKED",
      message: "locked",
      details: [{ code: "ACCOUNT_LOCKED", params: { until: "2026-10-29T10:15:00Z" } }],
    };
    expect(classifyError(err, "signin")).toEqual({
      kind: "accountLocked",
      until: "2026-10-29T10:15:00Z",
    });
  });

  it("maps licence server codes, including the mobile ones", () => {
    expect(classifyError({ code: "mobile_not_licensed", status: 403, message: "x" })).toEqual({
      kind: "mobileNotLicensed",
    });
    expect(classifyError({ code: "slot_limit_reached", status: 409, message: "x" })).toEqual({
      kind: "deviceLimit",
    });
    expect(classifyError({ status: 404, message: "x" }, "activation")).toEqual({
      kind: "licenseNotFound",
    });
    expect(classifyError({ status: 503, message: "x" })).toEqual({ kind: "serverError" });
    expect(licenceDenial("license_expired")).toEqual({ kind: "licenseExpired" });
    expect(licenceDenial(null)).toEqual({ kind: "licenseDenied" });
  });

  it("falls back to a generic sentence for anything else", () => {
    expect(classifyError({ code: "INTERNAL_ERROR", message: "keyguard unreachable" })).toEqual({
      kind: "generic",
    });
    expect(classifyError("boom")).toEqual({ kind: "generic" });
  });
});
