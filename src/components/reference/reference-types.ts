/**
 * Registry of reference-backed form field types.
 *
 * Governance: docs/engineering/REFERENCE_GOVERNANCE.md (A/B/C).
 * Create eligibility comes from get_reference_governance_capabilities_by_code
 * (`can_operational_create`) — never from registry flags.
 *
 * To add a new reference-backed field:
 * 1. Assign domain category A, B, or C and ensure the domain exists.
 * 2. Register a ReferenceTypeConfig here.
 * 3. Drop <ReferenceCombobox referenceType="…" /> into the form.
 */

export type ReferenceTypeId =
  | "equipment.family"
  | "equipment.subfamily"
  | "equipment.class"
  | "equipment.criticality"
  | "equipment.status"
  | "di.origin"
  | "di.symptom"
  | "di.priority"
  | "di.impact"
  | "di.request_type"
  | "personnel.skills"
  | "org.schedule_class"
  | "work.symptom"
  | "work.failure_mode"
  | "work.failure_cause"
  | "work.failure_effect"
  | "work.delay_reason"
  | "work.part_unused_reason"
  | "inventory.supplier_status"
  | "inventory.replenishment_policy"
  | "inventory.purchase_priority"
  | "inventory.demand_source_type"
  | "inventory.abc_class"
  | "inventory.xyz_class"
  | "inventory.equivalence_type"
  | "inventory.document_link_purpose";

export interface ReferenceTypeConfig {
  /** Stable form-facing type id. */
  id: ReferenceTypeId;
  /** Canonical reference_domains.code */
  domainCode: string;
  /** French entity noun for "Ajouter un(e) {entity}…" */
  entityLabelFr: string;
  /** English entity noun (fallback / EN locale). */
  entityLabelEn: string;
  /** Gender for French article: un / une */
  gender: "m" | "f";
  /**
   * Optional parent domain. When set, options are filtered by parentValueId
   * and create requires that parent.
   */
  parentDomainCode?: string;
  /**
   * Optional Manager value extensions for this domain (e.g. weekly schedule pattern).
   * Drives ReferenceValueEditor columns — not combobox create flags.
   */
  valueExtensions?: ReadonlyArray<"schedule_pattern">;
  /** Empty-state copy key suffix under reference.combobox.types.{id} */
  emptyMessageFr: string;
  emptyMessageEn: string;
}

