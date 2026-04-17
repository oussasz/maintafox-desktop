import { describe, expect, it } from "vitest";

import {
  VendorAdminMutationEnvelopeSchema,
  VendorAdminMfaEventSchema,
  VendorConsolePermissionSchema,
} from "../vendor-admin-console-contracts";

describe("vendor-admin-console-contracts", () => {
  it("parses vendor console permission literals", () => {
    expect(VendorConsolePermissionSchema.parse("console.view")).toBe("console.view");
  });

  it("rejects invalid admin mutation action slug", () => {
    const r = VendorAdminMutationEnvelopeSchema.safeParse({
      contract_family: "admin",
      api_version: "v1",
      action: "BadAction",
      idempotency_key: "idem-001",
    });
    expect(r.success).toBe(false);
  });

  it("accepts valid mutation envelope", () => {
    const r = VendorAdminMutationEnvelopeSchema.safeParse({
      contract_family: "admin",
      api_version: "v1",
      action: "tenant.suspend",
      idempotency_key: "550e8400-e29b-41d4-a716-446655440000",
      correlation_id: "corr-abc-123",
    });
    expect(r.success).toBe(true);
  });

  it("parses MFA audit event", () => {
    const e = VendorAdminMfaEventSchema.parse({
      kind: "mfa_success",
      correlation_id: "corr-12345678",
      actor_id: "admin-1",
      route: "https://console.maintafox.systems/vendor-console/entitlements",
      detail_code: "totp_ok",
    });
    expect(e.kind).toBe("mfa_success");
  });
});
