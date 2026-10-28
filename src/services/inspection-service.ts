import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  GenerateInspectionRoundsInput,
  GenerateInspectionRoundsResult,
  InspectionAnomalyPage,
  InspectionAnomalyRow,
  InspectionAnomalyRowFilter,
  InspectionKpis,
  InspectionKpisInput,
  InspectionRoundPage,
  InspectionScheduleSummary,
  SaveInspectionScheduleInput,
  AddInspectionEvidenceInput,
  AddInspectionPhotoInput,
  InspectionRoundActionInput,
  InspectionRoundRunner,
  InspectionRoundSummary,
  InspectionRoundSummaryFilter,
  CreateInspectionTemplateInput,
  DeferInspectionAnomalyInput,
  InspectionAnomaliesFilter,
  InspectionAnomaly,
  InspectionCheckpoint,
  InspectionCheckpointsFilter,
  InspectionEvidence,
  InspectionEvidenceFilter,
  InspectionReliabilitySignal,
  InspectionReliabilitySignalsFilter,
  InspectionResult,
  InspectionResultsFilter,
  InspectionRound,
  InspectionTemplate,
  InspectionTemplateVersion,
  InspectionTemplateVersionsFilter,
  PublishInspectionTemplateVersionInput,
  RecordInspectionResultInput,
  RefreshInspectionReliabilitySignalsInput,
  RouteInspectionAnomalyToDiInput,
  RouteInspectionAnomalyToWoInput,
  ScheduleInspectionRoundInput,
} from "@shared/ipc-types";

const InspectionTemplateSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  code: z.string(),
  name: z.string(),
  org_scope_id: z.number().nullable(),
  route_scope: z.string().nullable(),
  estimated_duration_minutes: z.number().nullable(),
  is_active: z.boolean(),
  current_version_id: z.number().nullable(),
  row_version: z.number(),
});

const InspectionTemplateVersionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  template_id: z.number(),
  version_no: z.number(),
  effective_from: z.string().nullable(),
  checkpoint_package_json: z.string(),
  tolerance_rules_json: z.string().nullable(),
  escalation_rules_json: z.string().nullable(),
  requires_review: z.boolean(),
  row_version: z.number(),
});

const InspectionCheckpointSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  template_version_id: z.number(),
  sequence_order: z.number(),
  asset_id: z.number().nullable(),
  component_id: z.number().nullable(),
  checkpoint_code: z.string(),
  description: z.string().nullable(),
  check_type: z.string(),
  measurement_unit_id: z.number().nullable(),
  measurement_unit: z.string().nullable(),
  normal_min: z.number().nullable(),
  normal_max: z.number().nullable(),
  warning_min: z.number().nullable(),
  warning_max: z.number().nullable(),
  requires_photo: z.boolean(),
  requires_comment_on_exception: z.boolean(),
  row_version: z.number(),
});

const InspectionRoundSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  template_id: z.number(),
  template_version_id: z.number(),
  scheduled_at: z.string().nullable(),
  assigned_to_id: z.number().nullable(),
  status: z.string(),
  started_at: z.string().nullable(),
  completed_at: z.string().nullable(),
  reviewed_at: z.string().nullable(),
  reviewed_by_id: z.number().nullable(),
  review_note: z.string().nullable(),
  completion_percentage: z.number().nullable(),
  row_version: z.number(),
  schedule_id: z.number().nullable(),
  due_basis: z.string().nullable(),
  window_end_at: z.string().nullable(),
  assigned_team_id: z.number().nullable(),
  due_meter_value: z.number().nullable(),
  missed_at: z.string().nullable(),
});

export async function listInspectionTemplates(): Promise<InspectionTemplate[]> {
  const raw = await invoke<unknown>("list_inspection_templates");
  return z.array(InspectionTemplateSchema).parse(raw);
}

export async function listInspectionTemplateVersions(
  filter: InspectionTemplateVersionsFilter,
): Promise<InspectionTemplateVersion[]> {
  const raw = await invoke<unknown>("list_inspection_template_versions", { filter });
  return z.array(InspectionTemplateVersionSchema).parse(raw);
}

export async function listInspectionCheckpoints(
  filter: InspectionCheckpointsFilter,
): Promise<InspectionCheckpoint[]> {
  const raw = await invoke<unknown>("list_inspection_checkpoints", { filter });
  return z.array(InspectionCheckpointSchema).parse(raw);
}

export async function listInspectionRounds(): Promise<InspectionRound[]> {
  const raw = await invoke<unknown>("list_inspection_rounds");
  return z.array(InspectionRoundSchema).parse(raw);
}

