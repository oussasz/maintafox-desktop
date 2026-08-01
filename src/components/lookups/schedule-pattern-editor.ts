/**
 * Pure schedule-pattern editor model for ORG.SCHEDULE_CLASS UX.
 * Maps to/from ScheduleDayPattern only at IPC boundaries — no schema changes.
 */

import type { ScheduleDayPattern } from "@shared/ipc-types";

export type DayMode = "work" | "rest";
export type DayOfWeek = 1 | 2 | 3 | 4 | 5 | 6 | 7;

export interface ShiftInterval {
  /** HH:mm */
  start: string;
  /** HH:mm */
  end: string;
}

export interface EditorDay {
  dayOfWeek: DayOfWeek;
  mode: DayMode;
  /** Work days: exactly one interval in current release. Rest: empty. */
  intervals: ShiftInterval[];
}

export type ScheduleTemplateId =
  | "normal_day"
  | "two_by_eight"
  | "three_by_eight"
  | "continuous_24_7"
  | "weekend_only"
  | "personalized";

export interface AppliedTemplateResult {
  days: EditorDay[];
  isContinuous: boolean;
  templateId: ScheduleTemplateId;
}

export interface ScheduleEditorMetrics {
  workingDays: number;
  restDays: number;
  weeklyHours: number;
  selectedDayHours: number;
  nominalHoursPerDay: number;
}

export interface ScheduleEditorValidation {
  valid: boolean;
  /** i18n key under schedulePattern.validation.* */
  errorKey: string | null;
}

const HH_MM = /^([01]\d|2[0-3]):([0-5]\d)$/;

export const DAY_KEYS = [
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
  "sunday",
] as const;

export function isValidHhMm(raw: string): boolean {
  return HH_MM.test(raw.trim());
}

/** Minutes from midnight; overnight end handled by caller. */
function toMinutes(hhmm: string): number | null {
  if (!isValidHhMm(hhmm)) return null;
  const parts = hhmm.split(":");
  const h = Number(parts[0]);
  const m = Number(parts[1]);
  if (!Number.isFinite(h) || !Number.isFinite(m)) return null;
  return h * 60 + m;
}

/**
 * Duration in hours. Overnight when end <= start (e.g. 22:00 → 06:00 = 8h).
 * Identical start/end is treated as 0 (invalid for work days).
 */
export function intervalDurationHours(start: string, end: string): number {
  const a = toMinutes(start);
  const b = toMinutes(end);
  if (a == null || b == null) return 0;
  if (a === b) return 0;
  const mins = b > a ? b - a : 24 * 60 - a + b;
  return mins / 60;
}

export function dayDurationHours(day: EditorDay): number {
  if (day.mode === "rest" || day.intervals.length === 0) return 0;
  const interval = day.intervals[0];
  if (!interval) return 0;
  return intervalDurationHours(interval.start, interval.end);
}

export function computeMetrics(
  days: EditorDay[],
  selectedDayOfWeek: DayOfWeek,
): ScheduleEditorMetrics {
  const workingDays = days.filter((d) => d.mode === "work").length;
  const restDays = days.filter((d) => d.mode === "rest").length;
  const weeklyHours = days.reduce((sum, d) => sum + dayDurationHours(d), 0);
  const selected = days.find((d) => d.dayOfWeek === selectedDayOfWeek);
  const selectedDayHours = selected ? dayDurationHours(selected) : 0;
  const nominalHoursPerDay =
    workingDays > 0 ? Math.round((weeklyHours / workingDays) * 100) / 100 : 0;
  return {
    workingDays,
    restDays,
    weeklyHours: Math.round(weeklyHours * 100) / 100,
    selectedDayHours: Math.round(selectedDayHours * 100) / 100,
    nominalHoursPerDay,
  };
}

function workDay(dayOfWeek: DayOfWeek, start: string, end: string): EditorDay {
  return { dayOfWeek, mode: "work", intervals: [{ start, end }] };
}

function restDay(dayOfWeek: DayOfWeek): EditorDay {
  return { dayOfWeek, mode: "rest", intervals: [] };
}

function weekFrom(
  weekday: { start: string; end: string } | "rest",
  weekend: { start: string; end: string } | "rest",
): EditorDay[] {
  return ([1, 2, 3, 4, 5, 6, 7] as DayOfWeek[]).map((d) => {
    const spec = d <= 5 ? weekday : weekend;
    if (spec === "rest") return restDay(d);
    return workDay(d, spec.start, spec.end);
  });
}

export function defaultEditorDays(): EditorDay[] {
  return applyTemplate("normal_day").days;
}

