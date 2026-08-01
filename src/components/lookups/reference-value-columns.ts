/**
 * Column visibility for ReferenceValueEditor — driven by domain structure + registry.
 */

import { findReferenceTypeByDomainCode } from "@/components/reference/reference-types";
import type { ReferenceDomain } from "@shared/ipc-types";

export interface ReferenceValueColumnFlags {
  showParent: boolean;
  showSchedule: boolean;
  /** Cross-domain parent (e.g. FAMILY → CLASS). Null when same-set hierarchy only. */
  crossDomainParentCode: string | null;
  /** Same-set parent select for hierarchical domains without cross-domain parent. */
  sameSetHierarchy: boolean;
}

export function resolveReferenceValueColumns(
  domain: ReferenceDomain | null | undefined,
): ReferenceValueColumnFlags {
  if (!domain) {
    return {
      showParent: false,
      showSchedule: false,
      crossDomainParentCode: null,
      sameSetHierarchy: false,
    };
  }

  const registry = findReferenceTypeByDomainCode(domain.code);
  const crossDomainParentCode = registry?.parentDomainCode ?? null;
  const sameSetHierarchy =
    domain.structure_type === "hierarchical" && crossDomainParentCode == null;
  const showParent = crossDomainParentCode != null || sameSetHierarchy;
  const showSchedule = Boolean(registry?.valueExtensions?.includes("schedule_pattern"));

  return {
    showParent,
    showSchedule,
    crossDomainParentCode,
    sameSetHierarchy,
  };
}
