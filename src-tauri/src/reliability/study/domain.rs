//! Types of the reliability study workspace (fleet view, study, records, life analysis,
//! decisions). Codes in `String` fields are stable snake_case values translated by the UI.

use serde::{Deserialize, Serialize};

use crate::reliability::life_metrics::AgeReplacementRejection;

// ── Fleet view ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FleetViewFilter {
    /// RFC 3339 start of the period (inclusive).
    pub period_start: String,
    /// RFC 3339 end of the period (inclusive).
    pub period_end: String,
    /// Org node: equipment installed at this node or below it.
    #[serde(default)]
    pub org_node_id: Option<i64>,
    #[serde(default)]
    pub equipment_class_id: Option<i64>,
    /// Rows returned (ranked by downtime); default 50, at most 200 per page.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Rows skipped (next page of the ranking); default 0.
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FleetKpis {
    pub equipment_count: i64,
    pub failure_count: i64,
    pub downtime_hours: f64,
    /// A_o = T / (T + downtime) over equipment with a known time base.
    pub availability_operational: Option<f64>,
    pub mtbf_hours: Option<f64>,
    pub mttr_hours: Option<f64>,
    /// Share of failure records with failure mode and cause coded (0–1).
    pub data_quality: Option<f64>,
    pub records_missing_mode: i64,
    pub records_missing_cause: i64,
    /// Equipment whose hours are operating hours (meter, runtime log or operating schedule).
    pub operating_basis_count: i64,
    /// Equipment counted on calendar time (no operating-hours data).
    pub calendar_basis_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetAssetRow {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub class_id: Option<i64>,
    pub class_name: Option<String>,
    pub failures: i64,
    pub downtime_hours: f64,
    /// Share of the fleet downtime (0–1).
    pub downtime_share: f64,
    pub mtbf_hours: Option<f64>,
    pub mttr_hours: Option<f64>,
    /// `run_hours_meter` | `runtime_logs` | `operating_schedule` | `calendar`.
    pub time_basis: String,
    /// `complete` | `to_check` | `no_operating_hours`.
    pub data_quality: String,
    pub records_to_check: i64,
    pub last_study_id: Option<i64>,
    pub last_study_title: Option<String>,
    pub last_study_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterOption {
    pub id: i64,
    pub label: String,
    /// Indentation level for tree options (org nodes); 0 otherwise.
    pub depth: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FleetView {
    pub period_start: String,
    pub period_end: String,
    pub kpis: FleetKpis,
    pub rows: Vec<FleetAssetRow>,
    /// Equipment with at least one failure in the period (rows may be fewer: `limit`).
    pub total_rows: i64,
    pub org_nodes: Vec<FilterOption>,
    pub equipment_classes: Vec<FilterOption>,
}

// ── Studies ─────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct StudyAsset {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub class_id: Option<i64>,
    pub class_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReliabilityStudy {
    pub id: i64,
    pub title: String,
    pub period_start: String,
    pub period_end: String,
    pub failure_mode_id: Option<i64>,
    pub failure_mode_code: Option<String>,
    pub failure_mode_label: Option<String>,
    /// `auto` (operating hours when every asset has them) | `calendar`.
    pub time_basis: String,
    /// Other failure modes in a single-mode study: `ignore` | `suspend`.
    pub competing_modes: String,
    /// Mission time of the system model (hours of the study time base).
    pub mission_hours: Option<f64>,
    pub planned_cost: Option<f64>,
    pub failure_cost: Option<f64>,
    pub cost_currency_code: Option<String>,
    pub pm_plan_id: Option<i64>,
    /// `active` | `archived`.
    pub status: String,
    pub assets: Vec<StudyAsset>,
    pub decision_count: i64,
    pub created_by_name: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateStudyInput {
    #[serde(default)]
    pub title: Option<String>,
    pub equipment_ids: Vec<i64>,
    pub period_start: String,
    pub period_end: String,
    #[serde(default)]
    pub failure_mode_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateStudyInput {
    pub id: i64,
    pub expected_row_version: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub period_start: Option<String>,
    #[serde(default)]
    pub period_end: Option<String>,
    /// `Some(None)` is not expressible in JSON; use `clear_failure_mode`.
    #[serde(default)]
    pub failure_mode_id: Option<i64>,
    #[serde(default)]
    pub clear_failure_mode: bool,
    #[serde(default)]
    pub time_basis: Option<String>,
    /// `ignore` | `suspend`.
    #[serde(default)]
    pub competing_modes: Option<String>,
    #[serde(default)]
    pub pm_plan_id: Option<i64>,
    #[serde(default)]
    pub clear_pm_plan: bool,
    #[serde(default)]
    pub equipment_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveStudyCostsInput {
    pub study_id: i64,
    pub expected_row_version: i64,
    pub planned_cost: f64,
    pub failure_cost: f64,
    pub currency_code: String,
    /// Also store the values as the default of this equipment class (needs `ram.manage`).
    #[serde(default)]
    pub save_as_class_default_for: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CostProfile {
    pub equipment_class_id: i64,
    pub class_name: String,
    pub planned_cost: f64,
    pub failure_cost: f64,
    pub currency_code: Option<String>,
    pub updated_at: String,
}

// ── Records (step 1) ────────────────────────────────────────────────────────────────────────

pub const RECORD_FAILURE: &str = "failure";
pub const RECORD_PM_SUSPENSION: &str = "pm_suspension";
pub const RECORD_END_OF_PERIOD: &str = "end_of_period";
/// Failure of another mode, kept as a suspension of the studied mode (competing modes).
pub const RECORD_COMPETING: &str = "competing_mode";

pub const ACTION_EXCLUDE: &str = "exclude";
pub const ACTION_INCLUDE: &str = "include";
pub const ACTION_USE_DI_TIME: &str = "use_di_time";
pub const ACTION_USE_REPAIR_TIME: &str = "use_repair_time";

#[derive(Debug, Clone, Serialize)]
pub struct StudyRecord {
    /// Stable key: `failure:<failure_event_id>`, `pm:<work_order_id>`, `end:<equipment_id>`,
    /// `competing:<failure_event_id>`.
    pub key: String,
    /// `failure` | `pm_suspension` | `end_of_period` | `competing_mode`.
    pub kind: String,
    pub equipment_id: i64,
    pub asset_code: String,
    pub failure_event_id: Option<i64>,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub di_id: Option<i64>,
    pub di_code: Option<String>,
    /// Instant used for the age: failure (detection) or removal / end of period.
    pub event_at: String,
    /// `repair_start` | `di` | `pm_completion` | `period_end`.
    pub event_time_source: String,
    pub restored_at: Option<String>,
    /// Age in the study time base; `None` when no renewal before it is known.
    pub age_hours: Option<f64>,
    pub included: bool,
    /// Code of the reason the record is excluded (`analyst`, `not_eligible`, `no_known_start`,
    /// `pm_not_confirmed`) or included by the analyst; `None` when the default applies.
    pub state_reason: Option<String>,
    /// Free text given by the analyst with the latest choice.
    pub analyst_note: Option<String>,
    pub failure_mode_code: Option<String>,
    pub failure_mode_label: Option<String>,
    pub failure_cause_label: Option<String>,
    pub failure_mechanism_label: Option<String>,
    pub cause_not_determined: bool,
    pub downtime_hours: Option<f64>,
    /// `di_earlier` | `cause_missing` | `mechanism_missing` | `mode_missing` | `no_known_start`.
    pub flags: Vec<String>,
    /// Request (DI) instant when it is earlier than the repair start.
    pub di_at: Option<String>,
    pub di_lead_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataCheck {
    /// `di_earlier` | `cause_missing` | `mode_missing` | `operating_hours` | `no_operating_hours`
    /// | `pm_to_confirm` | `uncoded_outside_mode`.
    pub kind: String,
    /// `warning` | `success` | `danger` | `info`.
    pub severity: String,
    pub record_key: Option<String>,
    pub equipment_id: Option<i64>,
    pub asset_code: Option<String>,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub di_code: Option<String>,
    pub hours: Option<f64>,
    pub count: Option<i64>,
    /// 0–1 (time-base coverage of the period).
    pub coverage: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetClockInfo {
    pub equipment_id: i64,
    pub asset_code: String,
    /// `run_hours_meter` | `runtime_logs` | `operating_schedule` | `calendar`.
    pub source: String,
    /// Share of the period covered by the time-base data (0–1).
    pub coverage: f64,
    /// Hours in the study time base over the whole period.
    pub period_hours: f64,
    /// Calendar hours of the period.
    pub calendar_hours: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordHistoryEntry {
    pub record_key: String,
    pub work_order_code: Option<String>,
    pub asset_code: Option<String>,
    pub action: String,
    pub reason: String,
    pub actor_name: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StudyDataset {
    pub study: ReliabilityStudy,
    /// Time base actually used: `operating` | `calendar`.
    pub effective_time_basis: String,
    pub clocks: Vec<AssetClockInfo>,
    pub records: Vec<StudyRecord>,
    pub checks: Vec<DataCheck>,
    pub failures_used: i64,
    pub suspensions_used: i64,
    pub excluded: i64,
    pub history: Vec<RecordHistoryEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetStudyRecordInput {
    pub study_id: i64,
    pub record_key: String,
    /// `exclude` | `include` | `use_di_time` | `use_repair_time`.
    pub action: String,
    #[serde(default)]
    pub reason: Option<String>,
}

// ── Life analysis (step 2) ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LifeFit {
    pub beta: f64,
    pub eta: f64,
    /// Two-sided confidence level of the bounds (0.90).
    pub confidence: f64,
    pub beta_lower: f64,
    pub beta_upper: f64,
    pub eta_lower: f64,
    pub eta_upper: f64,
    pub b10: f64,
    pub b10_lower: Option<f64>,
    pub b10_upper: Option<f64>,
    pub mean_life: f64,
    pub median_life: f64,
    /// Correlation coefficient of the probability plot (goodness of fit).
    pub rho: Option<f64>,
    /// `early_life` (β upper bound < 1) | `wear_out` (β lower bound > 1) | `random` (otherwise).
    pub pattern: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct PlotPoint {
    pub time: f64,
    pub median_rank: f64,
}

/// Fitted unreliability F(t) with Fisher-matrix bounds at one age.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct BandPoint {
    pub time: f64,
    pub unreliability: f64,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct CurvePoint {
    pub time: f64,
    pub reliability: f64,
    /// Failures per hour at that age.
    pub hazard: f64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct CostPoint {
    pub interval: f64,
    pub cost_rate: f64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct StrategyCost {
    /// Replacement age (hours); `None` for run-to-failure.
    pub interval: Option<f64>,
    pub cost_rate: f64,
    /// Cost per year for the study equipment (`cost_rate × operating hours per year`).
    pub annual_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CostAnalysis {
    pub planned_cost: f64,
    pub failure_cost: f64,
    pub run_to_failure: StrategyCost,
    pub current: Option<StrategyCost>,
    pub proposed: Option<StrategyCost>,
    pub rejection: Option<AgeReplacementRejection>,
    pub saving_vs_run_to_failure: Option<f64>,
    /// Annual saving of the proposed interval against current practice (positive = saves).
    pub annual_saving_vs_current: Option<f64>,
    pub curve: Vec<CostPoint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PmPlanCandidate {
    pub pm_plan_id: i64,
    pub code: String,
    pub title: String,
    pub equipment_id: i64,
    pub asset_code: String,
    /// `fixed` | `floating` | `meter`.
    pub strategy_type: String,
    pub interval_value: Option<f64>,
    /// Calendar unit (`day`, `week`, `month`, …) or `operating_hours` for meter plans.
    pub interval_unit: Option<String>,
    /// Current interval expressed in the study time base (hours).
    pub interval_hours: Option<f64>,
    /// Why the interval cannot be expressed in study hours, when it cannot.
    pub interval_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LifeAnalysis {
    pub effective_time_basis: String,
    pub n_failures: i64,
    pub n_suspensions: i64,
    pub min_failures: i64,
    pub fit: Option<LifeFit>,
    /// `not_enough_failures` | `no_finite_mle` | `no_covariance` when no fit is shown.
    pub fit_issue: Option<String>,
    pub plot_points: Vec<PlotPoint>,
    pub band: Vec<BandPoint>,
    pub curve: Vec<CurvePoint>,
    pub cost: Option<CostAnalysis>,
    /// `missing` | `study` | `class_default` | `input` (costs passed with the request).
    pub cost_source: String,
    pub cost_currency_code: Option<String>,
    /// Equipment class whose default costs apply (when every asset shares one).
    pub cost_class_id: Option<i64>,
    pub cost_class_name: Option<String>,
    pub pm_plans: Vec<PmPlanCandidate>,
    pub selected_pm_plan_id: Option<i64>,
    /// Operating hours per year of the study equipment, from the data (sum over equipment).
    pub operating_hours_per_year: Option<f64>,
    pub asset_count: i64,
    /// Study hours per calendar hour (1 for calendar basis), averaged over equipment.
    pub operating_ratio: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LifeAnalysisRequest {
    pub study_id: i64,
    /// Costs typed in the study (not yet saved); saved study costs / class defaults otherwise.
    #[serde(default)]
    pub planned_cost: Option<f64>,
    #[serde(default)]
    pub failure_cost: Option<f64>,
    /// PM plan to compare with; the study's plan, else the first candidate.
    #[serde(default)]
    pub pm_plan_id: Option<i64>,
}

// ── Decisions (step 6) ──────────────────────────────────────────────────────────────────────

pub const DECISION_PM_INTERVAL: &str = "pm_interval";
pub const DECISION_ONE_OFF_WO: &str = "one_off_wo";
pub const DECISION_MODIFICATION_WO: &str = "modification_wo";

pub const STATUS_PROPOSED: &str = "proposed";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_APPLIED: &str = "applied";
pub const STATUS_REJECTED: &str = "rejected";

#[derive(Debug, Clone, Deserialize)]
pub struct CreateDecisionInput {
    pub study_id: i64,
    pub title: String,
    /// `pm_interval` | `one_off_wo` | `modification_wo`.
    pub action_kind: String,
    #[serde(default)]
    pub equipment_id: Option<i64>,
    #[serde(default)]
    pub pm_plan_id: Option<i64>,
    #[serde(default)]
    pub proposed_interval_hours: Option<f64>,
    #[serde(default)]
    pub wo_type_code: Option<String>,
    #[serde(default)]
    pub approver_id: Option<i64>,
    #[serde(default)]
    pub justification: Option<String>,
    /// FMECA row and RCM decision the decision comes from (steps 3 and 4).
    #[serde(default)]
    pub fmeca_item_id: Option<i64>,
    #[serde(default)]
    pub rcm_decision_id: Option<i64>,
    /// Costs shown on screen when the decision is recorded (kept in the evidence).
    #[serde(default)]
    pub planned_cost: Option<f64>,
    #[serde(default)]
    pub failure_cost: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DecideDecisionInput {
    pub decision_id: i64,
    pub expected_row_version: i64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DecisionEvidence {
    pub time_basis: String,
    pub n_failures: i64,
    pub n_suspensions: i64,
    pub beta: Option<f64>,
    pub eta: Option<f64>,
    pub beta_lower: Option<f64>,
    pub beta_upper: Option<f64>,
    pub b10: Option<f64>,
    pub mean_life: Option<f64>,
    pub rho: Option<f64>,
    pub pattern: Option<String>,
    pub optimal_interval: Option<f64>,
    pub current_interval: Option<f64>,
    pub proposed_cost_rate: Option<f64>,
    pub current_cost_rate: Option<f64>,
    pub run_to_failure_cost_rate: Option<f64>,
    pub annual_saving_vs_current: Option<f64>,
    pub planned_cost: Option<f64>,
    pub failure_cost: Option<f64>,
    pub currency_code: Option<String>,
    pub operating_ratio: Option<f64>,
    pub study_period_start: String,
    pub study_period_end: String,
    /// FMECA row the decision answers: failure mode and RPN.
    #[serde(default)]
    pub fmeca_mode_label: Option<String>,
    #[serde(default)]
    pub rpn: Option<i64>,
    /// RCM tactic retained for that row.
    #[serde(default)]
    pub rcm_tactic: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DecisionEffect {
    /// `measured` | `not_yet_measured` | `not_applied`.
    pub state: String,
    pub mtbf_before: Option<f64>,
    pub mtbf_after: Option<f64>,
    /// `mtbf_after` is a lower bound (fewer than 2 failures since the change).
    pub after_is_lower_bound: bool,
    /// Relative change (after / before − 1).
    pub change: Option<f64>,
    pub failures_after: i64,
    pub hours_after: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StudyDecision {
    pub id: i64,
    pub study_id: i64,
    pub title: String,
    pub action_kind: String,
    pub equipment_id: Option<i64>,
    pub asset_code: Option<String>,
    pub pm_plan_id: Option<i64>,
    pub pm_plan_code: Option<String>,
    pub proposed_interval_hours: Option<f64>,
    /// Interval as the PM plan will store it (value + unit), for the approver.
    pub proposed_trigger_interval_value: Option<f64>,
    pub proposed_trigger_interval_unit: Option<String>,
    pub wo_type_code: Option<String>,
    pub justification: String,
    pub fmeca_item_id: Option<i64>,
    pub rcm_decision_id: Option<i64>,
    pub evidence: DecisionEvidence,
    pub status: String,
    pub approver_id: Option<i64>,
    pub approver_name: Option<String>,
    pub proposed_by_id: Option<i64>,
    pub proposed_by_name: Option<String>,
    pub decided_by_id: Option<i64>,
    pub proposed_at: String,
    pub decided_by_name: Option<String>,
    /// Approved (or applied) by the person who proposed it: under its site's opt-out, or an older
    /// record. Derived at read.
    pub self_approved: bool,
    /// The decision's site opted out of four-eyes: its proposer may approve it.
    pub site_allows_self_approval: bool,
    pub decided_at: Option<String>,
    pub decision_note: Option<String>,
    pub applied_at: Option<String>,
    pub linked_pm_plan_id: Option<i64>,
    pub linked_pm_plan_version_id: Option<i64>,
    pub linked_pm_version_no: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub linked_work_order_code: Option<String>,
    pub effect: DecisionEffect,
    /// Refused at synchronisation (here, or on another PC): never applied, its PM version never
    /// published (AU-76b). `sync_refused_reason` says why ("refusé (sync) : … — reason").
    pub sync_refused_at: Option<String>,
    pub sync_refused_reason: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApproverOption {
    pub user_id: i64,
    pub display_name: String,
}
