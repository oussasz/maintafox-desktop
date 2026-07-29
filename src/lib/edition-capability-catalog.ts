/**
 * Soft-phase UI fallback: when no signed entitlement envelope is stored yet,
 * derive module visibility from the commercial edition on the activation claim.
 * Backend soft-allow still applies until a verified envelope is present.
 */

export const EDITION_IDS = ["core", "professional", "enterprise", "development"] as const;
export type EditionId = (typeof EDITION_IDS)[number];

const MODULE_KEYS = [
  "equipment",
  "requests",
  "work_orders",
  "inventory",
  "personnel",
  "organization",
  "reference",
  "planning",
  "pm",
  "permits",
  "inspections",
  "reports",
  "rams",
  "finance",
  "sync",
  "erp",
] as const;

type ModuleKey = (typeof MODULE_KEYS)[number];

const CORE: ModuleKey[] = [
  "equipment",
  "requests",
  "work_orders",
  "inventory",
  "personnel",
  "organization",
  "reference",
];

const PROFESSIONAL_EXTRA: ModuleKey[] = ["planning", "pm", "permits", "inspections"];
const ENTERPRISE_EXTRA: ModuleKey[] = ["reports", "rams", "finance", "sync", "erp"];

export function parseEditionId(value: unknown): EditionId | null {
  if (typeof value !== "string") return null;
  const v = value.trim().toLowerCase();
  return (EDITION_IDS as readonly string[]).includes(v) ? (v as EditionId) : null;
}

/** Full boolean map for every known module key (matches control-plane edition-catalog). */
export function capabilityMapForEdition(edition: EditionId): Record<string, boolean> {
  const enabled = new Set<string>(CORE);
  if (edition === "professional" || edition === "enterprise" || edition === "development") {
    for (const m of PROFESSIONAL_EXTRA) enabled.add(m);
  }
  if (edition === "enterprise" || edition === "development") {
    for (const m of ENTERPRISE_EXTRA) enabled.add(m);
  }
  const out: Record<string, boolean> = {};
  for (const key of MODULE_KEYS) {
    out[key] = enabled.has(key);
  }
  return out;
}
