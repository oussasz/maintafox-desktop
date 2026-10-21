/**
 * Classe horaire editor model: a cycle of days (a week, or an N-day rotation from a start date),
 * each with a shift, a break and net hours; templates for the usual patterns. Mirrors the
 * validation of `personnel::timetable` so mistakes show before saving.
 */
import type { ScheduleCycleDay } from "@/services/workforce-service";

/** Longest rotation the backend accepts (eight weeks). */
export const MAX_CYCLE_DAYS = 56;
export const WEEK_DAYS = 7;

export type TimetableTemplateId = "day_week" | "two_by_eight" | "three_by_eight" | "continuous_day";

export const TIMETABLE_TEMPLATE_IDS: TimetableTemplateId[] = [
  "day_week",
  "two_by_eight",
  "three_by_eight",
  "continuous_day",
];

export interface TimetableDraft {
  /** `week`: cycle days 1–7 are Monday–Sunday. `rotation`: N days from `rotationStart`. */
  kind: "week" | "rotation";
  cycleLength: number;
  rotationStart: string;
  days: ScheduleCycleDay[];
  isContinuous: boolean;
}

function toMinutes(hhmm: string): number | null {
  const m = /^(\d{2}):(\d{2})(?::\d{2})?$/.exec(hhmm.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

/** Shift length in minutes; a shift ending at or before its start runs past midnight. */
export function grossMinutes(start: string, end: string): number | null {
  const s = toMinutes(start);
  const e = toMinutes(end);
  if (s == null || e == null) return null;
  const d = e - s;
  return d <= 0 ? d + 24 * 60 : d;
}

/** Net minutes of a day (shift minus break), 0 for a rest day or an unreadable shift. */
export function netMinutes(day: ScheduleCycleDay): number {
  if (day.is_rest_day) return 0;
  const gross = grossMinutes(day.shift_start, day.shift_end);
  return gross == null ? 0 : Math.max(0, gross - day.break_minutes);
}

export interface TimetableMetrics {
  workingDays: number;
  netHoursPerCycle: number;
  /** Hours available for work orders over one cycle (net × utilisation). */
  plannableHoursPerCycle: number | null;
}

export function timetableMetrics(
  days: ScheduleCycleDay[],
  utilisation: number | null,
): TimetableMetrics {
  const net = days.reduce((s, d) => s + netMinutes(d), 0) / 60;
  return {
    workingDays: days.filter((d) => !d.is_rest_day).length,
    netHoursPerCycle: Math.round(net * 100) / 100,
    plannableHoursPerCycle:
      utilisation == null ? null : Math.round(((net * utilisation) / 100) * 100) / 100,
  };
}

export type TimetableIssue =
  | "cycleLength"
  | "rotationStart"
  | "time"
  | "breakTooLong"
  | "noWorkingDay"
  | "utilisation";

/** First problem of a draft (or null), keyed like the editor's validation messages. */
export function validateTimetable(
  draft: TimetableDraft,
  utilisation: number | null,
): TimetableIssue | null {
  if (
    !Number.isInteger(draft.cycleLength) ||
    draft.cycleLength < 1 ||
    draft.cycleLength > MAX_CYCLE_DAYS ||
    draft.days.length !== draft.cycleLength
  ) {
    return "cycleLength";
  }
  if (draft.kind === "rotation" && !/^\d{4}-\d{2}-\d{2}$/.test(draft.rotationStart)) {
    return "rotationStart";
  }
  for (const d of draft.days) {
    if (d.is_rest_day) continue;
    const gross = grossMinutes(d.shift_start, d.shift_end);
    if (gross == null) return "time";
    if (d.break_minutes < 0 || d.break_minutes >= gross) return "breakTooLong";
  }
  if (!draft.days.some((d) => !d.is_rest_day)) return "noWorkingDay";
  if (
    utilisation == null ||
    !Number.isFinite(utilisation) ||
    utilisation < 1 ||
    utilisation > 100
  ) {
    return "utilisation";
  }
  return null;
}

function work(cycleDay: number, start: string, end: string, breakMinutes = 0): ScheduleCycleDay {
  return {
    cycle_day: cycleDay,
    shift_start: start,
    shift_end: end,
    break_minutes: breakMinutes,
    is_rest_day: false,
  };
}

function rest(cycleDay: number): ScheduleCycleDay {
  return {
    cycle_day: cycleDay,
    shift_start: "08:00",
    shift_end: "16:00",
    break_minutes: 0,
    is_rest_day: true,
  };
}

/** ISO weekdays 5 (Friday) and 6 (Saturday): the Algerian week-end. */
const ALGERIAN_REST = new Set([5, 6]);

/**
 * Template days. Rotations start on `rotationStart` (cycle day 1).
 * - `day_week`: Sunday–Thursday 08:00–16:00 with a one-hour break (7 h net).
 * - `two_by_eight`: a 14-day rotation — a week of mornings (06:00–14:00), a week of afternoons
 *   (14:00–22:00), both Sunday–Thursday when the rotation starts on a Sunday.
 * - `three_by_eight`: an 8-day continuous rotation — 2 mornings, 2 afternoons, 2 nights, 2 rest.
 * - `continuous_day`: 08:00–16:00 every day, through rest days and holidays.
 */
export function templateDraft(id: TimetableTemplateId, rotationStart: string): TimetableDraft {
  switch (id) {
    case "day_week":
      return {
        kind: "week",
        cycleLength: WEEK_DAYS,
        rotationStart,
        isContinuous: false,
        days: Array.from({ length: 7 }, (_, i) =>
          ALGERIAN_REST.has(i + 1) ? rest(i + 1) : work(i + 1, "08:00", "16:00", 60),
        ),
      };
    case "two_by_eight":
      return {
        kind: "rotation",
        cycleLength: 14,
        rotationStart,
        isContinuous: false,
        days: Array.from({ length: 14 }, (_, i) => {
          const inWeek = i % 7;
          if (inWeek >= 5) return rest(i + 1);
          return i < 7 ? work(i + 1, "06:00", "14:00", 30) : work(i + 1, "14:00", "22:00", 30);
        }),
      };
    case "three_by_eight":
      return {
        kind: "rotation",
        cycleLength: 8,
        rotationStart,
        isContinuous: true,
        days: [
          work(1, "06:00", "14:00", 30),
          work(2, "06:00", "14:00", 30),
          work(3, "14:00", "22:00", 30),
          work(4, "14:00", "22:00", 30),
          work(5, "22:00", "06:00", 30),
          work(6, "22:00", "06:00", 30),
          rest(7),
          rest(8),
        ],
      };
    case "continuous_day":
      return {
        kind: "week",
        cycleLength: WEEK_DAYS,
        rotationStart,
        isContinuous: true,
        days: Array.from({ length: 7 }, (_, i) => work(i + 1, "08:00", "16:00", 60)),
      };
  }
}

/** Resize a rotation, keeping the days already described. */
export function resizeCycle(days: ScheduleCycleDay[], length: number): ScheduleCycleDay[] {
  const n = Math.max(1, Math.min(MAX_CYCLE_DAYS, Math.trunc(length)));
  return Array.from({ length: n }, (_, i) => days[i] ?? rest(i + 1)).map((d, i) => ({
    ...d,
    cycle_day: i + 1,
  }));
}

/** ISO date + `days`. */
export function plusDaysIso(iso: string, days: number): string {
  const d = new Date(`${iso}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

/** ISO weekday (1 = Monday … 7 = Sunday) of an ISO date. */
export function isoWeekday(iso: string): number {
  const wd = new Date(`${iso}T00:00:00Z`).getUTCDay();
  return wd === 0 ? 7 : wd;
}
