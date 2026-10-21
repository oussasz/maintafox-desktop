/**
 * Reliability (RAMS) module, study steps 3–5 and data quality
 * (`src-tauri/src/commands/reliability_study.rs`, part B): FMECA worksheet, rating scales,
 * RCM decision, system model (RBD / fault tree), advanced tools.
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const num = z.number();
const optNum = num.nullable();
const optStr = z.string().nullable();

// ── Rating scales ──────────────────────────────────────────────────────────────────────────

const RatingLevelSchema = z.object({
  score: num,
  label_fr: z.string(),
  label_en: z.string(),
  consequence: z.string().optional(),
  min_per_year: num.optional(),
  hidden: z.boolean().optional(),
});

const RatingScalesSchema = z.object({
  version: num,
  /** Set while the grids must be reviewed (former grids unreadable: Maintafox grids in place). */
  review: z.string().nullable().optional(),
  severity: z.array(RatingLevelSchema),
  occurrence: z.array(RatingLevelSchema),
  detection: z.array(RatingLevelSchema),
  matrix: z.object({
    classes: num,
    zones: z.array(z.object({ min_product: num, zone: z.string() })),
  }),
});

export type RatingLevel = z.infer<typeof RatingLevelSchema>;
export type RatingScales = z.infer<typeof RatingScalesSchema>;

export async function getReliabilityRatingScales(): Promise<RatingScales> {
  return RatingScalesSchema.parse(await invoke<unknown>("get_reliability_rating_scales"));
}

export async function saveReliabilityRatingScales(scales: RatingScales): Promise<RatingScales> {
  return RatingScalesSchema.parse(
    await invoke<unknown>("save_reliability_rating_scales", { scales }),
  );
}

// ── FMECA (step 3) ─────────────────────────────────────────────────────────────────────────

const RefOptionSchema = z.object({ id: num, code: z.string(), label: z.string() });

const OccurrenceProposalSchema = z.object({
  source: z.string(),
  events_per_year: num,
  score: num,
  failures: num,
  equipment_years: num,
  mean_life_hours: optNum,
  hours_per_equipment_year: optNum,
});

const ModeProposalSchema = z.object({
  failure_mode_id: num,
  code: z.string(),
  label: z.string(),
  failures_in_period: num,
  failures_on_class: num,
  occurrence: OccurrenceProposalSchema,
  mechanism: RefOptionSchema.nullable(),
  cause: RefOptionSchema.nullable(),
});

const FmecaRowSchema = z.object({
  item_id: num,
  row_version: num,
  component_label: optStr,
  failure_mode_id: optNum,
  mode_code: optStr,
  mode_label: optStr,
  functional_failure: z.string(),
  failure_effect: z.string(),
  failure_mechanism_id: optNum,
  mechanism_label: optStr,
  failure_cause_id: optNum,
  cause_label: optStr,
  severity: num,
  occurrence: num,
  detectability: num,
  rpn: num,
  current_control: z.string(),
  recommended_action: z.string(),
  occurrence_source: optStr,
  occurrence_events_per_year: optNum,
  occurrence_proposal: OccurrenceProposalSchema.nullable(),
  severity_class: num,
  occurrence_class: num,
  zone: z.string(),
  rcm_decision_id: optNum,
  rcm_tactic: optStr,
  decision_id: optNum,
  decision_status: optStr,
  /** Ratings nobody chose (former root-cause placeholder): listed, not counted until saved. */
  review_required: z.boolean(),
});

const FmecaWorksheetSchema = z.object({
  study_id: num,
  analysis_id: optNum,
  rows: z.array(FmecaRowSchema),
  proposals: z.array(ModeProposalSchema),
  scales: RatingScalesSchema,
  matrix: z.array(
    z.object({ severity_class: num, occurrence_class: num, count: num, zone: z.string() }),
  ),
  rpn_threshold: num,
  equipment_years: num,
  class_names: z.array(z.string()),
  modes: z.array(RefOptionSchema),
  mechanisms: z.array(RefOptionSchema),
  causes: z.array(RefOptionSchema),
  editable: z.boolean(),
  rows_to_review: num,
});

