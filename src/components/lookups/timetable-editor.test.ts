import { describe, expect, it } from "vitest";

import {
  TIMETABLE_TEMPLATE_IDS,
  grossMinutes,
  isoWeekday,
  netMinutes,
  plusDaysIso,
  resizeCycle,
  templateDraft,
  timetableMetrics,
  validateTimetable,
} from "./timetable-editor";

describe("timetable editor", () => {
  it("counts net time: shift minus break, night shifts past midnight", () => {
    expect(grossMinutes("22:00", "06:00")).toBe(480);
    expect(grossMinutes("8h", "16:00")).toBeNull();
    expect(
      netMinutes({
        cycle_day: 1,
        shift_start: "08:00",
        shift_end: "16:00",
        break_minutes: 60,
        is_rest_day: false,
      }),
    ).toBe(420);
    expect(
      netMinutes({
        cycle_day: 1,
        shift_start: "08:00",
        shift_end: "16:00",
        break_minutes: 0,
        is_rest_day: true,
      }),
    ).toBe(0);
  });

  it("builds valid templates: Algerian day week, 2×8 and 3×8 rotations", () => {
    for (const id of TIMETABLE_TEMPLATE_IDS) {
      expect(validateTimetable(templateDraft(id, "2026-11-01"), 85)).toBeNull();
    }
    const week = templateDraft("day_week", "2026-11-01");
    // Friday (5) and Saturday (6) rest; 5 days × 7 h net.
    expect(week.days.filter((d) => d.is_rest_day).map((d) => d.cycle_day)).toEqual([5, 6]);
    expect(timetableMetrics(week.days, 85)).toEqual({
      workingDays: 5,
      netHoursPerCycle: 35,
      plannableHoursPerCycle: 29.75,
    });
    const threeByEight = templateDraft("three_by_eight", "2026-11-01");
    expect(threeByEight.isContinuous).toBe(true);
    expect(threeByEight.cycleLength).toBe(8);
  });

  it("explains what is wrong before saving", () => {
    const draft = templateDraft("day_week", "2026-11-01");
    expect(validateTimetable(draft, null)).toBe("utilisation");
    expect(validateTimetable(draft, 0)).toBe("utilisation");
    expect(validateTimetable(draft, 101)).toBe("utilisation");
    const longBreak = {
      ...draft,
      days: draft.days.map((d) => (d.cycle_day === 1 ? { ...d, break_minutes: 480 } : d)),
    };
    expect(validateTimetable(longBreak, 85)).toBe("breakTooLong");
    const allRest = { ...draft, days: draft.days.map((d) => ({ ...d, is_rest_day: true })) };
    expect(validateTimetable(allRest, 85)).toBe("noWorkingDay");
    const rotation = { ...templateDraft("two_by_eight", ""), rotationStart: "" };
    expect(validateTimetable(rotation, 85)).toBe("rotationStart");
    expect(validateTimetable({ ...draft, cycleLength: 8 }, 85)).toBe("cycleLength");
  });

  it("resizes a rotation and dates its days", () => {
    const days = resizeCycle(templateDraft("three_by_eight", "2026-11-01").days, 10);
    expect(days).toHaveLength(10);
    expect(days[9]).toMatchObject({ cycle_day: 10, is_rest_day: true });
    expect(resizeCycle(days, 3).map((d) => d.cycle_day)).toEqual([1, 2, 3]);
    expect(plusDaysIso("2026-12-31", 1)).toBe("2027-01-01");
    expect(isoWeekday("2026-10-15")).toBe(4);
    expect(isoWeekday("2026-10-18")).toBe(7);
  });
});
