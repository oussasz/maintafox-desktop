/**
 * Asset status and criticality → design-system tones and translation keys (`equipment` namespace).
 * Same vocabulary as `design-system/tone.ts`: one meaning, one colour in every module.
 */

import type { Tone } from "@/design-system/tone";

/** EQUIPMENT.STATUS codes (and legacy aliases) → tone. */
export const ASSET_STATUS_TONE: Record<string, Tone> = {
  ACTIVE_IN_SERVICE: "success",
  ACTIVE: "success",
  OPERATIONAL: "success",
  UNDER_MAINTENANCE: "progress",
  MAINTENANCE: "progress",
  STANDBY: "neutral",
  IN_STOCK: "info",
  SPARE: "info",
  OUT_OF_SERVICE: "danger",
  DECOMMISSIONED: "neutral",
  SCRAPPED: "neutral",
};

/** EQUIPMENT.STATUS code → `equipment:status.*` key. */
export const ASSET_STATUS_LABEL_KEY: Record<string, string> = {
  ACTIVE_IN_SERVICE: "status.operational",
  ACTIVE: "status.operational",
  OPERATIONAL: "status.operational",
  UNDER_MAINTENANCE: "status.maintenance",
  MAINTENANCE: "status.maintenance",
  STANDBY: "status.standby",
  IN_STOCK: "status.inStock",
  SPARE: "status.spare",
  OUT_OF_SERVICE: "status.outOfService",
  DECOMMISSIONED: "status.decommissioned",
  SCRAPPED: "status.scrapped",
};

export type CriticalityGrade = "A" | "B" | "C" | "D";

/** Criticality class → tone (A is the highest consequence). */
export const CRITICALITY_TONE: Record<CriticalityGrade, Tone> = {
  A: "danger",
  B: "warning",
  C: "info",
  D: "neutral",
};

/** Criticality class → `equipment:criticality.*` key. */
export const CRITICALITY_LABEL_KEY: Record<CriticalityGrade, string> = {
  A: "criticality.critical",
  B: "criticality.important",
  C: "criticality.standard",
  D: "criticality.low",
};

/** Legacy lookup codes still found on old rows → canonical A–D class. */
const LEGACY_CRITICALITY: Record<string, CriticalityGrade> = {
  CRITIQUE: "A",
  IMPORTANT: "B",
  STANDARD: "C",
  NON_CRITIQUE: "D",
};

export function toCriticalityGrade(code: string | null | undefined): CriticalityGrade | null {
  if (!code) return null;
  const upper = code.trim().toUpperCase();
  if (upper === "A" || upper === "B" || upper === "C" || upper === "D") return upper;
  return LEGACY_CRITICALITY[upper] ?? null;
}

export function normalizeAssetStatus(code: string): string {
  return code.trim().toUpperCase();
}
