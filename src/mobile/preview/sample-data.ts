// DESIGN PREVIEW ONLY: sample plant data shaped like the engine's answers, so the real screens
// can be shown without a phone, a licence or a plant. Never imported by the field app.

import type { ScanSample } from "../field/ScanEquipment";
import type { ExecutionSample } from "../field/WorkOrderExecution";

export const SAMPLE_TECHNICIAN = { id: 12, username: "k.benali", display_name: "Karim Benali" };

const NOW = new Date();
const at = (days: number, hour: number) => {
  const d = new Date(NOW);
  d.setDate(d.getDate() + days);
  d.setHours(hour, 0, 0, 0);
  return d.toISOString();
};

function workOrder(
  id: number,
  fields: {
    code: string;
    title: string;
    description: string | null;
    asset_code: string;
    asset_label: string;
    status_label: string;
    type_label: string;
    urgency_code: "emergency" | "urgent" | "high" | "normal" | "low";
    urgency_rank: number;
    urgency_label: string;
    urgency_color: string;
    planned_start: string;
    expected_duration_hours: number;
    requires_permit: boolean;
    requires_shutdown: boolean;
    permit_rule_permit_type: string | null;
  },
) {
  return {
    id,
    type_id: 1,
    status_id: 2,
    equipment_id: id * 10,
    component_id: null,
    location_id: null,
    requester_id: null,
    source_di_id: null,
    entity_id: 1,
    planner_id: 3,
    approver_id: null,
    assigned_team_id: null,
    primary_responsible_id: SAMPLE_TECHNICIAN.id,
    urgency_id: fields.urgency_rank,
    planned_end: null,
    shift: null,
    scheduled_at: null,
    actual_start: null,
    actual_end: null,
    mechanically_completed_at: null,
    technically_verified_at: null,
    closed_at: null,
    cancelled_at: null,
    planned_crew_size: 1,
    actual_duration_hours: null,
    active_labor_hours: null,
    total_waiting_hours: null,
    downtime_hours: null,
    labor_cost: null,
    parts_cost: null,
    service_cost: null,
    total_cost: null,
    recurrence_risk_level: null,
    production_impact_id: null,
    root_cause_summary: null,
    corrective_action_summary: null,
    verification_method: null,
    notes: null,
    cancel_reason: null,
    parts_actuals_confirmed: false,
    service_cost_input: null,
    reopen_count: 0,
    last_closed_at: null,
    permit_rule_code: null,
    permit_rule_cleared_by_id: null,
    permit_rule_cleared_at: null,
    permit_rule_clear_reason: null,
    row_version: 1,
    created_at: at(-2, 8),
    updated_at: at(-1, 8),
    status_code: "planned",
    ...fields,
  };
}

export const SAMPLE_WORK_ORDERS = [
  workOrder(1, {
    code: "OT-2026-0412",
    title: "Fuite au presse-étoupe de la pompe de reprise",
    description:
      "Fuite goutte à goutte constatée au presse-étoupe côté accouplement. Resserrer ou remplacer la tresse, contrôler l'alignement.",
    asset_code: "PMP-101",
    asset_label: "Pompe de reprise eau brute",
    status_label: "Planifié",
    type_label: "Correctif",
    urgency_code: "urgent",
    urgency_rank: 2,
    urgency_label: "Urgent",
    urgency_color: "#d92d20",
    planned_start: at(0, 9),
    expected_duration_hours: 2.5,
    requires_permit: true,
    requires_shutdown: true,
    permit_rule_permit_type: "Travail mécanique",
  }),
  workOrder(2, {
    code: "OT-2026-0407",
    title: "Contrôle des courroies et graissage des paliers",
    description: null,
    asset_code: "CNV-07",
    asset_label: "Convoyeur ligne 3",
    status_label: "Planifié",
    type_label: "Préventif",
    urgency_code: "normal",
    urgency_rank: 4,
    urgency_label: "Normal",
    urgency_color: "#1570ef",
    planned_start: at(0, 14),
    expected_duration_hours: 1,
    requires_permit: false,
    requires_shutdown: false,
    permit_rule_permit_type: null,
  }),
  workOrder(3, {
    code: "OT-2026-0398",
    title: "Remplacement du filtre d'aspiration",
    description: null,
    asset_code: "CMP-02",
    asset_label: "Compresseur d'air 2",
    status_label: "En attente pièces",
    type_label: "Préventif",
    urgency_code: "high",
    urgency_rank: 3,
    urgency_label: "Élevée",
    urgency_color: "#f79009",
    planned_start: at(1, 8),
    expected_duration_hours: 0.5,
    requires_permit: false,
    requires_shutdown: true,
    permit_rule_permit_type: null,
  }),
];

/** Reference domains and values (the plant's Données de référence). */
export const SAMPLE_REFERENCE: Record<string, Array<[string, string]>> = {
  "DI.ORIGIN": [
    ["operator", "Opérateur de production"],
    ["inspection", "Ronde d'inspection"],
    ["maintenance", "Technicien de maintenance"],
  ],
  "DI.REQUEST_TYPE": [
    ["corrective", "Dépannage"],
    ["improvement", "Amélioration"],
    ["safety", "Mise en sécurité"],
  ],
  "WORK.SYMPTOMS": [
    ["LEAK", "Fuite"],
    ["NOISE", "Bruit anormal"],
    ["VIB", "Vibrations"],
    ["HEAT", "Échauffement"],
    ["STOP", "Arrêt intempestif"],
  ],
  "DI.IMPACT_LEVEL": [
    ["none", "Aucun"],
    ["minor", "Mineur"],
    ["major", "Majeur"],
    ["critical", "Critique"],
  ],
};

