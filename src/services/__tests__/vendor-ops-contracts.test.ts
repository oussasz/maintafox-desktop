import { describe, expect, it } from "vitest";

import {
  repairActionAllowed,
  tenantSafeDrillThrough,
  TenantSyncHealthRowV1Schema,
  worstSeverity,
} from "../vendor-ops-contracts";

describe("vendor-ops-contracts", () => {
  it("parses tenant sync row", () => {
    const row = TenantSyncHealthRowV1Schema.parse({
      tenant_id: "t1",
      lag_seconds: 12,
      checkpoint_age_seconds: 120,
      rejection_rate_bps: 50,
      retry_pressure: 30,
      dead_letter_count: 0,
      severity: "warn",
    });
    expect(row.severity).toBe("warn");
  });

  it("worstSeverity picks critical", () => {
    expect(worstSeverity("info", "critical")).toBe("critical");
  });

  it("tenantSafeDrillThrough rejects email-like hints", () => {
    expect(tenantSafeDrillThrough({ tenant_id_hint: "a@b.com" })).toBe(false);
    expect(tenantSafeDrillThrough({ tenant_id_hint: "ten_acme", correlation_id: "c1" })).toBe(true);
  });

  it("repairActionAllowed matches escalation rules", () => {
    const base = {
      item_id: "i",
      tenant_id: "t",
      queue_kind: "pull_materialization",
      summary: "s",
      recommended_action: "replay" as const,
    };
    expect(
      repairActionAllowed({ ...base, severity: "info" }, "escalate"),
    ).toBe(false);
    expect(
      repairActionAllowed({ ...base, severity: "warn" }, "escalate"),
    ).toBe(true);
  });
});
