/**
 * Equipment status and criticality on screen, read from the published reference lists
 * (`useEquipmentTaxonomy`):
 * - a status's tone follows its availability tag in EQUIPMENT.STATUS (`up`, `down`,
 *   `not_installed`, `retired`), never its code;
 * - a criticality's tone follows its rank in EQUIPMENT.CRITICALITY (the list order: the first level
 *   is the most critical, the backend's "critical" rule);
 * - labels: both lists are Maintafox (SYSTEM) lists, so their values are translated by code
 *   (`equipment:status.*`, `equipment:criticality.*`); any other value shows its reference label.
 */

import type { Tone } from "@/design-system/tone";
import type { EquipmentTaxonomyCatalog, EquipmentTaxonomyOption } from "@shared/ipc-types";

/** Translation keys of the SYSTEM EQUIPMENT.STATUS values. */
export const ASSET_STATUS_LABEL_KEY: Record<string, string> = {
  ACTIVE_IN_SERVICE: "status.operational",
  UNDER_MAINTENANCE: "status.maintenance",
  IN_STOCK: "status.inStock",
  SPARE: "status.spare",
  OUT_OF_SERVICE: "status.outOfService",
  DECOMMISSIONED: "status.decommissioned",
  SCRAPPED: "status.scrapped",
};

/** Translation keys of the SYSTEM EQUIPMENT.CRITICALITY values. */
export const CRITICALITY_LABEL_KEY: Record<string, string> = {
  A: "criticality.critical",
  B: "criticality.important",
  C: "criticality.standard",
  D: "criticality.low",
};

/** Availability tag of EQUIPMENT.STATUS → tone. */
const STATUS_TAG_TONE: Record<string, Tone> = {
  up: "success",
  down: "danger",
  not_installed: "info",
  retired: "neutral",
};

/** Criticality rank (0 = most critical) → tone. */
const CRITICALITY_RANK_TONE: Tone[] = ["danger", "warning", "info"];

const sameCode = (a: string, b: string) => a.trim().toUpperCase() === b.trim().toUpperCase();

export function findStatus(
  catalog: EquipmentTaxonomyCatalog | null,
  code: string | null | undefined,
): EquipmentTaxonomyOption | null {
  if (!catalog || !code) return null;
  return catalog.statuses.find((s) => sameCode(s.code, code)) ?? null;
}

export function statusTone(catalog: EquipmentTaxonomyCatalog | null, code: string): Tone {
  const tag = findStatus(catalog, code)?.semantic_tag;
  return (tag && STATUS_TAG_TONE[tag]) || "neutral";
}

/** Retired (decommissioned, scrapped…): the status carries the `retired` tag. */
export function isRetiredStatus(
  catalog: EquipmentTaxonomyCatalog | null,
  code: string | null | undefined,
) {
  return findStatus(catalog, code)?.semantic_tag === "retired";
}

/** Translation key of a SYSTEM status, else null (show the reference label). */
export function statusLabelKey(code: string): string | null {
  return ASSET_STATUS_LABEL_KEY[code.trim().toUpperCase()] ?? null;
}

export function criticalityRank(
  catalog: EquipmentTaxonomyCatalog | null,
  code: string | null | undefined,
): number | null {
  if (!catalog || !code) return null;
  const i = catalog.criticalities.findIndex((c) => sameCode(c.code, code));
  return i < 0 ? null : i;
}

export function criticalityTone(catalog: EquipmentTaxonomyCatalog | null, code: string): Tone {
  const rank = criticalityRank(catalog, code);
  return rank == null ? "neutral" : (CRITICALITY_RANK_TONE[rank] ?? "neutral");
}

export function criticalityLabelKey(code: string): string | null {
  return CRITICALITY_LABEL_KEY[code.trim().toUpperCase()] ?? null;
}
