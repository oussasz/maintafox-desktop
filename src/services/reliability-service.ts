import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  CostOfFailureFilter,
  CostOfFailureRow,
  DeactivateFailureCodeInput,
  FailureCode,
  FailureCodeUpsertInput,
  FailureCodesFilter,
  FailureHierarchy,
  FailureHierarchyUpsertInput,
  FailureEvent,
  FailureEventsFilter,
  RefreshReliabilityKpiSnapshotInput,
  ComputationJob,
  ReliabilityAnalysisInputEvaluation,
  ReliabilityKpiSnapshot,
  ReliabilityKpiSnapshotsFilter,
  RuntimeExposureLog,
  RuntimeExposureLogsFilter,
  UpsertFailureEventInput,
  UpsertRuntimeExposureLogInput,
  RamDataQualityIssue,
  RamDataQualityIssuesFilter,
  WoMissingFailureModeRow,
  EquipmentMissingExposureRow,
  RamEquipmentQualityBadge,
  DismissRamDataQualityIssueInput,
  UserDismissal,
  WeibullFitRunInput,
  WeibullFitRecord,
  FmecaAnalysis,
  CreateFmecaAnalysisInput,
  UpdateFmecaAnalysisInput,
  FmecaAnalysesFilter,
  FmecaItem,
  FmecaItemWithContext,
  FmecaItemsEquipmentFilter,
  FmecaSeverityOccurrenceMatrix,
  Iso14224DatasetCompleteness,
  RamIshikawaDiagram,
  RamIshikawaDiagramsFilter,
  ReliabilityRulIndicator,
  UpsertFmecaItemInput,
  UpsertRamIshikawaDiagramInput,
  RcmStudy,
  CreateRcmStudyInput,
  UpdateRcmStudyInput,
  RcmStudiesFilter,
  RcmDecision,
  UpsertRcmDecisionInput,
  FtaModel,
  CreateFtaModelInput,
  UpdateFtaModelInput,
  FtaModelsFilter,
  RbdModel,
  CreateRbdModelInput,
  UpdateRbdModelInput,
  RbdModelsFilter,
  EventTreeModel,
  CreateEventTreeModelInput,
  UpdateEventTreeModelInput,
  EventTreeModelsFilter,
  RamAdvancedGuardrailFlags,
  McModel,
  CreateMcModelInput,
  UpdateMcModelInput,
  McModelsFilter,
  MarkovModel,
  CreateMarkovModelInput,
  UpdateMarkovModelInput,
  MarkovModelsFilter,
  RamExpertSignOff,
  CreateRamExpertSignOffInput,
  UpdateRamExpertSignOffInput,
  SignRamExpertReviewInput,
  RamExpertSignOffsFilter,
} from "@shared/ipc-types";

const FailureHierarchySchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  name: z.string(),
  asset_scope_json: z.string(),
  version_no: z.number(),
  is_active: z.boolean(),
  row_version: z.number(),
});

const FailureCodeSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  hierarchy_id: z.number(),
  parent_id: z.number().nullable(),
  code: z.string(),
  label: z.string(),
  code_type: z.string(),
  iso_14224_annex_ref: z.string().nullable(),
  is_active: z.boolean(),
  row_version: z.number(),
});

export async function listFailureHierarchies(): Promise<FailureHierarchy[]> {
  const raw = await invoke<unknown>("list_failure_hierarchies");
  return z.array(FailureHierarchySchema).parse(raw);
}

export async function upsertFailureHierarchy(
  input: FailureHierarchyUpsertInput,
): Promise<FailureHierarchy> {
  const raw = await invoke<unknown>("upsert_failure_hierarchy", { input });
  return FailureHierarchySchema.parse(raw);
}

export async function listFailureCodes(filter: FailureCodesFilter): Promise<FailureCode[]> {
  const raw = await invoke<unknown>("list_failure_codes", { filter });
  return z.array(FailureCodeSchema).parse(raw);
}

export async function upsertFailureCode(input: FailureCodeUpsertInput): Promise<FailureCode> {
  const raw = await invoke<unknown>("upsert_failure_code", { input });
  return FailureCodeSchema.parse(raw);
}

