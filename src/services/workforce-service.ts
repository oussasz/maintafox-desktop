/**
 * Workforce capacity (Classes horaires on the site's working days − absences, × utilisation),
 * timetables (Classe horaire versions, team links, person exceptions), the site calendar
 * (holidays, weekly rest days) and skills recorded by managers (Rust: `personnel::capacity`,
 * `personnel::timetable`, `personnel::calendar`, `personnel::skills`).
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

// ── Capacity ────────────────────────────────────────────────────────────────

export const PERSON_DAY_REASONS = [
  "working",
  "absent",
  "rest_day",
  "site_rest_day",
  "holiday",
  "no_schedule",
  "timetable_not_set_up",
  "not_employed",
] as const;
export type PersonDayReason = (typeof PERSON_DAY_REASONS)[number];

export const SCHEDULE_SOURCES = ["exception", "personal", "team"] as const;

const PersonDayCapacitySchema = z.object({
  personnel_id: z.number(),
  day: z.string(),
  schedule_class_id: z.number().nullable(),
  schedule_class_label: z.string().nullable(),
  schedule_source: z.enum(SCHEDULE_SOURCES).nullable(),
  shift_start: z.string().nullable(),
  shift_end: z.string().nullable(),
  break_minutes: z.number(),
  work_percent: z.number(),
  scheduled_minutes: z.number(),
  absent_minutes: z.number(),
  available_minutes: z.number(),
  utilisation_percent: z.number().nullable(),
  plannable_minutes: z.number(),
  reason: z.enum(PERSON_DAY_REASONS),
  holiday_label: z.string().nullable(),
  absence_types: z.array(z.string()),
});
export type PersonDayCapacity = z.infer<typeof PersonDayCapacitySchema>;

export const TEAM_CAPACITY_BASES = [
  "roster",
  "rest_day",
  "holiday",
  "no_schedule",
  "no_members",
] as const;

const TeamDayCapacitySchema = z.object({
  team_id: z.number(),
  day: z.string(),
  schedule_class_id: z.number().nullable(),
  schedule_class_label: z.string().nullable(),
  available_hours: z.number(),
  attendance_hours: z.number(),
  scheduled_hours: z.number(),
  absent_hours: z.number(),
  people: z.number(),
  people_scheduled: z.number(),
  people_absent: z.number(),
  basis: z.enum(TEAM_CAPACITY_BASES),
  rule_overtime_hours: z.number().nullable(),
  members: z.array(
    z.object({
      personnel_id: z.number(),
      full_name: z.string(),
      allocation_percent: z.number(),
      schedule_class_label: z.string().nullable(),
      schedule_source: z.enum(SCHEDULE_SOURCES).nullable(),
      available_hours: z.number(),
      reason: z.enum(PERSON_DAY_REASONS),
    }),
  ),
});
export type TeamDayCapacity = z.infer<typeof TeamDayCapacitySchema>;

/** Longest range the backend computes in one call. */
export const CAPACITY_MAX_RANGE_DAYS = 92;

export async function getTeamCapacity(
  teamId: number,
  dateFrom: string,
  dateTo: string,
): Promise<TeamDayCapacity[]> {
  const raw = await invoke<unknown>("get_team_capacity", { teamId, dateFrom, dateTo });
  return z.array(TeamDayCapacitySchema).parse(raw);
}

export async function getPersonCapacity(
  personnelId: number,
  dateFrom: string,
  dateTo: string,
): Promise<PersonDayCapacity[]> {
  const raw = await invoke<unknown>("get_person_capacity", { personnelId, dateFrom, dateTo });
  return z.array(PersonDayCapacitySchema).parse(raw);
}

// ── Holidays ────────────────────────────────────────────────────────────────

const WorkCalendarHolidaySchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  holiday_date: z.string(),
  label: z.string(),
  label_en: z.string().nullable(),
  entity_id: z.number().nullable(),
  entity_name: z.string().nullable(),
  is_active: z.boolean(),
  row_version: z.number(),
});
export type WorkCalendarHoliday = z.infer<typeof WorkCalendarHolidaySchema>;

export async function listWorkCalendarHolidays(
  filter: { date_from?: string | null; date_to?: string | null; include_inactive?: boolean } = {},
): Promise<WorkCalendarHoliday[]> {
  const raw = await invoke<unknown>("list_work_calendar_holidays", { filter });
  return z.array(WorkCalendarHolidaySchema).parse(raw);
}

export interface WorkCalendarHolidayUpsertInput {
  id?: number | null;
  expected_row_version?: number | null;
  holiday_date: string;
  /** French name. */
  label: string;
  /** English name. */
  label_en?: string | null;
  entity_id?: number | null;
  is_active?: boolean | null;
}

