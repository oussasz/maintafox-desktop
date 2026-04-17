import { describe, expect, it } from "vitest";

import {
  auditRecordPreimage,
  computeRecordIntegritySha256,
  verifyAuditChain,
} from "../vendor-audit-support-contracts";

const EXPECTED_PREIMAGE =
  "audit_v1|rec_1|1|2026-04-16T12:00:00Z|actor_1|entitlement_suspend|entitlement|corr_1|ten_a|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb|cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc||cust_request|appr_1|ten_a|ent_1|||||sup_9";

import type { VendorAdminAuditRecordV1 } from "@shared/ipc-types";

const aa = "a".repeat(64);
const bb = "b".repeat(64);
const cc = "c".repeat(64);

function baseRecord(overrides: Partial<VendorAdminAuditRecordV1>): VendorAdminAuditRecordV1 {
  return {
    record_id: "rec_1",
    sequence: 1,
    occurred_at_rfc3339: "2026-04-16T12:00:00Z",
    actor_id: "actor_1",
    action_code: "entitlement_suspend",
    action_category: "entitlement",
    correlation_id: "corr_1",
    scope_tenant_id: "ten_a",
    before_snapshot_sha256: aa,
    after_snapshot_sha256: bb,
    payload_canonical_sha256: cc,
    chain_prev_hash: null,
    record_integrity_sha256:
      "bf10b554ae7c52657f66c7f11b9fcac5f8455fb7a55c2be01f93a9339af2509d",
    reason_code: "cust_request",
    approval_correlation_id: "appr_1",
    entity_refs: {
      tenant_id: "ten_a",
      entitlement_id: "ent_1",
      support_ticket_id: "sup_9",
    },
    ...overrides,
  };
}

describe("vendor-audit-support-contracts", () => {
  it("preimage matches Rust canonical string", () => {
    expect(auditRecordPreimage(baseRecord({}))).toBe(EXPECTED_PREIMAGE);
  });

  it("matches golden integrity vector (aligned with Rust)", async () => {
    const r = baseRecord({});
    expect(await computeRecordIntegritySha256(r)).toBe(r.record_integrity_sha256);
  });

  it("preimage excludes integrity field", () => {
    const r = baseRecord({});
    expect(auditRecordPreimage(r)).not.toContain(r.record_integrity_sha256);
  });

  it("verifies two-record chain", async () => {
    const first = baseRecord({});
    const secondHash =
      "6e471038f40be62cdfe49df97c92225d81c9d1e3227ed16c2f299643e7b7b082";
    const second = baseRecord({
      record_id: "rec_b",
      sequence: 2,
      chain_prev_hash: first.record_integrity_sha256,
      record_integrity_sha256: secondHash,
    });
    expect(await verifyAuditChain([first, second])).toBe("ok");
  });
});