export async function deactivateFailureCode(
  input: DeactivateFailureCodeInput,
): Promise<FailureCode> {
  const raw = await invoke<unknown>("deactivate_failure_code", { input });
  return FailureCodeSchema.parse(raw);
}

const CostOfFailureRowSchema = z.object({
  equipment_id: z.number(),
  period: z.string(),
  total_downtime_cost: z.number(),
  total_corrective_cost: z.number(),
  currency_code: z.string(),
});

export async function listCostOfFailure(filter: CostOfFailureFilter): Promise<CostOfFailureRow[]> {
  const raw = await invoke<unknown>("list_cost_of_failure", { filter });
  return z.array(CostOfFailureRowSchema).parse(raw);
}

const FailureEventSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  source_type: z.string(),
  source_id: z.number(),
  equipment_id: z.number(),
  component_id: z.number().nullable(),
  detected_at: z.string().nullable(),
  failed_at: z.string().nullable(),
  restored_at: z.string().nullable(),
  downtime_duration_hours: z.number(),
  active_repair_hours: z.number(),
  waiting_hours: z.number(),
  is_planned: z.boolean(),
  failure_class_id: z.number().nullable(),
  failure_mode_id: z.number().nullable(),
  failure_cause_id: z.number().nullable(),
  failure_effect_id: z.number().nullable(),
  failure_mechanism_id: z.number().nullable(),
  cause_not_determined: z.boolean(),
  production_impact_level: z.number().nullable(),
  safety_impact_level: z.number().nullable(),
  recorded_by_id: z.number().nullable(),
  verification_status: z.string(),
  eligible_flags_json: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

export async function listFailureEvents(filter: FailureEventsFilter): Promise<FailureEvent[]> {
  const raw = await invoke<unknown>("list_failure_events", { filter });
  return z.array(FailureEventSchema).parse(raw);
}

export async function upsertFailureEvent(input: UpsertFailureEventInput): Promise<FailureEvent> {
  const raw = await invoke<unknown>("upsert_failure_event", { input });
  return FailureEventSchema.parse(raw);
}

const RuntimeExposureLogSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  exposure_type: z.string(),
  value: z.number(),
  recorded_at: z.string(),
  source_type: z.string(),
  row_version: z.number(),
});

export async function upsertRuntimeExposureLog(
  input: UpsertRuntimeExposureLogInput,
): Promise<RuntimeExposureLog> {
  const raw = await invoke<unknown>("upsert_runtime_exposure_log", { input });
  return RuntimeExposureLogSchema.parse(raw);
}

export async function listRuntimeExposureLogs(
  filter: RuntimeExposureLogsFilter,
): Promise<RuntimeExposureLog[]> {
  const raw = await invoke<unknown>("list_runtime_exposure_logs", { filter });
  return z.array(RuntimeExposureLogSchema).parse(raw);
}

const ReliabilityKpiSnapshotSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number().nullable(),
  asset_group_id: z.number().nullable(),
  period_start: z.string(),
  period_end: z.string(),
  mtbf: z.number().nullable(),
  mttr: z.number().nullable(),
  availability: z.number().nullable(),
  failure_rate: z.number().nullable(),
  repeat_failure_rate: z.number().nullable(),
  event_count: z.number(),
  data_quality_score: z.number(),
  inspection_signal_json: z.string().nullable(),
  analysis_dataset_hash_sha256: z.string(),
  analysis_input_spec_json: z.string(),
  plot_payload_json: z.string(),
  row_version: z.number(),
});

const ReliabilityAnalysisInputEvaluationSchema = z.object({
  equipment_id: z.number(),
  period_start: z.string(),
  period_end: z.string(),
  exposure_hours: z.number(),
  eligible_event_count: z.number(),
  min_sample_n: z.number(),
  analysis_dataset_hash_sha256: z.string(),
  analysis_input_spec_json: z.string(),
});

