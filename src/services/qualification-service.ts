import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  CertificationExpiryDrilldownRow,
  CertificationType,
  CertificationTypeUpsertInput,
  CrewPermitSkillGapInput,
  CrewPermitSkillGapResult,
  DocumentAcknowledgement,
  DocumentAcknowledgementListFilter,
  DocumentAcknowledgementUpsertInput,
  PersonnelCertification,
  PersonnelCertificationListFilter,
  PersonnelCertificationUpsertInput,
  PersonnelReadinessFilter,
  PersonnelReadinessRow,
  PersonnelReadinessSnapshot,
  PersonnelReadinessSnapshotUpsertInput,
  QualificationRequirementProfile,
  QualificationRequirementProfileUpsertInput,
  TrainingAttendance,
  TrainingAttendanceListFilter,
  TrainingAttendanceUpsertInput,
  TrainingExpiryAlertEvent,
  TrainingExpiryAlertEventListFilter,
  TrainingSession,
  TrainingSessionUpsertInput,
} from "@shared/ipc-types";

const CertificationTypeSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  code: z.string(),
  name: z.string(),
  default_validity_months: z.number().nullable(),
  renewal_lead_days: z.number().nullable(),
  row_version: z.number(),
});

const PersonnelCertificationSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  personnel_id: z.number(),
  certification_type_id: z.number(),
  issued_at: z.string().nullable(),
  expires_at: z.string().nullable(),
  issuing_body: z.string().nullable(),
  certificate_ref: z.string().nullable(),
  verification_status: z.string(),
  row_version: z.number(),
  readiness_status: z.string(),
  certification_type_code: z.string().nullable(),
  certification_type_name: z.string().nullable(),
});

const QualificationRequirementProfileSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  profile_name: z.string(),
  required_certification_type_ids_json: z.string(),
  applies_to_permit_type_codes_json: z.string(),
  row_version: z.number(),
});

const TrainingSessionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  course_code: z.string(),
  scheduled_start: z.string(),
  scheduled_end: z.string(),
  location: z.string().nullable(),
  instructor_id: z.number().nullable(),
  certification_type_id: z.number().nullable(),
  min_pass_score: z.number(),
  row_version: z.number(),
});

const TrainingAttendanceSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  session_id: z.number(),
  personnel_id: z.number(),
  attendance_status: z.string(),
  completed_at: z.string().nullable(),
  score: z.number().nullable(),
  row_version: z.number(),
});

const DocumentAcknowledgementSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  personnel_id: z.number(),
  document_version_id: z.number(),
  acknowledged_at: z.string(),
  row_version: z.number(),
});

export async function listCertificationTypes(): Promise<CertificationType[]> {
  const raw = await invoke<unknown>("list_certification_types");
  return z.array(CertificationTypeSchema).parse(raw);
}

export async function upsertCertificationType(
  input: CertificationTypeUpsertInput,
): Promise<CertificationType> {
  const raw = await invoke<unknown>("upsert_certification_type", { input });
  return CertificationTypeSchema.parse(raw);
}

export async function listQualificationRequirementProfiles(): Promise<
  QualificationRequirementProfile[]
> {
  const raw = await invoke<unknown>("list_qualification_requirement_profiles");
  return z.array(QualificationRequirementProfileSchema).parse(raw);
}

export async function upsertQualificationRequirementProfile(
  input: QualificationRequirementProfileUpsertInput,
): Promise<QualificationRequirementProfile> {
  const raw = await invoke<unknown>("upsert_qualification_requirement_profile", { input });
  return QualificationRequirementProfileSchema.parse(raw);
}

export async function listPersonnelCertifications(
  filter: PersonnelCertificationListFilter,
): Promise<PersonnelCertification[]> {
  const raw = await invoke<unknown>("list_personnel_certifications", { filter });
  return z.array(PersonnelCertificationSchema).parse(raw);
}

export async function upsertPersonnelCertification(
  input: PersonnelCertificationUpsertInput,
): Promise<PersonnelCertification> {
  const raw = await invoke<unknown>("upsert_personnel_certification", { input });
  return PersonnelCertificationSchema.parse(raw);
}

export async function listTrainingSessions(): Promise<TrainingSession[]> {
  const raw = await invoke<unknown>("list_training_sessions");
  return z.array(TrainingSessionSchema).parse(raw);
}

export async function upsertTrainingSession(
  input: TrainingSessionUpsertInput,
): Promise<TrainingSession> {
  const raw = await invoke<unknown>("upsert_training_session", { input });
  return TrainingSessionSchema.parse(raw);
}

export async function listTrainingAttendance(
  filter: TrainingAttendanceListFilter,
): Promise<TrainingAttendance[]> {
  const raw = await invoke<unknown>("list_training_attendance", { filter });
  return z.array(TrainingAttendanceSchema).parse(raw);
}

export async function upsertTrainingAttendance(
  input: TrainingAttendanceUpsertInput,
): Promise<TrainingAttendance> {
  const raw = await invoke<unknown>("upsert_training_attendance", { input });
  return TrainingAttendanceSchema.parse(raw);
}

