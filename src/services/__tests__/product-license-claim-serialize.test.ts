import { describe, expect, it } from "vitest";

import {
  assertClaimSerializableForDesktop,
  serializeClaimJsonForDesktop,
  toDesktopActivationClaimRecord,
  type ProductActivationClaim,
} from "@/services/product-license-service";

function baseClaim(overrides: Partial<ProductActivationClaim> = {}): ProductActivationClaim {
  return {
    tenant_id: "tenant-1",
    license_id: "license-1",
    machine_fingerprint: "fp-1",
    activation_token: "token-1",
    edition: "core",
    license_tier: "professional",
    license_plan: "professional",
    tenant_display_name: "Acme",
    device_limit: 5,
    ...overrides,
  };
}

describe("desktop activation claim serialization", () => {
  it("omits serde aliases that would duplicate Rust fields", () => {
    const record = toDesktopActivationClaimRecord(baseClaim());
    expect(record).not.toHaveProperty("edition");
    expect(record).not.toHaveProperty("slot_limit");
    expect(record).not.toHaveProperty("company_display_name");
    expect(record["license_tier"]).toBe("core");
    expect(record["license_plan"]).toBe("core");
    expect(record["tenant_display_name"]).toBe("Acme");
    expect(record["device_limit"]).toBe(5);
  });

  it("includes entitlement_envelope only in claimJson wrapper", () => {
    const claim = baseClaim({
      entitlement_envelope: {
        envelope_id: "env-1",
        lineage_version: 1,
        issuer: "maintafox",
        key_id: "k1",
        signature_alg: "sha256:issuer-key-v1",
        tier: "core",
        state: "active",
        channel: "stable",
        machine_slots: 5,
        feature_flags_json: "{}",
        capabilities_json: "{}",
        policy_json: "{}",
        issued_at: "2026-01-01T00:00:00Z",
        valid_from: "2026-01-01T00:00:00Z",
        valid_until: "2027-01-01T00:00:00Z",
        offline_grace_until: "2026-02-01T00:00:00Z",
        signature: "sig",
      },
    });
    const record = toDesktopActivationClaimRecord(claim);
    expect(record).not.toHaveProperty("entitlement_envelope");

    const parsed = JSON.parse(serializeClaimJsonForDesktop(claim)) as Record<string, unknown>;
    expect(parsed["entitlement_envelope"]).toEqual(claim.entitlement_envelope);
    expect(parsed["edition"]).toBeUndefined();
    expect(parsed["activation_token"]).toBe("token-1");
  });

  it("assertClaimSerializableForDesktop accepts normalized claims", () => {
    expect(() => assertClaimSerializableForDesktop(baseClaim())).not.toThrow();
  });

  it("assertClaimSerializableForDesktop rejects missing activation_token", () => {
    expect(() => assertClaimSerializableForDesktop(baseClaim({ activation_token: "   " }))).toThrow(
      /activation_token/,
    );
  });
});
