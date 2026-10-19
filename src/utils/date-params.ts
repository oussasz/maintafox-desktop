/**
 * Names of the display parameters that carry a date, in texts built by the backend (notification
 * message params, coded refusal params). The backend sends stored values (UTC timestamps,
 * calendar days); the screens show them in the reader's language and time zone.
 */

import { formatDateTime } from "@/utils/formatters";

/** Parameters holding an instant (stored UTC timestamp): shown as local date and time. */
export const DATE_TIME_PARAMS: ReadonlySet<string> = new Set([
  "planned_end",
  "missed_at",
  "expires_at",
  "deadline",
  "due_at",
  "start_at",
  "end_at",
  "created_at",
  // Last recorded change of a safety setting (…_CLOCK_BEHIND refusals).
  "last",
]);

/** Parameters holding a calendar day: shown as a local date. */
export const DATE_PARAMS: ReadonlySet<string> = new Set(["expires_on", "date", "until"]);

/** A time with no zone ("YYYY-MM-DD HH:MM:SS", SQLite's form): stored in UTC, as the backend reads it. */
const ZONELESS_TIME = /^(\d{4}-\d{2}-\d{2})[ T](\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?)$/;

/** A stored timestamp as an instant (RFC 3339, or zone-less read as UTC); null when unreadable. */
export function storedInstant(value: string): Date | null {
  const raw = value.trim();
  if (raw === "") return null;
  const zoneless = ZONELESS_TIME.exec(raw);
  const date = new Date(zoneless ? `${zoneless[1]}T${zoneless[2]}Z` : raw);
  return Number.isNaN(date.getTime()) ? null : date;
}

/**
 * The params with each instant ({@link DATE_TIME_PARAMS}) shown as local date and time in `locale`
 * ("fr" | "en"). A value that is not a readable time is kept as sent, never replaced.
 */
export function localizeDateTimeParams(
  params: Record<string, string>,
  locale: string,
): Record<string, string> {
  const out: Record<string, string> = { ...params };
  for (const [name, value] of Object.entries(params)) {
    if (!DATE_TIME_PARAMS.has(name) || typeof value !== "string") continue;
    const instant = storedInstant(value);
    if (instant) out[name] = formatDateTime(instant, locale);
  }
  return out;
}
