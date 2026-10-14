/**
 * Asset status and criticality → translation keys (`equipment` namespace) and code helpers.
 * Their tones live in `design-system/tone.ts` (`ASSET_STATUS_TONE`, `CRITICALITY_TONE`).
 */

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