export type RefOption = z.infer<typeof RefOptionSchema>;
export type OccurrenceProposal = z.infer<typeof OccurrenceProposalSchema>;
export type ModeProposal = z.infer<typeof ModeProposalSchema>;
export type FmecaRow = z.infer<typeof FmecaRowSchema>;
export type FmecaWorksheet = z.infer<typeof FmecaWorksheetSchema>;

export async function getReliabilityStudyFmeca(studyId: number): Promise<FmecaWorksheet> {
  return FmecaWorksheetSchema.parse(
    await invoke<unknown>("get_reliability_study_fmeca", { studyId }),
  );
}

export interface SaveFmecaItemInput {
  study_id: number;
  item_id?: number | null;
  expected_row_version?: number | null;
  failure_mode_id: number;
  component_label?: string | null;
  functional_failure?: string | null;
  failure_effect?: string | null;
  failure_mechanism_id?: number | null;
  failure_cause_id?: number | null;
  severity: number;
  occurrence: number;
  detectability: number;
  current_control?: string | null;
  recommended_action?: string | null;
  occurrence_source: "weibull" | "history" | "analyst";
  occurrence_events_per_year?: number | null;
  /** Root-cause diagram cause a new row comes from (traceability; ignored on update). */
  source_ram_ishikawa_diagram_id?: number | null;
  source_ishikawa_flow_node_id?: string | null;
}

export async function saveReliabilityStudyFmecaItem(
  input: SaveFmecaItemInput,
): Promise<FmecaWorksheet> {
  return FmecaWorksheetSchema.parse(
    await invoke<unknown>("save_reliability_study_fmeca_item", { input }),
  );
}

export async function deleteReliabilityStudyFmecaItem(
  studyId: number,
  itemId: number,
): Promise<FmecaWorksheet> {
  return FmecaWorksheetSchema.parse(
    await invoke<unknown>("delete_reliability_study_fmeca_item", { studyId, itemId }),
  );
}

// ── RCM (step 4) ───────────────────────────────────────────────────────────────────────────

const RcmAnswersSchema = z.object({
  failure_evident: z.boolean(),
  safety_or_environmental: z.boolean(),
  operational_impact: z.boolean(),
  multiple_failure_affects_safety: z.boolean(),
  on_condition_feasible: z.boolean(),
  scheduled_restoration_feasible: z.boolean(),
  scheduled_discard_feasible: z.boolean(),
  failure_finding_feasible: z.boolean(),
  combination_feasible: z.boolean(),
  weibull_beta: optNum,
});

export type StudyRcmAnswers = z.infer<typeof RcmAnswersSchema>;
export type RcmQuestion = Exclude<keyof StudyRcmAnswers, "weibull_beta">;

const RcmRowSchema = z.object({
  item_id: num,
  component_label: optStr,
  mode_code: optStr,
  mode_label: optStr,
  functional_failure: z.string(),
  severity: num,
  occurrence: num,
  detectability: num,
  rpn: num,
  answers: RcmAnswersSchema,
  prefilled: RcmAnswersSchema,
  bases: z.array(
    z.object({ question: z.string(), basis: z.string(), score: optNum, value: optNum }),
  ),
  recommendation: z.object({
    consequence_category: z.string(),
    recommended_tactic: z.string(),
    redesign_compulsory: z.boolean(),
    path: z.array(z.string()),
  }),
  saved: z
    .object({
      decision_id: num,
      row_version: num,
      selected_tactic: z.string(),
      recommended_tactic: optStr,
      justification: z.string(),
      path: z.array(z.string()),
    })
    .nullable(),
  pf_interval_hours: optNum,
  inspection_interval_hours: optNum,
  replacement_interval_hours: optNum,
  beta: optNum,
  pattern: optStr,
  decision_id: optNum,
  decision_status: optStr,
});

