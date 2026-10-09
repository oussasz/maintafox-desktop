/**
 * Soft-phase UI fallback: when no signed entitlement envelope is stored yet,
 * derive module visibility from the commercial edition on the activation claim.
 * Backend soft-allow still applies until a verified envelope is present.
 *
 * Custom / unknown editions without an envelope map fall back to the Core factory
 * map (conservative) — never "allow all".
 */

export const SYSTEM_EDITION_IDS = ["core", "professional", "enterprise", "development"] as const;
/** @deprecated Prefer SYSTEM_EDITION_IDS */
export const EDITION_IDS = SYSTEM_EDITION_IDS;
export type EditionId = (typeof SYSTEM_EDITION_IDS)[number];

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
  "training",
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
const ENTERPRISE_EXTRA: ModuleKey[] = ["training", "reports", "rams", "finance", "sync", "erp"];

export function isSystemEditionId(value: unknown): value is EditionId {
  if (typeof value !== "string") return false;
  const v = value.trim().toLowerCase();
  return (SYSTEM_EDITION_IDS as readonly string[]).includes(v);
}

/** Returns a system edition id, or null for custom/unknown (caller should use Core fallback). */
export function parseEditionId(value: unknown): EditionId | null {
  if (typeof value !== "string") return null;
  const v = value.trim().toLowerCase();
  return (SYSTEM_EDITION_IDS as readonly string[]).includes(v) ? (v as EditionId) : null;
}

/** Full boolean map for every known module key (matches control-plane factory defaults). */
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

/**
 * Resolve fallback map when envelope capabilities are empty.
 * System editions → factory map. Custom/unknown → Core (fail-closed).
 */
export function capabilityMapForClaimEdition(edition: unknown): Record<string, boolean> {
  const system = parseEditionId(edition);
  return capabilityMapForEdition(system ?? "core");
}
