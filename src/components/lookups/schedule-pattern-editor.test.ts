import { describe, expect, it } from "vitest";

import {
  applyTemplate,
  computeMetrics,
  fromScheduleDetails,
  intervalBarSegments,
  intervalDurationHours,
  toScheduleDetails,
  validateEditorDays,
} from "@/components/lookups/schedule-pattern-editor";

describe("schedule-pattern-editor", () => {
  it("computes overnight duration 22:00 → 06:00 as 8h", () => {
    expect(intervalDurationHours("22:00", "06:00")).toBe(8);
  });

  it("rejects identical start/end as zero duration", () => {
    expect(intervalDurationHours("08:00", "08:00")).toBe(0);
  });

  it("validates normal day template", () => {
    const { days } = applyTemplate("normal_day");
    expect(validateEditorDays(days).valid).toBe(true);
    const metrics = computeMetrics(days, 1);
    expect(metrics.workingDays).toBe(5);
    expect(metrics.restDays).toBe(2);
    expect(metrics.weeklyHours).toBe(40);
  });

  it("maps round-trip to schedule details", () => {
    const { days } = applyTemplate("two_by_eight");
    const details = toScheduleDetails(days);
    expect(details).toHaveLength(7);
    expect(details[0]?.is_rest_day).toBe(false);
    expect(details[0]?.shift_start).toBe("06:00");
    expect(details[5]?.is_rest_day).toBe(true);
    const back = fromScheduleDetails(details);
    expect(back[0]?.mode).toBe("work");
    expect(back[5]?.mode).toBe("rest");
  });

  it("splits overnight bars across midnight", () => {
    const segs = intervalBarSegments("22:00", "06:00");
    expect(segs).toHaveLength(2);
    expect(segs[0]?.leftPct).toBeCloseTo((22 / 24) * 100);
  });

  it("flags identical times as invalid on work days", () => {
    const { days } = applyTemplate("normal_day");
    days[0] = {
      dayOfWeek: 1,
      mode: "work",
      intervals: [{ start: "08:00", end: "08:00" }],
    };
    expect(validateEditorDays(days).errorKey).toBe("identicalTimes");
  });
});
