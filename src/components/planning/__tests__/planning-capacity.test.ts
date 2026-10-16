import { describe, expect, it } from "vitest";

import type { TeamCapacityLoad } from "@shared/ipc-types";

import { checkSlotCapacity, slotDays, slotShiftHours, spreadLabour } from "../planning-model";

/** A local date-time as an ISO instant (the dialog converts what the planner typed the same way). */
const local = (at: string) => new Date(at).toISOString();

/** A team day with one member on a shift window (`HH:MM`–`HH:MM`; a night shift ends the next day). */
function day(
  work_date: string,
  available: number,
  opts: { committed?: number; basis?: string; shift?: [string, string] | null } = {},
): TeamCapacityLoad {
  const { committed = 0, basis = "roster", shift = ["08:00", "16:00"] } = opts;
  const windows = [];
  if (shift && available > 0) {
    const [s, e] = shift;
    const end = new Date(`${work_date}T${e}:00`);
    if (e <= s) end.setDate(end.getDate() + 1);
    const pad = (n: number) => String(n).padStart(2, "0");
    const endLocal = `${end.getFullYear()}-${pad(end.getMonth() + 1)}-${pad(end.getDate())}T${e}:00`;
    windows.push({ start: `${work_date}T${s}:00`, end: endLocal, hours: available });
  }
  return {
    team_id: 3,
    work_date,
    available_hours: available,
    overtime_hours: 0,
    committed_hours: committed,
    utilization_ratio: available > 0 ? committed / available : null,
    basis,
    work_windows: windows,
    unwindowed_hours: shift ? 0 : available,
  };
}

describe("slot capacity — as the backend gate checks it", () => {
  it("covers the local days of the slot with the share of each", () => {
    const days = slotDays(local("2026-05-04T08:00"), local("2026-05-06T16:00"));
    expect(days.map((d) => d.day)).toEqual(["2026-05-04", "2026-05-05", "2026-05-06"]);
    expect(days[1]?.share).toBeCloseTo(1);
    expect(slotDays(local("2026-05-04T08:00"), local("2026-05-05T00:00"))).toHaveLength(1);
  });

  it("spreads by working hours, not calendar hours: Mon 08:00 → Wed 16:00 is 8 / 8 / 8", () => {
    const rows = [day("2026-05-04", 8), day("2026-05-05", 8), day("2026-05-06", 8)];
    const shares = spreadLabour(24, local("2026-05-04T08:00"), local("2026-05-06T16:00"), rows);
    expect(shares.get("2026-05-04")).toBeCloseTo(8);
    expect(shares.get("2026-05-05")).toBeCloseTo(8);
    expect(shares.get("2026-05-06")).toBeCloseTo(8);
    const check = checkSlotCapacity({
      capacity: rows,
      teamId: 3,
      startIso: local("2026-05-04T08:00"),
      endIso: local("2026-05-06T16:00"),
      labourHours: 24,
    });
    expect(check.overloaded).toBeNull();
  });

  it("puts nothing on a rest day inside the slot", () => {
    const rows = [
      day("2026-05-04", 8),
      day("2026-05-05", 0, { basis: "rest_day" }),
      day("2026-05-06", 8),
    ];
    const shares = spreadLabour(12, local("2026-05-04T08:00"), local("2026-05-06T16:00"), rows);
    expect(shares.get("2026-05-04")).toBeCloseTo(6);
    expect(shares.has("2026-05-05")).toBe(false);
    expect(shares.get("2026-05-06")).toBeCloseTo(6);
  });

  it("books a night shift on the day it starts", () => {
    const rows = [
      day("2026-05-04", 8, { shift: ["22:00", "06:00"] }),
      day("2026-05-05", 8, { shift: ["22:00", "06:00"] }),
    ];
    const shares = spreadLabour(8, local("2026-05-04T22:00"), local("2026-05-05T06:00"), rows);
    expect(shares.get("2026-05-04")).toBeCloseTo(8);
    expect(shares.has("2026-05-05")).toBe(false);
  });

  it("counts a member without a known window in full on the days the slot touches", () => {
    const rows = [day("2026-05-04", 8, { shift: null })];
    const shares = spreadLabour(3, local("2026-05-04T10:00"), local("2026-05-04T11:00"), rows);
    expect(shares.get("2026-05-04")).toBeCloseTo(3);
  });

  it("names the first overloaded day", () => {
    const capacity = [day("2026-05-04", 8), day("2026-05-05", 8, { committed: 4 })];
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
      capacity: [day("2026-05-04", 0, { basis: "no_schedule", shift: null })],
      teamId: 3,
      startIso: local("2026-05-04T08:00"),
      endIso: local("2026-05-04T10:00"),
      labourHours: 2,
    });
    expect(none.noSchedule).toBe(true);
  });

  it("measures the shift time one person can give the slot", () => {
    const rows = [day("2026-05-04", 8), day("2026-05-05", 8)];
    expect(slotShiftHours(local("2026-05-04T12:00"), local("2026-05-05T10:00"), rows)).toBeCloseTo(
      6,
    );
    expect(
      slotShiftHours(local("2026-05-04T10:00"), local("2026-05-04T11:00"), [
        day("2026-05-04", 8, { shift: null }),
      ]),
    ).toBeNull();
  });
});
