/**
 * Date and time text of the date fields.
 *
 * The fields show and read dates the way the app language writes them (dd/mm/yyyy in French), not
 * the way the Windows region does (native date inputs follow Windows, which put mm/dd/yyyy and a
 * 12-hour clock in French screens). Stored values stay ISO: `YYYY-MM-DD`, `YYYY-MM-DDTHH:mm`.
 */

import { intlLocaleForLanguage } from "@/utils/format-date";

export type DatePart = "day" | "month" | "year";

export interface DateInputFormat {
  /** Order of the three parts, e.g. `["day", "month", "year"]` for French. */
  order: [DatePart, DatePart, DatePart];
  separator: string;
}

const FORMATS = new Map<string, DateInputFormat>();

/** Order and separator of numeric dates in the language of the app. */
export function dateInputFormat(language: string | undefined): DateInputFormat {
  const locale = intlLocaleForLanguage(language);
  const cached = FORMATS.get(locale);
  if (cached) return cached;
  let format: DateInputFormat = { order: ["day", "month", "year"], separator: "/" };
  try {
    const parts = new Intl.DateTimeFormat(locale, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).formatToParts(new Date(2024, 10, 25));
    const order = parts
      .map((p) => p.type)
      .filter((t): t is DatePart => t === "day" || t === "month" || t === "year");
    const separator = parts.find((p) => p.type === "literal")?.value.trim() || "/";
    if (order.length === 3) {
      format = { order: [order[0], order[1], order[2]] as DateInputFormat["order"], separator };
    }
  } catch {
    // Unknown locale: day/month/year is the safe default for the users of this app.
  }
  FORMATS.set(locale, format);
  return format;
}

const ISO_DATE = /^(\d{4})-(\d{2})-(\d{2})$/;

/** True for a real calendar date written `YYYY-MM-DD`. */
export function isIsoDate(value: string): boolean {
  const m = ISO_DATE.exec(value);
  if (!m) return false;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const date = new Date(y, mo - 1, d);
  return date.getFullYear() === y && date.getMonth() === mo - 1 && date.getDate() === d;
}

/** `2026-10-25` becomes `25/10/2026` (French). An empty or invalid value gives an empty text. */
export function formatIsoDateForInput(iso: string, language: string | undefined): string {
  if (!isIsoDate(iso)) return "";
  const [y = "", m = "", d = ""] = iso.split("-");
  const { order, separator } = dateInputFormat(language);
  const byPart: Record<DatePart, string> = { day: d, month: m, year: y };
  return order.map((p) => byPart[p]).join(separator);
}

/** Placeholder text of the field, e.g. `jj/mm/aaaa`. */
export function dateInputPlaceholder(language: string | undefined): string {
  const names: Record<DatePart, string> = language?.startsWith("fr")
    ? { day: "jj", month: "mm", year: "aaaa" }
    : { day: "dd", month: "mm", year: "yyyy" };
  const { order, separator } = dateInputFormat(language);
  return order.map((p) => names[p]).join(separator);
}

/**
 * Reads what the user typed as an ISO date, or null when it is not a real date. Accepts `/`, `.`,
 * `-` or a space between the parts, a two-digit year (read as 20yy), and a full ISO date.
 */
export function parseDateInput(text: string, language: string | undefined): string | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  if (isIsoDate(trimmed)) return trimmed;
  const pieces = trimmed.split(/[\s/.-]+/).filter(Boolean);
  if (pieces.length !== 3 || pieces.some((p) => !/^\d+$/.test(p))) return null;
  const { order } = dateInputFormat(language);
  const parts: Partial<Record<DatePart, string>> = {};
  order.forEach((part, i) => {
    parts[part] = pieces[i] ?? "";
  });
  const day = Number(parts.day);
  const month = Number(parts.month);
  let yearText = parts.year ?? "";
  if (yearText.length === 2) yearText = `20${yearText}`;
  if (yearText.length !== 4) return null;
  const year = Number(yearText);
  const iso = `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
  return isIsoDate(iso) ? iso : null;
}

/** Whether `iso` lies within the inclusive `min` / `max` bounds (`YYYY-MM-DD`). */
export function isDateWithin(iso: string, min?: string, max?: string): boolean {
  if (min && iso < min) return false;
  if (max && iso > max) return false;
  return true;
}

// ── Time (24-hour `HH:mm`) ─────────────────────────────────────────────────

const TIME = /^([01]?\d|2[0-3]):([0-5]\d)$/;

/** True for a time written `HH:mm` on a 24-hour clock. */
export function isTime24(value: string): boolean {
  return /^([01]\d|2[0-3]):[0-5]\d$/.test(value);
}

/** Reads `8:05`, `0805`, `805`, `8h05` as `08:05`; null when it is not a time of day. */
export function parseTimeInput(text: string): string | null {
  const t = text.trim().toLowerCase().replace("h", ":");
  if (!t) return null;
  const direct = TIME.exec(t);
  if (direct) return `${(direct[1] ?? "").padStart(2, "0")}:${direct[2]}`;
  if (/^\d{3,4}$/.test(t)) {
    const digits = t.padStart(4, "0");
    const candidate = `${digits.slice(0, 2)}:${digits.slice(2)}`;
    return isTime24(candidate) ? candidate : null;
  }
  return null;
}

/** Splits `YYYY-MM-DDTHH:mm` into its two parts (either may be empty). */
export function splitDateTime(value: string): { date: string; time: string } {
  if (!value.trim()) return { date: "", time: "" };
  const [date = "", timePart = ""] = value.split("T");
  return { date, time: timePart.slice(0, 5) };
}

/** Joins the parts; an incomplete pair gives an empty value, like a native datetime field. */
export function joinDateTime(date: string, time: string): string {
  return date && time ? `${date}T${time}` : "";
}
