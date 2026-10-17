/**
 * Report schedule picker ↔ cron string.
 *
 * The backend (`src-tauri/src/commands/reports.rs`, `reports/scheduler.rs`) parses
 * `report_schedules.cron_expr` with the `cron` crate 0.12 and evaluates it in **UTC**:
 * six fields, seconds first — `sec min hour day-of-month month day-of-week`, day-of-week as
 * names `Sun`…`Sat` (the crate's numbers start at Sun = 1, so names avoid ambiguity).
 *
 * Users pick a plain schedule in the computer's local time (daily / weekly on a day / monthly
 * on a day, at HH:mm); this module converts it to the UTC cron string and back. Only the three
 * shapes it writes are read back; any other expression is reported as custom (`null`).
 */

export type ScheduleFrequency = "daily" | "weekly" | "monthly";

export interface ScheduleSpec {
  frequency: ScheduleFrequency;
  /** Local time `HH:mm` (24 h). */
  time: string;
  /** Local day of week for `weekly`: 0 = Sunday … 6 = Saturday. */
  weekday: number;
  /** Local day of month for `monthly`: 1 … {@link MAX_MONTH_DAY}. */
  monthDay: number;
}

/** Days 29–31 do not exist every month; the cron crate would silently skip those months. */
export const MAX_MONTH_DAY = 28;

export const DEFAULT_SCHEDULE_SPEC: ScheduleSpec = {
  frequency: "weekly",
  time: "07:00",
  weekday: 1,
  monthDay: 1,
};

export type BuildCronResult =
  | { ok: true; cron: string }
  | { ok: false; reason: "invalidTime" | "invalidDay" | "monthBoundary" };

const CRON_DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"] as const;
const MINUTES_PER_DAY = 24 * 60;

/** Minutes to add to UTC to get this computer's local time right now (UTC+1 → 60). */
export function localUtcOffsetMinutes(at: Date = new Date()): number {
  return -at.getTimezoneOffset();
}

function mod(n: number, m: number): number {
  return ((n % m) + m) % m;
}

function parseTime(time: string): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(time.trim());
  if (!match) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  if (h > 23 || m > 59) return null;
  return h * 60 + m;
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

/** Local schedule → UTC cron string the backend stores and evaluates. */
export function buildScheduleCron(spec: ScheduleSpec, utcOffsetMinutes: number): BuildCronResult {
  const local = parseTime(spec.time);
  if (local === null) return { ok: false, reason: "invalidTime" };
  const utcTotal = local - utcOffsetMinutes;
  const dayShift = Math.floor(utcTotal / MINUTES_PER_DAY);
  const utcMinutes = mod(utcTotal, MINUTES_PER_DAY);
  const hour = Math.floor(utcMinutes / 60);
  const minute = utcMinutes % 60;

  switch (spec.frequency) {
    case "daily":
      return { ok: true, cron: `0 ${minute} ${hour} * * *` };
    case "weekly": {
      if (!Number.isInteger(spec.weekday) || spec.weekday < 0 || spec.weekday > 6) {
        return { ok: false, reason: "invalidDay" };
      }
      const day = CRON_DAY_NAMES[mod(spec.weekday + dayShift, 7)];
      return { ok: true, cron: `0 ${minute} ${hour} * * ${day}` };
    }
    case "monthly": {
      if (!Number.isInteger(spec.monthDay) || spec.monthDay < 1 || spec.monthDay > MAX_MONTH_DAY) {
        return { ok: false, reason: "invalidDay" };
      }
      const utcDay = spec.monthDay + dayShift;
      // e.g. the 1st at 00:30 in UTC+1 is the last day of the previous month in UTC.
      if (utcDay < 1 || utcDay > MAX_MONTH_DAY) return { ok: false, reason: "monthBoundary" };
      return { ok: true, cron: `0 ${minute} ${hour} ${utcDay} * *` };
    }
  }
}

function parseDayOfWeek(field: string): number | null {
  const byName = CRON_DAY_NAMES.findIndex((d) => d.toLowerCase() === field.toLowerCase());
  if (byName >= 0) return byName;
  if (/^[1-7]$/.test(field)) return Number(field) - 1; // cron crate: Sun = 1
  return null;
}

/** UTC cron string → local schedule, or `null` when it is not one of the picker's shapes. */
export function parseScheduleCron(cron: string, utcOffsetMinutes: number): ScheduleSpec | null {
  const fields = cron.trim().split(/\s+/);
  if (fields.length === 7 && fields[6] === "*") fields.pop();
  if (fields.length !== 6) return null;
  const [sec, min, hour, dom, month, dow] = fields as [
    string,
    string,
    string,
    string,
    string,
    string,
  ];
  if (sec !== "0" || month !== "*") return null;
  if (!/^\d{1,2}$/.test(min) || !/^\d{1,2}$/.test(hour)) return null;
  const utcMinute = Number(min);
  const utcHour = Number(hour);
  if (utcMinute > 59 || utcHour > 23) return null;

  const localTotal = utcHour * 60 + utcMinute + utcOffsetMinutes;
  const dayShift = Math.floor(localTotal / MINUTES_PER_DAY);
  const localMinutes = mod(localTotal, MINUTES_PER_DAY);
  const time = `${pad2(Math.floor(localMinutes / 60))}:${pad2(localMinutes % 60)}`;
  const base = { time, weekday: DEFAULT_SCHEDULE_SPEC.weekday, monthDay: 1 };

  if (dom === "*" && dow === "*") return { ...base, frequency: "daily" };
  if (dom === "*") {
    const utcDay = parseDayOfWeek(dow);
    if (utcDay === null) return null;
    return { ...base, frequency: "weekly", weekday: mod(utcDay + dayShift, 7) };
  }
  if (dow === "*" && /^\d{1,2}$/.test(dom)) {
    const localDay = Number(dom) + dayShift;
    if (localDay < 1 || localDay > MAX_MONTH_DAY) return null;
    return { ...base, frequency: "monthly", monthDay: localDay };
  }
  return null;
}
