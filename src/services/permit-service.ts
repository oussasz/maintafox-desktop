// ADR-003: all invoke() calls for work permits (PRD §6.23) live here.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  PermitHandoverLogInput,
  PermitIsolationActionInput,
  PermitIsolationUpsertInput,
  PermitSuspendInput,
  PermitTypeUpsertInput,
  WorkPermitCreateInput,
  WorkPermitListFilter,
  WorkPermitStatusInput,
  WorkPermitUpdateInput,
} from "@shared/ipc-types";

const PermitTypeSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  code: z.string(),
  name: z.string(),
  description: z.string(),
  requires_hse_approval: z.boolean(),
  requires_operations_approval: z.boolean(),
  requires_atmospheric_test: z.boolean(),
  max_duration_hours: z.number().nullable(),
  mandatory_ppe_ids_json: z.string(),
  mandatory_control_rules_json: z.string(),
  row_version: z.number(),
});

const WorkPermitSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  code: z.string(),
  linked_work_order_id: z.number().nullable(),
  permit_type_id: z.number(),
  asset_id: z.number(),
  entity_id: z.number(),
  status: z.string(),
  requested_at: z.string().nullable(),
  issued_at: z.string().nullable(),
  activated_at: z.string().nullable(),
  expires_at: z.string().nullable(),
  closed_at: z.string().nullable(),
  handed_back_at: z.string().nullable(),
  row_version: z.number(),
  permit_type_code: z.string().nullable(),
  permit_type_name: z.string().nullable(),
  asset_code: z.string().nullable(),
  asset_label: z.string().nullable(),
  linked_work_order_code: z.string().nullable(),
});

const PermitSuspensionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  permit_id: z.number(),
  reason: z.string(),
  suspended_by_id: z.number(),
  suspended_at: z.string(),
  reinstated_by_id: z.number().nullable(),
  reinstated_at: z.string().nullable(),
  reactivation_conditions: z.string(),
  row_version: z.number(),
  suspended_by_name: z.string().nullable(),
  reinstated_by_name: z.string().nullable(),
});

const PermitHandoverLogSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  permit_id: z.number(),
  handed_from_role: z.string(),
  handed_to_role: z.string(),
  confirmation_note: z.string(),
  signed_at: z.string(),
  row_version: z.number(),
});

const PermitIsolationSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  permit_id: z.number(),
  isolation_point: z.string(),
  energy_type: z.string(),
  isolation_method: z.string(),
  lock_number: z.string().nullable(),
  applied_by_id: z.number().nullable(),
  verified_by_id: z.number().nullable(),
  applied_at: z.string().nullable(),
  verified_at: z.string().nullable(),
  removal_verified_at: z.string().nullable(),
  removal_verified_by_id: z.number().nullable(),
  row_version: z.number(),
  applied_by_name: z.string().nullable(),
  verified_by_name: z.string().nullable(),
  removal_verified_by_name: z.string().nullable(),
});

const LotoCardViewSchema = z.object({
  permit_code: z.string(),
  equipment_label: z.string(),
  energy_type: z.string(),
  isolation_id: z.number(),
  isolation_point: z.string(),
  lock_number: z.string().nullable(),
  verifier_signature: z.string().nullable(),
  expires_at: z.string().nullable(),
});

const LotoCardPrintJobSchema = z.object({
  id: z.number(),
  permit_id: z.number(),
  isolation_id: z.number(),
  printed_at: z.string(),
  printed_by_id: z.number(),
  entity_sync_id: z.string(),
  row_version: z.number(),
});

const PermitComplianceKpi30dSchema = z.object({
  activated_count: z.number(),
  handed_back_on_time_count: z.number(),
  rate: z.number().nullable(),
});

export type PermitTypeRecord = z.infer<typeof PermitTypeSchema>;
export type WorkPermitRecord = z.infer<typeof WorkPermitSchema>;
export type PermitSuspensionRecord = z.infer<typeof PermitSuspensionSchema>;
export type PermitHandoverLogRecord = z.infer<typeof PermitHandoverLogSchema>;
export type PermitIsolationRecord = z.infer<typeof PermitIsolationSchema>;
export type LotoCardViewRecord = z.infer<typeof LotoCardViewSchema>;
export type LotoCardPrintJobRecord = z.infer<typeof LotoCardPrintJobSchema>;
export type PermitComplianceKpi30dRecord = z.infer<typeof PermitComplianceKpi30dSchema>;

export async function listPermitTypes(): Promise<PermitTypeRecord[]> {
  const raw = await invoke<unknown>("list_permit_types");
  return z.array(PermitTypeSchema).parse(raw);
}

export async function listWorkPermits(filter: WorkPermitListFilter): Promise<WorkPermitRecord[]> {
  const raw = await invoke<unknown>("list_work_permits", { filter });
  return z.array(WorkPermitSchema).parse(raw);
}

export async function listPermitSuspensions(permitId: number): Promise<PermitSuspensionRecord[]> {
  const raw = await invoke<unknown>("list_permit_suspensions", { permitId });
  return z.array(PermitSuspensionSchema).parse(raw);
}

