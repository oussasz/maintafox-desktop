/**
 * Column visibility for ReferenceValueEditor — driven by domain structure + registry.
 */

import { isAbsenceTypesDomain } from "@/components/lookups/AbsenceBehaviourPicker";
import { isPermitRulesDomain } from "@/components/lookups/PermitRuleColumns";
import { findReferenceTypeByDomainCode } from "@/components/reference/reference-types";
import type { ReferenceDomain } from "@shared/ipc-types";

export interface ReferenceValueColumnFlags {
  showParent: boolean;
  showSchedule: boolean;
  /** EN 13306 nature picker (`semantic_tag`) of request types. */
  showNature: boolean;
  /** Absence-type behaviour picker (`semantic_tag`: leave / critical / training) of PERSONNEL.ABSENCE_TYPES. */
  showAbsenceBehaviour: boolean;
  /** Units of measure: symbol, EN label, dimension, base unit and factor (`MEASURE.UNITS`). */
  showUnit: boolean;
  /** Permit requirement rules: equipment class, work type and permit type (`PERMIT.REQUIREMENT_RULES`). */
  showPermitRule: boolean;
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
      showNature: false,
      showAbsenceBehaviour: false,
      showUnit: false,
      showPermitRule: false,
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
  const showNature = Boolean(registry?.valueExtensions?.includes("maintenance_nature"));
  const showUnit = Boolean(registry?.valueExtensions?.includes("measure_unit"));
  const showPermitRule = isPermitRulesDomain(domain.code);
  const showAbsenceBehaviour = isAbsenceTypesDomain(domain.code);

  return {
    showParent,
    showSchedule,
    showNature,
    showAbsenceBehaviour,
    showUnit,
    showPermitRule,
    crossDomainParentCode,
    sameSetHierarchy,
  };
}
