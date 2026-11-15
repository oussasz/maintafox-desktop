// ADR-003: invoke() calls for what a work permit says before it is issued, its HSE approval, its
// gas tests (L4-03 / L4-04) and the isolation points its equipment proposes (L4-12).

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const IssueBlockerSchema = z.object({
  /** A `GATE_` code translated under `errors:gateError`. */
  code: z.string(),
  params: z.record(z.string(), z.string()),
});

const GasLimitsSchema = z.object({
  o2_min_percent: z.number(),
  o2_max_percent: z.number(),
  lel_max_percent: z.number(),
  h2s_max_ppm: z.number(),
  co_max_ppm: z.number(),
});

const PermitGasTestSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  permit_id: z.number(),
  o2_percent: z.number(),
  lel_percent: z.number(),
  h2s_ppm: z.number(),
  co_ppm: z.number().nullable(),
  measured_at: z.string(),
  measured_by_id: z.number(),
  measured_by_name: z.string().nullable(),
  instrument: z.string(),
  row_version: z.number(),
  /** Judged against the current limits of the settings. */
  within_limits: z.boolean(),
  /** Measured after the permit was raised and its last reset: only such a reading counts. */
  counts: z.boolean(),
});

const PermitSafetyContentSchema = z.object({
  permit_id: z.number(),
  permit_status: z.string(),
  permit_row_version: z.number(),
  work_description: z.string().nullable(),
  risk_assessment: z.string().nullable(),
  preventive_measures: z.string().nullable(),
  ppe_required: z.string().nullable(),
  requires_hse_approval: z.boolean(),
  requires_gas_test: z.boolean(),
  hse_approved_by_id: z.number().nullable(),
  hse_approved_by_name: z.string().nullable(),
  hse_approved_at: z.string().nullable(),
  hse_approval_note: z.string().nullable(),
  /** Recorded by the author or submitter on a site that opted out of four-eyes. */
  hse_self_approved: z.boolean().nullable(),
  gas_limits: GasLimitsSchema,
  /** Re-test interval of the settings (hours). */
  gas_validity_hours: z.number(),
  /** Latest first. */
  gas_tests: z.array(PermitGasTestSchema),
  /** What blocks the issue now, as the server judges it (empty: nothing). */
  issue_blockers: z.array(IssueBlockerSchema),
});

const EquipmentIsolationPointSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  point_code: z.string(),
  label: z.string(),
  energy_type_id: z.number(),
  energy_type_code: z.string().nullable(),
  energy_type_label: z.string().nullable(),
  isolation_method_id: z.number(),
  isolation_method_label: z.string().nullable(),
  normal_state: z.string().nullable(),
  is_active: z.boolean(),
  row_version: z.number(),
});

const ProposedIsolationPointSchema = z.object({
  point: EquipmentIsolationPointSchema,
  isolation_point_text: z.string(),
  already_on_permit: z.boolean(),
});

export type IssueBlocker = z.infer<typeof IssueBlockerSchema>;
export type PermitGasTest = z.infer<typeof PermitGasTestSchema>;
export type PermitSafetyContent = z.infer<typeof PermitSafetyContentSchema>;
export type EquipmentIsolationPoint = z.infer<typeof EquipmentIsolationPointSchema>;
export type ProposedIsolationPoint = z.infer<typeof ProposedIsolationPointSchema>;
export { EquipmentIsolationPointSchema };

export interface PermitSafetyContentInput {
  permit_id: number;
  expected_row_version: number;
  work_description: string;
  risk_assessment: string;
  preventive_measures: string;
  ppe_required: string;
}

export interface PermitHseApprovalInput {
  permit_id: number;
  expected_row_version: number;
  note: string | null;
}

/** The person who measured is the session user (never sent). */
export interface PermitGasTestInput {
  permit_id: number;
  o2_percent: number;
  lel_percent: number;
  h2s_ppm: number;
  co_ppm: number | null;
  /** RFC 3339. */
  measured_at: string;
  instrument: string;
}

export async function getPermitSafetyContent(permitId: number): Promise<PermitSafetyContent> {
  const raw = await invoke<unknown>("get_permit_safety_content", { permitId });
  return PermitSafetyContentSchema.parse(raw);
}

export async function savePermitSafetyContent(
  input: PermitSafetyContentInput,
): Promise<PermitSafetyContent> {
  const raw = await invoke<unknown>("save_permit_safety_content", { input });
  return PermitSafetyContentSchema.parse(raw);
}

export async function recordPermitHseApproval(
  input: PermitHseApprovalInput,
): Promise<PermitSafetyContent> {
  const raw = await invoke<unknown>("record_permit_hse_approval", { input });
  return PermitSafetyContentSchema.parse(raw);
}

export async function recordPermitGasTest(input: PermitGasTestInput): Promise<PermitSafetyContent> {
  const raw = await invoke<unknown>("record_permit_gas_test", { input });
  return PermitSafetyContentSchema.parse(raw);
}

/** The active isolation points of the permit's equipment, proposed on its isolation form. */
export async function listPermitProposedIsolationPoints(
  permitId: number,
): Promise<ProposedIsolationPoint[]> {
  const raw = await invoke<unknown>("list_permit_proposed_isolation_points", { permitId });
  return z.array(ProposedIsolationPointSchema).parse(raw);
}
