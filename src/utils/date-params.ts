/**
 * Names of the display parameters that carry a date, in texts built by the backend (notification
 * message params, coded refusal params). The backend sends stored values (UTC timestamps,
 * calendar days); the screens show them in the reader's language and time zone.
 */

import { formatDateTime, formatNumber } from "@/utils/formatters";

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
  // When a safety rule received from another PC was changed there (notifications).
  "changed_at",
  // When a lock was taken off by a forced removal (notifications).
  "removed_at",
  // When a location's current shortage began (stock to reconcile, notifications).
  "opened_at",
  // When two concurrent versions of a PM plan were created (notifications).
  "version_created_at",
  "other_created_at",
  // When an administrator lost rights received from another PC (notifications; may be absent).
  "since",
  // When a suspension made on another PC was lifted (permit suspended around a start, notices).
  "reinstated_at",
]);

/**
 * Parameters holding a number of hours (coded refusals send it with "." as the decimal separator,
 * at most two decimals): shown in the reader's language ("2,5" in French).
 */
export const DECIMAL_PARAMS: ReadonlySet<string> = new Set(["hours"]);

/**
 * Parameters holding a quantity or an amount of the stock and planning refusals ("12.500"
 * available, "20.000" asked): shown in the reader's language with the decimals the backend sent
 * (none lost to rounding, trailing zeros dropped: "12,5" in French).
 */
export const NUMBER_PARAMS: ReadonlySet<string> = new Set([
  // GATE_INV_LOT_INSUFFICIENT, GATE_INV_RECEIPT_LOTS_TOTAL, GATE_INV_INSUFFICIENT_STOCK, GATE_INV_ISSUE_OVER_RESERVED.
  "available",
  "quantity",
  "total",
  "accepted",
  "reserved",
  // Hours of a crew member's day (planning capacity refusals).
  "share",
  "remaining",
  // Stock below its threshold (notifications).
  "available_qty",
  "threshold_qty",
]);

/** A decimal number as the backend sends it. */
const PLAIN_DECIMAL = /^-?\d+(?:\.\d+)?$/;

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
 * ("fr" | "en"), each number of hours ({@link DECIMAL_PARAMS}) with the locale's decimal
 * separator (at most two decimals), and each quantity ({@link NUMBER_PARAMS}) with it too (all the
 * decimals sent). A value that is not a readable time or number is kept as sent,
 * never replaced.
 */
export function localizeDateTimeParams(
  params: Record<string, string>,
  locale: string,
): Record<string, string> {
  const out: Record<string, string> = { ...params };
  for (const [name, value] of Object.entries(params)) {
    if (typeof value !== "string") continue;
    if (DECIMAL_PARAMS.has(name)) {
      if (PLAIN_DECIMAL.test(value.trim())) out[name] = formatNumber(Number(value), locale);
      continue;
    }
    if (NUMBER_PARAMS.has(name)) {
      const text = value.trim();
      if (PLAIN_DECIMAL.test(text)) {
        const sent = text.split(".")[1]?.length ?? 0;
        out[name] = formatNumber(Number(text), locale, { maximumFractionDigits: sent });
      }
      continue;
    }
    if (!DATE_TIME_PARAMS.has(name)) continue;
    const instant = storedInstant(value);
    if (instant) out[name] = formatDateTime(instant, locale);
  }
  return out;
}
