/**
 * personnel-service.ts
 *
 * IPC wrappers for Personnel (PRD §6.6). All invoke names match Rust `#[tauri::command]` ids.
 */

import { invoke } from "@tauri-apps/api/core";
import { z, ZodError } from "zod";

import type {
  DeclareOwnSkillInput,
  AvailabilityBlockCreateInput,
  AvailabilityCalendarEntry,
  AvailabilityCalendarFilter,
  CompanyListFilter,
  ExternalCompany,
  ExternalCompanyContact,
  Personnel,
  PersonnelAvailabilityBlock,
  PersonnelAuthorization,
  PersonnelCreateInput,
  PersonnelDetailPayload,
  PersonnelListFilter,
  PersonnelListPage,
  PersonnelRateCard,
  PersonnelSkillReferenceValue,
  PersonnelTeamAssignment,
  PersonnelUpdateInput,
  PersonnelWorkHistoryEntry,
  PersonnelWorkloadSummary,
  Position,
  SkillMatrixRow,
  SkillsMatrixFilter,
  ScheduleClass,
  ScheduleClassWithDetails,
  ScheduleDetail,
  TeamCapacityFilter,
  TeamCapacitySummaryRow,
  SuccessionRiskRow,
  PersonnelImportCreateInput,
  PersonnelImportApplyResult,
  PersonnelImportBatchSummary,
  PersonnelImportMessage,
  PersonnelImportPreview,
  PersonnelImportPreviewRow,
  WorkforceKpiReport,
  WorkforceSkillsGapRow,
  WorkforceSummaryReport,
} from "@shared/ipc-types";

// ── Zod enums & shared types ─────────────────────────────────────────────────

export const EmploymentTypeSchema = z.enum(["employee", "contractor", "temp", "vendor"]);
export type EmploymentType = z.infer<typeof EmploymentTypeSchema>;

export const AvailabilityStatusSchema = z.enum([
  "available",
  "assigned",
  "in_training",
  "on_leave",
  "blocked",
  "inactive",
]);
export type AvailabilityStatus = z.infer<typeof AvailabilityStatusSchema>;

export const PositionCategorySchema = z.enum([
  "technician",
  "supervisor",
  "engineer",
  "operator",
  "contractor",
  "planner",
  "storekeeper",
  "hse",
]);
export type PositionCategory = z.infer<typeof PositionCategorySchema>;

// ── Row schemas ───────────────────────────────────────────────────────────────