export const SAMPLE_URGENCY = [
  ["emergency", 1, "Immédiate", "Immediate", "#b42318"],
  ["urgent", 2, "Urgent", "Urgent", "#d92d20"],
  ["high", 3, "Élevée", "High", "#f79009"],
  ["normal", 4, "Normal", "Normal", "#1570ef"],
  ["low", 5, "Faible", "Low", "#667085"],
].map(([code, rank, fr, en, color]) => ({
  id: rank as number,
  code,
  rank,
  label_fr: fr,
  label_en: en,
  description: null,
  color_hex: color,
  response_target_hours: null,
  completion_target_hours: null,
  escalation_threshold_hours: null,
  targets_source: "company",
}));

export const SAMPLE_ASSETS = [
  ["PMP-101", "Pompe de reprise eau brute", "Station de pompage › Bâche 1"],
  ["PMP-102", "Pompe de reprise eau brute (secours)", "Station de pompage › Bâche 1"],
  ["CMP-02", "Compresseur d'air 2", "Utilités › Salle compresseurs"],
  ["CNV-07", "Convoyeur ligne 3", "Atelier conditionnement › Ligne 3"],
].map(([code, name, path], i) => ({
  id: 100 + i,
  entity_sync_id: `ast-${i}`,
  asset_code: code,
  asset_name: name,
  class_code: null,
  class_name: null,
  family_code: null,
  family_name: null,
  subfamily_code: null,
  subfamily_name: null,
  criticality_code: null,
  criticality_to_review: false,
  status_code: "ACTIVE",
  org_node_id: 5,
  org_node_name: path?.split(" › ").pop() ?? null,
  org_path: path,
  parent_asset_id: null,
  parent_asset_code: null,
  parent_asset_name: null,
  primary_meter_name: null,
  primary_meter_reading: null,
  primary_meter_unit: null,
  primary_meter_last_read_at: null,
  external_id_count: 0,
  row_version: 1,
}));

export const SAMPLE_SYNC = {
  state: "idle",
  blocker: null,
  paused_until: null,
  last_success_at: new Date(NOW.getTime() - 4 * 60_000).toISOString(),
  last_attempt_at: new Date(NOW.getTime() - 4 * 60_000).toISOString(),
  next_attempt_at: null,
  pending_outbox_count: 0,
  dead_letter_outbox_count: 0,
  unresolved_conflict_count: 0,
  media_pending_count: 0,
  media_failed_count: 0,
  media_download_pending_count: 0,
  media_download_failed_count: 0,
  consecutive_failures: 0,
  last_error: null,
};

export const SAMPLE_EXECUTION: ExecutionSample = {
  code: "OT-2026-0412",
  title: "Fuite au presse-étoupe de la pompe de reprise",
  equipment: "PMP-101 — Pompe de reprise eau brute",
  location: "Station de pompage › Bâche 1",
  status: "Planifié",
  urgency: "Urgent",
  type: "Correctif",
  permit: "Permis de travail mécanique n° PT-0193 (validé)",
  isolation: "Consignation électrique du moteur M-101 — cadenas LOTO-12",
  ppe: "Gants, lunettes, chaussures de sécurité",
  elapsed: "00:42:17",
  tasks: [
    { label: "Vérifier la consignation (essai de démarrage)", result: "ok" },
    { label: "Démonter le fouloir et retirer la tresse usée", result: "ok" },
    { label: "Poser la tresse neuve et resserrer", result: null },
    { label: "Remise en service, contrôle d'absence de fuite", result: null },
  ],
  parts: [
    { code: "TRS-12x12", name: "Tresse graphitée 12 mm", qty: 3, unit: "m" },
    { code: "GRS-EP2", name: "Graisse EP2", qty: 1, unit: "cartouche" },
  ],
  failureModes: [
    { value: "LEAK", label: "Fuite externe" },
    { value: "WEAR", label: "Usure" },
    { value: "MISAL", label: "Désalignement" },
  ],
  failureCauses: [
    { value: "AGE", label: "Usure normale" },
    { value: "INST", label: "Montage incorrect" },
    { value: "LUB", label: "Défaut de lubrification" },
  ],
  failureActions: [
    { value: "REPL", label: "Remplacement" },
    { value: "ADJ", label: "Réglage / resserrage" },
    { value: "CLEAN", label: "Nettoyage" },
  ],
  failure: { mode: "LEAK", cause: "AGE", action: "REPL" },
  photos: 2,
};

export const SAMPLE_SCAN: ScanSample = {
  code: "PMP-101",
  name: "Pompe de reprise eau brute",
  location: "Station de pompage › Bâche 1",
  criticality: "Criticité A",
  status: "En service",
  openWos: [
    { code: "OT-2026-0412", title: "Fuite au presse-étoupe", status: "Planifié" },
    { code: "OT-2026-0371", title: "Analyse vibratoire trimestrielle", status: "À planifier" },
  ],
  history: [
    { date: "12/10/2026", text: "Remplacement garniture mécanique" },
    { date: "03/09/2026", text: "Graissage paliers (préventif)" },
    { date: "28/07/2026", text: "Arrêt intempestif — relais thermique" },
  ],
};