export async function evaluateReliabilityAnalysisInput(
  input: RefreshReliabilityKpiSnapshotInput,
): Promise<ReliabilityAnalysisInputEvaluation> {
  const raw = await invoke<unknown>("evaluate_reliability_analysis_input", { input });
  return ReliabilityAnalysisInputEvaluationSchema.parse(raw);
}

export async function refreshReliabilityKpiSnapshot(
  input: RefreshReliabilityKpiSnapshotInput,
): Promise<ReliabilityKpiSnapshot> {
  const raw = await invoke<unknown>("refresh_reliability_kpi_snapshot", { input });
  return ReliabilityKpiSnapshotSchema.parse(raw);
}

export async function listReliabilityKpiSnapshots(
  filter: ReliabilityKpiSnapshotsFilter,
): Promise<ReliabilityKpiSnapshot[]> {
  const raw = await invoke<unknown>("list_reliability_kpi_snapshots", { filter });
  return z.array(ReliabilityKpiSnapshotSchema).parse(raw);
}

export async function getReliabilityKpiSnapshot(id: number): Promise<ReliabilityKpiSnapshot> {
  const raw = await invoke<unknown>("get_reliability_kpi_snapshot", { id });
  return ReliabilityKpiSnapshotSchema.parse(raw);
}

const ComputationJobSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  job_kind: z.string(),
  status: z.string(),
  progress_pct: z.number(),
  input_json: z.string(),
  result_json: z.string().nullable(),
  error_message: z.string().nullable(),
  created_at: z.string(),
  started_at: z.string().nullable(),
  finished_at: z.string().nullable(),
  row_version: z.number(),
});

export async function submitReliabilityKpiComputationJob(
  input: RefreshReliabilityKpiSnapshotInput,
): Promise<number> {
  const raw = await invoke<unknown>("submit_reliability_kpi_computation_job", { input });
  return z.number().parse(raw);
}

export async function cancelComputationJob(jobId: number): Promise<void> {
  await invoke("cancel_computation_job", { jobId });
}

export async function getComputationJob(jobId: number): Promise<ComputationJob | null> {
  const raw = await invoke<unknown>("get_computation_job", { jobId });
  return ComputationJobSchema.nullable().parse(raw);
}

export async function listComputationJobs(limit: number | null): Promise<ComputationJob[]> {
  const raw = await invoke<unknown>("list_computation_jobs", { limit });
  return z.array(ComputationJobSchema).parse(raw);
}

const RamDataQualityIssueSchema = z.object({
  equipment_id: z.number(),
  issue_code: z.string(),
  severity: z.string(),
  remediation_url: z.string(),
});

export async function listRamDataQualityIssues(
  filter: RamDataQualityIssuesFilter,
): Promise<RamDataQualityIssue[]> {
  const raw = await invoke<unknown>("list_ram_data_quality_issues", { filter });
  return z.array(RamDataQualityIssueSchema).parse(raw);
}

const WoMissingFailureModeRowSchema = z.object({
  work_order_id: z.number(),
  equipment_id: z.number(),
  closed_at: z.string().nullable(),
  type_code: z.string(),
});

export async function listWosMissingFailureMode(
  equipment_id: number | null,
  limit: number | null,
): Promise<WoMissingFailureModeRow[]> {
  const raw = await invoke<unknown>("list_wos_missing_failure_mode", {
    equipmentId: equipment_id,
    limit,
  });
  return z.array(WoMissingFailureModeRowSchema).parse(raw);
}

const EquipmentMissingExposureRowSchema = z.object({
  equipment_id: z.number(),
  equipment_name: z.string(),
});

export async function listEquipmentMissingExposure90d(
  limit: number | null,
): Promise<EquipmentMissingExposureRow[]> {
  const raw = await invoke<unknown>("list_equipment_missing_exposure_90d", { limit });
  return z.array(EquipmentMissingExposureRowSchema).parse(raw);
}

const RamEquipmentQualityBadgeSchema = z.object({
  equipment_id: z.number(),
  data_quality_score: z.number().nullable(),
  badge: z.string(),
  blocking_issue_codes: z.array(z.string()),
});