export const PersonnelSchema = z.object({
  id: z.number(),
  employee_code: z.string(),
  full_name: z.string(),
  employment_type: z.string(),
  position_id: z.number().nullable(),
  primary_entity_id: z.number().nullable(),
  primary_team_id: z.number().nullable(),
  supervisor_id: z.number().nullable(),
  home_schedule_id: z.number().nullable(),
  availability_status: z.string(),
  hire_date: z.string().nullable(),
  termination_date: z.string().nullable(),
  email: z.string().nullable(),
  phone: z.string().nullable(),
  photo_path: z.string().nullable(),
  hr_external_id: z.string().nullable(),
  external_company_id: z.number().nullable(),
  notes: z.string().nullable(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
  position_name: z.string().nullable(),
  position_category: z.string().nullable(),
  entity_name: z.string().nullable(),
  team_name: z.string().nullable(),
  supervisor_name: z.string().nullable(),
  schedule_name: z.string().nullable(),
  company_name: z.string().nullable(),
});
export const PersonnelListPageSchema = z.object({
  items: z.array(PersonnelSchema),
  total: z.number(),
});

export const PositionSchema = z.object({
  id: z.number(),
  code: z.string(),
  name: z.string(),
  category: z.string(),
  requirement_profile_id: z.number().nullable(),
  is_active: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const ScheduleClassSchema = z.object({
  id: z.number(),
  name: z.string(),
  shift_pattern_code: z.string(),
  is_continuous: z.number(),
  nominal_hours_per_day: z.number(),
  is_active: z.number(),
  created_at: z.string(),
});

export const ScheduleDetailSchema = z.object({
  id: z.number(),
  schedule_class_id: z.number(),
  day_of_week: z.number(),
  shift_start: z.string(),
  shift_end: z.string(),
  is_rest_day: z.number(),
});

export const ScheduleClassWithDetailsSchema = z.object({
  class: ScheduleClassSchema,
  details: z.array(ScheduleDetailSchema),
});

export const PersonnelRateCardSchema = z.object({
  id: z.number(),
  personnel_id: z.number(),
  effective_from: z.string(),
  labor_rate: z.number(),
  overtime_rate: z.number(),
  cost_center_id: z.number().nullable(),
  source_type: z.string(),
  created_at: z.string(),
});

export const PersonnelAuthorizationSchema = z.object({
  id: z.number(),
  personnel_id: z.number(),
  authorization_type: z.string(),
  valid_from: z.string(),
  valid_to: z.string().nullable(),
  source_certification_type_id: z.number().nullable(),
  is_active: z.number(),
  created_at: z.string(),
});

export const ExternalCompanySchema = z.object({
  id: z.number(),
  name: z.string(),
  service_domain: z.string().nullable(),
  contract_start: z.string().nullable(),
  contract_end: z.string().nullable(),
  onboarding_status: z.string(),
  insurance_status: z.string(),
  notes: z.string().nullable(),
  is_active: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const ExternalCompanyContactSchema = z.object({
  id: z.number(),
  company_id: z.number(),
  contact_name: z.string(),
  contact_role: z.string().nullable(),
  phone: z.string().nullable(),
  email: z.string().nullable(),
  is_primary: z.number(),
  created_at: z.string(),
});

export const PersonnelDetailPayloadSchema = z.object({
  personnel: PersonnelSchema,
  rate_cards: z.array(PersonnelRateCardSchema),
  authorizations: z.array(PersonnelAuthorizationSchema),
});

export const SkillMatrixRowSchema = z.object({
  personnel_id: z.number(),
  employee_code: z.string(),
  full_name: z.string(),
  employment_type: z.string(),
  availability_status: z.string(),
  entity_id: z.number().nullable(),
  entity_name: z.string().nullable(),
  team_id: z.number().nullable(),
  team_name: z.string().nullable(),
  skill_code: z.string().nullable(),
  skill_label: z.string().nullable(),
  proficiency_level: z.number().nullable(),
  coverage_status: z.enum(["active", "expired", "missing"]),
});

export const AvailabilityCalendarEntrySchema = z.object({
  personnel_id: z.number(),
  employee_code: z.string(),
  full_name: z.string(),
  entity_id: z.number().nullable(),
  entity_name: z.string().nullable(),
  team_id: z.number().nullable(),
  team_name: z.string().nullable(),
  work_date: z.string(),
  shift_start: z.string().nullable(),
  shift_end: z.string().nullable(),
  scheduled_minutes: z.number(),
  blocked_minutes: z.number(),
  available_minutes: z.number(),
  has_critical_block: z.boolean(),
  block_types: z.array(z.string()),
});

export const TeamCapacitySummaryRowSchema = z.object({
  team_id: z.number(),
  team_code: z.string(),
  team_name: z.string(),
  member_count: z.number(),
  lead_count: z.number(),
  total_scheduled_minutes: z.number(),
  total_available_minutes: z.number(),
  total_blocked_minutes: z.number(),
  avg_availability_ratio: z.number(),
});

export const PersonnelAvailabilityBlockSchema = z.object({
  id: z.number(),
  personnel_id: z.number(),
  block_type: z.string(),
  start_at: z.string(),
  end_at: z.string(),
  reason_note: z.string().nullable(),
  is_critical: z.boolean(),
  created_by_id: z.number().nullable(),
  created_at: z.string(),
});

export const PersonnelTeamAssignmentSchema = z.object({
  id: z.number(),
  personnel_id: z.number(),
  team_id: z.number(),
  team_code: z.string().nullable(),
  team_name: z.string().nullable(),
  role_code: z.string(),
  allocation_percent: z.number(),
  valid_from: z.string().nullable(),
  valid_to: z.string().nullable(),
  is_lead: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const PersonnelWorkHistoryEntrySchema = z.object({
  source_module: z.string(),
  record_id: z.number(),
  record_code: z.string().nullable(),
  role_code: z.string(),
  status_code: z.string().nullable(),
  title: z.string(),
  happened_at: z.string(),
});

export const PersonnelWorkloadSummarySchema = z.object({
  open_work_orders: z.number(),
  in_progress_work_orders: z.number(),
  pending_interventions: z.number(),
  interventions_last_30d: z.number(),
});

export const SuccessionRiskRowSchema = z.object({
  personnel_id: z.number(),
  full_name: z.string(),
  employee_code: z.string(),
  position_name: z.string().nullable(),
  team_name: z.string().nullable(),
  coverage_count: z.number(),
  risk_level: z.enum(["high", "medium", "low"]),
  reason: z.string(),
});

export const PersonnelSkillReferenceValueSchema = z.object({
  id: z.number(),
  code: z.string(),
  label: z.string(),
});

export const PersonnelImportMessageSchema = z.object({
  category: z.string(),
  severity: z.string(),
  message: z.string(),
});

export const PersonnelImportBatchSummarySchema = z.object({
  id: z.number(),
  source_filename: z.string(),
  source_sha256: z.string(),
  source_kind: z.string(),
  mode: z.string(),
  status: z.string(),
  total_rows: z.number(),
  valid_rows: z.number(),
  warning_rows: z.number(),
  error_rows: z.number(),
  initiated_by_id: z.number().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const PersonnelImportPreviewRowSchema = z.object({
  id: z.number(),
  row_no: z.number(),
  employee_code: z.string().nullable(),
  hr_external_id: z.string().nullable(),
  target_personnel_id: z.number().nullable(),
  target_row_version: z.number().nullable(),
  validation_status: z.string(),
  messages: z.array(PersonnelImportMessageSchema),
  proposed_action: z.string().nullable(),
  raw_json: z.string(),
});

export const PersonnelImportPreviewSchema = z.object({
  batch: PersonnelImportBatchSummarySchema,
  rows: z.array(PersonnelImportPreviewRowSchema),
});

export const PersonnelImportApplyResultSchema = z.object({
  batch: PersonnelImportBatchSummarySchema,
  created: z.number(),
  updated: z.number(),
  skipped: z.number(),
  protected_ignored: z.number(),
});

export const WorkforceSummaryRowSchema = z.object({
  bucket: z.string(),
  count: z.number(),
});

export const WorkforceSummaryReportSchema = z.object({
  total_personnel: z.number(),
  active_personnel: z.number(),
  employment_breakdown: z.array(WorkforceSummaryRowSchema),
  availability_breakdown: z.array(WorkforceSummaryRowSchema),
});

export const WorkforceSkillsGapRowSchema = z.object({
  personnel_id: z.number(),
  employee_code: z.string(),
  full_name: z.string(),
  position_name: z.string().nullable(),
  team_name: z.string().nullable(),
  active_skill_count: z.number(),
  gap_score: z.number(),
});

export const WorkforceKpiReportSchema = z.object({
  avg_skills_per_person: z.number(),
  blocked_ratio: z.number(),
  contractor_ratio: z.number(),
  team_coverage_ratio: z.number(),
});

// ── IPC errors ────────────────────────────────────────────────────────────────

interface IpcErrorShape {
  code: string;
  message: string;
}

function isIpcError(err: unknown): err is IpcErrorShape {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

export class PersonnelIpcError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "PersonnelIpcError";
    this.code = code;
  }
}

function mapInvokeError(err: unknown): never {
  if (isIpcError(err)) {
    throw new PersonnelIpcError(err.code, err.message);
  }
  if (err instanceof Error) throw err;
  throw new Error(String(err));
}

async function invokeParsed<T>(command: string, args: Record<string, unknown> | undefined, schema: z.ZodType<T>): Promise<T> {
  try {
    const raw = await invoke<unknown>(command, args);
    return schema.parse(raw);
  } catch (err) {
    if (err instanceof ZodError) {
      throw new PersonnelIpcError("VALIDATION_FAILED", err.message);
    }
    mapInvokeError(err);
  }
}

// ── Commands ──────────────────────────────────────────────────────────────────

export async function listPersonnel(filter: PersonnelListFilter): Promise<PersonnelListPage> {
  const normalized: PersonnelListFilter = {
    ...filter,
    employment_type:
      filter.employment_type != null && filter.employment_type.length > 0
        ? filter.employment_type
        : null,
    availability_status:
      filter.availability_status != null && filter.availability_status.length > 0
        ? filter.availability_status
        : null,
  };
  const page = await invokeParsed("list_personnel", { filter: normalized }, PersonnelListPageSchema);
  return page as PersonnelListPage;
}

export async function getPersonnel(id: number): Promise<PersonnelDetailPayload> {
  const detail = await invokeParsed("get_personnel", { id }, PersonnelDetailPayloadSchema);
  return detail as PersonnelDetailPayload;
}

export async function createPersonnel(input: PersonnelCreateInput): Promise<Personnel> {
  const p = await invokeParsed("create_personnel", { input }, PersonnelSchema);
  return p as Personnel;
}

export async function updatePersonnel(input: PersonnelUpdateInput): Promise<Personnel> {
  const p = await invokeParsed("update_personnel", { input }, PersonnelSchema);
  return p as Personnel;
}

export async function deactivatePersonnel(id: number, expectedRowVersion: number): Promise<Personnel> {
  const p = await invokeParsed(
    "deactivate_personnel",
    { id, expected_row_version: expectedRowVersion },
    PersonnelSchema,
  );
  return p as Personnel;
}

export async function listPositions(): Promise<Position[]> {
  const rows = await invokeParsed("list_positions", undefined, z.array(PositionSchema));
  return rows as Position[];
}

export async function createPosition(code: string, name: string, category: string): Promise<Position> {
  const p = await invokeParsed("create_position", { code, name, category }, PositionSchema);
  return p as Position;
}

export async function listScheduleClasses(): Promise<ScheduleClassWithDetails[]> {
  const rows = await invokeParsed("list_schedule_classes", undefined, z.array(ScheduleClassWithDetailsSchema));
  return rows as ScheduleClassWithDetails[];
}

export async function listRateCards(personnelId: number): Promise<PersonnelRateCard[]> {
  const rows = await invokeParsed("list_rate_cards", { personnel_id: personnelId }, z.array(PersonnelRateCardSchema));
  return rows as PersonnelRateCard[];
}

export async function createRateCard(
  personnelId: number,
  laborRate: number,
  overtimeRate: number,
  costCenterId: number | null,
  sourceType: string,
): Promise<PersonnelRateCard> {
  const rc = await invokeParsed(
    "create_rate_card",
    {
      personnel_id: personnelId,
      labor_rate: laborRate,
      overtime_rate: overtimeRate,
      cost_center_id: costCenterId,
      source_type: sourceType,
    },
    PersonnelRateCardSchema,
  );
  return rc as PersonnelRateCard;
}

export async function listAuthorizations(personnelId: number): Promise<PersonnelAuthorization[]> {
  const rows = await invokeParsed(
    "list_authorizations",
    { personnel_id: personnelId },
    z.array(PersonnelAuthorizationSchema),
  );
  return rows as PersonnelAuthorization[];
}

export async function createAuthorization(
  personnelId: number,
  authorizationType: string,
  validFrom: string,
  validTo: string | null,
  sourceCertificationTypeId: number | null,
): Promise<PersonnelAuthorization> {
  const row = await invokeParsed(
    "create_authorization",
    {
      personnel_id: personnelId,
      authorization_type: authorizationType,
      valid_from: validFrom,
      valid_to: validTo,
      source_certification_type_id: sourceCertificationTypeId,
    },
    PersonnelAuthorizationSchema,
  );
  return row as PersonnelAuthorization;
}

export async function listExternalCompanies(filter?: CompanyListFilter): Promise<ExternalCompany[]> {
  const rows = await invokeParsed(
    "list_external_companies",
    { filter: filter ?? {} },
    z.array(ExternalCompanySchema),
  );
  return rows as ExternalCompany[];
}

export async function createExternalCompany(
  name: string,
  serviceDomain?: string,
  contractStart?: string,
  contractEnd?: string,
  notes?: string,
): Promise<ExternalCompany> {
  const row = await invokeParsed(
    "create_external_company",
    {
      name,
      service_domain: serviceDomain ?? null,
      contract_start: contractStart ?? null,
      contract_end: contractEnd ?? null,
      notes: notes ?? null,
    },
    ExternalCompanySchema,
  );
  return row as ExternalCompany;
}

export async function listCompanyContacts(companyId: number): Promise<ExternalCompanyContact[]> {
  const rows = await invokeParsed(
    "list_company_contacts",
    { company_id: companyId },
    z.array(ExternalCompanyContactSchema),
  );
  return rows as ExternalCompanyContact[];
}

export async function listSkillsMatrix(filter: SkillsMatrixFilter): Promise<SkillMatrixRow[]> {
  const rows = await invokeParsed(
    "list_skills_matrix",
    { filter },
    z.array(SkillMatrixRowSchema),
  );
  return rows as SkillMatrixRow[];
}

export async function listAvailabilityCalendar(
  filter: AvailabilityCalendarFilter,
): Promise<AvailabilityCalendarEntry[]> {
  const rows = await invokeParsed(
    "list_availability_calendar",
    { filter },
    z.array(AvailabilityCalendarEntrySchema),
  );
  return rows as AvailabilityCalendarEntry[];
}

export async function listTeamCapacitySummary(
  filter: TeamCapacityFilter,
): Promise<TeamCapacitySummaryRow[]> {
  const rows = await invokeParsed(
    "list_team_capacity_summary",
    { filter },
    z.array(TeamCapacitySummaryRowSchema),
  );
  return rows as TeamCapacitySummaryRow[];
}

export async function createAvailabilityBlock(
  input: AvailabilityBlockCreateInput,
): Promise<PersonnelAvailabilityBlock> {
  const row = await invokeParsed(
    "create_availability_block",
    { input },
    PersonnelAvailabilityBlockSchema,
  );
  return row as PersonnelAvailabilityBlock;
}

export async function listPersonnelTeamAssignments(personnelId: number): Promise<PersonnelTeamAssignment[]> {
  const rows = await invokeParsed(
    "list_personnel_team_assignments",
    { personnel_id: personnelId },
    z.array(PersonnelTeamAssignmentSchema),
  );
  return rows as PersonnelTeamAssignment[];
}

export async function listPersonnelAvailabilityBlocks(
  personnelId: number,
  limit = 50,
): Promise<PersonnelAvailabilityBlock[]> {
  const rows = await invokeParsed(
    "list_personnel_availability_blocks",
    { personnel_id: personnelId, limit },
    z.array(PersonnelAvailabilityBlockSchema),
  );
  return rows as PersonnelAvailabilityBlock[];
}

export async function listPersonnelWorkHistory(
  personnelId: number,
  limit = 60,
): Promise<PersonnelWorkHistoryEntry[]> {
  const rows = await invokeParsed(
    "list_personnel_work_history",
    { personnel_id: personnelId, limit },
    z.array(PersonnelWorkHistoryEntrySchema),
  );
  return rows as PersonnelWorkHistoryEntry[];
}

export async function getPersonnelWorkloadSummary(personnelId: number): Promise<PersonnelWorkloadSummary> {
  const row = await invokeParsed(
    "get_personnel_workload_summary",
    { personnel_id: personnelId },
    PersonnelWorkloadSummarySchema,
  );
  return row as PersonnelWorkloadSummary;
}

export async function scanSuccessionRisk(
  entityId?: number | null,
  teamId?: number | null,
): Promise<SuccessionRiskRow[]> {
  const rows = await invokeParsed(
    "scan_succession_risk",
    { entity_id: entityId ?? null, team_id: teamId ?? null },
    z.array(SuccessionRiskRowSchema),
  );
  return rows as SuccessionRiskRow[];
}

export async function declareOwnSkill(input: DeclareOwnSkillInput): Promise<void> {
  await invokeParsed("declare_own_skill", { payload: input }, z.void());
}

export async function listPersonnelSkillReferenceValues(): Promise<PersonnelSkillReferenceValue[]> {
  const rows = await invokeParsed(
    "list_personnel_skill_reference_values",
    undefined,
    z.array(PersonnelSkillReferenceValueSchema),
  );
  return rows as PersonnelSkillReferenceValue[];
}

export async function createPersonnelImportBatch(
  input: PersonnelImportCreateInput,
): Promise<PersonnelImportBatchSummary> {
  const batch = await invokeParsed(
    "create_personnel_import_batch",
    { input },
    PersonnelImportBatchSummarySchema,
  );
  return batch as PersonnelImportBatchSummary;
}

export async function getPersonnelImportPreview(batchId: number): Promise<PersonnelImportPreview> {
  const preview = await invokeParsed(
    "get_personnel_import_preview",
    { batch_id: batchId },
    PersonnelImportPreviewSchema,
  );
  return preview as PersonnelImportPreview;
}

export async function applyPersonnelImportBatch(batchId: number): Promise<PersonnelImportApplyResult> {
  const result = await invokeParsed(
    "apply_personnel_import_batch",
    { batch_id: batchId },
    PersonnelImportApplyResultSchema,
  );
  return result as PersonnelImportApplyResult;
}

export async function getWorkforceSummaryReport(): Promise<WorkforceSummaryReport> {
  const report = await invokeParsed(
    "get_workforce_summary_report",
    undefined,
    WorkforceSummaryReportSchema,
  );
  return report as WorkforceSummaryReport;
}

export async function getWorkforceSkillsGapReport(limit = 100): Promise<WorkforceSkillsGapRow[]> {
  const rows = await invokeParsed(
    "get_workforce_skills_gap_report",
    { limit },
    z.array(WorkforceSkillsGapRowSchema),
  );
  return rows as WorkforceSkillsGapRow[];
}

export async function getWorkforceKpiReport(): Promise<WorkforceKpiReport> {
  const kpis = await invokeParsed("get_workforce_kpi_report", undefined, WorkforceKpiReportSchema);
  return kpis as WorkforceKpiReport;
}

export async function exportWorkforceReportCsv(
  reportKind: "summary" | "skills_gap" | "kpi",
): Promise<string> {
  const csvPayload = await invokeParsed("export_workforce_report_csv", { report_kind: reportKind }, z.string());
  return csvPayload;
}

// Re-export schedule row types for consumers that need them without importing ipc-types
export type { ScheduleClass, ScheduleDetail, ScheduleClassWithDetails };
