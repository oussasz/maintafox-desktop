//! Budget and finance IPC commands.

use tauri::State;

use crate::auth::rbac::{check_permission, PermissionScope};
use crate::errors::{AppError, AppResult};
use crate::finance::domain::{
    AcknowledgeBudgetAlertInput, BudgetActual, BudgetActualFilter, BudgetAlertConfig, BudgetAlertConfigFilter,
    BudgetAlertEvaluationResult, BudgetAlertEvent, BudgetAlertEventFilter, BudgetCommitment, BudgetCommitmentFilter,
    BudgetDashboardFilter, BudgetDashboardRow, BudgetDrilldownRow, BudgetForecast, BudgetForecastFilter,
    BudgetForecastGenerationResult, BudgetLine, BudgetLineFilter, BudgetReportPack, BudgetReportPackExport,
    BudgetReportPackFilter, BudgetVarianceReview, BudgetVarianceReviewFilter, BudgetVersion, BudgetVersionFilter,
    CostCenter, CostCenterFilter, CreateBudgetActualInput, CreateBudgetAlertConfigInput, CreateBudgetCommitmentInput,
    CreateBudgetLineInput, CreateBudgetSuccessorInput, CreateBudgetVarianceReviewInput, CreateBudgetVersionInput,
    CreateCostCenterInput, ErpApprovedReforecastExportItem, ErpExportBatchResult, ErpMasterImportResult,
    ErpPostedActualExportItem, EvaluateBudgetAlertsInput, ExportBudgetReportPackInput, ForecastRun,
    GenerateBudgetForecastInput, ImportErpCostCenterMasterInput, IntegrationException, IntegrationExceptionFilter,
    PostBudgetActualInput, PostedExportBatch, PostedExportBatchFilter, RecordErpExportBatchInput,
    ReverseBudgetActualInput, TransitionBudgetVarianceReviewInput, TransitionBudgetVersionLifecycleInput,
    UpdateBudgetAlertConfigInput, UpdateBudgetLineInput, UpdateBudgetVersionInput, UpdateCostCenterInput,
    UpdateIntegrationExceptionInput,
};
use crate::finance::queries;
use crate::state::AppState;
use crate::{require_permission, require_session};

async fn require_fin_budget_or_legacy_manage(state: &State<'_, AppState>, user_id: i32) -> AppResult<()> {
    if check_permission(&state.db, user_id, "fin.budget", &PermissionScope::Global).await? {
        return Ok(());
    }
    if check_permission(&state.db, user_id, "fin.manage", &PermissionScope::Global).await? {
        return Ok(());
    }
    Err(AppError::PermissionDenied(
        "Required permission: fin.budget (or legacy fin.manage compatibility).".to_string(),
    ))
}

async fn require_fin_post(state: &State<'_, AppState>, user_id: i32) -> AppResult<()> {
    if check_permission(&state.db, user_id, "fin.post", &PermissionScope::Global).await? {
        return Ok(());
    }
    Err(AppError::PermissionDenied(
        "Required permission: fin.post.".to_string(),
    ))
}

async fn require_fin_report(state: &State<'_, AppState>, user_id: i32) -> AppResult<()> {
    if check_permission(&state.db, user_id, "fin.report", &PermissionScope::Global).await? {
        return Ok(());
    }
    Err(AppError::PermissionDenied(
        "Required permission: fin.report.".to_string(),
    ))
}

#[tauri::command]
pub async fn list_cost_centers(
    filter: CostCenterFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<CostCenter>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_cost_centers(&state.db, filter).await
}

#[tauri::command]
pub async fn create_cost_center(
    input: CreateCostCenterInput,
    state: State<'_, AppState>,
) -> AppResult<CostCenter> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_cost_center(&state.db, input).await
}

