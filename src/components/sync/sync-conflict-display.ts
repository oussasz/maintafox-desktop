/**
 * How a sync conflict is named on screen: a translated record type and the record's own number
 * and name taken from its payload — never the uuid or the table name.
 */
import type { SyncConflictRecord } from "@shared/ipc-types";

/** Record types with a translated name in `shell:sync.entity.*`; others read "other record". */
export const KNOWN_SYNC_ENTITY_TYPES = new Set([
  "equipment",
  "intervention_requests",
  "work_orders",
  "work_order_tasks",
  "work_order_parts",
  "work_order_attachments",
  "di_attachments",
  "pm_plans",
  "pm_plan_versions",
  "pm_occurrences",
  "pm_executions",
  "inspection_rounds",
  "inspection_templates",
  "inspection_results",
  "articles",
  "inventory_transactions",
  "stock_reservations",
  "purchase_orders",
  "procurement_requisitions",
  "goods_receipts",
  "inventory_suppliers",
  "personnel",
  "personnel_availability_blocks",
  "teams",
  "positions",
  "user_accounts",
  "roles",
  "org_nodes",
  "org_node_types",
  "org_structure_models",
  "permit_types",
  "permit_isolations",
  "library_documents",
  "reference_values",
  "lookup_values",
  "cost_centers",
  "budget_versions",
  "budget_lines",
  "training_sessions",
  "failure_events",
  "app_settings",
]);

/** Conflict kinds with a dedicated explanation in `shell:sync.conflictKind`. */
export const KNOWN_CONFLICT_KINDS = new Set([
  "CONCURRENT_EDIT",
  "SERVER_CONFLICT",
  "STALE_INBOUND",
  "EQUAL_VERSION_DIVERGENCE",
  "MATERIALIZE_ERROR",
  "INVALID_INBOUND_PAYLOAD",
  "OUTBOX_DEAD_LETTER",
  "AUTHORITY_MISMATCH",
  "CODE_CLASH",
  // Rules re-checked on a received row (`sync::apply_hooks`): safety, four-eyes, site rules.
  "APPEND_ONLY_DELETE",
  "INVALID_VALUE",
  "SETTING_LOCKED",
  "WEEK_LIFECYCLE_BACKWARDS",
  "DECISION_SELF_APPROVAL",
  // A work-order part line received as cancelled although it has recorded use.
  "WO_PART_LINE_HAS_USE",
  "SITE_SETTING_UNAUDITED",
  "SITE_SETTING_INVALID",
  "SITE_SETTING_EVENT_INVALID",
  "SITE_SETTING_NOT_A_SITE",
  "SITE_SETTING_STALE",
  "SITE_SETTING_UNKNOWN",
  // A site rule received on a site that does not replace the deleted site holding it here.
  "SITE_SETTING_MOVED",
  "SYNC_STATE_TIME_MISSING",
  "PERMIT_STATUS_BACKWARDS",
  "PERMIT_EXPIRY_MISSING",
  "PERMIT_EXPIRED",
  "PERMIT_EXPIRES_BEFORE_PLANNED_END",
  "PERMIT_EXPIRES_BEFORE_ESTIMATED_END",
  "WO_PLANNED_END_UNREADABLE",
  "WO_PLANNED_END_PASSED",
  "WO_EXPECTED_DURATION_UNREADABLE",
  "WO_REMAINING_TIME_INVALID",
  "LOTO_NO_ISOLATION",
  "LOTO_NOT_VERIFIED",
  "LOTO_ISOLATION_REMOVED",
  "PERMIT_REMOVAL_PENDING",
  "PERMIT_HANDOVER_REQUIRED",
  "PERMIT_SUSPENSION_MISSING",
  "PERMIT_ISOLATION_NOT_APPLIED",
  "PERMIT_TWO_PERSON_VERIFY",
  "PERMIT_REMOVAL_NOT_VERIFIABLE",
  "PERMIT_TWO_PERSON_REMOVAL",
  "PERMIT_REQUIRED_TO_START",
  "PERMIT_SUSPENDED",
  "PERMIT_NOT_ACTIVE",
  "QUALIFICATION_OVERRIDE_REASON_SHORT",
  "QUALIFICATION_OVERRIDE_INVALID",
  "QUALIFICATION_OVERRIDE_SELF",
  "QUALIFICATION_OVERRIDE_UNAUTHORISED",
  "PERMIT_ISOLATION_MOVED",
  "PERMIT_ISOLATION_WRONG_STATUS",
  // A planned isolation line cancelled on another PC (CMMS Advisor decision).
  "PERMIT_ISOLATION_CANCELLED",
  "PERMIT_ISOLATION_CANCEL_APPLIED",
  "PERMIT_ISOLATION_CANCEL_WRONG_STATUS",
  "PERMIT_ISOLATION_CANCEL_INVALID",
  "PERMIT_DELETE_REFUSED",
  "QUALIFICATION_OVERRIDE_USE_PERSON",
  "SYNC_GROUP_REFUSED",
  "SYNC_TIME_INCONSISTENT",
  "QUALIFICATION_REQUIREMENT_DELETE_REFUSED",
  "WO_PERMIT_RULE_CLEAR_INVALID",
  "WO_PERMIT_RULE_CLEAR_UNAUTHORISED",
  "PERMIT_TYPE_REQUIRED_TO_START",
  "WO_PERMIT_RULE_CHANGED",
  "PERMIT_THREE_PERSON_REMOVAL",
  // A lock taken off, forced removal and the worker told (migration 249).
  "PERMIT_LOCK_REMOVAL_INVALID",
  "PERMIT_LOCK_REMOVAL_NOT_APPLIER",
  "PERMIT_REMOVAL_PRE_UPGRADE",
  "PERMIT_REMOVAL_LOCK_NOT_REMOVED",
  "PERMIT_REMOVAL_VERIFIER_IS_REMOVER",
  "PERMIT_FORCED_REMOVAL_INVALID",
  "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_APPLIER",
  "PERMIT_FORCED_REMOVAL_AUTHORISER_IS_REMOVER",
  "PERMIT_FORCED_REMOVAL_REMOVER_IS_APPLIER",
  "PERMIT_FORCED_REMOVAL_UNAUTHORISED",
  "PERMIT_WORKER_INFORMED_INVALID",
  "PERMIT_WORKER_ACK_NOT_YOURS",
  "PERMIT_WORKER_INFORMED_UNAUTHORISED",
  "PERMIT_WORKER_NOT_INFORMED",
  "PERMIT_LOCK_REMOVAL_RECORD_CHANGED",
  "PERMIT_HANDOVER_RECEIVER_IS_SIGNER",
  "PERMIT_TYPE_THREE_PERSON_UNAUTHORISED",
  "PERMIT_TYPE_THREE_PERSON_UNAUDITED",
  "PERMIT_TYPE_CHANGED",
  "PERMIT_TYPE_THREE_PERSON_STALE",
  // A permit's equipment, work order or site changed on another PC (CMMS Advisor decision A).
  "PERMIT_EQUIPMENT_LOCKED",
  "PERMIT_SITE_LOCKED",
  "PERMIT_EQUIPMENT_LOCK_FITTED",
  "PERMIT_EQUIPMENT_ISOLATIONS_PLANNED",
  "PERMIT_SITE_NOT_EQUIPMENT_SITE",
  "PERMIT_WO_EQUIPMENT_MISMATCH",
  "PERMIT_WO_UNLINKED",
  "PERMIT_TYPE_ISOLATION_UNAUDITED",
  "PERMIT_TYPE_ISOLATION_UNAUTHORISED",
  "PERMIT_TYPE_ISOLATION_STALE",
  "PERMIT_TYPE_LOTO_ISOLATION_LOCKED",
  "PERMIT_TYPE_CODE_LOCKED",
  // Four-eyes on permit approval (owner decision 2026-10-17).
  "PERMIT_SELF_APPROVAL",
  "PERMIT_APPROVER_MISSING",
  "PERMIT_APPROVAL_RECORD_CHANGED",
  "PERMIT_APPROVAL_RECORD_INVALID",
  // The notice of a safety rule changed on another PC cannot be muted or re-routed by sync.
  "NOTIFICATION_CATEGORY_LOCKED",
  "LAST_ADMINISTRATOR_REQUIRED",
  // Vendor recovery-code notices: never changed, acknowledged by their recipient only.
  "ADMIN_RECOVERY_NOTICE_CHANGED",
  "ADMIN_RECOVERY_NOTICE_INVALID",
  "ADMIN_RECOVERY_ACK_NOT_RECIPIENT",
  "ADMIN_RECOVERY_ACK_CHANGED",
  // A site given to an old record without one (CMMS Advisor decision Q1, `org::site_assignment`).
  "SITE_ASSIGNMENT_TRACE_CHANGED",
  "SITE_ASSIGNMENT_INCOMPLETE",
  "SITE_ASSIGNMENT_SITE_NOT_EMPTY",
  "SITE_ASSIGNMENT_NODE_INVALID",
  "SITE_ASSIGNMENT_NOT_ALLOWED",
  "SITE_SET_WITHOUT_ASSIGNMENT",
]);

