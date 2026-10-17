/**
 * Reliability (RAMS) module — fleet view, studies, life analysis and decision log
 * (`src-tauri/src/commands/reliability_study.rs`).
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const num = z.number();
const optNum = num.nullable();
const optStr = z.string().nullable();

// ── Fleet view ─────────────────────────────────────────────────────────────────────────────

const FilterOptionSchema = z.object({ id: num, label: z.string(), depth: num });

const FleetKpisSchema = z.object({
  equipment_count: num,
  failure_count: num,
  downtime_hours: num,
  availability_operational: optNum,
  mtbf_hours: optNum,
  mttr_hours: optNum,
  data_quality: optNum,
  records_missing_mode: num,
  records_missing_cause: num,
  operating_basis_count: num,
  calendar_basis_count: num,
});

const FleetAssetRowSchema = z.object({
  equipment_id: num,
  asset_code: z.string(),
  asset_name: z.string(),
  class_id: optNum,
  class_name: optStr,
  failures: num,
  downtime_hours: num,
  downtime_share: num,
  mtbf_hours: optNum,
  mttr_hours: optNum,
  time_basis: z.string(),
  data_quality: z.string(),
  records_to_check: num,
  last_study_id: optNum,
  last_study_title: optStr,
  last_study_updated_at: optStr,
});

const FleetViewSchema = z.object({
  period_start: z.string(),
  period_end: z.string(),
  kpis: FleetKpisSchema,
  rows: z.array(FleetAssetRowSchema),
  total_rows: num,
  org_nodes: z.array(FilterOptionSchema),
  equipment_classes: z.array(FilterOptionSchema),
});

export type FleetView = z.infer<typeof FleetViewSchema>;
export type FleetAssetRow = z.infer<typeof FleetAssetRowSchema>;
export type FilterOption = z.infer<typeof FilterOptionSchema>;

export interface FleetViewFilter {
  period_start: string;
  period_end: string;
  org_node_id?: number | null;
  equipment_class_id?: number | null;
  limit?: number | null;
  /** Rows skipped (next page of the ranking). */
  offset?: number | null;
}

export async function getReliabilityFleetView(filter: FleetViewFilter): Promise<FleetView> {
  const raw = await invoke<unknown>("get_reliability_fleet_view", { filter });
  return FleetViewSchema.parse(raw);
}

// ── Studies ────────────────────────────────────────────────────────────────────────────────

const StudyAssetSchema = z.object({
  equipment_id: num,
  asset_code: z.string(),
  asset_name: z.string(),
  class_id: optNum,
  class_name: optStr,
});

const StudySchema = z.object({
  id: num,
  title: z.string(),
  period_start: z.string(),
  period_end: z.string(),
  failure_mode_id: optNum,
  failure_mode_code: optStr,
  failure_mode_label: optStr,
  time_basis: z.string(),
  competing_modes: z.string(),
  mission_hours: optNum,
  planned_cost: optNum,
  failure_cost: optNum,
  cost_currency_code: optStr,
  pm_plan_id: optNum,
  status: z.string(),
  assets: z.array(StudyAssetSchema),
  decision_count: num,
  created_by_name: optStr,
  row_version: num,
  created_at: z.string(),
  updated_at: z.string(),
});

export type ReliabilityStudy = z.infer<typeof StudySchema>;

export async function listReliabilityStudies(includeArchived = false): Promise<ReliabilityStudy[]> {
  const raw = await invoke<unknown>("list_reliability_studies", { includeArchived });
  return z.array(StudySchema).parse(raw);
}

export interface CreateStudyInput {
  title?: string | null;
  equipment_ids: number[];
  period_start: string;
  period_end: string;
  failure_mode_id?: number | null;
}

export async function createReliabilityStudy(input: CreateStudyInput): Promise<ReliabilityStudy> {
  const raw = await invoke<unknown>("create_reliability_study", { input });
  return StudySchema.parse(raw);
}

export interface UpdateStudyInput {
  id: number;
  expected_row_version: number;
  title?: string | null;
  period_start?: string | null;
  period_end?: string | null;
  failure_mode_id?: number | null;
  clear_failure_mode?: boolean;
  time_basis?: "auto" | "calendar" | null;
  competing_modes?: "ignore" | "suspend" | null;
  pm_plan_id?: number | null;
  clear_pm_plan?: boolean;
  equipment_ids?: number[] | null;
  status?: "active" | "archived" | null;
}

export async function updateReliabilityStudy(input: UpdateStudyInput): Promise<ReliabilityStudy> {
  const raw = await invoke<unknown>("update_reliability_study", { input });
  return StudySchema.parse(raw);
}

export interface SaveStudyCostsInput {
  study_id: number;
  expected_row_version: number;
  planned_cost: number;
  failure_cost: number;
  currency_code: string;
  save_as_class_default_for?: number | null;
}

export async function saveReliabilityStudyCosts(
  input: SaveStudyCostsInput,
): Promise<ReliabilityStudy> {
  const raw = await invoke<unknown>("save_reliability_study_costs", { input });
  return StudySchema.parse(raw);
}

