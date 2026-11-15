/**
 * Validity of a library document (L7-18): its `validUntil` date against today.
 *
 * - `expired`: the last valid day is before today;
 * - `soon`: it ends today or within the next {@link EXPIRY_WARNING_DAYS} days;
 * - `valid`: later;
 * - `null`: the document never expires (no date).
 */

/** Days before the end of validity from which a document is shown as expiring soon. */
export const EXPIRY_WARNING_DAYS = 30;

export type DocumentValidity = "expired" | "soon" | "valid";

/** Local calendar date `YYYY-MM-DD` of `now`. */
export function localIsoDate(now: Date): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function addDays(iso: string, days: number): string {
  const [y, m, d] = iso.split("-").map(Number) as [number, number, number];
  return localIsoDate(new Date(y, m - 1, d + days));
}

export function documentValidity(
  validUntil: string | null | undefined,
  now: Date = new Date(),
): DocumentValidity | null {
  if (!validUntil) return null;
  const today = localIsoDate(now);
  // ISO dates compare as text.
  if (validUntil < today) return "expired";
  if (validUntil <= addDays(today, EXPIRY_WARNING_DAYS)) return "soon";
  return "valid";
}
