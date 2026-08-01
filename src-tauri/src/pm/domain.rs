use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlan {
    pub id: i64,
    pub code: String,
    pub title: String,
    pub description: Option<String>,
    pub asset_scope_type: String,
    pub asset_scope_id: Option<i64>,
    pub strategy_type: String,
    pub criticality_value_id: Option<i64>,
    pub criticality_code: Option<String>,
    pub criticality_label: Option<String>,
    pub assigned_group_id: Option<i64>,
    pub requires_shutdown: i64,
    pub requires_permit: i64,
    pub is_active: i64,
    pub lifecycle_status: String,
    pub current_version_id: Option<i64>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanVersion {
    pub id: i64,
    pub pm_plan_id: i64,
    pub version_no: i64,
    pub status: String,
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub trigger_definition_json: String,
    pub task_package_json: Option<String>,
    pub required_parts_json: Option<String>,
    pub required_skills_json: Option<String>,
    pub required_tools_json: Option<String>,
    pub estimated_duration_hours: Option<f64>,
    pub estimated_labor_cost: Option<f64>,
    pub estimated_parts_cost: Option<f64>,
    pub estimated_service_cost: Option<f64>,
    pub change_reason: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanFilter {
    pub search: Option<String>,
    pub strategy_type: Option<String>,
    pub lifecycle_status: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePmPlanInput {
    pub code: String,
    pub title: String,
    pub description: Option<String>,
    pub asset_scope_type: String,
    pub asset_scope_id: Option<i64>,
    pub strategy_type: String,
    pub criticality_value_id: Option<i64>,
    pub assigned_group_id: Option<i64>,
    pub requires_shutdown: bool,
    pub requires_permit: bool,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePmPlanInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub asset_scope_type: Option<String>,
    pub asset_scope_id: Option<i64>,
    pub strategy_type: Option<String>,
    pub criticality_value_id: Option<i64>,
    pub assigned_group_id: Option<i64>,
    pub requires_shutdown: Option<bool>,
    pub requires_permit: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionPmPlanLifecycleInput {
    pub plan_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePmPlanVersionInput {
    pub effective_from: String,
    pub effective_to: Option<String>,
    pub trigger_definition_json: String,
    pub task_package_json: Option<String>,
    pub required_parts_json: Option<String>,
    pub required_skills_json: Option<String>,
    pub required_tools_json: Option<String>,
    pub estimated_duration_hours: Option<f64>,
    pub estimated_labor_cost: Option<f64>,
    pub estimated_parts_cost: Option<f64>,
    pub estimated_service_cost: Option<f64>,
    pub change_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePmPlanVersionInput {
    pub effective_from: Option<String>,
    pub effective_to: Option<String>,
    pub trigger_definition_json: Option<String>,
    pub task_package_json: Option<String>,
    pub required_parts_json: Option<String>,
    pub required_skills_json: Option<String>,
    pub required_tools_json: Option<String>,
    pub estimated_duration_hours: Option<f64>,
    pub estimated_labor_cost: Option<f64>,
    pub estimated_parts_cost: Option<f64>,
    pub estimated_service_cost: Option<f64>,
    pub change_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishPmPlanVersionInput {
    pub version_id: i64,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmOccurrence {
    pub id: i64,
    pub pm_plan_id: i64,
    pub plan_version_id: i64,
    pub due_basis: String,
    pub due_at: Option<String>,
    pub due_meter_value: Option<f64>,
    pub generated_at: String,
    pub status: String,
    pub linked_work_order_id: Option<i64>,
    pub linked_work_order_code: Option<String>,
    pub deferral_reason: Option<String>,
    pub missed_reason: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub plan_code: Option<String>,
    pub plan_title: Option<String>,
    pub strategy_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmOccurrenceFilter {
    pub pm_plan_id: Option<i64>,
    pub status: Option<String>,
    pub due_from: Option<String>,
    pub due_to: Option<String>,
    pub include_completed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratePmOccurrencesInput {
    pub as_of: Option<String>,
    pub horizon_days: Option<i64>,
    pub pm_plan_id: Option<i64>,
    pub event_codes: Option<Vec<String>>,
    pub condition_codes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratePmOccurrencesResult {
    pub generated_count: i64,
    pub skipped_count: i64,
    pub trigger_events_recorded: i64,
    pub occurrence_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionPmOccurrenceInput {
    pub occurrence_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
    pub reason_code: Option<String>,
    pub note: Option<String>,
    pub generate_work_order: Option<bool>,
    pub work_order_type_id: Option<i64>,
    pub actor_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmDueMetrics {
    pub as_of: String,
    pub overdue_count: i64,
    pub due_today_count: i64,
    pub due_next_7d_count: i64,
    pub ready_for_scheduling_count: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanningReadinessInput {
    pub pm_plan_id: Option<i64>,
    pub due_from: Option<String>,
    pub due_to: Option<String>,
    pub include_linked_work_orders: Option<bool>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanningReadinessBlocker {
    pub code: String,
    pub message: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanningCandidate {
    pub occurrence: PmOccurrence,
    pub ready_for_scheduling: bool,
    pub blockers: Vec<PmPlanningReadinessBlocker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmPlanningReadinessProjection {
    pub as_of: String,
    pub candidate_count: i64,
    pub ready_count: i64,
    pub blocked_count: i64,
    pub derivation_rules: Vec<String>,
    pub candidates: Vec<PmPlanningCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmExecution {
    pub id: i64,
    pub pm_occurrence_id: i64,
    pub work_order_id: Option<i64>,
    pub work_order_code: Option<String>,
    pub execution_result: String,
    pub executed_at: String,
    pub notes: Option<String>,
    pub actor_id: Option<i64>,
    pub actual_duration_hours: Option<f64>,
    pub actual_labor_hours: Option<f64>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmFinding {
    pub id: i64,
    pub pm_execution_id: i64,
    pub finding_type: String,
    pub severity: Option<String>,
    pub description: String,
    pub follow_up_di_id: Option<i64>,
    pub follow_up_work_order_id: Option<i64>,
    pub follow_up_di_code: Option<String>,
    pub follow_up_work_order_code: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmExecutionFindingInput {
    pub finding_type: String,
    pub severity: Option<String>,
    pub description: String,
    pub create_follow_up_di: Option<bool>,
    pub create_follow_up_work_order: Option<bool>,
    pub follow_up_work_order_type_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutePmOccurrenceInput {
    pub occurrence_id: i64,
    pub expected_occurrence_row_version: i64,
    pub execution_result: String,
    pub note: Option<String>,
    pub actor_id: Option<i64>,
    pub work_order_id: Option<i64>,
    pub defer_reason_code: Option<String>,
    pub miss_reason_code: Option<String>,
    pub findings: Option<Vec<PmExecutionFindingInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutePmOccurrenceResult {
    pub occurrence: PmOccurrence,
    pub execution: PmExecution,
    pub findings: Vec<PmFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmExecutionFilter {
    pub occurrence_id: Option<i64>,
    pub pm_plan_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmRecurringFindingsInput {
    pub days_window: Option<i64>,
    pub min_occurrences: Option<i64>,
    pub pm_plan_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmRecurringFinding {
    pub pm_plan_id: i64,
    pub plan_code: Option<String>,
    pub finding_type: String,
    pub occurrence_count: i64,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub latest_severity: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmGovernanceKpiInput {
    pub from: Option<String>,
    pub to: Option<String>,
    pub pm_plan_id: Option<i64>,
    pub criticality_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmRateKpi {
    pub numerator: i64,
    pub denominator: i64,
    pub value_pct: Option<f64>,
    pub derivation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmEffortVarianceKpi {
    pub sample_size: i64,
    pub estimated_hours: f64,
    pub actual_hours: f64,
    pub variance_hours: f64,
    pub variance_pct: Option<f64>,
    pub derivation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PmGovernanceKpiReport {
    pub as_of: String,
    pub from: String,
    pub to: String,
    pub pm_plan_id: Option<i64>,
    pub criticality_code: Option<String>,
    pub compliance: PmRateKpi,
    pub overdue_risk: PmRateKpi,
    pub first_pass_completion: PmRateKpi,
    pub follow_up_ratio: PmRateKpi,
    pub effort_variance: PmEffortVarianceKpi,
    pub derivation_rules: Vec<String>,
}