export async function listDocumentAcknowledgements(
  filter: DocumentAcknowledgementListFilter,
): Promise<DocumentAcknowledgement[]> {
  const raw = await invoke<unknown>("list_document_acknowledgements", { filter });
  return z.array(DocumentAcknowledgementSchema).parse(raw);
}

export async function upsertDocumentAcknowledgement(
  input: DocumentAcknowledgementUpsertInput,
): Promise<DocumentAcknowledgement> {
  const raw = await invoke<unknown>("upsert_document_acknowledgement", { input });
  return DocumentAcknowledgementSchema.parse(raw);
}

export async function listMyTrainingSessions(): Promise<TrainingAttendance[]> {
  const raw = await invoke<unknown>("list_my_training_sessions");
  return z.array(TrainingAttendanceSchema).parse(raw);
}

export async function listMyPersonnelCertifications(): Promise<PersonnelCertification[]> {
  const raw = await invoke<unknown>("list_my_personnel_certifications");
  return z.array(PersonnelCertificationSchema).parse(raw);
}

const PersonnelReadinessRowSchema = z.object({
  personnel_id: z.number(),
  permit_type_code: z.string(),
  is_qualified: z.boolean(),
  blocking_reason: z.string().nullable(),
  expires_at: z.string().nullable(),
});

const CrewPermitSkillGapResultSchema = z.object({
  permit_type_code: z.string(),
  work_order_id: z.number(),
  rows: z.array(
    z.object({
      personnel_id: z.number(),
      is_qualified: z.boolean(),
      blocking_reason: z.string().nullable(),
      missing_certification_type_ids: z.array(z.number()),
      expires_at: z.string().nullable(),
    }),
  ),
});

const PersonnelReadinessSnapshotSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  period: z.string(),
  payload_json: z.string(),
  row_version: z.number(),
  created_at: z.string(),
});

export async function listPersonnelReadiness(
  filter: PersonnelReadinessFilter,
): Promise<PersonnelReadinessRow[]> {
  const raw = await invoke<unknown>("list_personnel_readiness", { filter });
  return z.array(PersonnelReadinessRowSchema).parse(raw);
}

export async function evaluateCrewPermitSkillGaps(
  input: CrewPermitSkillGapInput,
): Promise<CrewPermitSkillGapResult> {
  const raw = await invoke<unknown>("evaluate_crew_permit_skill_gaps", { input });
  return CrewPermitSkillGapResultSchema.parse(raw);
}

export async function listPersonnelReadinessSnapshots(): Promise<PersonnelReadinessSnapshot[]> {
  const raw = await invoke<unknown>("list_personnel_readiness_snapshots");
  return z.array(PersonnelReadinessSnapshotSchema).parse(raw);
}

export async function upsertPersonnelReadinessSnapshot(
  input: PersonnelReadinessSnapshotUpsertInput,
): Promise<PersonnelReadinessSnapshot> {
  const raw = await invoke<unknown>("upsert_personnel_readiness_snapshot", { input });
  return PersonnelReadinessSnapshotSchema.parse(raw);
}

export async function refreshPersonnelReadinessSnapshot(
  period: string,
): Promise<PersonnelReadinessSnapshot> {
  const raw = await invoke<unknown>("refresh_personnel_readiness_snapshot", { period });
  return PersonnelReadinessSnapshotSchema.parse(raw);
}

const TrainingExpiryAlertEventSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  certification_id: z.number(),
  alert_dedupe_key: z.string(),
  fired_at: z.string(),
  severity: z.string(),
  row_version: z.number(),
});

const CertificationExpiryDrilldownRowSchema = z.object({
  certification_id: z.number(),
  personnel_id: z.number(),
  employee_code: z.string(),
  full_name: z.string(),
  primary_entity_id: z.number().nullable(),
  certification_type_id: z.number(),
  certification_type_code: z.string(),
  expires_at: z.string().nullable(),
  verification_status: z.string(),
  readiness_status: z.string(),
});

export async function listTrainingExpiryAlertEvents(
  filter: TrainingExpiryAlertEventListFilter,
): Promise<TrainingExpiryAlertEvent[]> {
  const raw = await invoke<unknown>("list_training_expiry_alert_events", { filter });
  return z.array(TrainingExpiryAlertEventSchema).parse(raw);
}

export async function scanTrainingExpiryAlerts(
  lookaheadDays?: number | null,
): Promise<TrainingExpiryAlertEvent[]> {
  const raw = await invoke<unknown>("scan_training_expiry_alerts", {
    lookaheadDays: lookaheadDays ?? null,
  });
  return z.array(TrainingExpiryAlertEventSchema).parse(raw);
}

export async function listCertificationExpiryDrilldown(
  entityId: number | null | undefined,
  lookaheadDays?: number | null,
): Promise<CertificationExpiryDrilldownRow[]> {
  const raw = await invoke<unknown>("list_certification_expiry_drilldown", {
    entityId: entityId ?? null,
    lookaheadDays: lookaheadDays ?? null,
  });
  return z.array(CertificationExpiryDrilldownRowSchema).parse(raw);
}
