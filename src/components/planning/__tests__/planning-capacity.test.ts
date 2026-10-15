import { describe, expect, it } from "vitest";

import type { TeamCapacityLoad } from "@shared/ipc-types";

import { checkSlotCapacity, slotDays, spreadLabour } from "../planning-model";

/** A local date-time as an ISO instant (the dialog converts what the planner typed the same way). */
const local = (at: string) => new Date(at).toISOString();

function day(
  work_date: string,
  available: number,
  committed = 0,
  basis = "roster",
): TeamCapacityLoad {
  return {
    team_id: 3,
    work_date,
    available_hours: available,
    overtime_hours: 0,
    committed_hours: committed,
    utilization_ratio: available > 0 ? committed / available : null,
    basis,
  };
}

describe("slot capacity — as the backend gate checks it", () => {
  it("covers the local days of the slot with the share of each", () => {
    const days = slotDays(local("2026-05-04T08:00"), local("2026-05-06T16:00"));
    expect(days.map((d) => d.day)).toEqual(["2026-05-04", "2026-05-05", "2026-05-06"]);
    expect(days[1]?.share).toBeCloseTo(1);
    expect(slotDays(local("2026-05-04T08:00"), local("2026-05-05T00:00"))).toHaveLength(1);
  });

  it("spreads the labour over working days only", () => {
    const days = slotDays(local("2026-05-04T08:00"), local("2026-05-06T16:00"));
    const avail = new Map([
      ["2026-05-04", 8],
      ["2026-05-05", 0],
      ["2026-05-06", 8],
    ]);
    const shares = spreadLabour(12, days, (d) => avail.get(d) ?? 0);
    expect(shares.get("2026-05-04")).toBeCloseTo(6);
    expect(shares.has("2026-05-05")).toBe(false);
    expect(shares.get("2026-05-06")).toBeCloseTo(6);
  });

  it("lets a multi-day job fit and names the first overloaded day", () => {
    const capacity = [day("2026-05-04", 8), day("2026-05-05", 8, 4)];
    const fits = checkSlotCapacity({
      capacity,
      teamId: 3,
      startIso: local("2026-05-04T08:00"),
      endIso: local("2026-05-05T16:00"),
      labourHours: 8,
    });
    expect(fits.overloaded).toBeNull();
    const tooMuch = checkSlotCapacity({
      capacity,
      teamId: 3,
      startIso: local("2026-05-04T08:00"),
      endIso: local("2026-05-05T16:00"),
      labourHours: 12,
    });
    expect(tooMuch.overloaded?.day).toBe("2026-05-05");
  });

  it("says the team has no timetable rather than calling it full", () => {
    const none = checkSlotCapacity({
      capacity: [day("2026-05-04", 0, 0, "no_schedule")],
      teamId: 3,
      startIso: local("2026-05-04T08:00"),
      endIso: local("2026-05-04T10:00"),
      labourHours: 2,
    });
    expect(none.noSchedule).toBe(true);
  });
});
