/**
 * Workforce capacity (shift calendars − absences − holidays), the plant holiday calendar and
 * skills recorded by managers (Rust: `personnel::capacity`, `personnel::calendar`,
 * `personnel::skills`).
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

// ── Capacity ────────────────────────────────────────────────────────────────

export const PERSON_DAY_REASONS = [
  "working",
  "absent",
  "rest_day",
  "holiday",
  "no_schedule",
  "not_employed",
] as const;
export type PersonDayReason = (typeof PERSON_DAY_REASONS)[number];

const PersonDayCapacitySchema = z.object({
  personnel_id: z.number(),
  day: z.string(),
  shift_start: z.string().nullable(),
  shift_end: z.string().nullable(),
  scheduled_minutes: z.number(),
  absent_minutes: z.number(),
  available_minutes: z.number(),
  reason: z.enum(PERSON_DAY_REASONS),
  holiday_label: z.string().nullable(),
  absence_types: z.array(z.string()),
});
export type PersonDayCapacity = z.infer<typeof PersonDayCapacitySchema>;

export const TEAM_CAPACITY_BASES = ["roster", "holiday", "no_schedule", "no_members"] as const;

const TeamDayCapacitySchema = z.object({
  team_id: z.number(),
  day: z.string(),
  available_hours: z.number(),
  scheduled_hours: z.number(),
  absent_hours: z.number(),
  people: z.number(),
  people_scheduled: z.number(),
  people_absent: z.number(),
  basis: z.enum(TEAM_CAPACITY_BASES),
  rule_available_hours: z.number().nullable(),
  rule_overtime_hours: z.number().nullable(),
  members: z.array(
    z.object({
      personnel_id: z.number(),
      full_name: z.string(),
      allocation_percent: z.number(),
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
  label: string;
  entity_id?: number | null;
  is_active?: boolean | null;
}

export async function upsertWorkCalendarHoliday(
  input: WorkCalendarHolidayUpsertInput,
): Promise<WorkCalendarHoliday> {
  const raw = await invoke<unknown>("upsert_work_calendar_holiday", { input });
  return WorkCalendarHolidaySchema.parse(raw);
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