export async function getRamEquipmentQualityBadge(
  equipment_id: number,
): Promise<RamEquipmentQualityBadge> {
  const raw = await invoke<unknown>("get_ram_equipment_quality_badge", {
    equipmentId: equipment_id,
  });
  return RamEquipmentQualityBadgeSchema.parse(raw);
}

const UserDismissalSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  user_id: z.number(),
  equipment_id: z.number(),
  issue_code: z.string(),
  scope_key: z.string(),
  dismissed_at: z.string(),
  row_version: z.number(),
});

export async function dismissRamDataQualityIssue(
  input: DismissRamDataQualityIssueInput,
): Promise<UserDismissal> {
  const raw = await invoke<unknown>("dismiss_ram_data_quality_issue", { input });
  return UserDismissalSchema.parse(raw);
}

const Iso14224DatasetCompletenessSchema = z.object({
  equipment_id: z.number(),
  event_count: z.number(),
  completeness_percent: z.number(),
  dim_equipment_id_pct: z.number(),
  dim_failure_interval_pct: z.number(),
  dim_failure_mode_pct: z.number(),
  dim_corrective_closure_pct: z.number(),
});

export async function iso14224FailureDatasetCompleteness(
  equipment_id: number,
): Promise<Iso14224DatasetCompleteness> {
  const raw = await invoke<unknown>("iso_14224_failure_dataset_completeness", {
    equipmentId: equipment_id,
  });
  return Iso14224DatasetCompletenessSchema.parse(raw);
}

const WeibullFitRecordSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  period_start: z.string().nullable(),
  period_end: z.string().nullable(),
  n_points: z.number(),
  inter_arrival_hours_json: z.string(),
  beta: z.number().nullable(),
  eta: z.number().nullable(),
  beta_ci_low: z.number().nullable(),
  beta_ci_high: z.number().nullable(),
  eta_ci_low: z.number().nullable(),
  eta_ci_high: z.number().nullable(),
  adequate_sample: z.boolean(),
  message: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  created_by_id: z.number().nullable(),
});

export async function runWeibullFit(input: WeibullFitRunInput): Promise<WeibullFitRecord> {
  const raw = await invoke<unknown>("run_weibull_fit", { input });
  return WeibullFitRecordSchema.parse(raw);
}

export async function getLatestWeibullFitForEquipment(
  equipment_id: number,
): Promise<WeibullFitRecord | null> {
  const raw = await invoke<unknown>("get_latest_weibull_fit_for_equipment", {
    equipmentId: equipment_id,
  });
  return z.union([WeibullFitRecordSchema, z.null()]).parse(raw);
}

export async function getRamFmecaRpnCriticalThreshold(): Promise<number> {
  const raw = await invoke<unknown>("get_ram_fmeca_rpn_critical_threshold");
  return z.number().int().parse(raw);
}

const FmecaAnalysisSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  title: z.string(),
  boundary_definition: z.string(),
  status: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  created_by_id: z.number().nullable(),
  updated_at: z.string(),
});

export async function listFmecaAnalyses(filter: FmecaAnalysesFilter): Promise<FmecaAnalysis[]> {
  const raw = await invoke<unknown>("list_fmeca_analyses", { filter });
  return z.array(FmecaAnalysisSchema).parse(raw);
}

export async function createFmecaAnalysis(input: CreateFmecaAnalysisInput): Promise<FmecaAnalysis> {
  const raw = await invoke<unknown>("create_fmeca_analysis", { input });
  return FmecaAnalysisSchema.parse(raw);
}

export async function updateFmecaAnalysis(input: UpdateFmecaAnalysisInput): Promise<FmecaAnalysis> {
  const raw = await invoke<unknown>("update_fmeca_analysis", { input });
  return FmecaAnalysisSchema.parse(raw);
}

export async function deleteFmecaAnalysis(id: number): Promise<void> {
  await invoke("delete_fmeca_analysis", { id });
}

const FmecaItemSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  analysis_id: z.number(),
  component_id: z.number().nullable(),
  functional_failure: z.string(),
  failure_mode_id: z.number().nullable(),
  failure_effect: z.string(),
  severity: z.number(),
  occurrence: z.number(),
  detectability: z.number(),
  rpn: z.number(),
  recommended_action: z.string(),
  current_control: z.string(),
  linked_pm_plan_id: z.number().nullable(),
  linked_work_order_id: z.number().nullable(),
  revised_rpn: z.number().nullable(),
  source_ram_ishikawa_diagram_id: z.number().nullable(),
  source_ishikawa_flow_node_id: z.string().nullable(),
  row_version: z.number(),
  updated_at: z.string(),
});

export async function listFmecaItems(analysisId: number): Promise<FmecaItem[]> {
  const raw = await invoke<unknown>("list_fmeca_items", { analysisId });
  return z.array(FmecaItemSchema).parse(raw);
}

export async function upsertFmecaItem(input: UpsertFmecaItemInput): Promise<FmecaItem> {
  const raw = await invoke<unknown>("upsert_fmeca_item", { input });
  return FmecaItemSchema.parse(raw);
}

export async function deleteFmecaItem(id: number): Promise<void> {
  await invoke("delete_fmeca_item", { id });
}

const FmecaSoCellSchema = z.object({
  severity: z.number(),
  occurrence: z.number(),
  count: z.number(),
});

const FmecaSeverityOccurrenceMatrixSchema = z.object({
  equipment_id: z.number(),
  cells: z.array(FmecaSoCellSchema),
});

export async function getFmecaSeverityOccurrenceMatrix(
  equipment_id: number,
): Promise<FmecaSeverityOccurrenceMatrix> {
  const raw = await invoke<unknown>("get_fmeca_severity_occurrence_matrix", {
    equipmentId: equipment_id,
  });
  return FmecaSeverityOccurrenceMatrixSchema.parse(raw);
}

const FmecaItemWithContextSchema = FmecaItemSchema.extend({
  analysis_title: z.string(),
  equipment_id: z.number(),
  spare_stock_total: z.number().nullable(),
  inventory_status: z.string(),
});

export async function listFmecaItemsForEquipment(
  filter: FmecaItemsEquipmentFilter,
): Promise<FmecaItemWithContext[]> {
  const raw = await invoke<unknown>("list_fmeca_items_for_equipment", { filter });
  return z.array(FmecaItemWithContextSchema).parse(raw);
}

const ReliabilityRulIndicatorSchema = z.object({
  equipment_id: z.number(),
  weibull_beta: z.number().nullable(),
  weibull_eta_hours: z.number().nullable(),
  reliability_at_t: z.number().nullable(),
  predicted_rul_hours: z.number().nullable(),
  t_hours: z.number().nullable(),
  message: z.string(),
});

export async function getReliabilityRulIndicator(
  equipment_id: number,
): Promise<ReliabilityRulIndicator> {
  const raw = await invoke<unknown>("get_reliability_rul_indicator", { equipmentId: equipment_id });
  return ReliabilityRulIndicatorSchema.parse(raw);
}

const RamIshikawaDiagramSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  title: z.string(),
  flow_json: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

export async function listRamIshikawaDiagrams(
  filter: RamIshikawaDiagramsFilter,
): Promise<RamIshikawaDiagram[]> {
  const raw = await invoke<unknown>("list_ram_ishikawa_diagrams", { filter });
  return z.array(RamIshikawaDiagramSchema).parse(raw);
}

export async function listRamIshikawaDiagramsByEquipments(
  equipmentIds: number[],
  opts?: { limitPerEquipment?: number | null },
): Promise<Array<{ equipmentId: number; diagrams: RamIshikawaDiagram[] }>> {
  const uniqueIds = [
    ...new Set(equipmentIds.map((v) => Number(v)).filter((n) => Number.isFinite(n) && n > 0)),
  ];
  if (uniqueIds.length === 0) {
    return [];
  }
  const limit = opts?.limitPerEquipment ?? 1;
  const rows = await Promise.all(
    uniqueIds.map(async (equipmentId) => ({
      equipmentId,
      diagrams: await listRamIshikawaDiagrams({ equipment_id: equipmentId, limit }),
    })),
  );
  return rows;
}