#[tauri::command]
pub async fn update_cost_center(
    cost_center_id: i64,
    expected_row_version: i64,
    input: UpdateCostCenterInput,
    state: State<'_, AppState>,
) -> AppResult<CostCenter> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::update_cost_center(&state.db, cost_center_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_budget_versions(
    filter: BudgetVersionFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetVersion>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_versions(&state.db, filter).await
}

#[tauri::command]
pub async fn create_budget_version(
    input: CreateBudgetVersionInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetVersion> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_budget_version(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn create_budget_successor_version(
    input: CreateBudgetSuccessorInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetVersion> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_budget_successor_version(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn update_budget_version(
    version_id: i64,
    expected_row_version: i64,
    input: UpdateBudgetVersionInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetVersion> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::update_budget_version(&state.db, version_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn transition_budget_version_lifecycle(
    input: TransitionBudgetVersionLifecycleInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetVersion> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::transition_budget_version_lifecycle(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn list_budget_lines(
    filter: BudgetLineFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetLine>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_lines(&state.db, filter).await
}

#[tauri::command]
pub async fn create_budget_line(
    input: CreateBudgetLineInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetLine> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_budget_line(&state.db, input).await
}

#[tauri::command]
pub async fn update_budget_line(
    line_id: i64,
    expected_row_version: i64,
    input: UpdateBudgetLineInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetLine> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::update_budget_line(&state.db, line_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_budget_actuals(
    filter: BudgetActualFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetActual>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_actuals(&state.db, filter).await
}

#[tauri::command]
pub async fn create_budget_actual(
    input: CreateBudgetActualInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetActual> {
    let user = require_session!(state);
    let allow_posted = check_permission(&state.db, user.user_id, "fin.post", &PermissionScope::Global).await?;
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_budget_actual(&state.db, i64::from(user.user_id), input, allow_posted).await
}

#[tauri::command]
pub async fn post_budget_actual(
    input: PostBudgetActualInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetActual> {
    let user = require_session!(state);
    require_fin_post(&state, user.user_id).await?;
    queries::post_budget_actual(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn reverse_budget_actual(
    input: ReverseBudgetActualInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetActual> {
    let user = require_session!(state);
    require_fin_post(&state, user.user_id).await?;
    queries::reverse_budget_actual(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn list_budget_commitments(
    filter: BudgetCommitmentFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetCommitment>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_commitments(&state.db, filter).await
}

#[tauri::command]
pub async fn create_budget_commitment(
    input: CreateBudgetCommitmentInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetCommitment> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_budget_commitment(&state.db, input).await
}

#[tauri::command]
pub async fn list_forecast_runs(
    budget_version_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ForecastRun>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_forecast_runs(&state.db, budget_version_id).await
}

#[tauri::command]
pub async fn list_budget_forecasts(
    filter: BudgetForecastFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetForecast>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_forecasts(&state.db, filter).await
}

#[tauri::command]
pub async fn generate_budget_forecasts(
    input: GenerateBudgetForecastInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetForecastGenerationResult> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::generate_budget_forecasts(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn list_budget_variance_reviews(
    filter: BudgetVarianceReviewFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetVarianceReview>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_variance_reviews(&state.db, filter).await
}

#[tauri::command]
pub async fn create_budget_variance_review(
    input: CreateBudgetVarianceReviewInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetVarianceReview> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_budget_variance_review(&state.db, input).await
}

#[tauri::command]
pub async fn transition_budget_variance_review(
    input: TransitionBudgetVarianceReviewInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetVarianceReview> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::transition_budget_variance_review(&state.db, input).await
}

#[tauri::command]
pub async fn list_budget_dashboard_rows(
    filter: BudgetDashboardFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetDashboardRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_dashboard_rows(&state.db, filter).await
}

#[tauri::command]
pub async fn list_budget_dashboard_drilldown(
    filter: BudgetDashboardFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetDrilldownRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_dashboard_drilldown(&state.db, filter).await
}

#[tauri::command]
pub async fn import_erp_cost_center_master(
    input: ImportErpCostCenterMasterInput,
    state: State<'_, AppState>,
) -> AppResult<ErpMasterImportResult> {
    let user = require_session!(state);
    require_fin_report(&state, user.user_id).await?;
    queries::import_erp_cost_center_master(&state.db, input).await
}

#[tauri::command]
pub async fn export_posted_actuals_for_erp(
    state: State<'_, AppState>,
) -> AppResult<Vec<ErpPostedActualExportItem>> {
    let user = require_session!(state);
    require_fin_report(&state, user.user_id).await?;
    queries::export_posted_actuals_for_erp(&state.db).await
}

#[tauri::command]
pub async fn export_approved_reforecasts_for_erp(
    state: State<'_, AppState>,
) -> AppResult<Vec<ErpApprovedReforecastExportItem>> {
    let user = require_session!(state);
    require_fin_report(&state, user.user_id).await?;
    queries::export_approved_reforecasts_for_erp(&state.db).await
}

#[tauri::command]
pub async fn record_erp_export_batch(
    input: RecordErpExportBatchInput,
    state: State<'_, AppState>,
) -> AppResult<ErpExportBatchResult> {
    let user = require_session!(state);
    require_fin_report(&state, user.user_id).await?;
    queries::record_erp_export_batch(&state.db, input).await
}

#[tauri::command]
pub async fn list_posted_export_batches(
    filter: PostedExportBatchFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<PostedExportBatch>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_posted_export_batches(&state.db, filter).await
}

#[tauri::command]
pub async fn list_integration_exceptions(
    filter: IntegrationExceptionFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<IntegrationException>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_integration_exceptions(&state.db, filter).await
}

#[tauri::command]
pub async fn update_integration_exception(
    exception_id: i64,
    expected_row_version: i64,
    input: UpdateIntegrationExceptionInput,
    state: State<'_, AppState>,
) -> AppResult<IntegrationException> {
    let user = require_session!(state);
    require_fin_report(&state, user.user_id).await?;
    queries::update_integration_exception(&state.db, exception_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_budget_alert_configs(
    filter: BudgetAlertConfigFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetAlertConfig>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_alert_configs(&state.db, filter).await
}

#[tauri::command]
pub async fn create_budget_alert_config(
    input: CreateBudgetAlertConfigInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetAlertConfig> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::create_budget_alert_config(&state.db, input).await
}

#[tauri::command]
pub async fn update_budget_alert_config(
    config_id: i64,
    expected_row_version: i64,
    input: UpdateBudgetAlertConfigInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetAlertConfig> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::update_budget_alert_config(&state.db, config_id, expected_row_version, input).await
}

#[tauri::command]
pub async fn list_budget_alert_events(
    filter: BudgetAlertEventFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<BudgetAlertEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::list_budget_alert_events(&state.db, filter).await
}

#[tauri::command]
pub async fn evaluate_budget_alerts(
    input: EvaluateBudgetAlertsInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetAlertEvaluationResult> {
    let user = require_session!(state);
    require_fin_budget_or_legacy_manage(&state, user.user_id).await?;
    queries::evaluate_budget_alerts(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn acknowledge_budget_alert(
    input: AcknowledgeBudgetAlertInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetAlertEvent> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::acknowledge_budget_alert(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn build_budget_report_pack(
    filter: BudgetReportPackFilter,
    state: State<'_, AppState>,
) -> AppResult<BudgetReportPack> {
    let user = require_session!(state);
    require_permission!(state, &user, "fin.view", PermissionScope::Global);
    queries::build_budget_report_pack(&state.db, filter).await
}

#[tauri::command]
pub async fn export_budget_report_pack(
    input: ExportBudgetReportPackInput,
    state: State<'_, AppState>,
) -> AppResult<BudgetReportPackExport> {
    let user = require_session!(state);
    require_fin_report(&state, user.user_id).await?;
    queries::export_budget_report_pack(&state.db, input).await
}
