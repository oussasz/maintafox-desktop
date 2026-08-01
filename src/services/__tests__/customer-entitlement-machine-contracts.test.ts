import { describe, expect, it } from "vitest";

import {
  BulkEntitlementOperationRequestV1Schema,
  entitlementTransitionAllowed,
  SignedClaimPreviewV1Schema,
  validateSlotLimits,
} from "../customer-entitlement-machine-contracts";

describe("customer-entitlement-machine-contracts", () => {
  it("rejects active+issue transition", () => {
    expect(entitlementTransitionAllowed("active", "issue")).toBe(false);
    expect(entitlementTransitionAllowed("expired", "issue")).toBe(true);
  });

  it("validates slot limits", () => {
    expect(validateSlotLimits(5, 3)).toBe(true);
    expect(validateSlotLimits(2, 5)).toBe(false);
  });

  it("parses signed claim preview", () => {
    const v = SignedClaimPreviewV1Schema.parse({
      schema_version: 1,
      tenant_id: "t1",
      tier: "pro",
      machine_slots: 10,
      offline_grace_hours: 72,
      update_channel: "stable",
      valid_from_rfc3339: "2026-01-01T00:00:00Z",
      valid_until_rfc3339: "2027-01-01T00:00:00Z",
      feature_flags_digest: "a".repeat(64),
      capabilities_digest: "b".repeat(64),
      issuer: "maintafox",
      key_id: "k1",
      payload_sha256: "c".repeat(64),
      signature_alg: "ed25519",
    });
    expect(v.tier).toBe("pro");
  });

  it("parses bulk dry-run request", () => {
    const b = BulkEntitlementOperationRequestV1Schema.parse({
      dry_run: true,
      tenant_ids: ["a", "b"],
      target_channel: "pilot",
      expected_lineage_version_by_tenant: [
        ["a", 2],
        ["b", 1],
      ],
    });
    expect(b.dry_run).toBe(true);
  });
});
