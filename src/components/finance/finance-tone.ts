/**
 * Finance status → tone maps (design-system/tone.ts vocabulary) and the codes of the cost
 * categories the cost flow posts to. Kept with the finance components; the shared tone file stays generic.
 */
import type { Tone } from "@/design-system/tone";

/**
 * Codes of the cost categories OT settlement posts to. They are protected product values of the
 * reference domain `FINANCE.COST_CATEGORIES` (labels and the full list come from the reference
 * data, see `use-finance-catalogs`).
 */
export const COST_BUCKETS = ["labor", "parts", "services", "other"] as const;
export type CostBucket = (typeof COST_BUCKETS)[number];

export const SETTLEMENT_STATUS_TONE: Record<string, Tone> = {
  posted: "success",
  pending: "attention",
  reversed: "neutral",
};

export const PERIOD_STATUS_TONE: Record<string, Tone> = {
  open: "info",
  closed: "neutral",
};

export const VERSION_STATUS_TONE: Record<string, Tone> = {
  draft: "neutral",
  submitted: "info",
  approved: "success",
  frozen: "progress",
  closed: "neutral",
  superseded: "neutral",
};

export const REVIEW_STATUS_TONE: Record<string, Tone> = {
  open: "attention",
  in_review: "progress",
  actioned: "info",
  accepted: "success",
  closed: "neutral",
};

/**
 * Tone of a forecast against its budget: overrun = danger, ≥ 90 % consumed = warning.
 * No budget but spend = attention (unbudgeted cost).
 */
export function forecastTone(budget: number, forecast: number): Tone | undefined {
  if (budget <= 0) return forecast > 0 ? "attention" : undefined;
  if (forecast > budget + 0.005) return "danger";
  if (forecast >= budget * 0.9) return "warning";
  return undefined;
}

/** Share of a value in a total, as a percentage (null without total). */
export function pct(value: number, total: number): number | null {
  return total > 0 ? (value / total) * 100 : null;
}