export async function upsertWorkCalendarHoliday(
  input: WorkCalendarHolidayUpsertInput,
): Promise<WorkCalendarHoliday> {
  const raw = await invoke<unknown>("upsert_work_calendar_holiday", { input });
  return WorkCalendarHolidaySchema.parse(raw);
}

// ── Site working week ───────────────────────────────────────────────────────

const SiteWorkWeekSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  entity_id: z.number().nullable(),
  entity_name: z.string().nullable(),
  effective_from: z.string(),
  /** ISO weekdays, 1 = Monday … 7 = Sunday. */
  weekend_days: z.array(z.number()),
  row_version: z.number(),
});
export type SiteWorkWeek = z.infer<typeof SiteWorkWeekSchema>;

export async function listSiteWorkWeeks(): Promise<SiteWorkWeek[]> {
  const raw = await invoke<unknown>("list_site_work_weeks");
  return z.array(SiteWorkWeekSchema).parse(raw);
}

export interface SiteWorkWeekInput {
  entity_id: number | null;
  effective_from: string;
  weekend_days: number[];
}

export async function setSiteWorkWeek(input: SiteWorkWeekInput): Promise<SiteWorkWeek[]> {
  const raw = await invoke<unknown>("set_site_work_week", { input });
  return z.array(SiteWorkWeekSchema).parse(raw);
}

// ── Classes horaires (timetables) ───────────────────────────────────────────

const ScheduleCycleDaySchema = z.object({
  cycle_day: z.number(),
  shift_start: z.string(),
  shift_end: z.string(),
  break_minutes: z.number(),
  is_rest_day: z.boolean(),
});
export type ScheduleCycleDay = z.infer<typeof ScheduleCycleDaySchema>;

const ScheduleClassVersionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  reference_value_id: z.number(),
  class_code: z.string(),
  class_label: z.string(),
  is_continuous: z.boolean(),
  effective_from: z.string(),
  cycle_length_days: z.number(),
  rotation_start_date: z.string().nullable(),
  days: z.array(ScheduleCycleDaySchema),
  utilisation_percent: z.number(),
  review_status: z.string().nullable(),
  note: z.string().nullable(),
  row_version: z.number(),
  working_days_per_cycle: z.number(),
  net_hours_per_cycle: z.number(),
});
export type ScheduleClassVersion = z.infer<typeof ScheduleClassVersionSchema>;

export async function listScheduleClassVersions(
  referenceValueId: number | null,
): Promise<ScheduleClassVersion[]> {
  const raw = await invoke<unknown>("list_schedule_class_versions", { referenceValueId });
  return z.array(ScheduleClassVersionSchema).parse(raw);
}

export interface ScheduleClassVersionInput {
  id?: number | null;
  expected_row_version?: number | null;
  reference_value_id: number;
  effective_from: string;
  cycle_length_days: number;
  rotation_start_date: string | null;
  days: ScheduleCycleDay[];
  is_continuous: boolean;
  utilisation_percent: number;
  note?: string | null;
}

export async function upsertScheduleClassVersion(
  input: ScheduleClassVersionInput,
): Promise<ScheduleClassVersion> {
  const raw = await invoke<unknown>("upsert_schedule_class_version", { input });
  return ScheduleClassVersionSchema.parse(raw);
}

export async function confirmScheduleClassReview(
  referenceValueId: number,
): Promise<ScheduleClassVersion[]> {
  const raw = await invoke<unknown>("confirm_schedule_class_review", { referenceValueId });
  return z.array(ScheduleClassVersionSchema).parse(raw);
}

// ── A team's Classe horaire ─────────────────────────────────────────────────

const TeamScheduleAssignmentSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  team_id: z.number(),
  schedule_reference_value_id: z.number().nullable(),
  schedule_code: z.string().nullable(),
  schedule_label: z.string().nullable(),
  effective_from: z.string(),
  review_status: z.string().nullable(),
  row_version: z.number(),
});
export type TeamScheduleAssignment = z.infer<typeof TeamScheduleAssignmentSchema>;

export async function listTeamSchedule(teamId: number): Promise<TeamScheduleAssignment[]> {
  const raw = await invoke<unknown>("list_team_schedule", { teamId });
  return z.array(TeamScheduleAssignmentSchema).parse(raw);
}

export interface TeamScheduleAssignmentInput {
  team_id: number;
  schedule_reference_value_id: number | null;
  effective_from: string;
}

export async function setTeamSchedule(
  input: TeamScheduleAssignmentInput,
): Promise<TeamScheduleAssignment[]> {
  const raw = await invoke<unknown>("set_team_schedule", { input });
  return z.array(TeamScheduleAssignmentSchema).parse(raw);
}

