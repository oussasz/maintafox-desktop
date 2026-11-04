import { describe, expect, it } from "vitest";

import { isPermitOverdue } from "../permit-rules";

const NOW = Date.parse("2030-01-01T10:00:00Z");

describe("isPermitOverdue (L4-08)", () => {
  it("counts an open permit as expired from its expiry time on, like the backend", () => {
    for (const status of ["issued", "active", "suspended", "revalidation_required"]) {
      expect(isPermitOverdue({ status, expires_at: "2030-01-01T10:00:00Z" }, NOW)).toBe(true);
      expect(isPermitOverdue({ status, expires_at: "2030-01-01T09:00:00Z" }, NOW)).toBe(true);
      expect(isPermitOverdue({ status, expires_at: "2030-01-01T10:00:01Z" }, NOW)).toBe(false);
    }
  });

  it("never flags a permit that is not open", () => {
    for (const status of [
      "draft",
      "pending_review",
      "approved",
      "closed",
      "handed_back",
      "cancelled",
      "expired",
    ]) {
      expect(isPermitOverdue({ status, expires_at: "2020-01-01T00:00:00Z" }, NOW)).toBe(false);
    }
  });

  it("does not flag a permit without a readable expiry", () => {
    expect(isPermitOverdue({ status: "active", expires_at: null }, NOW)).toBe(false);
    expect(isPermitOverdue({ status: "active", expires_at: "pas une date" }, NOW)).toBe(false);
  });
});