export async function upsertRamIshikawaDiagram(
  input: UpsertRamIshikawaDiagramInput,
): Promise<RamIshikawaDiagram> {
  const raw = await invoke<unknown>("upsert_ram_ishikawa_diagram", { input });
  return RamIshikawaDiagramSchema.parse(raw);
}

export async function deleteRamIshikawaDiagram(id: number): Promise<void> {
  await invoke("delete_ram_ishikawa_diagram", { id });
}

const RcmStudySchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  title: z.string(),
  status: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  created_by_id: z.number().nullable(),
  updated_at: z.string(),
});

export async function listRcmStudies(filter: RcmStudiesFilter): Promise<RcmStudy[]> {
  const raw = await invoke<unknown>("list_rcm_studies", { filter });
  return z.array(RcmStudySchema).parse(raw);
}

export async function createRcmStudy(input: CreateRcmStudyInput): Promise<RcmStudy> {
  const raw = await invoke<unknown>("create_rcm_study", { input });
  return RcmStudySchema.parse(raw);
}

export async function updateRcmStudy(input: UpdateRcmStudyInput): Promise<RcmStudy> {
  const raw = await invoke<unknown>("update_rcm_study", { input });
  return RcmStudySchema.parse(raw);
}

export async function deleteRcmStudy(id: number): Promise<void> {
  await invoke("delete_rcm_study", { id });
}

const RcmDecisionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  study_id: z.number(),
  function_description: z.string(),
  functional_failure: z.string(),
  failure_mode_id: z.number().nullable(),
  consequence_category: z.string(),
  selected_tactic: z.string(),
  justification: z.string(),
  review_due_at: z.string().nullable(),
  linked_pm_plan_id: z.number().nullable(),
  row_version: z.number(),
  updated_at: z.string(),
});

export async function listRcmDecisions(studyId: number): Promise<RcmDecision[]> {
  const raw = await invoke<unknown>("list_rcm_decisions", { studyId });
  return z.array(RcmDecisionSchema).parse(raw);
}

export async function upsertRcmDecision(input: UpsertRcmDecisionInput): Promise<RcmDecision> {
  const raw = await invoke<unknown>("upsert_rcm_decision", { input });
  return RcmDecisionSchema.parse(raw);
}

export async function deleteRcmDecision(id: number): Promise<void> {
  await invoke("delete_rcm_decision", { id });
}

/** Shared FTA/RBD/ETA graph table row — strict parse for mutations. */
const GraphModelSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  title: z.string(),
  graph_json: z.string(),
  result_json: z.string(),
  status: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  created_by_id: z.number().nullable(),
  updated_at: z.string(),
});

/**
 * List responses: tolerate numeric ids as strings, null/empty result_json, missing created_by_id.
 * Does not change graph_json semantics (opaque string for the canvas).
 */
const GraphModelListRowSchema = z
  .object({
    id: z.coerce.number(),
    entity_sync_id: z.string(),
    equipment_id: z.coerce.number(),
    title: z.string(),
    graph_json: z.string(),
    result_json: z
      .union([z.string(), z.null()])
      .optional()
      .transform((v) => (v == null || v === "" ? "{}" : v)),
    status: z.string(),
    row_version: z.coerce.number(),
    created_at: z.string(),
    created_by_id: z
      .union([z.coerce.number(), z.null()])
      .optional()
      .transform((v) => (v === undefined ? null : v)),
    updated_at: z.string(),
  })
  .passthrough();