// ── Step 1: records ────────────────────────────────────────────────────────────────────────

const StudyRecordSchema = z.object({
  key: z.string(),
  kind: z.string(),
  equipment_id: num,
  asset_code: z.string(),
  failure_event_id: optNum,
  work_order_id: optNum,
  work_order_code: optStr,
  di_id: optNum,
  di_code: optStr,
  event_at: z.string(),
  event_time_source: z.string(),
  restored_at: optStr,
  age_hours: optNum,
  included: z.boolean(),
  state_reason: optStr,
  analyst_note: optStr,
  failure_mode_code: optStr,
  failure_mode_label: optStr,
  failure_cause_label: optStr,
  failure_mechanism_label: optStr,
  cause_not_determined: z.boolean(),
  downtime_hours: optNum,
  flags: z.array(z.string()),
  di_at: optStr,
  di_lead_hours: optNum,
});

const DataCheckSchema = z.object({
  kind: z.string(),
  severity: z.string(),
  record_key: optStr,
  equipment_id: optNum,
  asset_code: optStr,
  work_order_id: optNum,
  work_order_code: optStr,
  di_code: optStr,
  hours: optNum,
  count: optNum,
  coverage: optNum,
});

const ClockInfoSchema = z.object({
  equipment_id: num,
  asset_code: z.string(),
  source: z.string(),
  coverage: num,
  period_hours: num,
  calendar_hours: num,
});

const HistorySchema = z.object({
  record_key: z.string(),
  work_order_code: optStr,
  asset_code: optStr,
  action: z.string(),
  reason: z.string(),
  actor_name: optStr,
  created_at: z.string(),
});

const StudyDatasetSchema = z.object({
  study: StudySchema,
  effective_time_basis: z.string(),
  clocks: z.array(ClockInfoSchema),
  records: z.array(StudyRecordSchema),
  checks: z.array(DataCheckSchema),
  failures_used: num,
  suspensions_used: num,
  excluded: num,
  history: z.array(HistorySchema),
});

export type StudyRecord = z.infer<typeof StudyRecordSchema>;
export type DataCheck = z.infer<typeof DataCheckSchema>;
export type StudyDataset = z.infer<typeof StudyDatasetSchema>;

export async function getReliabilityStudyDataset(studyId: number): Promise<StudyDataset> {
  const raw = await invoke<unknown>("get_reliability_study_dataset", { studyId });
  return StudyDatasetSchema.parse(raw);
}

export type StudyRecordAction = "exclude" | "include" | "use_di_time" | "use_repair_time";

export async function setReliabilityStudyRecord(input: {
  study_id: number;
  record_key: string;
  action: StudyRecordAction;
  reason?: string | null;
}): Promise<StudyDataset> {
  const raw = await invoke<unknown>("set_reliability_study_record", { input });
  return StudyDatasetSchema.parse(raw);
}

// ── Step 2: life analysis ──────────────────────────────────────────────────────────────────

const LifeFitSchema = z.object({
  beta: num,
  eta: num,
  confidence: num,
  beta_lower: num,
  beta_upper: num,
  eta_lower: num,
  eta_upper: num,
  b10: num,
  b10_lower: optNum,
  b10_upper: optNum,
  mean_life: num,
  median_life: num,
  rho: optNum,
  pattern: z.string(),
});

const StrategyCostSchema = z.object({
  interval: optNum,
  cost_rate: num,
  annual_cost: optNum,
});

const CostAnalysisSchema = z.object({
  planned_cost: num,
  failure_cost: num,
  run_to_failure: StrategyCostSchema,
  current: StrategyCostSchema.nullable(),
  proposed: StrategyCostSchema.nullable(),
  rejection: optStr,
  saving_vs_run_to_failure: optNum,
  annual_saving_vs_current: optNum,
  curve: z.array(z.object({ interval: num, cost_rate: num })),
});

const PmPlanCandidateSchema = z.object({
  pm_plan_id: num,
  code: z.string(),
  title: z.string(),
  equipment_id: num,
  asset_code: z.string(),
  strategy_type: z.string(),
  interval_value: optNum,
  interval_unit: optStr,
  interval_hours: optNum,
  interval_note: optStr,
});

const LifeAnalysisSchema = z.object({
  effective_time_basis: z.string(),
  n_failures: num,
  n_suspensions: num,
  min_failures: num,
  fit: LifeFitSchema.nullable(),
  fit_issue: optStr,
  plot_points: z.array(z.object({ time: num, median_rank: num })),
  band: z.array(z.object({ time: num, unreliability: num, lower: num, upper: num })),
  curve: z.array(z.object({ time: num, reliability: num, hazard: num })),
  cost: CostAnalysisSchema.nullable(),
  cost_source: z.string(),
  cost_currency_code: optStr,
  cost_class_id: optNum,
  cost_class_name: optStr,
  pm_plans: z.array(PmPlanCandidateSchema),
  selected_pm_plan_id: optNum,
  operating_hours_per_year: optNum,
  asset_count: num,
  operating_ratio: optNum,
});