const CODE_FIELDS = [
  "code",
  "asset_code",
  "wo_code",
  "request_code",
  "di_code",
  "employee_code",
  "po_number",
  "requisition_number",
  "article_code",
  "permit_code",
  "plan_code",
];

const NAME_FIELDS = ["name", "title", "asset_name", "label", "full_name", "designation"];

function parsePayload(json: string | null): Record<string, unknown> | null {
  if (!json) return null;
  try {
    const parsed: unknown = JSON.parse(json);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function firstText(payload: Record<string, unknown>, fields: string[]): string | null {
  for (const f of fields) {
    const v = payload[f];
    if (typeof v === "string" && v.trim()) return v.trim();
    if (typeof v === "number" && Number.isFinite(v) && f !== "id") return String(v);
  }
  return null;
}

export interface ConflictRecordIdentity {
  /** Number users know (OT-2026-0144, EQ-0012 …), when the payload carries one. */
  code: string | null;
  name: string | null;
}

/** Number and name of the record, from this device's version first, else the received one. */
export function conflictRecordIdentity(c: SyncConflictRecord): ConflictRecordIdentity {
  for (const json of [c.local_payload_json, c.inbound_payload_json]) {
    const payload = parsePayload(json);
    if (!payload) continue;
    const code = firstText(payload, CODE_FIELDS);
    const name = firstText(payload, NAME_FIELDS);
    if (code || name) return { code, name };
  }
  return { code: null, name: null };
}

/** The number a received record carries (`code` or `employee_code`) — for CODE_CLASH. */
export function clashingNumber(c: SyncConflictRecord): string | null {
  const payload = parsePayload(c.inbound_payload_json);
  if (!payload) return null;
  const value = payload["code"] ?? payload["employee_code"];
  return typeof value === "string" ? value : null;
}
