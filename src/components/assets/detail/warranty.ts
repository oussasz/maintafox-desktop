/**
 * Warranty state of an asset from its expiry date (`YYYY-MM-DD`), compared in local calendar days.
 */

import type { Tone } from "@/design-system/tone";

/** Days before the end of warranty when the asset is flagged "ends soon" (claims need lead time). */
export const WARRANTY_EXPIRING_SOON_DAYS = 90;

export type WarrantyState = "none" | "active" | "expiring" | "expired";

export const WARRANTY_TONE: Record<WarrantyState, Tone> = {
  none: "neutral",
  active: "success",
  expiring: "warning",
  expired: "neutral",
};

function dayNumber(date: Date): number {
  return Math.floor(Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) / 86_400_000);
}

/** Days from `today` to the expiry date (negative once expired); null without a valid date. */
export function warrantyDaysLeft(expiry: string | null | undefined, today: Date): number | null {
  if (!expiry) return null;
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(expiry.trim());
  if (!match) return null;
  const end = Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])) / 86_400_000;
  return end - dayNumber(today);
}

export function warrantyState(expiry: string | null | undefined, today: Date): WarrantyState {
  const left = warrantyDaysLeft(expiry, today);
  if (left === null) return "none";
  if (left < 0) return "expired";
  if (left <= WARRANTY_EXPIRING_SOON_DAYS) return "expiring";
  return "active";
}
