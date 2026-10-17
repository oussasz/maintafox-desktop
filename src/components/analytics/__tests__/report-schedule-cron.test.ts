import { describe, expect, it } from "vitest";

import {
  type ScheduleSpec,
  buildScheduleCron,
  parseScheduleCron,
} from "@/components/analytics/report-schedule-cron";

/** Shape accepted by the backend's `cron` 0.12 parser for the three picker forms. */
const BACKEND_SHAPE = /^0 \d{1,2} \d{1,2} (\*|\d{1,2}) \* (\*|Sun|Mon|Tue|Wed|Thu|Fri|Sat)$/;

const ALGIERS = 60; // UTC+1, no DST
const MONTREAL_WINTER = -300; // UTC-5

function spec(partial: Partial<ScheduleSpec>): ScheduleSpec {
  return { frequency: "daily", time: "07:00", weekday: 1, monthDay: 1, ...partial };
}

function cronOf(s: ScheduleSpec, offset: number): string {
  const r = buildScheduleCron(s, offset);
  if (!r.ok) throw new Error(`unexpected ${r.reason}`);
  return r.cron;
}

describe("buildScheduleCron", () => {
  it("writes six fields, seconds first, in UTC", () => {
    expect(cronOf(spec({ frequency: "daily", time: "08:00" }), 0)).toBe("0 0 8 * * *");
    expect(cronOf(spec({ frequency: "daily", time: "08:30" }), ALGIERS)).toBe("0 30 7 * * *");
    expect(cronOf(spec({ frequency: "weekly", weekday: 1, time: "07:15" }), ALGIERS)).toBe(
      "0 15 6 * * Mon",
    );
    expect(cronOf(spec({ frequency: "monthly", monthDay: 15, time: "06:00" }), ALGIERS)).toBe(
      "0 0 5 15 * *",
    );
  });

  it("moves the day when the UTC time falls on the previous or next day", () => {
    // Monday 00:30 in UTC+1 is Sunday 23:30 UTC.
    expect(cronOf(spec({ frequency: "weekly", weekday: 1, time: "00:30" }), ALGIERS)).toBe(
      "0 30 23 * * Sun",
    );
    // Sunday 00:30 in UTC+1 is Saturday 23:30 UTC (wraps the week).
    expect(cronOf(spec({ frequency: "weekly", weekday: 0, time: "00:30" }), ALGIERS)).toBe(
      "0 30 23 * * Sat",
    );
    // Saturday 21:00 in UTC-5 is Sunday 02:00 UTC.
    expect(cronOf(spec({ frequency: "weekly", weekday: 6, time: "21:00" }), MONTREAL_WINTER)).toBe(
      "0 0 2 * * Sun",
    );
    expect(cronOf(spec({ frequency: "monthly", monthDay: 10, time: "00:15" }), ALGIERS)).toBe(
      "0 15 23 9 * *",
    );
  });

  it("refuses schedules the backend cannot express", () => {
    expect(
      buildScheduleCron(spec({ frequency: "monthly", monthDay: 1, time: "00:30" }), ALGIERS),
    ).toEqual({ ok: false, reason: "monthBoundary" });
    expect(buildScheduleCron(spec({ frequency: "monthly", monthDay: 29 }), 0)).toEqual({
      ok: false,
      reason: "invalidDay",
    });
    expect(buildScheduleCron(spec({ time: "25:00" }), 0)).toEqual({
      ok: false,
      reason: "invalidTime",
    });
    expect(buildScheduleCron(spec({ time: "" }), 0)).toEqual({ ok: false, reason: "invalidTime" });
  });
});

describe("parseScheduleCron round trip", () => {
  const offsets = [0, ALGIERS, MONTREAL_WINTER, 330 /* India, half-hour offset */];
  const specs: ScheduleSpec[] = [
    spec({ frequency: "daily", time: "00:00" }),
    spec({ frequency: "daily", time: "23:45" }),
    ...[0, 1, 2, 3, 4, 5, 6].map((weekday) =>
      spec({ frequency: "weekly", weekday, time: "00:30" }),
    ),
    ...[0, 3, 6].map((weekday) => spec({ frequency: "weekly", weekday, time: "22:10" })),
    spec({ frequency: "monthly", monthDay: 2, time: "00:20" }),
    spec({ frequency: "monthly", monthDay: 15, time: "12:00" }),
    spec({ frequency: "monthly", monthDay: 27, time: "23:50" }),
  ];

  for (const offset of offsets) {
    it(`gives back the same schedule at UTC offset ${offset} min`, () => {
      for (const s of specs) {
        const r = buildScheduleCron(s, offset);
        if (!r.ok) continue; // month-boundary cases are refused, covered above
        expect(r.cron).toMatch(BACKEND_SHAPE);
        const back = parseScheduleCron(r.cron, offset);
        expect(back?.frequency).toBe(s.frequency);
        expect(back?.time).toBe(s.time);
        if (s.frequency === "weekly") expect(back?.weekday).toBe(s.weekday);
        if (s.frequency === "monthly") expect(back?.monthDay).toBe(s.monthDay);
      }
    });
  }

  it("reads the legacy default and numeric day-of-week forms", () => {
    expect(parseScheduleCron("0 0 8 * * *", ALGIERS)).toMatchObject({
      frequency: "daily",
      time: "09:00",
    });
    // cron crate numbering: Sun = 1, Mon = 2.
    expect(parseScheduleCron("0 0 6 * * 2", 0)).toMatchObject({ frequency: "weekly", weekday: 1 });
    expect(parseScheduleCron("0 0 6 * * Mon *", 0)).toMatchObject({ frequency: "weekly" });
  });

  it("reports anything else as a custom expression", () => {
    expect(parseScheduleCron("0 */15 * * * *", 0)).toBeNull();
    expect(parseScheduleCron("0 0 8 1 * Mon", 0)).toBeNull();
    expect(parseScheduleCron("0 0 8 * 1 *", 0)).toBeNull();
    expect(parseScheduleCron("30 0 8 * * *", 0)).toBeNull();
    expect(parseScheduleCron("0 0 8 * *", 0)).toBeNull();
    expect(parseScheduleCron("not a cron", 0)).toBeNull();
  });
});
