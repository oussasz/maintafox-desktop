use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostCenter {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub entity_id: Option<i64>,
    pub entity_name: Option<String>,
    pub parent_cost_center_id: Option<i64>,
    pub parent_cost_center_code: Option<String>,
    pub budget_owner_id: Option<i64>,
    pub erp_external_id: Option<String>,
    pub is_active: i64,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostCenterFilter {
    pub entity_id: Option<i64>,
    pub include_inactive: Option<bool>,
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCostCenterInput {
    pub code: String,
    pub name: String,
    pub entity_id: Option<i64>,
    pub parent_cost_center_id: Option<i64>,
    pub budget_owner_id: Option<i64>,
    pub erp_external_id: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateCostCenterInput {
    pub code: Option<String>,
    pub name: Option<String>,
    pub entity_id: Option<i64>,
    pub parent_cost_center_id: Option<i64>,
    pub budget_owner_id: Option<i64>,
    pub erp_external_id: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetVersion {
    pub id: i64,
    pub entity_sync_id: String,
    pub fiscal_year: i64,
    pub scenario_type: String,
    pub version_no: i64,
    pub status: String,
    pub currency_code: String,
    pub title: Option<String>,
    pub planning_basis: Option<String>,
    pub source_basis_mix_json: Option<String>,
    pub labor_assumptions_json: Option<String>,
    pub baseline_reference: Option<String>,
    pub erp_external_ref: Option<String>,
    pub successor_of_version_id: Option<i64>,
    pub created_by_id: Option<i64>,
    pub approved_at: Option<String>,
    pub approved_by_id: Option<i64>,
    pub frozen_at: Option<String>,
    pub frozen_by_id: Option<i64>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetVersionFilter {
    pub fiscal_year: Option<i64>,
    pub scenario_type: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBudgetVersionInput {
    pub fiscal_year: i64,
    pub scenario_type: String,
    pub currency_code: String,
    pub title: Option<String>,
    pub planning_basis: Option<String>,
    pub source_basis_mix_json: Option<String>,
    pub labor_assumptions_json: Option<String>,
    pub baseline_reference: Option<String>,
    pub erp_external_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateBudgetVersionInput {
    pub currency_code: Option<String>,
    pub title: Option<String>,
    pub planning_basis: Option<String>,
    pub source_basis_mix_json: Option<String>,
    pub labor_assumptions_json: Option<String>,
    pub baseline_reference: Option<String>,
    pub erp_external_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBudgetSuccessorInput {
    pub source_version_id: i64,
    pub fiscal_year: Option<i64>,
    pub scenario_type: Option<String>,
    pub title: Option<String>,
    pub baseline_reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionBudgetVersionLifecycleInput {
    pub version_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetLine {
    pub id: i64,
    pub entity_sync_id: String,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub planned_amount: f64,
    pub source_basis: Option<String>,
    pub justification_note: Option<String>,
    pub asset_family: Option<String>,
    pub work_category: Option<String>,
    pub shutdown_package_ref: Option<String>,
    pub team_id: Option<i64>,
    pub skill_pool_id: Option<i64>,
    pub labor_lane: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetLineFilter {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBudgetLineInput {
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub planned_amount: f64,
    pub source_basis: Option<String>,
    pub justification_note: Option<String>,
    pub asset_family: Option<String>,
    pub work_category: Option<String>,
    pub shutdown_package_ref: Option<String>,
    pub team_id: Option<i64>,
    pub skill_pool_id: Option<i64>,
    pub labor_lane: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateBudgetLineInput {
    pub period_month: Option<i64>,
    pub budget_bucket: Option<String>,
    pub planned_amount: Option<f64>,
    pub source_basis: Option<String>,
    pub justification_note: Option<String>,
    pub asset_family: Option<String>,
    pub work_category: Option<String>,
    pub shutdown_package_ref: Option<String>,
    pub team_id: Option<i64>,
    pub skill_pool_id: Option<i64>,
    pub labor_lane: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetActual {
    pub id: i64,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub amount_source: f64,
    pub source_currency: String,
    pub amount_base: f64,
    pub base_currency: String,
    pub source_type: String,
    pub source_id: String,
    pub work_order_id: Option<i64>,
    pub equipment_id: Option<i64>,
    pub posting_status: String,
    pub provisional_reason: Option<String>,
    pub posted_at: Option<String>,
    pub posted_by_id: Option<i64>,
    pub reversal_of_actual_id: Option<i64>,
    pub reversal_reason: Option<String>,
    pub personnel_id: Option<i64>,
    pub team_id: Option<i64>,
    pub rate_card_lane: Option<String>,
    pub event_at: String,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetActualFilter {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub period_month: Option<i64>,
    pub budget_bucket: Option<String>,
    pub posting_status: Option<String>,
    pub source_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBudgetActualInput {
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub amount_source: f64,
    pub source_currency: String,
    pub amount_base: f64,
    pub base_currency: String,
    pub source_type: String,
    pub source_id: String,
    pub work_order_id: Option<i64>,
    pub equipment_id: Option<i64>,
    pub posting_status: Option<String>,
    pub provisional_reason: Option<String>,
    pub personnel_id: Option<i64>,
    pub team_id: Option<i64>,
    pub rate_card_lane: Option<String>,
    pub event_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostBudgetActualInput {
    pub actual_id: i64,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseBudgetActualInput {
    pub actual_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCommitment {
    pub id: i64,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub commitment_type: String,
    pub source_type: String,
    pub source_id: String,
    pub obligation_amount: f64,
    pub source_currency: String,
    pub base_amount: f64,
    pub base_currency: String,
    pub commitment_status: String,
    pub work_order_id: Option<i64>,
    pub contract_id: Option<i64>,
    pub purchase_order_id: Option<i64>,
    pub planning_commitment_ref: Option<String>,
    pub due_at: Option<String>,
    pub explainability_note: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetCommitmentFilter {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub period_month: Option<i64>,
    pub budget_bucket: Option<String>,
    pub commitment_status: Option<String>,
    pub source_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBudgetCommitmentInput {
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub commitment_type: String,
    pub source_type: String,
    pub source_id: String,
    pub obligation_amount: f64,
    pub source_currency: String,
    pub base_amount: f64,
    pub base_currency: String,
    pub commitment_status: Option<String>,
    pub work_order_id: Option<i64>,
    pub contract_id: Option<i64>,
    pub purchase_order_id: Option<i64>,
    pub planning_commitment_ref: Option<String>,
    pub due_at: Option<String>,
    pub explainability_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastRun {
    pub id: i64,
    pub budget_version_id: i64,
    pub generated_by_id: Option<i64>,
    pub idempotency_key: String,
    pub scope_signature: String,
    pub method_mix_json: Option<String>,
    pub confidence_policy_json: Option<String>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetForecast {
    pub id: i64,
    pub forecast_run_id: i64,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub forecast_amount: f64,
    pub forecast_method: String,
    pub confidence_level: String,
    pub driver_type: Option<String>,
    pub driver_reference: Option<String>,
    pub explainability_json: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetForecastFilter {
    pub budget_version_id: Option<i64>,
    pub forecast_run_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub period_month: Option<i64>,
    pub budget_bucket: Option<String>,
    pub forecast_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateBudgetForecastInput {
    pub budget_version_id: i64,
    pub idempotency_key: String,
    pub scope_signature: String,
    pub period_month_start: Option<i64>,
    pub period_month_end: Option<i64>,
    pub include_pm_occurrence: Option<bool>,
    pub include_backlog_demand: Option<bool>,
    pub include_shutdown_demand: Option<bool>,
    pub include_planning_demand: Option<bool>,
    pub include_burn_rate: Option<bool>,
    pub confidence_policy_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetForecastGenerationResult {
    pub run: ForecastRun,
    pub forecasts: Vec<BudgetForecast>,
    pub reused_existing_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetVarianceReview {
    pub id: i64,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub variance_amount: f64,
    pub variance_pct: f64,
    pub driver_code: String,
    pub action_owner_id: i64,
    pub review_status: String,
    pub review_commentary: String,
    pub snapshot_context_json: String,
    pub opened_at: String,
    pub reviewed_at: Option<String>,
    pub closed_at: Option<String>,
    pub reopened_from_review_id: Option<i64>,
    pub reopen_reason: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetVarianceReviewFilter {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub period_month: Option<i64>,
    pub review_status: Option<String>,
    pub driver_code: Option<String>,
    pub action_owner_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBudgetVarianceReviewInput {
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub variance_amount: f64,
    pub variance_pct: f64,
    pub driver_code: String,
    pub action_owner_id: i64,
    pub review_commentary: String,
    pub snapshot_context_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionBudgetVarianceReviewInput {
    pub review_id: i64,
    pub expected_row_version: i64,
    pub next_status: String,
    pub review_commentary: Option<String>,
    pub reopen_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetDashboardFilter {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub period_month: Option<i64>,
    pub budget_bucket: Option<String>,
    pub spend_mix: Option<String>,
    pub team_id: Option<i64>,
    pub assignee_id: Option<i64>,
    pub labor_lane: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetDashboardRow {
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub spend_mix: String,
    pub team_id: Option<i64>,
    pub assignee_id: Option<i64>,
    pub labor_lane: Option<String>,
    pub planned_amount: f64,
    pub committed_amount: f64,
    pub actual_amount: f64,
    pub forecast_amount: f64,
    pub variance_to_plan: f64,
    pub variance_to_forecast: f64,
    pub currency_code: String,
    pub source_links_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetDrilldownRow {
    pub layer_type: String,
    pub record_id: i64,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub amount: f64,
    pub currency_code: String,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub work_order_id: Option<i64>,
    pub pm_occurrence_ref: Option<String>,
    pub inspection_ref: Option<String>,
    pub shutdown_package_ref: Option<String>,
    pub team_id: Option<i64>,
    pub assignee_id: Option<i64>,
    pub labor_lane: Option<String>,
    pub hours_overrun_rate: Option<f64>,
    pub first_pass_effect: Option<f64>,
    pub repeat_work_penalty: Option<f64>,
    pub schedule_discipline_impact: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpCostCenterMasterRecordInput {
    pub external_code: String,
    pub external_name: String,
    pub local_cost_center_code: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportErpCostCenterMasterInput {
    pub import_batch_id: String,
    pub records: Vec<ErpCostCenterMasterRecordInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpMasterImportResult {
    pub imported_count: i64,
    pub linked_count: i64,
    pub inactive_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpPostedActualExportItem {
    pub actual_id: i64,
    pub budget_version_id: i64,
    pub fiscal_year: i64,
    pub scenario_type: String,
    pub external_cost_center_code: Option<String>,
    pub local_cost_center_code: String,
    pub budget_bucket: String,
    pub amount_source: f64,
    pub source_currency: String,
    pub amount_base: f64,
    pub base_currency: String,
    pub source_type: String,
    pub source_id: String,
    pub posted_at: Option<String>,
    pub reconciliation_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpApprovedReforecastExportItem {
    pub forecast_id: i64,
    pub forecast_run_id: i64,
    pub budget_version_id: i64,
    pub fiscal_year: i64,
    pub scenario_type: String,
    pub version_status: String,
    pub external_cost_center_code: Option<String>,
    pub local_cost_center_code: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub forecast_amount: f64,
    pub base_currency: String,
    pub forecast_method: String,
    pub confidence_level: String,
    pub reconciliation_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostedExportBatch {
    pub id: i64,
    pub entity_sync_id: String,
    pub batch_uuid: String,
    pub export_kind: String,
    pub tenant_id: Option<String>,
    pub relay_payload_json: String,
    pub total_posted: f64,
    pub line_count: i64,
    pub status: String,
    pub erp_ack_at: Option<String>,
    pub erp_http_code: Option<i64>,
    pub rejection_code: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationException {
    pub id: i64,
    pub entity_sync_id: String,
    pub posted_export_batch_id: i64,
    pub source_record_kind: String,
    pub source_record_id: i64,
    pub maintafox_value_snapshot: String,
    pub external_value_snapshot: Option<String>,
    pub resolution_status: String,
    pub rejection_code: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordErpExportBatchInput {
    pub export_kind: String,
    pub tenant_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErpExportBatchResult {
    pub batch: PostedExportBatch,
    pub jsonl: String,
    pub integration_exceptions: Vec<IntegrationException>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PostedExportBatchFilter {
    pub export_kind: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationExceptionFilter {
    pub posted_export_batch_id: Option<i64>,
    pub resolution_status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateIntegrationExceptionInput {
    pub resolution_status: String,
    pub external_value_snapshot: Option<String>,
    pub rejection_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertConfig {
    pub id: i64,
    pub entity_sync_id: String,
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub budget_bucket: Option<String>,
    pub alert_type: String,
    pub threshold_pct: Option<f64>,
    pub threshold_amount: Option<f64>,
    pub recipient_user_id: Option<i64>,
    pub recipient_role_id: Option<i64>,
    pub labor_template: Option<String>,
    pub dedupe_window_minutes: i64,
    pub requires_ack: bool,
    pub is_active: bool,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetAlertConfigFilter {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub alert_type: Option<String>,
    pub active_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBudgetAlertConfigInput {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub budget_bucket: Option<String>,
    pub alert_type: String,
    pub threshold_pct: Option<f64>,
    pub threshold_amount: Option<f64>,
    pub recipient_user_id: Option<i64>,
    pub recipient_role_id: Option<i64>,
    pub labor_template: Option<String>,
    pub dedupe_window_minutes: Option<i64>,
    pub requires_ack: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateBudgetAlertConfigInput {
    pub budget_bucket: Option<String>,
    pub threshold_pct: Option<f64>,
    pub threshold_amount: Option<f64>,
    pub recipient_user_id: Option<i64>,
    pub recipient_role_id: Option<i64>,
    pub labor_template: Option<String>,
    pub dedupe_window_minutes: Option<i64>,
    pub requires_ack: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertEvent {
    pub id: i64,
    pub entity_sync_id: String,
    pub alert_config_id: Option<i64>,
    pub budget_version_id: i64,
    pub cost_center_id: i64,
    pub cost_center_code: String,
    pub cost_center_name: String,
    pub period_month: Option<i64>,
    pub budget_bucket: String,
    pub alert_type: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub dedupe_key: String,
    pub current_value: f64,
    pub threshold_value: Option<f64>,
    pub variance_amount: Option<f64>,
    pub currency_code: String,
    pub payload_json: Option<String>,
    pub notification_event_id: Option<i64>,
    pub notification_id: Option<i64>,
    pub acknowledged_at: Option<String>,
    pub acknowledged_by_id: Option<i64>,
    pub acknowledgement_note: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetAlertEventFilter {
    pub budget_version_id: Option<i64>,
    pub cost_center_id: Option<i64>,
    pub alert_type: Option<String>,
    pub acknowledged_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluateBudgetAlertsInput {
    pub budget_version_id: i64,
    pub emit_notifications: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcknowledgeBudgetAlertInput {
    pub alert_event_id: i64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertEvaluationResult {
    pub evaluated_at: String,
    pub emitted_count: i64,
    pub deduped_count: i64,
    pub considered_rows: i64,
    pub events: Vec<BudgetAlertEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReportPackFilter {
    pub budget_version_id: i64,
    pub cost_center_id: Option<i64>,
    pub period_month_start: Option<i64>,
    pub period_month_end: Option<i64>,
    pub budget_bucket: Option<String>,
    pub spend_mix: Option<String>,
    pub team_id: Option<i64>,
    pub assignee_id: Option<i64>,
    pub labor_lane: Option<String>,
    pub variance_driver_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReportPackTotals {
    pub baseline_amount: f64,
    pub commitment_amount: f64,
    pub posted_actual_amount: f64,
    pub forecast_amount: f64,
    pub variance_amount: f64,
    pub variance_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReportPack {
    pub generated_at: String,
    pub budget_version_id: i64,
    pub fiscal_year: i64,
    pub scenario_type: String,
    pub version_status: String,
    pub currency_code: String,
    pub posting_status_filter: String,
    pub forecast_method_mix_json: String,
    pub totals: BudgetReportPackTotals,
    pub spend_mix_json: String,
    pub top_work_orders_json: String,
    pub top_assets_json: String,
    pub workforce_efficiency_json: String,
    pub explainability_json: String,
    pub multi_currency_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBudgetReportPackInput {
    pub filter: BudgetReportPackFilter,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReportPackExport {
    pub format: String,
    pub file_name: String,
    pub mime_type: String,
    pub content: String,
    pub report: BudgetReportPack,
}