export const REFERENCE_TYPE_REGISTRY: Record<ReferenceTypeId, ReferenceTypeConfig> = {
  "equipment.family": {
    id: "equipment.family",
    domainCode: "EQUIPMENT.FAMILY",
    entityLabelFr: "famille",
    entityLabelEn: "family",
    gender: "f",
    parentDomainCode: "EQUIPMENT.CLASS",
    emptyMessageFr: "Aucune famille disponible",
    emptyMessageEn: "No family available",
  },
  "equipment.subfamily": {
    id: "equipment.subfamily",
    domainCode: "EQUIPMENT.SUBFAMILY",
    entityLabelFr: "sous-famille",
    entityLabelEn: "sub-family",
    gender: "f",
    parentDomainCode: "EQUIPMENT.FAMILY",
    emptyMessageFr: "Aucune sous-famille disponible",
    emptyMessageEn: "No sub-family available",
  },
  "equipment.class": {
    id: "equipment.class",
    domainCode: "EQUIPMENT.CLASS",
    entityLabelFr: "classe",
    entityLabelEn: "class",
    gender: "f",
    emptyMessageFr: "Aucune classe disponible",
    emptyMessageEn: "No class available",
  },
  "equipment.criticality": {
    id: "equipment.criticality",
    domainCode: "EQUIPMENT.CRITICALITY",
    entityLabelFr: "criticité",
    entityLabelEn: "criticality",
    gender: "f",
    emptyMessageFr: "Aucune criticité disponible",
    emptyMessageEn: "No criticality available",
  },
  "equipment.status": {
    id: "equipment.status",
    domainCode: "EQUIPMENT.STATUS",
    entityLabelFr: "état du cycle de vie",
    entityLabelEn: "lifecycle status",
    gender: "m",
    emptyMessageFr: "Aucun état disponible",
    emptyMessageEn: "No status available",
  },
  "di.origin": {
    id: "di.origin",
    domainCode: "DI.ORIGIN",
    entityLabelFr: "origine",
    entityLabelEn: "origin",
    gender: "f",
    emptyMessageFr: "Aucune origine disponible",
    emptyMessageEn: "No origin available",
  },
  "di.symptom": {
    id: "di.symptom",
    domainCode: "DI.SYMPTOM",
    entityLabelFr: "symptôme",
    entityLabelEn: "symptom",
    gender: "m",
    emptyMessageFr: "Aucun symptôme disponible",
    emptyMessageEn: "No symptom available",
  },
  "di.priority": {
    id: "di.priority",
    domainCode: "DI.PRIORITY",
    entityLabelFr: "priorité",
    entityLabelEn: "priority",
    gender: "f",
    emptyMessageFr: "Aucune priorité disponible",
    emptyMessageEn: "No priority available",
  },
  "di.impact": {
    id: "di.impact",
    domainCode: "DI.IMPACT_LEVEL",
    entityLabelFr: "niveau d'impact",
    entityLabelEn: "impact level",
    gender: "m",
    emptyMessageFr: "Aucun niveau d'impact disponible",
    emptyMessageEn: "No impact level available",
  },
  "di.request_type": {
    id: "di.request_type",
    domainCode: "DI.REQUEST_TYPE",
    entityLabelFr: "type de demande",
    entityLabelEn: "request type",
    gender: "m",
    emptyMessageFr: "Aucun type de demande disponible",
    emptyMessageEn: "No request type available",
  },
  "personnel.skills": {
    id: "personnel.skills",
    domainCode: "PERSONNEL.SKILLS",
    entityLabelFr: "compétence",
    entityLabelEn: "skill",
    gender: "f",
    emptyMessageFr: "Aucune compétence disponible",
    emptyMessageEn: "No skill available",
  },
  "org.schedule_class": {
    id: "org.schedule_class",
    domainCode: "ORG.SCHEDULE_CLASS",
    entityLabelFr: "classe horaire",
    entityLabelEn: "schedule class",
    gender: "f",
    valueExtensions: ["schedule_pattern"],
    emptyMessageFr: "Aucune classe horaire disponible",
    emptyMessageEn: "No schedule class available",
  },
  "work.symptom": {
    id: "work.symptom",
    domainCode: "WORK.SYMPTOMS",
    entityLabelFr: "symptôme",
    entityLabelEn: "symptom",
    gender: "m",
    emptyMessageFr: "Aucun symptôme disponible",
    emptyMessageEn: "No symptom available",
  },
  "work.failure_mode": {
    id: "work.failure_mode",
    domainCode: "WORK.FAILURE_MODES",
    entityLabelFr: "mode de défaillance",
    entityLabelEn: "failure mode",
    gender: "m",
    emptyMessageFr: "Aucun mode de défaillance disponible",
    emptyMessageEn: "No failure mode available",
  },
  "work.failure_cause": {
    id: "work.failure_cause",
    domainCode: "WORK.FAILURE_CAUSES",
    entityLabelFr: "cause de défaillance",
    entityLabelEn: "failure cause",
    gender: "f",
    emptyMessageFr: "Aucune cause disponible",
    emptyMessageEn: "No failure cause available",
  },
  "work.failure_effect": {
    id: "work.failure_effect",
    domainCode: "WORK.FAILURE_EFFECTS",
    entityLabelFr: "effet de défaillance",
    entityLabelEn: "failure effect",
    gender: "m",
    emptyMessageFr: "Aucun effet disponible",
    emptyMessageEn: "No failure effect available",
  },
  "work.delay_reason": {
    id: "work.delay_reason",
    domainCode: "WORK.DELAY_REASONS",
    entityLabelFr: "motif de délai",
    entityLabelEn: "delay reason",
    gender: "m",
    emptyMessageFr: "Aucun motif de délai disponible",
    emptyMessageEn: "No delay reason available",
  },
  "work.part_unused_reason": {
    id: "work.part_unused_reason",
    domainCode: "WORK.PART_UNUSED_REASON",
    entityLabelFr: "motif de non-utilisation",
    entityLabelEn: "unused part reason",
    gender: "m",
    emptyMessageFr: "Aucun motif disponible",
    emptyMessageEn: "No unused reason available",
  },
  "inventory.supplier_status": {
    id: "inventory.supplier_status",
    domainCode: "INVENTORY.SUPPLIER_STATUS",
    entityLabelFr: "statut fournisseur",
    entityLabelEn: "supplier status",
    gender: "m",
    emptyMessageFr: "Aucun statut fournisseur disponible",
    emptyMessageEn: "No supplier status available",
  },
  "inventory.replenishment_policy": {
    id: "inventory.replenishment_policy",
    domainCode: "INVENTORY.REPLENISHMENT_POLICY",
    entityLabelFr: "politique de réapprovisionnement",
    entityLabelEn: "replenishment policy",
    gender: "f",
    emptyMessageFr: "Aucune politique de réapprovisionnement disponible",
    emptyMessageEn: "No replenishment policy available",
  },
  "inventory.purchase_priority": {
    id: "inventory.purchase_priority",
    domainCode: "INVENTORY.PURCHASE_PRIORITY",
    entityLabelFr: "priorité d'achat",
    entityLabelEn: "purchase priority",
    gender: "f",
    emptyMessageFr: "Aucune priorité d'achat disponible",
    emptyMessageEn: "No purchase priority available",
  },
  "inventory.demand_source_type": {
    id: "inventory.demand_source_type",
    domainCode: "INVENTORY.DEMAND_SOURCE_TYPE",
    entityLabelFr: "type de source de demande",
    entityLabelEn: "demand source type",
    gender: "m",
    emptyMessageFr: "Aucun type de source disponible",
    emptyMessageEn: "No demand source type available",
  },
  "inventory.abc_class": {
    id: "inventory.abc_class",
    domainCode: "INVENTORY.ABC_CLASS",
    entityLabelFr: "classe ABC",
    entityLabelEn: "ABC class",
    gender: "f",
    emptyMessageFr: "Aucune classe ABC disponible",
    emptyMessageEn: "No ABC class available",
  },
  "inventory.xyz_class": {
    id: "inventory.xyz_class",
    domainCode: "INVENTORY.XYZ_CLASS",
    entityLabelFr: "classe XYZ",
    entityLabelEn: "XYZ class",
    gender: "f",
    emptyMessageFr: "Aucune classe XYZ disponible",
    emptyMessageEn: "No XYZ class available",
  },
  "inventory.equivalence_type": {
    id: "inventory.equivalence_type",
    domainCode: "INVENTORY.EQUIVALENCE_TYPE",
    entityLabelFr: "type d'équivalence",
    entityLabelEn: "equivalence type",
    gender: "m",
    emptyMessageFr: "Aucun type d'équivalence disponible",
    emptyMessageEn: "No equivalence type available",
  },
  "inventory.document_link_purpose": {
    id: "inventory.document_link_purpose",
    domainCode: "INVENTORY.DOCUMENT_LINK_PURPOSE",
    entityLabelFr: "objet du lien documentaire",
    entityLabelEn: "document link purpose",
    gender: "m",
    emptyMessageFr: "Aucun objet disponible",
    emptyMessageEn: "No document link purpose available",
  },
};

