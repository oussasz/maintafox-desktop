/**
 * Client-side adjustments for reference domains (sidebar + editor).
 * DB seeds EQUIPMENT.CRITICALITY / STATUS as `system_seeded`; we surface them as
 * protected analytical so they match WO reference UX (lock icon, banner).
 */

import type { ReferenceDomain } from "@shared/ipc-types";

export const REFERENCE_UI_PROTECTED_CODES = new Set<string>([
  "EQUIPMENT.CRITICALITY",
  "EQUIPMENT.STATUS",
]);

export function normalizeReferenceDomainForUi(domain: ReferenceDomain): ReferenceDomain {
  const code = domain.code.trim().toUpperCase();
  if (!REFERENCE_UI_PROTECTED_CODES.has(code)) {
    return domain;
  }
  return {
    ...domain,
    governance_level: "protected_analytical",
  };
}

export function isReferenceDomainProtected(domain: ReferenceDomain): boolean {
  if (domain.governance_level === "protected_analytical") {
    return true;
  }
  return REFERENCE_UI_PROTECTED_CODES.has(domain.code.trim().toUpperCase());
}