export async function createInspectionTemplate(
  input: CreateInspectionTemplateInput,
): Promise<[InspectionTemplate, InspectionTemplateVersion, InspectionCheckpoint[]]> {
  const raw = await invoke<unknown>("create_inspection_template", { input });
  const tuple = z.tuple([
    InspectionTemplateSchema,
    InspectionTemplateVersionSchema,
    z.array(InspectionCheckpointSchema),
  ]);
  return tuple.parse(raw);
}

export async function publishInspectionTemplateVersion(
  input: PublishInspectionTemplateVersionInput,
): Promise<[InspectionTemplate, InspectionTemplateVersion, InspectionCheckpoint[]]> {
  const raw = await invoke<unknown>("publish_inspection_template_version", { input });
  const tuple = z.tuple([
    InspectionTemplateSchema,
    InspectionTemplateVersionSchema,
    z.array(InspectionCheckpointSchema),
  ]);
  return tuple.parse(raw);
}

export async function scheduleInspectionRound(
  input: ScheduleInspectionRoundInput,
): Promise<InspectionRound> {
  const raw = await invoke<unknown>("schedule_inspection_round", { input });
  return InspectionRoundSchema.parse(raw);
}

const InspectionResultSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  round_id: z.number(),
  checkpoint_id: z.number(),
  result_status: z.string(),
  numeric_value: z.number().nullable(),
  text_value: z.string().nullable(),
  boolean_value: z.boolean().nullable(),
  comment: z.string().nullable(),
  recorded_at: z.string(),
  recorded_by_id: z.number(),
  row_version: z.number(),
});

const InspectionEvidenceSchema = z.object({
  id: z.number(),
  result_id: z.number(),
  evidence_type: z.string(),
  file_path_or_value: z.string(),
  captured_at: z.string(),
  entity_sync_id: z.string(),
  row_version: z.number(),
});

const InspectionAnomalySchema = z.object({
  id: z.number(),
  round_id: z.number(),
  result_id: z.number().nullable(),
  anomaly_type: z.string(),
  severity: z.number(),
  description: z.string(),
  linked_di_id: z.number().nullable(),
  linked_work_order_id: z.number().nullable(),
  requires_permit_review: z.boolean(),
  resolution_status: z.string(),
  routing_decision: z.string().nullable(),
  entity_sync_id: z.string(),
  row_version: z.number(),
});

const InspectionRoundSummarySchema = z.object({
  round: InspectionRoundSchema,
  template_code: z.string(),
  template_name: z.string(),
  version_no: z.number(),
  requires_review: z.boolean(),
  assigned_to_name: z.string().nullable(),
  assigned_team_name: z.string().nullable(),
  schedule_code: z.string().nullable(),
  schedule_name: z.string().nullable(),
  is_overdue: z.boolean(),
  reviewed_by_name: z.string().nullable(),
  checkpoint_count: z.number(),
  recorded_count: z.number(),
  exception_count: z.number(),
  open_anomaly_count: z.number(),
  pending_sync_count: z.number(),
});

const InspectionRunnerCheckpointSchema = z.object({
  checkpoint: InspectionCheckpointSchema,
  asset_code: z.string().nullable(),
  asset_name: z.string().nullable(),
  result: InspectionResultSchema.nullable(),
  evidence: z.array(InspectionEvidenceSchema),
  open_anomalies: z.array(InspectionAnomalySchema),
  sync_state: z.enum(["not_recorded", "waiting", "synced", "error", "conflict"]),
  missing: z.array(z.string()),
  history: z.array(
    z.object({
      round_id: z.number(),
      round_scheduled_at: z.string().nullable(),
      recorded_at: z.string(),
      result_status: z.string(),
      numeric_value: z.number().nullable(),
      text_value: z.string().nullable(),
      boolean_value: z.boolean().nullable(),
      comment: z.string().nullable(),
    }),
  ),
});

const InspectionRoundRunnerSchema = z.object({
  summary: InspectionRoundSummarySchema,
  checkpoints: z.array(InspectionRunnerCheckpointSchema),
});

export async function listInspectionRoundSummaries(
  filter: InspectionRoundSummaryFilter,
): Promise<InspectionRoundSummary[]> {
  const raw = await invoke<unknown>("list_inspection_round_summaries", { filter });
  return z.array(InspectionRoundSummarySchema).parse(raw);
}

export async function getInspectionRoundRunner(roundId: number): Promise<InspectionRoundRunner> {
  const raw = await invoke<unknown>("get_inspection_round_runner", { roundId });
  return InspectionRoundRunnerSchema.parse(raw);
}

export async function startInspectionRound(
  input: InspectionRoundActionInput,
): Promise<InspectionRoundSummary> {
  const raw = await invoke<unknown>("start_inspection_round", { input });
  return InspectionRoundSummarySchema.parse(raw);
}