/** Look up a registry entry by canonical domain code (case-insensitive). */
export function findReferenceTypeByDomainCode(
  domainCode: string,
): ReferenceTypeConfig | undefined {
  const needle = domainCode.trim().toUpperCase();
  return Object.values(REFERENCE_TYPE_REGISTRY).find(
    (cfg) => cfg.domainCode.toUpperCase() === needle,
  );
}

export function getReferenceTypeConfig(referenceType: ReferenceTypeId): ReferenceTypeConfig {
  const cfg = REFERENCE_TYPE_REGISTRY[referenceType];
  if (!cfg) {
    throw new Error(`Unknown referenceType: ${referenceType}`);
  }
  return cfg;
}

export function addOptionLabel(cfg: ReferenceTypeConfig, locale: string): string {
  const isFr = locale.toLowerCase().startsWith("fr");
  if (isFr) {
    const article = cfg.gender === "f" ? "une" : "un";
    return `Ajouter ${article} ${cfg.entityLabelFr}…`;
  }
  return `Add ${cfg.entityLabelEn}…`;
}

export function createButtonLabel(cfg: ReferenceTypeConfig, locale: string): string {
  const isFr = locale.toLowerCase().startsWith("fr");
  if (isFr) {
    const article = cfg.gender === "f" ? "une" : "un";
    return `+ Créer ${article} ${cfg.entityLabelFr}`;
  }
  return `+ Create ${cfg.entityLabelEn}`;
}

export function emptyStateLabel(cfg: ReferenceTypeConfig, locale: string): string {
  return locale.toLowerCase().startsWith("fr") ? cfg.emptyMessageFr : cfg.emptyMessageEn;
}

export function modalTitle(cfg: ReferenceTypeConfig, locale: string): string {
  const isFr = locale.toLowerCase().startsWith("fr");
  if (isFr) {
    const article = cfg.gender === "f" ? "une" : "un";
    return `Ajouter ${article} ${cfg.entityLabelFr}`;
  }
  return `Add ${cfg.entityLabelEn}`;
}
