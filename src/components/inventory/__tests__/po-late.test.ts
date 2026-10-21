import { describe, expect, it } from "vitest";

import { isPoLate, poLateDays, utcNowText } from "@/components/inventory/po-late";

const LATE = ["APPROVED", "PARTIALLY_RECEIVED"];

describe("late purchase orders (the server's UTC rule)", () => {
  it("writes now in the server's UTC text form", () => {
    expect(utcNowText(new Date("2026-10-17T23:30:00+01:00"))).toBe("2026-10-17T22:30:00Z");
  });

  it("compares in UTC, not in the PC's clock zone", () => {
    // 00:30 on the 18th in Algiers (UTC+1) is still the 17th in UTC.
    const now = new Date("2026-10-18T00:30:00+01:00");
    expect(isPoLate("2026-10-17T23:45:00Z", "APPROVED", LATE, now)).toBe(false);
    expect(isPoLate("2026-10-17T23:00:00Z", "APPROVED", LATE, now)).toBe(true);
  });

  it("is late only in a late-able status sent by the server", () => {
    const now = new Date("2026-10-20T08:00:00Z");
    expect(isPoLate("2026-10-01", "APPROVED", LATE, now)).toBe(true);
    expect(isPoLate("2026-10-01", "RECEIVED_CLOSED", LATE, now)).toBe(false);
    expect(isPoLate("2026-10-01", "DRAFT", LATE, now)).toBe(false);
    expect(isPoLate(null, "APPROVED", LATE, now)).toBe(false);
    expect(isPoLate("2026-10-01", "APPROVED", [], now)).toBe(false);
  });

  it("counts whole days late, at least one once late", () => {
    const now = new Date("2026-10-20T08:00:00Z");
    expect(poLateDays("2026-10-15T08:00:00Z", "APPROVED", LATE, now)).toBe(5);
    expect(poLateDays("2026-10-20T07:00:00Z", "APPROVED", LATE, now)).toBe(1);
    expect(poLateDays("2026-10-21T00:00:00Z", "APPROVED", LATE, now)).toBeNull();
  });
});
