import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  AddInspectionEvidenceInput,
  CreateInspectionTemplateInput,
  DeferInspectionAnomalyInput,
  EnqueueInspectionOfflineInput,
  InspectionAnomaliesFilter,
  InspectionAnomaly,
  InspectionCheckpoint,
  InspectionCheckpointsFilter,
  InspectionEvidence,
  InspectionEvidenceFilter,
  InspectionOfflineQueueItem,
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
  UpdateInspectionAnomalyInput,
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
  check_type: z.string(),
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
  row_version: z.number(),
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

const InspectionOfflineQueueItemSchema = z.object({
  id: z.number(),
  payload_json: z.string(),
  local_temp_id: z.string(),
  sync_status: z.string(),
});

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

export async function updateInspectionAnomaly(
  input: UpdateInspectionAnomalyInput,
): Promise<InspectionAnomaly> {
  const raw = await invoke<unknown>("update_inspection_anomaly", { input });
  return InspectionAnomalySchema.parse(raw);
}

export async function enqueueInspectionOffline(
  input: EnqueueInspectionOfflineInput,
): Promise<number> {
  const raw = await invoke<unknown>("enqueue_inspection_offline", { input });
  return z.number().parse(raw);
}

export async function listInspectionOfflineQueue(): Promise<InspectionOfflineQueueItem[]> {
  const raw = await invoke<unknown>("list_inspection_offline_queue");
  return z.array(InspectionOfflineQueueItemSchema).parse(raw);
}

export async function markInspectionOfflineSynced(queueId: number): Promise<void> {
  await invoke("mark_inspection_offline_synced", { queueId });
}

export async function routeInspectionAnomalyToDi(
  input: RouteInspectionAnomalyToDiInput,
): Promise<[unknown, InspectionAnomaly]> {
  const raw = await invoke<unknown>("route_inspection_anomaly_to_di", { input });
  return z.tuple([z.unknown(), InspectionAnomalySchema]).parse(raw);
}

export async function routeInspectionAnomalyToWo(
  input: RouteInspectionAnomalyToWoInput,
): Promise<[unknown, InspectionAnomaly]> {
  const raw = await invoke<unknown>("route_inspection_anomaly_to_wo", { input });
  return z.tuple([z.unknown(), InspectionAnomalySchema]).parse(raw);
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
