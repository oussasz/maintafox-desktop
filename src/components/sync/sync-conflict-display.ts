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
  "SITE_SETTING_UNAUDITED",
  "SITE_SETTING_INVALID",
  "SITE_SETTING_EVENT_INVALID",
  "SITE_SETTING_NOT_A_SITE",
  "SITE_SETTING_STALE",
  "SITE_SETTING_UNKNOWN",
  "SYNC_STATE_TIME_MISSING",
  "PERMIT_STATUS_BACKWARDS",
  "PERMIT_EXPIRY_MISSING",
  "PERMIT_EXPIRED",
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
  "PERMIT_DELETE_REFUSED",
  "QUALIFICATION_OVERRIDE_USE_PERSON",
  "SYNC_GROUP_REFUSED",
  "SYNC_TIME_INCONSISTENT",
  "QUALIFICATION_REQUIREMENT_DELETE_REFUSED",
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
