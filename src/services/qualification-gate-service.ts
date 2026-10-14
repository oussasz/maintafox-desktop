/**
 * Qualification gate, job requirements, supervisor overrides and the qualification matrix
 * (Rust: `qualification::gate`, `qualification::requirements`, `qualification::matrix`).
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

// ── Requirements ────────────────────────────────────────────────────────────

export const REQUIREMENT_TARGET_KINDS = [
  "work_order",
  "work_order_type",
  "permit_type",
  "loto_role",
] as const;
export type RequirementTargetKind = (typeof REQUIREMENT_TARGET_KINDS)[number];

export const LOTO_ROLES = ["isolation_apply", "isolation_verify", "removal_verify"] as const;
export type LotoRole = (typeof LOTO_ROLES)[number];

const QualificationRequirementSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  target_kind: z.enum(REQUIREMENT_TARGET_KINDS),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  target_code: z.string().nullable(),
  target_label: z.string().nullable(),
  certification_type_id: z.number().nullable(),
  certification_code: z.string().nullable(),
  certification_name: z.string().nullable(),
  skill_reference_value_id: z.number().nullable(),
  skill_code: z.string().nullable(),
  skill_label: z.string().nullable(),
  min_proficiency_level: z.number().nullable(),
  coverage: z.enum(["each", "any"]),
  notes: z.string().nullable(),
  is_active: z.boolean(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});
export type QualificationRequirement = z.infer<typeof QualificationRequirementSchema>;

export interface QualificationRequirementFilter {
  target_kind?: RequirementTargetKind | null;
  work_order_id?: number | null;
  target_code?: string | null;
  include_inactive?: boolean;
}

export interface QualificationRequirementUpsertInput {
  id?: number | null;
  expected_row_version?: number | null;
  target_kind: RequirementTargetKind;
  work_order_id?: number | null;
  target_code?: string | null;
  certification_type_id?: number | null;
  skill_reference_value_id?: number | null;
  min_proficiency_level?: number | null;
  coverage?: "each" | "any" | null;
  notes?: string | null;
  is_active?: boolean | null;
}

export async function listQualificationRequirements(
  filter: QualificationRequirementFilter = {},
): Promise<QualificationRequirement[]> {
  const raw = await invoke<unknown>("list_qualification_requirements", { filter });
  return z.array(QualificationRequirementSchema).parse(raw);
}

export async function upsertQualificationRequirement(
  input: QualificationRequirementUpsertInput,
): Promise<QualificationRequirement> {
  const raw = await invoke<unknown>("upsert_qualification_requirement", { input });
  return QualificationRequirementSchema.parse(raw);
}

// ── Gate ────────────────────────────────────────────────────────────────────

const JobRequirementSchema = z.object({
  requirement_id: z.number().nullable(),
  source_kind: z.string(),
  source_code: z.string().nullable(),
  certification_type_id: z.number().nullable(),
  skill_reference_value_id: z.number().nullable(),
  code: z.string().nullable(),
  label: z.string(),
  min_proficiency_level: z.number().nullable(),
  coverage: z.enum(["each", "any"]),
});
export type JobRequirement = z.infer<typeof JobRequirementSchema>;

export const GAP_STATUSES = [
  "missing",
  "expired",
  "expires_before_work",
  "pending_verification",
  "rejected",
  "level_too_low",
  "unvalidated",
] as const;
export type GapStatus = (typeof GAP_STATUSES)[number];

const QualificationGapSchema = z.object({
  requirement_key: z.string(),
  kind: z.enum(["certification", "skill"]),
  code: z.string().nullable(),
  label: z.string(),
  status: z.enum(GAP_STATUSES),
  expires_at: z.string().nullable(),
  held_level: z.number().nullable(),
  required_level: z.number().nullable(),
  source_kind: z.string(),
  source_code: z.string().nullable(),
});
export type QualificationGap = z.infer<typeof QualificationGapSchema>;

const PersonQualificationCheckSchema = z.object({
  personnel_id: z.number(),
  full_name: z.string(),
  employee_code: z.string(),
  roles: z.array(z.string()),
  qualified: z.boolean(),
  gaps: z.array(QualificationGapSchema),
  warnings: z.array(
    z.object({
      requirement_key: z.string(),
      code: z.string().nullable(),
      label: z.string(),
      expires_at: z.string(),
    }),
  ),
  override_info: z
    .object({
      id: z.number(),
      reason: z.string(),
      approved_by_id: z.number(),
      approved_by_name: z.string().nullable(),
      approved_at: z.string(),
    })
    .nullable(),
});
export type PersonQualificationCheck = z.infer<typeof PersonQualificationCheckSchema>;

const WoQualificationAssessmentSchema = z.object({
  work_order_id: z.number(),
  work_day: z.string(),
  requirements: z.array(JobRequirementSchema),
  people: z.array(PersonQualificationCheckSchema),
  uncovered: z.array(JobRequirementSchema),
  assessed_team_id: z.number().nullable(),
  ok: z.boolean(),
});
export type WoQualificationAssessment = z.infer<typeof WoQualificationAssessmentSchema>;

export async function getWoQualificationAssessment(
  woId: number,
): Promise<WoQualificationAssessment> {
  const raw = await invoke<unknown>("get_wo_qualification_assessment", { woId });
  return WoQualificationAssessmentSchema.parse(raw);
}

export async function checkPersonForWorkOrder(
  personnelId: number,
  woId: number,
): Promise<PersonQualificationCheck> {
  const raw = await invoke<unknown>("check_person_for_work_order", { personnelId, woId });
  return PersonQualificationCheckSchema.parse(raw);
}

export async function checkPersonForPermitRole(
  personnelId: number,
  permitId: number,
  role: LotoRole,
): Promise<PersonQualificationCheck> {
  const raw = await invoke<unknown>("check_person_for_permit_role", {
    personnelId,
    permitId,
    role,
  });
  return PersonQualificationCheckSchema.parse(raw);
}

export type QualificationOverrideContext =
  | { context_kind: "work_order"; work_order_id: number }
  | { context_kind: "permit_role"; permit_id: number; role: LotoRole };

/** Records a supervisor override (needs `trn.certify` and a recent step-up). */
export async function recordQualificationOverride(
  personnelId: number,
  context: QualificationOverrideContext,
  reason: string,
): Promise<PersonQualificationCheck> {
  const raw = await invoke<unknown>("record_qualification_override", {
    input: { personnel_id: personnelId, reason, ...context },
  });
  return PersonQualificationCheckSchema.parse(raw);
}