const RcmWorksheetSchema = z.object({
  study_id: num,
  rcm_study_id: optNum,
  rows: z.array(RcmRowSchema),
  editable: z.boolean(),
  time_basis: z.string(),
  /** FMECA rows still to review: not offered for an RCM decision. */
  rows_to_review: num,
  /** Set while the plant's FMECA grids must be reviewed (Maintafox grids standing in). */
  grids_review: z.string().nullable().optional(),
});

export type RcmRow = z.infer<typeof RcmRowSchema>;
export type RcmWorksheet = z.infer<typeof RcmWorksheetSchema>;

export async function getReliabilityStudyRcm(studyId: number): Promise<RcmWorksheet> {
  return RcmWorksheetSchema.parse(await invoke<unknown>("get_reliability_study_rcm", { studyId }));
}

export async function previewStudyRcm(
  answers: StudyRcmAnswers,
): Promise<z.infer<typeof RcmRowSchema.shape.recommendation>> {
  const raw = await invoke<unknown>("preview_rcm_recommendation", { answers });
  return RcmRowSchema.shape.recommendation.parse(raw);
}

export async function saveReliabilityStudyRcmDecision(input: {
  study_id: number;
  fmeca_item_id: number;
  answers: StudyRcmAnswers;
  selected_tactic?: string | null;
  justification?: string | null;
  pf_interval_hours?: number | null;
}): Promise<RcmWorksheet> {
  return RcmWorksheetSchema.parse(
    await invoke<unknown>("save_reliability_study_rcm_decision", { input }),
  );
}

// ── System model (step 5) ──────────────────────────────────────────────────────────────────

export type LifeSource =
  | { kind: "study_fit" }
  | { kind: "equipment_history"; equipment_id: number }
  | { kind: "weibull"; beta: number; eta: number }
  | { kind: "exponential"; mtbf_hours: number }
  | { kind: "reliability"; value: number }
  | { kind: "probability"; value: number }
  | { kind: "rate"; per_hour: number };

const LifeSourceSchema: z.ZodType<LifeSource> = z.union([
  z.object({ kind: z.literal("study_fit") }),
  z.object({ kind: z.literal("equipment_history"), equipment_id: num }),
  z.object({ kind: z.literal("weibull"), beta: num, eta: num }),
  z.object({ kind: z.literal("exponential"), mtbf_hours: num }),
  z.object({ kind: z.literal("reliability"), value: num }),
  z.object({ kind: z.literal("probability"), value: num }),
  z.object({ kind: z.literal("rate"), per_hour: num }),
]);

export interface RbdNode {
  id: string;
  label: string;
  node_type: "block" | "series" | "parallel";
  children: string[];
  k: number | null;
  source: LifeSource | null;
  group: string | null;
  mttr_hours: number | null;
}

export interface RbdSpec {
  root_id: string;
  nodes: RbdNode[];
}

export interface FtNode {
  id: string;
  label: string;
  node_type: "basic" | "and" | "or";
  inputs: string[];
  source: LifeSource | null;
}

export interface FtSpec {
  top_id: string;
  nodes: FtNode[];
}

const RbdNodeSchema: z.ZodType<RbdNode> = z.object({
  id: z.string(),
  label: z.string(),
  node_type: z.enum(["block", "series", "parallel"]),
  children: z.array(z.string()),
  k: optNum,
  source: LifeSourceSchema.nullable(),
  group: optStr,
  mttr_hours: optNum,
});

const FtNodeSchema: z.ZodType<FtNode> = z.object({
  id: z.string(),
  label: z.string(),
  node_type: z.enum(["basic", "and", "or"]),
  inputs: z.array(z.string()),
  source: LifeSourceSchema.nullable(),
});