export function applyTemplate(id: ScheduleTemplateId): AppliedTemplateResult {
  switch (id) {
    case "normal_day":
      return {
        templateId: id,
        isContinuous: false,
        days: weekFrom({ start: "08:00", end: "16:00" }, "rest"),
      };
    case "two_by_eight":
      return {
        templateId: id,
        isContinuous: false,
        days: weekFrom({ start: "06:00", end: "22:00" }, "rest"),
      };
    case "three_by_eight":
      return {
        templateId: id,
        isContinuous: false,
        days: ([1, 2, 3, 4, 5, 6, 7] as DayOfWeek[]).map((d) => workDay(d, "00:00", "23:59")),
      };
    case "continuous_24_7":
      return {
        templateId: id,
        isContinuous: true,
        days: ([1, 2, 3, 4, 5, 6, 7] as DayOfWeek[]).map((d) => workDay(d, "00:00", "23:59")),
      };
    case "weekend_only":
      return {
        templateId: id,
        isContinuous: false,
        days: weekFrom("rest", { start: "08:00", end: "16:00" }),
      };
    case "personalized":
      return {
        templateId: id,
        isContinuous: false,
        days: defaultEditorDays(),
      };
  }
}

export const SCHEDULE_TEMPLATE_IDS: ScheduleTemplateId[] = [
  "normal_day",
  "two_by_eight",
  "three_by_eight",
  "continuous_24_7",
  "weekend_only",
  "personalized",
];

/** Bar placement on a 0–24h track; overnight splits into [start→24] + [0→end]. */
export function intervalBarSegments(
  start: string,
  end: string,
): Array<{ leftPct: number; widthPct: number }> {
  const a = toMinutes(start);
  const b = toMinutes(end);
  if (a == null || b == null || a === b) return [];
  const dayMins = 24 * 60;
  if (b > a) {
    return [{ leftPct: (a / dayMins) * 100, widthPct: ((b - a) / dayMins) * 100 }];
  }
  return [
    { leftPct: (a / dayMins) * 100, widthPct: ((dayMins - a) / dayMins) * 100 },
    { leftPct: 0, widthPct: (b / dayMins) * 100 },
  ];
}

export function validateEditorDays(days: EditorDay[]): ScheduleEditorValidation {
  if (days.length !== 7) {
    return { valid: false, errorKey: "incompleteWeek" };
  }
  const seen = new Set<number>();
  for (const day of days) {
    if (seen.has(day.dayOfWeek)) {
      return { valid: false, errorKey: "incompleteWeek" };
    }
    seen.add(day.dayOfWeek);
    if (day.mode === "rest") continue;
    const interval = day.intervals[0];
    if (!interval || !interval.start || !interval.end) {
      return { valid: false, errorKey: "missingTime" };
    }
    if (!isValidHhMm(interval.start) || !isValidHhMm(interval.end)) {
      return { valid: false, errorKey: "invalidFormat" };
    }
    if (interval.start === interval.end) {
      return { valid: false, errorKey: "identicalTimes" };
    }
  }
  return { valid: true, errorKey: null };
}

export function fromScheduleDetails(details: ScheduleDayPattern[]): EditorDay[] {
  const byDow = new Map(details.map((d) => [d.day_of_week, d]));
  return ([1, 2, 3, 4, 5, 6, 7] as DayOfWeek[]).map((dow) => {
    const row = byDow.get(dow);
    if (!row || row.is_rest_day) return restDay(dow);
    const start = normalizeTime(row.shift_start);
    const end = normalizeTime(row.shift_end);
    return workDay(dow, start, end);
  });
}

/** Persist rest days with stable placeholder times (backend requires HH:mm). */
export function toScheduleDetails(days: EditorDay[]): ScheduleDayPattern[] {
  return days.map((day) => {
    if (day.mode === "rest") {
      return {
        day_of_week: day.dayOfWeek,
        shift_start: "08:00",
        shift_end: "16:00",
        is_rest_day: true,
      };
    }
    const interval = day.intervals[0] ?? { start: "08:00", end: "16:00" };
    return {
      day_of_week: day.dayOfWeek,
      shift_start: interval.start,
      shift_end: interval.end,
      is_rest_day: false,
    };
  });
}

function normalizeTime(raw: string): string {
  const t = raw.trim();
  if (isValidHhMm(t)) return t;
  // Accept HH:mm:ss from some backends
  const m = /^([01]\d|2[0-3]):([0-5]\d):([0-5]\d)$/.exec(t);
  if (m) return `${m[1]}:${m[2]}`;
  return "08:00";
}

export function updateDayMode(day: EditorDay, mode: DayMode): EditorDay {
  if (mode === "rest") {
    return { ...day, mode: "rest", intervals: [] };
  }
  if (day.intervals.length === 1) {
    return { ...day, mode: "work" };
  }
  return {
    ...day,
    mode: "work",
    intervals: [{ start: "08:00", end: "16:00" }],
  };
}

export function updateDayInterval(day: EditorDay, patch: Partial<ShiftInterval>): EditorDay {
  if (day.mode === "rest") return day;
  const current = day.intervals[0] ?? { start: "08:00", end: "16:00" };
  return {
    ...day,
    mode: "work",
    intervals: [{ ...current, ...patch }],
  };
}