export async function completeInspectionRound(
  input: InspectionRoundActionInput,
): Promise<InspectionRoundSummary> {
  const raw = await invoke<unknown>("complete_inspection_round", { input });
  return InspectionRoundSummarySchema.parse(raw);
}

export async function reviewInspectionRound(
  input: InspectionRoundActionInput,
): Promise<InspectionRoundSummary> {
  const raw = await invoke<unknown>("review_inspection_round", { input });
  return InspectionRoundSummarySchema.parse(raw);
}

export async function addInspectionPhotoEvidence(
  input: AddInspectionPhotoInput,
): Promise<InspectionEvidence> {
  const raw = await invoke<unknown>("add_inspection_photo_evidence", { input });
  return InspectionEvidenceSchema.parse(raw);
}

export async function listInspectionResults(
  filter: InspectionResultsFilter,
): Promise<InspectionResult[]> {
  const raw = await invoke<unknown>("list_inspection_results", { filter });
  return z.array(InspectionResultSchema).parse(raw);
}

export async function listInspectionEvidence(
  filter: InspectionEvidenceFilter,
): Promise<InspectionEvidence[]> {
  const raw = await invoke<unknown>("list_inspection_evidence", { filter });
  return z.array(InspectionEvidenceSchema).parse(raw);
}

export async function listInspectionAnomalies(
  filter: InspectionAnomaliesFilter,
): Promise<InspectionAnomaly[]> {
  const raw = await invoke<unknown>("list_inspection_anomalies", { filter });
  return z.array(InspectionAnomalySchema).parse(raw);
}

export async function recordInspectionResult(
  input: RecordInspectionResultInput,
): Promise<InspectionResult> {
  const raw = await invoke<unknown>("record_inspection_result", { input });
  return InspectionResultSchema.parse(raw);
}

export async function addInspectionEvidence(
  input: AddInspectionEvidenceInput,
): Promise<InspectionEvidence> {
  const raw = await invoke<unknown>("add_inspection_evidence", { input });
  return InspectionEvidenceSchema.parse(raw);
}

/** The record created from an anomaly (request or work order), as far as the link needs it. */
const CreatedFollowUpSchema = z.object({ id: z.number(), code: z.string(), title: z.string() });
export type InspectionCreatedFollowUp = z.infer<typeof CreatedFollowUpSchema>;

export async function routeInspectionAnomalyToDi(
  input: RouteInspectionAnomalyToDiInput,
): Promise<[InspectionCreatedFollowUp, InspectionAnomaly]> {
  const raw = await invoke<unknown>("route_inspection_anomaly_to_di", { input });
  return z.tuple([CreatedFollowUpSchema, InspectionAnomalySchema]).parse(raw);
}

export async function routeInspectionAnomalyToWo(
  input: RouteInspectionAnomalyToWoInput,
): Promise<[InspectionCreatedFollowUp, InspectionAnomaly]> {
  const raw = await invoke<unknown>("route_inspection_anomaly_to_wo", { input });
  return z.tuple([CreatedFollowUpSchema, InspectionAnomalySchema]).parse(raw);
}

export async function deferInspectionAnomaly(
  input: DeferInspectionAnomalyInput,
): Promise<InspectionAnomaly> {
  const raw = await invoke<unknown>("defer_inspection_anomaly", { input });
  return InspectionAnomalySchema.parse(raw);
}

const InspectionReliabilitySignalSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  period_start: z.string(),
  period_end: z.string(),
  warning_count: z.number(),
  fail_count: z.number(),
  anomaly_open_count: z.number(),
  checkpoint_coverage_ratio: z.number(),
  row_version: z.number(),
  asset_code: z.string().nullable(),
  asset_name: z.string().nullable(),
});

export async function listInspectionReliabilitySignals(
  filter: InspectionReliabilitySignalsFilter,
): Promise<InspectionReliabilitySignal[]> {
  const raw = await invoke<unknown>("list_inspection_reliability_signals", { filter });
  return z.array(InspectionReliabilitySignalSchema).parse(raw);
}

export async function refreshInspectionReliabilitySignals(
  input: RefreshInspectionReliabilitySignalsInput,
): Promise<InspectionReliabilitySignal[]> {
  const raw = await invoke<unknown>("refresh_inspection_reliability_signals", { input });
  return z.array(InspectionReliabilitySignalSchema).parse(raw);
}

export async function listInspectionRoundPage(
  filter: InspectionRoundSummaryFilter,
): Promise<InspectionRoundPage> {
  const raw = await invoke<unknown>("list_inspection_round_page", { filter });
  return z.object({ items: z.array(InspectionRoundSummarySchema), total: z.number() }).parse(raw);
}