const SourceBasisSchema = z.object({
  kind: z.string(),
  beta: optNum,
  eta: optNum,
  mtbf_hours: optNum,
  failures: optNum,
  hours: optNum,
  asset_code: optStr,
});

const IssueSchema = z.object({ node_id: z.string(), code: z.string() });

const EquipmentLifeSchema = z.object({
  equipment_id: num,
  asset_code: z.string(),
  asset_name: z.string(),
  failures: num,
  hours: num,
  mtbf_hours: optNum,
  mttr_hours: optNum,
});

const SystemModelSchema = z.object({
  study_id: num,
  mission_hours: optNum,
  time_basis: z.string(),
  fit: z.tuple([num, num]).nullable(),
  study_mttr_hours: optNum,
  equipment: z.array(EquipmentLifeSchema),
  rbd: z
    .object({
      model_id: num,
      row_version: num,
      title: z.string(),
      spec: z.object({ root_id: z.string(), nodes: z.array(RbdNodeSchema) }),
    })
    .nullable(),
  fta: z
    .object({
      model_id: num,
      row_version: num,
      title: z.string(),
      spec: z.object({ top_id: z.string(), nodes: z.array(FtNodeSchema) }),
    })
    .nullable(),
  editable: z.boolean(),
});

const RbdResultSchema = z.object({
  mission_hours: num,
  system_reliability: optNum,
  nodes: z.array(
    z.object({
      id: z.string(),
      label: z.string(),
      node_type: z.string(),
      reliability: num,
      birnbaum: optNum,
      criticality: optNum,
      basis: SourceBasisSchema.nullable(),
    }),
  ),
  groups: z.array(z.object({ group: z.string(), share: num })),
  issues: z.array(IssueSchema),
});

const FtResultSchema = z.object({
  mission_hours: num,
  top_probability: optNum,
  probability_method: optStr,
  cut_sets: z.array(
    z.object({ event_ids: z.array(z.string()), labels: z.array(z.string()), probability: num }),
  ),
  events: z.array(
    z.object({
      id: z.string(),
      label: z.string(),
      probability: num,
      birnbaum: num,
      fussell_vesely: num,
      risk_achievement_worth: num,
      basis: SourceBasisSchema.nullable(),
    }),
  ),
  issues: z.array(IssueSchema),
});

export type EquipmentLife = z.infer<typeof EquipmentLifeSchema>;
export type SystemModel = z.infer<typeof SystemModelSchema>;
export type RbdResult = z.infer<typeof RbdResultSchema>;
export type FtResult = z.infer<typeof FtResultSchema>;
export type ModelIssue = z.infer<typeof IssueSchema>;

export async function getReliabilityStudySystemModel(studyId: number): Promise<SystemModel> {
  return SystemModelSchema.parse(
    await invoke<unknown>("get_reliability_study_system_model", { studyId }),
  );
}

export async function evaluateReliabilityStudyRbd(input: {
  study_id: number;
  mission_hours: number;
  spec: RbdSpec;
}): Promise<RbdResult> {
  return RbdResultSchema.parse(await invoke<unknown>("evaluate_reliability_study_rbd", { input }));
}

export async function evaluateReliabilityStudyFaultTree(input: {
  study_id: number;
  mission_hours: number;
  spec: FtSpec;
}): Promise<FtResult> {
  return FtResultSchema.parse(
    await invoke<unknown>("evaluate_reliability_study_fault_tree", { input }),
  );
}

export async function saveReliabilityStudySystemModel(input: {
  study_id: number;
  mission_hours: number;
  rbd?: RbdSpec | null;
  fta?: FtSpec | null;
}): Promise<SystemModel> {
  return SystemModelSchema.parse(
    await invoke<unknown>("save_reliability_study_system_model", { input }),
  );
}

// ── Advanced tools ─────────────────────────────────────────────────────────────────────────

