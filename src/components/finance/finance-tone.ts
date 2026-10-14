/**
 * Finance status → tone maps (design-system/tone.ts vocabulary) and the budget categories the
 * cost flow posts to. Kept with the finance components; the shared tone file stays generic.
 */
import type { Tone } from "@/design-system/tone";

/** Cost categories posted by OT settlement (`finance::wo_costing::BUCKET_*`). */
export const COST_BUCKETS = ["labor", "parts", "services", "other"] as const;
export type CostBucket = (typeof COST_BUCKETS)[number];

/** Every bucket a budget line may use (cost flow + planning-only categories). */
export const BUDGET_BUCKETS = [
  "labor",
  "parts",
  "services",
  "contracts",
  "shutdown",
  "capex",
  "other",
] as const;

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

/** Variance drivers accepted by the backend (`valid_variance_driver`). */
export const VARIANCE_DRIVERS = [
  "emergency_break_in",
  "vendor_delay",
  "labor_overrun",
  "estimate_error",
  "scope_change",
  "price_increase",
  "permit_delay",
  "availability_loss",
  "repeat_failure",
  "shutdown_scope_growth",
] as const;

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