function parseGraphModelList(raw: unknown): FtaModel[] {
  const parsed = z.array(GraphModelListRowSchema).safeParse(raw);
  if (parsed.success) {
    return parsed.data as FtaModel[];
  }
  console.warn(
    "[reliability] list graph models parse failed, using compatibility fallback:",
    parsed.error.flatten(),
  );
  if (!Array.isArray(raw)) {
    return [];
  }
  const fallback: FtaModel[] = [];
  for (const row of raw) {
    if (row == null || typeof row !== "object") {
      continue;
    }
    const rec = row as Record<string, unknown>;
    const id = Number(rec["id"]);
    const equipmentId = Number(rec["equipment_id"]);
    const rowVersion = Number(rec["row_version"] ?? 1);
    if (!Number.isFinite(id) || !Number.isFinite(equipmentId) || !Number.isFinite(rowVersion)) {
      continue;
    }
    fallback.push({
      id,
      entity_sync_id: String(rec["entity_sync_id"] ?? ""),
      equipment_id: equipmentId,
      title: String(rec["title"] ?? ""),
      graph_json:
        typeof rec["graph_json"] === "string"
          ? rec["graph_json"]
          : JSON.stringify(rec["graph_json"] ?? {}),
      result_json:
        rec["result_json"] == null || rec["result_json"] === "" ? "{}" : String(rec["result_json"]),
      status: String(rec["status"] ?? "draft"),
      row_version: rowVersion,
      created_at: String(rec["created_at"] ?? ""),
      created_by_id: rec["created_by_id"] == null ? null : Number(rec["created_by_id"]),
      updated_at: String(rec["updated_at"] ?? ""),
    });
  }
  return fallback;
}

export async function listFtaModels(filter: FtaModelsFilter): Promise<FtaModel[]> {
  const raw = await invoke<unknown>("list_fta_models", { filter });
  return parseGraphModelList(raw);
}