export interface MarkovInput {
  states: { id: string; label: string; up: boolean }[];
  transitions: { from: string; to: string; rate_per_hour: number }[];
}

const MarkovResultSchema = z.object({
  states: z.array(
    z.object({ id: z.string(), label: z.string(), up: z.boolean(), probability: num }),
  ),
  availability: num,
  failures_per_year: num,
  mean_up_time_hours: optNum,
  mean_down_time_hours: optNum,
});

export type MarkovResult = z.infer<typeof MarkovResultSchema>;

export async function evaluateReliabilityMarkov(input: MarkovInput): Promise<MarkovResult> {
  return MarkovResultSchema.parse(await invoke<unknown>("evaluate_reliability_markov", { input }));
}

export interface EventTreeInput {
  initiating_event: string;
  frequency_per_year: number;
  barriers: { label: string; failure_probability: number }[];
  consequences: (string | null)[];
}

const EventTreeResultSchema = z.object({
  sequences: z.array(
    z.object({
      index: num,
      failed: z.array(z.boolean()),
      consequence: optStr,
      probability: num,
      frequency_per_year: num,
    }),
  ),
  total_frequency_per_year: num,
});

export type EventTreeResult = z.infer<typeof EventTreeResultSchema>;

export async function evaluateReliabilityEventTree(
  input: EventTreeInput,
): Promise<EventTreeResult> {
  return EventTreeResultSchema.parse(
    await invoke<unknown>("evaluate_reliability_event_tree", { input }),
  );
}

const SpreadSchema = z.object({ mean: num, p05: num, p50: num, p95: num });

const SimulationResultSchema = z.object({
  trials: num,
  seed: num,
  mission_hours: num,
  availability: SpreadSchema,
  system_failures: SpreadSchema,
  downtime_hours: SpreadSchema,
  cost: SpreadSchema.nullable(),
  mission_reliability: num,
  blocks: z.array(
    z.object({
      id: z.string(),
      label: z.string(),
      beta: num,
      eta: num,
      mttr_hours: num,
      mttr_source: z.string(),
      mean_failures: num,
    }),
  ),
  issues: z.array(IssueSchema),
});

export type SimulationResult = z.infer<typeof SimulationResultSchema>;

export async function simulateReliabilityStudySystem(input: {
  study_id: number;
  mission_hours: number;
  trials: number;
  seed?: number | null;
  spec: RbdSpec;
  downtime_cost_per_hour?: number | null;
  repair_cost?: number | null;
}): Promise<SimulationResult> {
  return SimulationResultSchema.parse(
    await invoke<unknown>("simulate_reliability_study_system", { input }),
  );
}

// ── Data quality ───────────────────────────────────────────────────────────────────────────

const DataQualitySchema = z.object({
  period_start: z.string(),
  period_end: z.string(),
  coding_gaps: z.array(
    z.object({
      work_order_id: num,
      work_order_code: z.string(),
      equipment_id: optNum,
      asset_code: optStr,
      closed_at: optStr,
      gap: z.string(),
    }),
  ),
  no_operating_hours: z.array(
    z.object({ equipment_id: num, asset_code: z.string(), asset_name: z.string(), failures: num }),
  ),
  review_issues: z.array(
    z.object({
      equipment_id: num,
      asset_code: z.string(),
      asset_name: z.string(),
      issue_code: z.string(),
      severity: z.string(),
    }),
  ),
  records: num,
  records_complete: num,
  /** A list stopped at its maximum: more exist than are listed. */
  coding_gaps_truncated: z.boolean(),
  no_operating_hours_truncated: z.boolean(),
  review_issues_truncated: z.boolean(),
});

export type DataQualityView = z.infer<typeof DataQualitySchema>;

export async function getReliabilityDataQuality(months: number): Promise<DataQualityView> {
  return DataQualitySchema.parse(await invoke<unknown>("get_reliability_data_quality", { months }));
}
