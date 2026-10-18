/**
 * The one "late purchase order" rule, as the backend applies it (`inventory::queries`, the
 * procurement dashboard's late figure): a late-able status (`late_po_statuses`, sent by the server)
 * and an expected delivery earlier than now in UTC, compared as text exactly like the SQL
 * `expected_delivery_date < strftime('%Y-%m-%dT%H:%M:%SZ','now')`. The PC's clock zone plays no part.
 */

/** "Now" in the backend's UTC text form (`YYYY-MM-DDTHH:MM:SSZ`). */
export function utcNowText(now: Date = new Date()): string {
  return `${now.toISOString().slice(0, 19)}Z`;
}

export function isPoLate(
  expectedDelivery: string | null | undefined,
  status: string,
  lateStatuses: readonly string[],
  now: Date = new Date(),
): boolean {
  if (!expectedDelivery || !lateStatuses.includes(status)) return false;
  return expectedDelivery < utcNowText(now);
}

/** Whole days an order is late (at least 1 once late), or null when it is not late. */
export function poLateDays(
  expectedDelivery: string | null | undefined,
  status: string,
  lateStatuses: readonly string[],
  now: Date = new Date(),
): number | null {
  if (!isPoLate(expectedDelivery, status, lateStatuses, now)) return null;
  const expected = Date.parse(expectedDelivery as string);
  if (Number.isNaN(expected)) return 1;
  return Math.max(1, Math.floor((now.getTime() - expected) / (24 * 60 * 60 * 1000)));
}