export async function createFtaModel(input: CreateFtaModelInput): Promise<FtaModel> {
  const raw = await invoke<unknown>("create_fta_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function updateFtaModel(input: UpdateFtaModelInput): Promise<FtaModel> {
  const raw = await invoke<unknown>("update_fta_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function deleteFtaModel(id: number): Promise<void> {
  await invoke("delete_fta_model", { id });
}

export async function evaluateFtaModel(id: number): Promise<FtaModel> {
  const raw = await invoke<unknown>("evaluate_fta_model", { id });
  return GraphModelSchema.parse(raw);
}

export async function listRbdModels(filter: RbdModelsFilter): Promise<RbdModel[]> {
  const raw = await invoke<unknown>("list_rbd_models", { filter });
  return parseGraphModelList(raw) as RbdModel[];
}

export async function createRbdModel(input: CreateRbdModelInput): Promise<RbdModel> {
  const raw = await invoke<unknown>("create_rbd_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function updateRbdModel(input: UpdateRbdModelInput): Promise<RbdModel> {
  const raw = await invoke<unknown>("update_rbd_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function deleteRbdModel(id: number): Promise<void> {
  await invoke("delete_rbd_model", { id });
}

export async function evaluateRbdModel(id: number): Promise<RbdModel> {
  const raw = await invoke<unknown>("evaluate_rbd_model", { id });
  return GraphModelSchema.parse(raw);
}

export async function listEventTreeModels(
  filter: EventTreeModelsFilter,
): Promise<EventTreeModel[]> {
  const raw = await invoke<unknown>("list_event_tree_models", { filter });
  return z.array(GraphModelSchema).parse(raw);
}

export async function createEventTreeModel(
  input: CreateEventTreeModelInput,
): Promise<EventTreeModel> {
  const raw = await invoke<unknown>("create_event_tree_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function updateEventTreeModel(
  input: UpdateEventTreeModelInput,
): Promise<EventTreeModel> {
  const raw = await invoke<unknown>("update_event_tree_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function deleteEventTreeModel(id: number): Promise<void> {
  await invoke("delete_event_tree_model", { id });
}

export async function evaluateEventTreeModel(id: number): Promise<EventTreeModel> {
  const raw = await invoke<unknown>("evaluate_event_tree_model", { id });
  return GraphModelSchema.parse(raw);
}

const RamAdvancedGuardrailFlagsSchema = z.object({
  monte_carlo_enabled: z.boolean(),
  markov_enabled: z.boolean(),
  mc_max_trials: z.number(),
  markov_max_states: z.number(),
});

export async function getRamAdvancedGuardrails(): Promise<RamAdvancedGuardrailFlags> {
  const raw = await invoke<unknown>("get_ram_advanced_guardrails");
  return RamAdvancedGuardrailFlagsSchema.parse(raw);
}

export async function setRamAdvancedGuardrails(flags: RamAdvancedGuardrailFlags): Promise<void> {
  await invoke("set_ram_advanced_guardrails", { flags });
}

const McModelSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  title: z.string(),
  graph_json: z.string(),
  trials: z.number(),
  seed: z.number().nullable(),
  result_json: z.string(),
  status: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  created_by_id: z.number().nullable(),
  updated_at: z.string(),
});

export async function listMcModels(filter: McModelsFilter): Promise<McModel[]> {
  const raw = await invoke<unknown>("list_mc_models", { filter });
  return z.array(McModelSchema).parse(raw);
}

export async function createMcModel(input: CreateMcModelInput): Promise<McModel> {
  const raw = await invoke<unknown>("create_mc_model", { input });
  return McModelSchema.parse(raw);
}

export async function updateMcModel(input: UpdateMcModelInput): Promise<McModel> {
  const raw = await invoke<unknown>("update_mc_model", { input });
  return McModelSchema.parse(raw);
}

export async function deleteMcModel(id: number): Promise<void> {
  await invoke("delete_mc_model", { id });
}

export async function evaluateMcModel(id: number): Promise<McModel> {
  const raw = await invoke<unknown>("evaluate_mc_model", { id });
  return McModelSchema.parse(raw);
}

export async function listMarkovModels(filter: MarkovModelsFilter): Promise<MarkovModel[]> {
  const raw = await invoke<unknown>("list_markov_models", { filter });
  return z.array(GraphModelSchema).parse(raw);
}

export async function createMarkovModel(input: CreateMarkovModelInput): Promise<MarkovModel> {
  const raw = await invoke<unknown>("create_markov_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function updateMarkovModel(input: UpdateMarkovModelInput): Promise<MarkovModel> {
  const raw = await invoke<unknown>("update_markov_model", { input });
  return GraphModelSchema.parse(raw);
}

export async function deleteMarkovModel(id: number): Promise<void> {
  await invoke("delete_markov_model", { id });
}

export async function evaluateMarkovModel(id: number): Promise<MarkovModel> {
  const raw = await invoke<unknown>("evaluate_markov_model", { id });
  return GraphModelSchema.parse(raw);
}

const RamExpertSignOffSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  equipment_id: z.number(),
  method_category: z.string(),
  target_ref: z.string().nullable(),
  title: z.string(),
  reviewer_name: z.string(),
  reviewer_role: z.string(),
  status: z.string(),
  signed_at: z.string().nullable(),
  notes: z.string(),
  row_version: z.number(),
  created_at: z.string(),
  created_by_id: z.number().nullable(),
  updated_at: z.string(),
});

export async function listRamExpertSignOffs(
  filter: RamExpertSignOffsFilter,
): Promise<RamExpertSignOff[]> {
  const raw = await invoke<unknown>("list_ram_expert_sign_offs", { filter });
  return z.array(RamExpertSignOffSchema).parse(raw);
}

export async function createRamExpertSignOff(
  input: CreateRamExpertSignOffInput,
): Promise<RamExpertSignOff> {
  const raw = await invoke<unknown>("create_ram_expert_sign_off", { input });
  return RamExpertSignOffSchema.parse(raw);
}

export async function updateRamExpertSignOff(
  input: UpdateRamExpertSignOffInput,
): Promise<RamExpertSignOff> {
  const raw = await invoke<unknown>("update_ram_expert_sign_off", { input });
  return RamExpertSignOffSchema.parse(raw);
}

export async function signRamExpertReview(
  input: SignRamExpertReviewInput,
): Promise<RamExpertSignOff> {
  const raw = await invoke<unknown>("sign_ram_expert_review", { input });
  return RamExpertSignOffSchema.parse(raw);
}

export async function deleteRamExpertSignOff(id: number): Promise<void> {
  await invoke("delete_ram_expert_sign_off", { id });
}