export async function confirmTeamScheduleReview(teamId: number): Promise<TeamScheduleAssignment[]> {
  const raw = await invoke<unknown>("confirm_team_schedule_review", { teamId });
  return z.array(TeamScheduleAssignmentSchema).parse(raw);
}

// ── A person's timetable exceptions ─────────────────────────────────────────

const PersonnelScheduleExceptionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  personnel_id: z.number(),
  schedule_reference_value_id: z.number().nullable(),
  schedule_code: z.string().nullable(),
  schedule_label: z.string().nullable(),
  work_percent: z.number().nullable(),
  valid_from: z.string(),
  valid_to: z.string().nullable(),
  /** Hidden without `per.sensitiveview`. */
  reason: z.string().nullable(),
  row_version: z.number(),
});
export type PersonnelScheduleException = z.infer<typeof PersonnelScheduleExceptionSchema>;

export async function listPersonnelScheduleExceptions(
  personnelId: number,
): Promise<PersonnelScheduleException[]> {
  const raw = await invoke<unknown>("list_personnel_schedule_exceptions", { personnelId });
  return z.array(PersonnelScheduleExceptionSchema).parse(raw);
}

export interface PersonnelScheduleExceptionInput {
  id?: number | null;
  expected_row_version?: number | null;
  personnel_id: number;
  schedule_reference_value_id: number | null;
  work_percent: number | null;
  valid_from: string;
  valid_to: string | null;
  reason?: string | null;
}

export async function upsertPersonnelScheduleException(
  input: PersonnelScheduleExceptionInput,
): Promise<PersonnelScheduleException> {
  const raw = await invoke<unknown>("upsert_personnel_schedule_exception", { input });
  return PersonnelScheduleExceptionSchema.parse(raw);
}

export async function deletePersonnelScheduleException(id: number): Promise<void> {
  await invoke<unknown>("delete_personnel_schedule_exception", { id });
}

// ── Skills on a person's sheet ──────────────────────────────────────────────

const PersonnelSkillRowSchema = z.object({
  id: z.number(),
  personnel_id: z.number(),
  reference_value_id: z.number(),
  skill_code: z.string(),
  skill_label: z.string(),
  proficiency_level: z.number(),
  valid_from: z.string().nullable(),
  valid_to: z.string().nullable(),
  source_type: z.string(),
  is_primary: z.boolean(),
  is_validated: z.boolean(),
  verified_by_name: z.string().nullable(),
  verified_at: z.string().nullable(),
});
export type PersonnelSkillRow = z.infer<typeof PersonnelSkillRowSchema>;

export async function listPersonnelSkills(personnelId: number): Promise<PersonnelSkillRow[]> {
  const raw = await invoke<unknown>("list_personnel_skills", { personnelId });
  return z.array(PersonnelSkillRowSchema).parse(raw);
}

export interface PersonnelSkillUpsertInput {
  personnel_id: number;
  reference_value_id: number;
  proficiency_level: number;
  valid_to?: string | null;
  is_primary?: boolean;
  validate?: boolean;
}

export async function upsertPersonnelSkill(
  input: PersonnelSkillUpsertInput,
): Promise<PersonnelSkillRow[]> {
  const raw = await invoke<unknown>("upsert_personnel_skill", { input });
  return z.array(PersonnelSkillRowSchema).parse(raw);
}

export async function removePersonnelSkill(
  personnelId: number,
  referenceValueId: number,
): Promise<PersonnelSkillRow[]> {
  const raw = await invoke<unknown>("remove_personnel_skill", { personnelId, referenceValueId });
  return z.array(PersonnelSkillRowSchema).parse(raw);
}

// ── Contractor companies ────────────────────────────────────────────────────

export const ONBOARDING_STATUSES = ["pending", "active", "suspended", "expired"] as const;
export const INSURANCE_STATUSES = ["unknown", "valid", "expired", "not_required"] as const;

const ExternalCompanyRecordSchema = z.object({
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

export interface ExternalCompanyUpdateInput {
  id: number;
  name: string;
  service_domain?: string | null;
  contract_start?: string | null;
  contract_end?: string | null;
  onboarding_status: string;
  insurance_status: string;
  notes?: string | null;
  is_active: boolean;
}

export async function updateExternalCompany(
  input: ExternalCompanyUpdateInput,
): Promise<z.infer<typeof ExternalCompanyRecordSchema>> {
  const raw = await invoke<unknown>("update_external_company", { input });
  return ExternalCompanyRecordSchema.parse(raw);
}