export async function listPermitHandoverLogs(permitId: number): Promise<PermitHandoverLogRecord[]> {
  const raw = await invoke<unknown>("list_permit_handover_logs", { permitId });
  return z.array(PermitHandoverLogSchema).parse(raw);
}

export async function listPermitIsolations(permitId: number): Promise<PermitIsolationRecord[]> {
  const raw = await invoke<unknown>("list_permit_isolations", { permitId });
  return z.array(PermitIsolationSchema).parse(raw);
}

export async function getLotoCardView(
  permitId: number,
  isolationId: number,
): Promise<LotoCardViewRecord> {
  const raw = await invoke<unknown>("get_loto_card_view", { permitId, isolationId });
  return LotoCardViewSchema.parse(raw);
}

export async function recordLotoCardPrint(input: {
  permit_id: number;
  isolation_id: number;
}): Promise<LotoCardPrintJobRecord> {
  const raw = await invoke<unknown>("record_loto_card_print", { input });
  return LotoCardPrintJobSchema.parse(raw);
}

export async function listOpenPermitsReport(): Promise<WorkPermitRecord[]> {
  const raw = await invoke<unknown>("list_open_permits_report");
  return z.array(WorkPermitSchema).parse(raw);
}

export async function permitComplianceKpi30d(): Promise<PermitComplianceKpi30dRecord> {
  const raw = await invoke<unknown>("permit_compliance_kpi_30d");
  return PermitComplianceKpi30dSchema.parse(raw);
}

// ── Writes ────────────────────────────────────────────────────────────────

export async function getWorkPermit(id: number): Promise<WorkPermitRecord> {
  const raw = await invoke<unknown>("get_work_permit", { id });
  return WorkPermitSchema.parse(raw);
}

export const TERMINAL_PERMIT_STATUSES: ReadonlySet<string> = new Set([
  "closed",
  "handed_back",
  "cancelled",
  "expired",
]);

/** Returns the permit the WO gate evaluates (active first, then newest open). */
export async function getPermitForWorkOrder(workOrderId: number): Promise<WorkPermitRecord | null> {
  const rows = await listWorkPermits({ linked_work_order_id: workOrderId, limit: 50 });
  const rank = (p: WorkPermitRecord): number =>
    p.status === "active" ? 0 : TERMINAL_PERMIT_STATUSES.has(p.status) ? 2 : 1;
  return [...rows].sort((a, b) => rank(a) - rank(b) || b.id - a.id)[0] ?? null;
}

export async function upsertPermitType(input: PermitTypeUpsertInput): Promise<PermitTypeRecord> {
  const raw = await invoke<unknown>("upsert_permit_type", { input });
  return PermitTypeSchema.parse(raw);
}

export async function createWorkPermit(input: WorkPermitCreateInput): Promise<WorkPermitRecord> {
  const raw = await invoke<unknown>("create_work_permit", { input });
  return WorkPermitSchema.parse(raw);
}

export async function updateWorkPermit(input: WorkPermitUpdateInput): Promise<WorkPermitRecord> {
  const raw = await invoke<unknown>("update_work_permit", { input });
  return WorkPermitSchema.parse(raw);
}

export async function setWorkPermitStatus(input: WorkPermitStatusInput): Promise<WorkPermitRecord> {
  const raw = await invoke<unknown>("set_work_permit_status", { input });
  return WorkPermitSchema.parse(raw);
}

export async function suspendWorkPermit(
  input: PermitSuspendInput,
): Promise<{ permit: WorkPermitRecord; suspension: PermitSuspensionRecord }> {
  const raw = await invoke<unknown>("suspend_work_permit", { input });
  const [permit, suspension] = z.tuple([WorkPermitSchema, PermitSuspensionSchema]).parse(raw);
  return { permit, suspension };
}

export async function appendPermitHandoverLog(
  input: PermitHandoverLogInput,
): Promise<PermitHandoverLogRecord> {
  const raw = await invoke<unknown>("append_permit_handover_log", { input });
  return PermitHandoverLogSchema.parse(raw);
}

export async function upsertPermitIsolation(
  input: PermitIsolationUpsertInput,
): Promise<PermitIsolationRecord> {
  const raw = await invoke<unknown>("upsert_permit_isolation", { input });
  return PermitIsolationSchema.parse(raw);
}

export async function applyPermitIsolation(
  input: PermitIsolationActionInput,
): Promise<PermitIsolationRecord> {
  const raw = await invoke<unknown>("apply_permit_isolation", { input });
  return PermitIsolationSchema.parse(raw);
}

export async function verifyPermitIsolation(
  input: PermitIsolationActionInput,
): Promise<PermitIsolationRecord> {
  const raw = await invoke<unknown>("verify_permit_isolation", { input });
  return PermitIsolationSchema.parse(raw);
}

export async function verifyPermitIsolationRemoval(
  input: PermitIsolationActionInput,
): Promise<PermitIsolationRecord> {
  const raw = await invoke<unknown>("verify_permit_isolation_removal", { input });
  return PermitIsolationSchema.parse(raw);
}