const QualificationOverrideRowSchema = z.object({
  id: z.number(),
  personnel_id: z.number(),
  person_name: z.string(),
  context_kind: z.enum(["work_order", "permit_role"]),
  work_order_id: z.number().nullable(),
  work_order_code: z.string().nullable(),
  permit_id: z.number().nullable(),
  permit_code: z.string().nullable(),
  role_code: z.string().nullable(),
  gaps: z.array(QualificationGapSchema),
  reason: z.string(),
  approved_by_id: z.number(),
  approved_by_name: z.string().nullable(),
  approved_at: z.string(),
});
export type QualificationOverrideRow = z.infer<typeof QualificationOverrideRowSchema>;

export async function listQualificationOverrides(
  filter: {
    work_order_id?: number | null;
    personnel_id?: number | null;
    limit?: number | null;
  } = {},
): Promise<QualificationOverrideRow[]> {
  const raw = await invoke<unknown>("list_qualification_overrides", { filter });
  return z.array(QualificationOverrideRowSchema).parse(raw);
}

// ── Matrix ──────────────────────────────────────────────────────────────────

export const MATRIX_CELL_STATUSES = [
  "valid",
  "expiring_soon",
  "expired",
  "pending",
  "rejected",
  "unvalidated",
] as const;
export type MatrixCellStatus = (typeof MATRIX_CELL_STATUSES)[number];

const QualificationMatrixSchema = z.object({
  columns: z.array(
    z.object({
      key: z.string(),
      kind: z.enum(["certification", "skill"]),
      id: z.number(),
      code: z.string(),
      label: z.string(),
      category: z.string(),
    }),
  ),
  rows: z.array(
    z.object({
      personnel_id: z.number(),
      full_name: z.string(),
      employee_code: z.string(),
      employment_type: z.string(),
      team_id: z.number().nullable(),
      team_name: z.string().nullable(),
      position_name: z.string().nullable(),
      company_name: z.string().nullable(),
      cells: z.array(
        z.object({
          key: z.string(),
          status: z.enum(MATRIX_CELL_STATUSES),
          level: z.number().nullable(),
          expires_at: z.string().nullable(),
        }),
      ),
    }),
  ),
  generated_on: z.string(),
});
export type QualificationMatrix = z.infer<typeof QualificationMatrixSchema>;
export type QualificationMatrixColumn = QualificationMatrix["columns"][number];
export type QualificationMatrixRow = QualificationMatrix["rows"][number];
export type QualificationMatrixCell = QualificationMatrixRow["cells"][number];

export interface QualificationMatrixFilter {
  team_id?: number | null;
  entity_id?: number | null;
  company_id?: number | null;
  include_inactive?: boolean;
}

export async function getQualificationMatrix(
  filter: QualificationMatrixFilter = {},
): Promise<QualificationMatrix> {
  const raw = await invoke<unknown>("get_qualification_matrix", { filter });
  return QualificationMatrixSchema.parse(raw);
}
