use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleCandidate {
    pub id: i64,
    pub source_type: String,
    pub source_id: i64,
    pub source_di_id: Option<i64>,
    pub readiness_status: String,
    pub readiness_score: f64,
    pub priority_id: Option<i64>,
    pub required_skill_set_json: Option<String>,
    pub required_parts_ready: i64,
    pub permit_status: String,
    pub shutdown_requirement: Option<String>,
    pub prerequisite_status: String,
    pub estimated_duration_hours: Option<f64>,
    pub assigned_personnel_id: Option<i64>,
    pub assigned_team_id: Option<i64>,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
    pub suggested_assignees_json: Option<String>,
    pub availability_conflict_count: i64,
    pub skill_match_score: Option<f64>,
    pub estimated_labor_cost_range_json: Option<String>,
    pub blocking_flags_json: Option<String>,
    pub open_work_count: Option<i64>,
    pub next_available_window: Option<String>,
    pub estimated_assignment_risk: Option<f64>,
    pub risk_reason_codes_json: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulingConflict {
    pub id: i64,
    pub candidate_id: i64,
    pub conflict_type: String,
    pub reference_type: Option<String>,
    pub reference_id: Option<i64>,
    pub reason_code: String,
    pub severity: String,
    pub details_json: Option<String>,
    pub resolved_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleCandidateFilter {
    pub source_type: Option<String>,
    pub readiness_status: Option<String>,
    pub assigned_personnel_id: Option<i64>,
    pub include_resolved_conflicts: Option<bool>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateConflictSummary {
    pub candidate_id: i64,
    pub blocker_codes: Vec<String>,
    pub blocker_dimensions: Vec<String>,
    pub readiness_status: String,
    pub readiness_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleBacklogSnapshot {
    pub as_of: String,
    pub candidate_count: i64,
    pub ready_count: i64,
    pub blocked_count: i64,
    pub candidates: Vec<ScheduleCandidate>,
    pub conflict_summary: Vec<CandidateConflictSummary>,
    pub derivation_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshScheduleCandidatesInput {
    pub include_work_orders: Option<bool>,
    pub include_pm_occurrences: Option<bool>,
    pub include_approved_di: Option<bool>,
    pub limit_per_source: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshScheduleCandidatesResult {
    pub inserted_count: i64,
    pub updated_count: i64,
    pub evaluated_count: i64,
    pub ready_count: i64,
    pub blocked_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityRule {
    pub id: i64,
    pub entity_id: Option<i64>,
    pub team_id: i64,
    pub effective_start: String,
    pub effective_end: Option<String>,
    pub available_hours_per_day: f64,
    pub max_overtime_hours_per_day: f64,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningWindow {
    pub id: i64,
    pub entity_id: Option<i64>,
    pub window_type: String,
    pub start_datetime: String,
    pub end_datetime: String,
    pub is_locked: i64,
    pub lock_reason: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleCommitment {
    pub id: i64,
    pub schedule_candidate_id: i64,
    pub source_type: String,
    pub source_id: i64,
    pub schedule_period_start: String,
    pub schedule_period_end: String,
    pub committed_start: String,
    pub committed_end: String,
    pub assigned_team_id: i64,
    pub assigned_personnel_id: Option<i64>,
    pub committed_by_id: Option<i64>,
    pub frozen_at: Option<String>,
    pub estimated_labor_cost: Option<f64>,
    pub budget_threshold: Option<f64>,
    pub cost_variance_warning: i64,
    pub has_blocking_conflict: i64,
    pub nearest_feasible_window: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleChangeLogEntry {
    pub id: i64,
    pub commitment_id: Option<i64>,
    pub action_type: String,
    pub actor_id: Option<i64>,
    pub field_changed: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub reason_code: Option<String>,
    pub reason_note: Option<String>,
    pub reason: Option<String>,
    pub details_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleBreakIn {
    pub id: i64,
    pub schedule_commitment_id: i64,
    pub break_in_reason: String,
    pub approved_by_user_id: Option<i64>,
    pub approved_by_personnel_id: Option<i64>,
    pub override_reason: Option<String>,
    pub old_slot_start: String,
    pub old_slot_end: String,
    pub new_slot_start: String,
    pub new_slot_end: String,
    pub old_assignee_id: Option<i64>,
    pub new_assignee_id: Option<i64>,
    pub cost_impact_delta: Option<f64>,
    pub notification_dedupe_key: Option<String>,
    pub row_version: i64,
    pub created_by_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamCapacityLoad {
    pub team_id: i64,
    pub work_date: String,
    pub available_hours: f64,
    pub overtime_hours: f64,
    pub committed_hours: f64,
    pub utilization_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningAssigneeLane {
    pub personnel_id: i64,
    pub full_name: String,
    pub blocked_intervals_json: String,
    pub commitments_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningGanttSnapshot {
    pub period_start: String,
    pub period_end: String,
    pub commitments: Vec<ScheduleCommitment>,
    pub locked_windows: Vec<PlanningWindow>,
    pub capacity: Vec<TeamCapacityLoad>,
    pub assignee_lanes: Vec<PlanningAssigneeLane>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityRuleFilter {
    pub entity_id: Option<i64>,
    pub team_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCapacityRuleInput {
    pub entity_id: Option<i64>,
    pub team_id: i64,
    pub effective_start: String,
    pub effective_end: Option<String>,
    pub available_hours_per_day: f64,
    pub max_overtime_hours_per_day: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCapacityRuleInput {
    pub effective_start: Option<String>,
    pub effective_end: Option<String>,
    pub available_hours_per_day: Option<f64>,
    pub max_overtime_hours_per_day: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningWindowFilter {
    pub entity_id: Option<i64>,
    pub window_type: Option<String>,
    pub include_locked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePlanningWindowInput {
    pub entity_id: Option<i64>,
    pub window_type: String,
    pub start_datetime: String,
    pub end_datetime: String,
    pub is_locked: Option<bool>,
    pub lock_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePlanningWindowInput {
    pub window_type: Option<String>,
    pub start_datetime: Option<String>,
    pub end_datetime: Option<String>,
    pub is_locked: Option<bool>,
    pub lock_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleCommitmentFilter {
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub team_id: Option<i64>,
    pub personnel_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduleCommitmentInput {
    pub schedule_candidate_id: i64,
    pub expected_candidate_row_version: Option<i64>,
    pub committed_start: String,
    pub committed_end: String,
    pub assigned_team_id: i64,
    pub assigned_personnel_id: Option<i64>,
    pub allow_double_booking_override: Option<bool>,
    pub override_reason: Option<String>,
    pub budget_threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RescheduleCommitmentInput {
    pub commitment_id: i64,
    pub expected_row_version: i64,
    pub committed_start: String,
    pub committed_end: String,
    pub assigned_team_id: i64,
    pub assigned_personnel_id: Option<i64>,
    pub allow_double_booking_override: Option<bool>,
    pub override_reason: Option<String>,
    pub budget_threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeSchedulePeriodInput {
    pub period_start: String,
    pub period_end: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleBreakInFilter {
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub break_in_reason: Option<String>,
    pub approved_by_user_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduleBreakInInput {
    pub schedule_commitment_id: i64,
    pub expected_commitment_row_version: i64,
    pub break_in_reason: String,
    pub approved_by_user_id: Option<i64>,
    pub new_slot_start: String,
    pub new_slot_end: String,
    pub new_assigned_team_id: Option<i64>,
    pub new_assigned_personnel_id: Option<i64>,
    pub bypass_availability: Option<bool>,
    pub bypass_qualification: Option<bool>,
    pub override_reason: Option<String>,
    pub dangerous_override_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyTeamsInput {
    pub period_start: String,
    pub period_end: String,
    pub team_id: Option<i64>,
    pub include_break_ins: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyTeamsResult {
    pub emitted_count: i64,
    pub skipped_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningGanttFilter {
    pub period_start: String,
    pub period_end: String,
    pub team_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPlanningGanttPdfInput {
    pub period_start: String,
    pub period_end: String,
    pub team_id: Option<i64>,
    pub paper_size: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedBinaryDocument {
    pub file_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

