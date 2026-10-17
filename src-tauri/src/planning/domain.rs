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
    /// People the job takes (work order / PM version); labour hours = duration × crew size.
    #[serde(default)]
    pub planned_crew_size: Option<i64>,
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
    /// Business code of the source WO / DI / PM plan (enriched via JOIN).
    pub source_code: Option<String>,
    /// Human-readable title of the source record (enriched via JOIN).
    pub source_title: Option<String>,
    pub assigned_team_label: Option<String>,
    pub assigned_personnel_name: Option<String>,
    /// Level of the shared DI / OT urgency scale held by `priority_id`.
    pub priority_label: Option<String>,
    pub priority_code: Option<String>,
    /// 1 = most urgent … 5 = least urgent (backlog order).
    pub priority_rank: Option<i64>,
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
    /// Each open conflict with its reason and details: what the card explains.
    #[serde(default)]
    pub reasons: Vec<ConflictReason>,
}

/// Why a job cannot be committed (or why its bar is critical): the reason code and its details
/// (`details_json`, a JSON object whose fields depend on the reason).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReason {
    pub conflict_type: String,
    pub reason_code: String,
    pub severity: String,
    pub details_json: Option<String>,
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
    /// History only: capacity comes from the members' Classes horaires.
    pub available_hours_per_day: f64,
    /// Overtime ceiling per day — what planning adds on top of the team's capacity.
    pub max_overtime_hours_per_day: f64,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningWindow {
    pub id: i64,
    /// The site (org node) the window applies to, with its sub-nodes; none = plant-wide.
    pub entity_id: Option<i64>,
    /// The one equipment of the site the window applies to; none = the whole site.
    pub equipment_id: Option<i64>,
    /// A code of the SYSTEM list `PLANNING.WINDOW_TYPES`.
    pub window_type: String,
    pub start_datetime: String,
    pub end_datetime: String,
    pub is_locked: i64,
    pub lock_reason: Option<String>,
    /// 1 when the site must be re-entered (a window from before migration 211, whose site was a
    /// team id): it applies plant-wide until then.
    pub scope_review_required: i64,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
    /// Names for display (read-only).
    pub entity_name: Option<String>,
    pub equipment_code: Option<String>,
    pub equipment_name: Option<String>,
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
    /// 1 when the job has open conflicts now, or was forced over a capacity overload or a locked
    /// window: the bar shows critical.
    pub has_blocking_conflict: i64,
    /// The job's open conflicts (JSON array of `{conflict_type, reason_code, severity,
    /// details_json}`): why the bar is critical. Read-only.
    #[serde(default)]
    pub conflict_reasons_json: Option<String>,
    pub nearest_feasible_window: Option<String>,
    /// `committed` | `released` | `cancelled` | `closed` (`planning::lifecycle`).
    pub status: String,
    pub status_reason: Option<String>,
    pub status_changed_at: Option<String>,
    /// Labour hours the commitment books (`planning::load`); `None` for a commitment from before
    /// migration 205 without an estimate (its slot length then counts).
    #[serde(default)]
    pub planned_labor_hours: Option<f64>,
    /// Hours the assigned technician works on it (the job's duration, migration 210); `None` for a
    /// commitment from before (its slot then counts for the technician).
    #[serde(default)]
    pub planned_person_hours: Option<f64>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub source_code: Option<String>,
    pub source_title: Option<String>,
    pub assigned_team_label: Option<String>,
    pub assigned_personnel_name: Option<String>,
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
    pub actor_display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleBreakIn {
    pub id: i64,
    pub schedule_commitment_id: i64,
    pub break_in_reason: String,
    pub approved_by_user_id: Option<i64>,
    pub approved_by_personnel_id: Option<i64>,
    pub override_reason: Option<String>,
    /// `move`: a held slot moved (its old slot recorded); `insert`: new work entering a frozen
    /// week (no old slot).
    pub kind: String,
    pub old_slot_start: Option<String>,
    pub old_slot_end: Option<String>,
    pub new_slot_start: String,
    pub new_slot_end: String,
    pub old_assignee_id: Option<i64>,
    pub new_assignee_id: Option<i64>,
    pub cost_impact_delta: Option<f64>,
    pub notification_dedupe_key: Option<String>,
    pub row_version: i64,
    pub created_by_id: Option<i64>,
    pub created_at: String,
    pub commitment_source_code: Option<String>,
    pub approved_by_display_name: Option<String>,
    pub old_assignee_name: Option<String>,
    pub new_assignee_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamCapacityLoad {
    pub team_id: i64,
    pub work_date: String,
    pub available_hours: f64,
    pub overtime_hours: f64,
    /// Labour hours committed that day (`planning::load`: spread over working days, finished work
    /// excluded).
    pub committed_hours: f64,
    /// committed ÷ (available + overtime); `None` when the team has no hours that day (see `basis`)
    /// — never shown as 100 %.
    pub utilization_ratio: Option<f64>,
    /// Where `available_hours` comes from: `roster` (the members' Classes horaires minus
    /// absences, × utilisation), or why there are none: `rest_day`, `holiday`, `no_schedule`
    /// (no Classe horaire) or `no_members`.
    #[serde(default)]
    pub basis: String,
    /// The members' shift windows that day and the hours they bring: a slot is spread over the
    /// working time it covers, exactly as the capacity gate does (`planning::load`).
    #[serde(default)]
    pub work_windows: Vec<CapacityWindow>,
    /// Hours of members whose shift window is not known (counted in full on the days a slot touches).
    #[serde(default)]
    pub unwindowed_hours: f64,
}

/// The assignee picker's question: who in the crew can take this slot, and with how many hours
/// left. `candidate_id` (a job to commit) or `commitment_id` (a commitment being moved) gives the
/// job's hours per person, so each member's refusals can be shown before committing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssigneeCapacityInput {
    pub team_id: i64,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub candidate_id: Option<i64>,
    #[serde(default)]
    pub commitment_id: Option<i64>,
}

/// One member's shift window (local time, `YYYY-MM-DDTHH:MM:SS`; a night shift ends the next day).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapacityWindow {
    pub start: String,
    pub end: String,
    pub hours: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningAssigneeLane {
    pub personnel_id: i64,
    pub full_name: String,
    pub blocked_intervals_json: String,
    pub commitments_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningCrewLane {
    pub team_id: i64,
    pub team_code: String,
    pub team_name: String,
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
    /// Populated when `team_id` filter is None (plant-wide All Crews overview).
    #[serde(default)]
    pub crew_lanes: Vec<PlanningCrewLane>,
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
    /// No longer counts toward capacity (it comes from the members' Classes horaires); kept on
    /// older rules as history. Stored as 0 when not given.
    #[serde(default)]
    pub available_hours_per_day: Option<f64>,
    /// The planner's overtime ceiling per day: the part of the rule planning uses.
    pub max_overtime_hours_per_day: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCapacityRuleInput {
    pub effective_start: Option<String>,
    /// A new end; leave `None` to keep the current one (see `clear_effective_end`).
    pub effective_end: Option<String>,
    /// The rule applies with no end again (an end date set by mistake is removed).
    #[serde(default)]
    pub clear_effective_end: bool,
    pub available_hours_per_day: Option<f64>,
    pub max_overtime_hours_per_day: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningWindowFilter {
    pub entity_id: Option<i64>,
    #[serde(default)]
    pub equipment_id: Option<i64>,
    pub window_type: Option<String>,
    pub include_locked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePlanningWindowInput {
    pub entity_id: Option<i64>,
    #[serde(default)]
    pub equipment_id: Option<i64>,
    pub window_type: String,
    pub start_datetime: String,
    pub end_datetime: String,
    pub is_locked: Option<bool>,
    pub lock_reason: Option<String>,
}

/// A field that is present (possibly `null`) deserialises to `Some(value)`; an absent one stays `None`.
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePlanningWindowInput {
    /// Absent = unchanged; `null` = plant-wide.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<Option<i64>>,
    /// Absent = unchanged; `null` = the whole site.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub equipment_id: Option<Option<i64>>,
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
    /// The assignee refusals (`PLANNING_ASSIGNEE_*`) the planner saw and overrides with the reason:
    /// each refusal must be listed, so a reason given for one never covers another.
    #[serde(default)]
    pub acknowledged_assignee_issues: Vec<String>,
    /// Committing into a frozen week: only as a recorded break-in (`planning::weekly_schedule`).
    #[serde(default)]
    pub frozen_week_break_in: Option<CommitBreakIn>,
}

/// Why new work enters a frozen week, and the evidence an urgent reason needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitBreakIn {
    /// A code of the COMPANY list `PLANNING.BREAK_IN_REASONS`.
    pub reason: String,
    pub approved_by_user_id: Option<i64>,
    pub dangerous_override_reason: Option<String>,
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
    /// The assignee refusals (`PLANNING_ASSIGNEE_*`) the planner saw and overrides with the reason:
    /// each refusal must be listed, so a reason given for one never covers another.
    #[serde(default)]
    pub acknowledged_assignee_issues: Vec<String>,
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
    /// "Unassigned" chosen explicitly: the technician is removed (otherwise kept when none is sent).
    #[serde(default)]
    pub clear_assignee: Option<bool>,
    pub bypass_availability: Option<bool>,
    pub override_reason: Option<String>,
    pub dangerous_override_reason: Option<String>,
    /// The assignee refusals (`PLANNING_ASSIGNEE_*`) the planner saw and overrides with the reason:
    /// each refusal must be listed, so a reason given for one never covers another.
    #[serde(default)]
    pub acknowledged_assignee_issues: Vec<String>,
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
