use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionTemplate {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub name: String,
    pub org_scope_id: Option<i64>,
    pub route_scope: Option<String>,
    pub estimated_duration_minutes: Option<i64>,
    pub is_active: bool,
    pub current_version_id: Option<i64>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionTemplateVersion {
    pub id: i64,
    pub entity_sync_id: String,
    pub template_id: i64,
    pub version_no: i64,
    pub effective_from: Option<String>,
    pub checkpoint_package_json: String,
    pub tolerance_rules_json: Option<String>,
    pub escalation_rules_json: Option<String>,
    pub requires_review: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionCheckpoint {
    pub id: i64,
    pub entity_sync_id: String,
    pub template_version_id: i64,
    pub sequence_order: i64,
    pub asset_id: Option<i64>,
    pub component_id: Option<i64>,
    pub checkpoint_code: String,
    /// Instruction shown to the technician in the field.
    pub description: Option<String>,
    pub check_type: String,
    /// Unit of a measurement (`reference_values.id` of `MEASURE.UNITS`).
    pub measurement_unit_id: Option<i64>,
    /// Symbol of that unit, read from the reference data (never stored on the checkpoint).
    pub measurement_unit: Option<String>,
    pub normal_min: Option<f64>,
    pub normal_max: Option<f64>,
    pub warning_min: Option<f64>,
    pub warning_max: Option<f64>,
    pub requires_photo: bool,
    pub requires_comment_on_exception: bool,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRound {
    pub id: i64,
    pub entity_sync_id: String,
    pub template_id: i64,
    pub template_version_id: i64,
    pub scheduled_at: Option<String>,
    pub assigned_to_id: Option<i64>,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub reviewed_at: Option<String>,
    pub reviewed_by_id: Option<i64>,
    pub review_note: Option<String>,
    pub completion_percentage: Option<f64>,
    pub row_version: i64,
    /// Schedule that generated the round (`None` for a round scheduled by hand).
    pub schedule_id: Option<i64>,
    /// Identity of the occurrence in its schedule (`calendar:<due>` or `meter:<meter>:<threshold>`).
    pub due_basis: Option<String>,
    /// The round may still be done until then; an unstarted round is `missed` afterwards.
    pub window_end_at: Option<String>,
    pub assigned_team_id: Option<i64>,
    /// Meter threshold that triggered a meter-based round.
    pub due_meter_value: Option<f64>,
    pub missed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionCheckpointDraft {
    pub sequence_order: i64,
    pub asset_id: Option<i64>,
    pub component_id: Option<i64>,
    pub checkpoint_code: String,
    #[serde(default)]
    pub description: Option<String>,
    pub check_type: String,
    /// Unit of a measurement: an active unit of `MEASURE.UNITS`.
    #[serde(default)]
    pub measurement_unit_id: Option<i64>,
    pub normal_min: Option<f64>,
    pub normal_max: Option<f64>,
    pub warning_min: Option<f64>,
    pub warning_max: Option<f64>,
    pub requires_photo: Option<bool>,
    pub requires_comment_on_exception: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInspectionTemplateInput {
    pub code: String,
    pub name: String,
    pub org_scope_id: Option<i64>,
    pub route_scope: Option<String>,
    pub estimated_duration_minutes: Option<i64>,
    pub is_active: Option<bool>,
    pub checkpoints: Vec<InspectionCheckpointDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishInspectionTemplateVersionInput {
    pub template_id: i64,
    pub expected_row_version: i64,
    pub effective_from: Option<String>,
    pub requires_review: Option<bool>,
    pub tolerance_rules_json: Option<String>,
    pub escalation_rules_json: Option<String>,
    pub checkpoints: Vec<InspectionCheckpointDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleInspectionRoundInput {
    pub template_id: i64,
    pub scheduled_at: Option<String>,
    pub assigned_to_id: Option<i64>,
    pub explicit_template_version_id: Option<i64>,
    #[serde(default)]
    pub assigned_team_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionTemplateVersionsFilter {
    pub template_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionCheckpointsFilter {
    pub template_version_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionResult {
    pub id: i64,
    pub entity_sync_id: String,
    pub round_id: i64,
    pub checkpoint_id: i64,
    pub result_status: String,
    pub numeric_value: Option<f64>,
    pub text_value: Option<String>,
    pub boolean_value: Option<bool>,
    pub comment: Option<String>,
    pub recorded_at: String,
    pub recorded_by_id: i64,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionEvidence {
    pub id: i64,
    pub result_id: i64,
    pub evidence_type: String,
    pub file_path_or_value: String,
    pub captured_at: String,
    pub entity_sync_id: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionAnomaly {
    pub id: i64,
    pub round_id: i64,
    pub result_id: Option<i64>,
    pub anomaly_type: String,
    pub severity: i64,
    pub description: String,
    pub linked_di_id: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub requires_permit_review: bool,
    pub resolution_status: String,
    pub routing_decision: Option<String>,
    pub entity_sync_id: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordInspectionResultInput {
    pub round_id: i64,
    pub checkpoint_id: i64,
    pub result_status: Option<String>,
    pub numeric_value: Option<f64>,
    pub text_value: Option<String>,
    pub boolean_value: Option<bool>,
    pub comment: Option<String>,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddInspectionEvidenceInput {
    pub result_id: i64,
    pub evidence_type: String,
    pub file_path_or_value: String,
    pub captured_at: Option<String>,
    pub expected_row_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInspectionAnomalyInput {
    pub id: i64,
    pub resolution_status: String,
    pub linked_di_id: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    pub requires_permit_review: Option<bool>,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionResultsFilter {
    pub round_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionEvidenceFilter {
    pub result_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionAnomaliesFilter {
    pub round_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionReliabilitySignal {
    pub id: i64,
    pub entity_sync_id: String,
    pub equipment_id: i64,
    pub period_start: String,
    pub period_end: String,
    pub warning_count: i64,
    pub fail_count: i64,
    pub anomaly_open_count: i64,
    pub checkpoint_coverage_ratio: f64,
    pub row_version: i64,
    pub asset_code: Option<String>,
    pub asset_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionReliabilitySignalsFilter {
    pub equipment_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshInspectionReliabilitySignalsInput {
    pub window_days: i64,
}

// ── Field runner (Phase 6.2) ─────────────────────────────────────────────────────────────

/// A round as listed for people: labels resolved, never raw ids in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRoundSummary {
    pub round: InspectionRound,
    pub template_code: String,
    pub template_name: String,
    pub version_no: i64,
    /// The version asks for a supervisor review even after a clean round.
    pub requires_review: bool,
    pub assigned_to_name: Option<String>,
    pub assigned_team_name: Option<String>,
    pub schedule_code: Option<String>,
    pub schedule_name: Option<String>,
    /// Still to do (scheduled, released or in progress) and its planned time has passed.
    pub is_overdue: bool,
    pub reviewed_by_name: Option<String>,
    pub checkpoint_count: i64,
    pub recorded_count: i64,
    pub exception_count: i64,
    pub open_anomaly_count: i64,
    /// Rows of this round saved on this device and not yet accepted by the server.
    pub pending_sync_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionRoundSummaryFilter {
    #[serde(default)]
    pub statuses: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    /// Only rounds still to do whose planned time has passed.
    #[serde(default)]
    pub overdue_only: Option<bool>,
    /// Planned between these instants (RFC 3339, inclusive).
    #[serde(default)]
    pub due_from: Option<String>,
    #[serde(default)]
    pub due_to: Option<String>,
    #[serde(default)]
    pub schedule_id: Option<i64>,
    /// Checklist name or code, assignee, team or schedule containing this text.
    #[serde(default)]
    pub search: Option<String>,
}

/// One page of rounds and the number of rounds matching the filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRoundPage {
    pub items: Vec<InspectionRoundSummary>,
    pub total: i64,
}

/// A previous result of the same checkpoint (same template, same checkpoint code) in another round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionCheckpointHistoryPoint {
    pub round_id: i64,
    pub round_scheduled_at: Option<String>,
    pub recorded_at: String,
    pub result_status: String,
    pub numeric_value: Option<f64>,
    pub text_value: Option<String>,
    pub boolean_value: Option<bool>,
    pub comment: Option<String>,
}

/// One checkpoint in the runner, with its labels, current result and sync state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRunnerCheckpoint {
    pub checkpoint: InspectionCheckpoint,
    pub asset_code: Option<String>,
    pub asset_name: Option<String>,
    pub result: Option<InspectionResult>,
    pub evidence: Vec<InspectionEvidence>,
    pub open_anomalies: Vec<InspectionAnomaly>,
    /// `not_recorded`, `waiting` (saved here, not yet accepted by the server), `sent` or `error`.
    pub sync_state: String,
    /// What is still required before the round can be completed (machine codes).
    pub missing: Vec<String>,
    /// Latest results of this checkpoint in earlier rounds, newest first.
    pub history: Vec<InspectionCheckpointHistoryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRoundRunner {
    pub summary: InspectionRoundSummary,
    pub checkpoints: Vec<InspectionRunnerCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionRoundActionInput {
    pub round_id: i64,
    pub expected_row_version: i64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddInspectionPhotoInput {
    pub result_id: i64,
    pub file_name: String,
    pub mime_type: String,
    pub file_bytes: Vec<u8>,
}

// ── Recurring rounds (Phase 6.3) ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionSchedule {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub name: String,
    pub template_id: i64,
    /// `calendar` or `meter`.
    pub trigger_kind: String,
    pub interval_unit: Option<String>,
    pub interval_value: Option<i64>,
    pub anchor_at: Option<String>,
    pub meter_id: Option<i64>,
    pub meter_interval: Option<f64>,
    pub meter_start_reading: Option<f64>,
    pub assigned_to_id: Option<i64>,
    pub assigned_team_id: Option<i64>,
    pub grace_hours: i64,
    pub horizon_days: i64,
    pub is_active: bool,
    pub last_generated_at: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A schedule as listed for people: labels resolved and the state of its rounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionScheduleSummary {
    pub schedule: InspectionSchedule,
    pub template_code: String,
    pub template_name: String,
    pub assigned_to_name: Option<String>,
    pub assigned_team_name: Option<String>,
    pub meter_name: Option<String>,
    pub meter_unit: Option<String>,
    pub meter_asset_id: Option<i64>,
    pub meter_asset_code: Option<String>,
    pub meter_lifetime_reading: Option<f64>,
    /// Earliest planned time of a round still to do.
    pub next_due_at: Option<String>,
    pub open_round_count: i64,
    pub overdue_round_count: i64,
    pub missed_round_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveInspectionScheduleInput {
    /// `None` creates a schedule.
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub expected_row_version: Option<i64>,
    pub code: String,
    pub name: String,
    pub template_id: i64,
    pub trigger_kind: String,
    #[serde(default)]
    pub interval_unit: Option<String>,
    #[serde(default)]
    pub interval_value: Option<i64>,
    #[serde(default)]
    pub anchor_at: Option<String>,
    #[serde(default)]
    pub meter_id: Option<i64>,
    #[serde(default)]
    pub meter_interval: Option<f64>,
    #[serde(default)]
    pub meter_start_reading: Option<f64>,
    #[serde(default)]
    pub assigned_to_id: Option<i64>,
    #[serde(default)]
    pub assigned_team_id: Option<i64>,
    pub grace_hours: i64,
    pub horizon_days: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerateInspectionRoundsInput {
    /// Generation instant (RFC 3339); now when omitted.
    #[serde(default)]
    pub as_of: Option<String>,
    /// Only this schedule; every active schedule when omitted.
    #[serde(default)]
    pub schedule_id: Option<i64>,
}

/// A schedule that could not generate its rounds (the others still did).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionScheduleIssue {
    pub schedule_code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerateInspectionRoundsResult {
    pub generated: i64,
    /// Occurrences that already had their round.
    pub skipped: i64,
    /// Rounds whose window closed before anyone started them, marked `missed` by this run.
    pub missed: i64,
    pub round_ids: Vec<i64>,
    pub issues: Vec<InspectionScheduleIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionKpisInput {
    /// Length of the period ending now that the due / done / missed / compliance figures cover.
    pub period_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionKpis {
    pub period_start: String,
    pub period_end: String,
    /// Rounds planned in the period (cancelled rounds excluded).
    pub due_count: i64,
    /// Of those, completed or reviewed.
    pub done_count: i64,
    /// Of those, completed before the end of their window.
    pub done_on_time_count: i64,
    /// Of those, missed.
    pub missed_count: i64,
    /// Rounds still to do whose planned time has passed (any date).
    pub overdue_count: i64,
    /// `done_on_time_count / due_count` in percent; `None` when nothing was due.
    pub compliance_pct: Option<f64>,
    pub open_anomaly_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InspectionAnomalyRowFilter {
    #[serde(default)]
    pub statuses: Option<Vec<String>>,
    #[serde(default)]
    pub round_id: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// An anomaly with everything needed to decide on it and to write its follow-up request: the
/// round, the checkpoint and its limits, the equipment, the reading, and the request or work
/// order created from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionAnomalyRow {
    pub anomaly: InspectionAnomaly,
    pub round_status: String,
    pub round_scheduled_at: Option<String>,
    pub round_completed_at: Option<String>,
    pub template_code: String,
    pub template_name: String,
    pub checkpoint_id: Option<i64>,
    pub checkpoint_code: Option<String>,
    pub checkpoint_description: Option<String>,
    pub check_type: Option<String>,
    pub measurement_unit: Option<String>,
    pub normal_min: Option<f64>,
    pub normal_max: Option<f64>,
    pub warning_min: Option<f64>,
    pub warning_max: Option<f64>,
    pub asset_id: Option<i64>,
    pub asset_code: Option<String>,
    pub asset_name: Option<String>,
    /// The equipment has an installation location (a request needs one).
    pub asset_has_location: bool,
    pub result_status: Option<String>,
    pub numeric_value: Option<f64>,
    pub text_value: Option<String>,
    pub result_comment: Option<String>,
    pub recorded_at: Option<String>,
    /// `above` or `below` when the reading is outside its limits.
    pub violation_direction: Option<String>,
    /// The limit the reading crossed.
    pub violation_limit: Option<f64>,
    pub linked_di_code: Option<String>,
    pub linked_di_title: Option<String>,
    pub linked_di_status: Option<String>,
    pub linked_wo_code: Option<String>,
    pub linked_wo_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionAnomalyPage {
    pub items: Vec<InspectionAnomalyRow>,
    pub total: i64,
}