export type LifeFit = z.infer<typeof LifeFitSchema>;
export type LifeAnalysis = z.infer<typeof LifeAnalysisSchema>;
export type CostAnalysis = z.infer<typeof CostAnalysisSchema>;
export type PmPlanCandidate = z.infer<typeof PmPlanCandidateSchema>;

export async function getReliabilityStudyLifeAnalysis(input: {
  study_id: number;
  planned_cost?: number | null;
  failure_cost?: number | null;
  pm_plan_id?: number | null;
}): Promise<LifeAnalysis> {
  const raw = await invoke<unknown>("get_reliability_study_life_analysis", { input });
  return LifeAnalysisSchema.parse(raw);
}

// ── Step 6: decisions ──────────────────────────────────────────────────────────────────────

const EvidenceSchema = z.object({
  time_basis: z.string(),
  n_failures: num,
  n_suspensions: num,
  beta: optNum,
  eta: optNum,
  beta_lower: optNum,
  beta_upper: optNum,
  b10: optNum,
  mean_life: optNum,
  rho: optNum,
  pattern: optStr,
  optimal_interval: optNum,
  current_interval: optNum,
  proposed_cost_rate: optNum,
  current_cost_rate: optNum,
  run_to_failure_cost_rate: optNum,
  annual_saving_vs_current: optNum,
  planned_cost: optNum,
  failure_cost: optNum,
  currency_code: optStr,
  operating_ratio: optNum,
  study_period_start: z.string(),
  study_period_end: z.string(),
  fmeca_mode_label: optStr,
  rpn: optNum,
  rcm_tactic: optStr,
});

const EffectSchema = z.object({
  state: z.string(),
  mtbf_before: optNum,
  mtbf_after: optNum,
  after_is_lower_bound: z.boolean(),
  change: optNum,
  failures_after: num,
  hours_after: num,
});

const DecisionSchema = z.object({
  id: num,
  study_id: num,
  title: z.string(),
  action_kind: z.string(),
  equipment_id: optNum,
  asset_code: optStr,
  pm_plan_id: optNum,
  pm_plan_code: optStr,
  proposed_interval_hours: optNum,
  proposed_trigger_interval_value: optNum,
  proposed_trigger_interval_unit: optStr,
  wo_type_code: optStr,
  justification: z.string(),
  fmeca_item_id: optNum,
  rcm_decision_id: optNum,
  evidence: EvidenceSchema,
  status: z.string(),
  approver_id: optNum,
  approver_name: optStr,
  proposed_by_name: optStr,
  proposed_at: z.string(),
  decided_by_name: optStr,
  decided_at: optStr,
  decision_note: optStr,
  applied_at: optStr,
  linked_pm_plan_id: optNum,
  linked_pm_plan_version_id: optNum,
  linked_pm_version_no: optNum,
  linked_work_order_id: optNum,
  linked_work_order_code: optStr,
  effect: EffectSchema,
  row_version: num,
});

export type StudyDecision = z.infer<typeof DecisionSchema>;

export async function listReliabilityStudyDecisions(studyId: number): Promise<StudyDecision[]> {
  const raw = await invoke<unknown>("list_reliability_study_decisions", { studyId });
  return z.array(DecisionSchema).parse(raw);
}

export async function listReliabilityDecisionApprovers(): Promise<
  { user_id: number; display_name: string }[]
> {
  const raw = await invoke<unknown>("list_reliability_decision_approvers");
  return z.array(z.object({ user_id: num, display_name: z.string() })).parse(raw);
}

export type DecisionKind = "pm_interval" | "one_off_wo" | "modification_wo";

export interface CreateDecisionInput {
  study_id: number;
  title: string;
  action_kind: DecisionKind;
  equipment_id?: number | null;
  pm_plan_id?: number | null;
  proposed_interval_hours?: number | null;
  wo_type_code?: string | null;
  approver_id?: number | null;
  justification?: string | null;
  fmeca_item_id?: number | null;
  rcm_decision_id?: number | null;
  planned_cost?: number | null;
  failure_cost?: number | null;
}

export async function createReliabilityStudyDecision(
  input: CreateDecisionInput,
): Promise<StudyDecision> {
  const raw = await invoke<unknown>("create_reliability_study_decision", { input });
  return DecisionSchema.parse(raw);
}

export interface DecideInput {
  decision_id: number;
  expected_row_version: number;
  note?: string | null;
}

async function decide(cmd: string, input: DecideInput): Promise<StudyDecision> {
  const raw = await invoke<unknown>(cmd, { input });
  return DecisionSchema.parse(raw);
}

export const approveReliabilityStudyDecision = (input: DecideInput): Promise<StudyDecision> =>
  decide("approve_reliability_study_decision", input);
export const rejectReliabilityStudyDecision = (input: DecideInput): Promise<StudyDecision> =>
  decide("reject_reliability_study_decision", input);
export const applyReliabilityStudyDecision = (input: DecideInput): Promise<StudyDecision> =>
  decide("apply_reliability_study_decision", input);