const InspectionKpisSchema = z.object({
  period_start: z.string(),
  period_end: z.string(),
  due_count: z.number(),
  done_count: z.number(),
  done_on_time_count: z.number(),
  missed_count: z.number(),
  overdue_count: z.number(),
  compliance_pct: z.number().nullable(),
  open_anomaly_count: z.number(),
});

export async function getInspectionKpis(input: InspectionKpisInput): Promise<InspectionKpis> {
  const raw = await invoke<unknown>("get_inspection_kpis", { input });
  return InspectionKpisSchema.parse(raw);
}

const InspectionAnomalyRowSchema = z.object({
  anomaly: InspectionAnomalySchema,
  round_status: z.string(),
  round_scheduled_at: z.string().nullable(),
  round_completed_at: z.string().nullable(),
  template_code: z.string(),
  template_name: z.string(),
  checkpoint_id: z.number().nullable(),
  checkpoint_code: z.string().nullable(),
  checkpoint_description: z.string().nullable(),
  check_type: z.string().nullable(),
  measurement_unit: z.string().nullable(),
  normal_min: z.number().nullable(),
  normal_max: z.number().nullable(),
  warning_min: z.number().nullable(),
  warning_max: z.number().nullable(),
  asset_id: z.number().nullable(),
  asset_code: z.string().nullable(),
  asset_name: z.string().nullable(),
  asset_has_location: z.boolean(),
  result_status: z.string().nullable(),
  numeric_value: z.number().nullable(),
  text_value: z.string().nullable(),
  result_comment: z.string().nullable(),
  recorded_at: z.string().nullable(),
  violation_direction: z.enum(["above", "below"]).nullable(),
  violation_limit: z.number().nullable(),
  linked_di_code: z.string().nullable(),
  linked_di_title: z.string().nullable(),
  linked_di_status: z.string().nullable(),
  linked_wo_code: z.string().nullable(),
  linked_wo_title: z.string().nullable(),
});

export async function listInspectionAnomalyRows(
  filter: InspectionAnomalyRowFilter,
): Promise<InspectionAnomalyPage> {
  const raw = await invoke<unknown>("list_inspection_anomaly_rows", { filter });
  return z.object({ items: z.array(InspectionAnomalyRowSchema), total: z.number() }).parse(raw);
}

export async function getInspectionAnomalyRow(anomalyId: number): Promise<InspectionAnomalyRow> {
  const raw = await invoke<unknown>("get_inspection_anomaly_row", { anomalyId });
  return InspectionAnomalyRowSchema.parse(raw);
}

const InspectionScheduleSummarySchema = z.object({
  schedule: z.object({
    id: z.number(),
    entity_sync_id: z.string(),
    code: z.string(),
    name: z.string(),
    template_id: z.number(),
    trigger_kind: z.string(),
    interval_unit: z.string().nullable(),
    interval_value: z.number().nullable(),
    anchor_at: z.string().nullable(),
    meter_id: z.number().nullable(),
    meter_interval: z.number().nullable(),
    meter_start_reading: z.number().nullable(),
    assigned_to_id: z.number().nullable(),
    assigned_team_id: z.number().nullable(),
    grace_hours: z.number(),
    horizon_days: z.number(),
    is_active: z.boolean(),
    last_generated_at: z.string().nullable(),
    row_version: z.number(),
    created_at: z.string(),
    updated_at: z.string(),
  }),
  template_code: z.string(),
  template_name: z.string(),
  assigned_to_name: z.string().nullable(),
  assigned_team_name: z.string().nullable(),
  meter_name: z.string().nullable(),
  meter_unit: z.string().nullable(),
  meter_asset_id: z.number().nullable(),
  meter_asset_code: z.string().nullable(),
  meter_lifetime_reading: z.number().nullable(),
  next_due_at: z.string().nullable(),
  open_round_count: z.number(),
  overdue_round_count: z.number(),
  missed_round_count: z.number(),
});

export async function listInspectionSchedules(): Promise<InspectionScheduleSummary[]> {
  const raw = await invoke<unknown>("list_inspection_schedules");
  return z.array(InspectionScheduleSummarySchema).parse(raw);
}

export async function saveInspectionSchedule(
  input: SaveInspectionScheduleInput,
): Promise<InspectionScheduleSummary> {
  const raw = await invoke<unknown>("save_inspection_schedule", { input });
  return InspectionScheduleSummarySchema.parse(raw);
}

export async function generateInspectionRounds(
  input: GenerateInspectionRoundsInput,
): Promise<GenerateInspectionRoundsResult> {
  const raw = await invoke<unknown>("generate_inspection_rounds", { input });
  return z
    .object({
      generated: z.number(),
      skipped: z.number(),
      missed: z.number(),
      round_ids: z.array(z.number()),
      issues: z.array(z.object({ schedule_code: z.string(), message: z.string() })),
    })
    .parse(raw);
}
