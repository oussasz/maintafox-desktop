use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, Value};

use crate::errors::{AppError, AppResult};
use crate::finance::domain::{
    AcknowledgeBudgetAlertInput, BudgetActual, BudgetActualFilter, BudgetAlertConfig, BudgetAlertConfigFilter,
    BudgetAlertEvaluationResult, BudgetAlertEvent, BudgetAlertEventFilter, BudgetCommitment, BudgetCommitmentFilter,
    BudgetDashboardFilter, BudgetDashboardRow, BudgetDrilldownRow, BudgetForecast, BudgetForecastFilter,
    BudgetForecastGenerationResult, BudgetLine, BudgetLineFilter, BudgetReportPack, BudgetReportPackExport,
    BudgetReportPackFilter, BudgetReportPackTotals, BudgetVarianceReview, BudgetVarianceReviewFilter, BudgetVersion,
    BudgetVersionFilter, CostCenter, CostCenterFilter, CreateBudgetActualInput, CreateBudgetAlertConfigInput,
    CreateBudgetCommitmentInput, CreateBudgetLineInput, CreateBudgetSuccessorInput, CreateBudgetVarianceReviewInput,
    CreateBudgetVersionInput, CreateCostCenterInput, ErpApprovedReforecastExportItem, ErpCostCenterMasterRecordInput,
    ErpMasterImportResult, ErpPostedActualExportItem, EvaluateBudgetAlertsInput, ExportBudgetReportPackInput,
    ForecastRun, GenerateBudgetForecastInput, ImportErpCostCenterMasterInput, PostBudgetActualInput,
    ReverseBudgetActualInput, TransitionBudgetVarianceReviewInput, TransitionBudgetVersionLifecycleInput,
    UpdateBudgetAlertConfigInput, UpdateBudgetLineInput, UpdateBudgetVersionInput, UpdateCostCenterInput,
};

fn map_cost_center(row: &QueryResult) -> AppResult<CostCenter> {
    Ok(CostCenter {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        entity_id: row.try_get("", "entity_id")?,
        entity_name: row.try_get("", "entity_name")?,
        parent_cost_center_id: row.try_get("", "parent_cost_center_id")?,
        parent_cost_center_code: row.try_get("", "parent_cost_center_code")?,
        budget_owner_id: row.try_get("", "budget_owner_id")?,
        erp_external_id: row.try_get("", "erp_external_id")?,
        is_active: row.try_get("", "is_active")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_budget_version(row: &QueryResult) -> AppResult<BudgetVersion> {
    Ok(BudgetVersion {
        id: row.try_get("", "id")?,
        fiscal_year: row.try_get("", "fiscal_year")?,
        scenario_type: row.try_get("", "scenario_type")?,
        version_no: row.try_get("", "version_no")?,
        status: row.try_get("", "status")?,
        currency_code: row.try_get("", "currency_code")?,
        title: row.try_get("", "title")?,
        planning_basis: row.try_get("", "planning_basis")?,
        source_basis_mix_json: row.try_get("", "source_basis_mix_json")?,
        labor_assumptions_json: row.try_get("", "labor_assumptions_json")?,
        baseline_reference: row.try_get("", "baseline_reference")?,
        erp_external_ref: row.try_get("", "erp_external_ref")?,
        successor_of_version_id: row.try_get("", "successor_of_version_id")?,
        created_by_id: row.try_get("", "created_by_id")?,
        approved_at: row.try_get("", "approved_at")?,
        approved_by_id: row.try_get("", "approved_by_id")?,
        frozen_at: row.try_get("", "frozen_at")?,
        frozen_by_id: row.try_get("", "frozen_by_id")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_budget_line(row: &QueryResult) -> AppResult<BudgetLine> {
    Ok(BudgetLine {
        id: row.try_get("", "id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        cost_center_name: row.try_get("", "cost_center_name")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        planned_amount: row.try_get("", "planned_amount")?,
        source_basis: row.try_get("", "source_basis")?,
        justification_note: row.try_get("", "justification_note")?,
        asset_family: row.try_get("", "asset_family")?,
        work_category: row.try_get("", "work_category")?,
        shutdown_package_ref: row.try_get("", "shutdown_package_ref")?,
        team_id: row.try_get("", "team_id")?,
        skill_pool_id: row.try_get("", "skill_pool_id")?,
        labor_lane: row.try_get("", "labor_lane")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_budget_actual(row: &QueryResult) -> AppResult<BudgetActual> {
    Ok(BudgetActual {
        id: row.try_get("", "id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        cost_center_name: row.try_get("", "cost_center_name")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        amount_source: row.try_get("", "amount_source")?,
        source_currency: row.try_get("", "source_currency")?,
        amount_base: row.try_get("", "amount_base")?,
        base_currency: row.try_get("", "base_currency")?,
        source_type: row.try_get("", "source_type")?,
        source_id: row.try_get("", "source_id")?,
        work_order_id: row.try_get("", "work_order_id")?,
        equipment_id: row.try_get("", "equipment_id")?,
        posting_status: row.try_get("", "posting_status")?,
        provisional_reason: row.try_get("", "provisional_reason")?,
        posted_at: row.try_get("", "posted_at")?,
        posted_by_id: row.try_get("", "posted_by_id")?,
        reversal_of_actual_id: row.try_get("", "reversal_of_actual_id")?,
        reversal_reason: row.try_get("", "reversal_reason")?,
        personnel_id: row.try_get("", "personnel_id")?,
        team_id: row.try_get("", "team_id")?,
        rate_card_lane: row.try_get("", "rate_card_lane")?,
        event_at: row.try_get("", "event_at")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_budget_commitment(row: &QueryResult) -> AppResult<BudgetCommitment> {
    Ok(BudgetCommitment {
        id: row.try_get("", "id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        cost_center_name: row.try_get("", "cost_center_name")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        commitment_type: row.try_get("", "commitment_type")?,
        source_type: row.try_get("", "source_type")?,
        source_id: row.try_get("", "source_id")?,
        obligation_amount: row.try_get("", "obligation_amount")?,
        source_currency: row.try_get("", "source_currency")?,
        base_amount: row.try_get("", "base_amount")?,
        base_currency: row.try_get("", "base_currency")?,
        commitment_status: row.try_get("", "commitment_status")?,
        work_order_id: row.try_get("", "work_order_id")?,
        contract_id: row.try_get("", "contract_id")?,
        purchase_order_id: row.try_get("", "purchase_order_id")?,
        planning_commitment_ref: row.try_get("", "planning_commitment_ref")?,
        due_at: row.try_get("", "due_at")?,
        explainability_note: row.try_get("", "explainability_note")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_forecast_run(row: &QueryResult) -> AppResult<ForecastRun> {
    Ok(ForecastRun {
        id: row.try_get("", "id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        generated_by_id: row.try_get("", "generated_by_id")?,
        idempotency_key: row.try_get("", "idempotency_key")?,
        scope_signature: row.try_get("", "scope_signature")?,
        method_mix_json: row.try_get("", "method_mix_json")?,
        confidence_policy_json: row.try_get("", "confidence_policy_json")?,
        generated_at: row.try_get("", "generated_at")?,
    })
}

fn map_budget_forecast(row: &QueryResult) -> AppResult<BudgetForecast> {
    Ok(BudgetForecast {
        id: row.try_get("", "id")?,
        forecast_run_id: row.try_get("", "forecast_run_id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        cost_center_name: row.try_get("", "cost_center_name")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        forecast_amount: row.try_get("", "forecast_amount")?,
        forecast_method: row.try_get("", "forecast_method")?,
        confidence_level: row.try_get("", "confidence_level")?,
        driver_type: row.try_get("", "driver_type")?,
        driver_reference: row.try_get("", "driver_reference")?,
        explainability_json: row.try_get("", "explainability_json")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_budget_variance_review(row: &QueryResult) -> AppResult<BudgetVarianceReview> {
    Ok(BudgetVarianceReview {
        id: row.try_get("", "id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        cost_center_name: row.try_get("", "cost_center_name")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        variance_amount: row.try_get("", "variance_amount")?,
        variance_pct: row.try_get("", "variance_pct")?,
        driver_code: row.try_get("", "driver_code")?,
        action_owner_id: row.try_get("", "action_owner_id")?,
        review_status: row.try_get("", "review_status")?,
        review_commentary: row.try_get("", "review_commentary")?,
        snapshot_context_json: row.try_get("", "snapshot_context_json")?,
        opened_at: row.try_get("", "opened_at")?,
        reviewed_at: row.try_get("", "reviewed_at")?,
        closed_at: row.try_get("", "closed_at")?,
        reopened_from_review_id: row.try_get("", "reopened_from_review_id")?,
        reopen_reason: row.try_get("", "reopen_reason")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_budget_dashboard_row(row: &QueryResult) -> AppResult<BudgetDashboardRow> {
    Ok(BudgetDashboardRow {
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        cost_center_name: row.try_get("", "cost_center_name")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        spend_mix: row.try_get("", "spend_mix")?,
        team_id: row.try_get("", "team_id")?,
        assignee_id: row.try_get("", "assignee_id")?,
        labor_lane: row.try_get("", "labor_lane")?,
        planned_amount: row.try_get("", "planned_amount")?,
        committed_amount: row.try_get("", "committed_amount")?,
        actual_amount: row.try_get("", "actual_amount")?,
        forecast_amount: row.try_get("", "forecast_amount")?,
        variance_to_plan: row.try_get("", "variance_to_plan")?,
        variance_to_forecast: row.try_get("", "variance_to_forecast")?,
        currency_code: row.try_get("", "currency_code")?,
        source_links_json: row.try_get("", "source_links_json")?,
    })
}

fn map_budget_drilldown_row(row: &QueryResult) -> AppResult<BudgetDrilldownRow> {
    Ok(BudgetDrilldownRow {
        layer_type: row.try_get("", "layer_type")?,
        record_id: row.try_get("", "record_id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        amount: row.try_get("", "amount")?,
        currency_code: row.try_get("", "currency_code")?,
        source_type: row.try_get("", "source_type")?,
        source_id: row.try_get("", "source_id")?,
        work_order_id: row.try_get("", "work_order_id")?,
        pm_occurrence_ref: row.try_get("", "pm_occurrence_ref")?,
        inspection_ref: row.try_get("", "inspection_ref")?,
        shutdown_package_ref: row.try_get("", "shutdown_package_ref")?,
        team_id: row.try_get("", "team_id")?,
        assignee_id: row.try_get("", "assignee_id")?,
        labor_lane: row.try_get("", "labor_lane")?,
        hours_overrun_rate: row.try_get("", "hours_overrun_rate")?,
        first_pass_effect: row.try_get("", "first_pass_effect")?,
        repeat_work_penalty: row.try_get("", "repeat_work_penalty")?,
        schedule_discipline_impact: row.try_get("", "schedule_discipline_impact")?,
    })
}

fn map_budget_alert_config(row: &QueryResult) -> AppResult<BudgetAlertConfig> {
    Ok(BudgetAlertConfig {
        id: row.try_get("", "id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        alert_type: row.try_get("", "alert_type")?,
        threshold_pct: row.try_get("", "threshold_pct")?,
        threshold_amount: row.try_get("", "threshold_amount")?,
        recipient_user_id: row.try_get("", "recipient_user_id")?,
        recipient_role_id: row.try_get("", "recipient_role_id")?,
        labor_template: row.try_get("", "labor_template")?,
        dedupe_window_minutes: row.try_get("", "dedupe_window_minutes")?,
        requires_ack: row.try_get::<i64>("", "requires_ack")? == 1,
        is_active: row.try_get::<i64>("", "is_active")? == 1,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn map_budget_alert_event(row: &QueryResult) -> AppResult<BudgetAlertEvent> {
    Ok(BudgetAlertEvent {
        id: row.try_get("", "id")?,
        alert_config_id: row.try_get("", "alert_config_id")?,
        budget_version_id: row.try_get("", "budget_version_id")?,
        cost_center_id: row.try_get("", "cost_center_id")?,
        cost_center_code: row.try_get("", "cost_center_code")?,
        cost_center_name: row.try_get("", "cost_center_name")?,
        period_month: row.try_get("", "period_month")?,
        budget_bucket: row.try_get("", "budget_bucket")?,
        alert_type: row.try_get("", "alert_type")?,
        severity: row.try_get("", "severity")?,
        title: row.try_get("", "title")?,
        message: row.try_get("", "message")?,
        dedupe_key: row.try_get("", "dedupe_key")?,
        current_value: row.try_get("", "current_value")?,
        threshold_value: row.try_get("", "threshold_value")?,
        variance_amount: row.try_get("", "variance_amount")?,
        currency_code: row.try_get("", "currency_code")?,
        payload_json: row.try_get("", "payload_json")?,
        notification_event_id: row.try_get("", "notification_event_id")?,
        notification_id: row.try_get("", "notification_id")?,
        acknowledged_at: row.try_get("", "acknowledged_at")?,
        acknowledged_by_id: row.try_get("", "acknowledged_by_id")?,
        acknowledgement_note: row.try_get("", "acknowledgement_note")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn optional_trimmed(value: &Option<String>) -> Option<String> {
    value.as_ref().map(|item| item.trim().to_string()).filter(|item| !item.is_empty())
}

fn required_trimmed(field: &str, value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!("{field} is required.")]));
    }
    Ok(trimmed.to_string())
}

fn valid_version_transition(current: &str, next: &str) -> bool {
    matches!(
        (current, next),
        ("draft", "submitted")
            | ("submitted", "approved")
            | ("approved", "frozen")
            | ("approved", "closed")
            | ("frozen", "closed")
            | ("frozen", "superseded")
    )
}

fn valid_variance_driver(driver_code: &str) -> bool {
    matches!(
        driver_code,
        "emergency_break_in"
            | "vendor_delay"
            | "labor_overrun"
            | "estimate_error"
            | "scope_change"
            | "price_increase"
            | "permit_delay"
            | "availability_loss"
            | "repeat_failure"
            | "shutdown_scope_growth"
    )
}

fn valid_variance_transition(current: &str, next: &str) -> bool {
    matches!(
        (current, next),
        ("open", "in_review")
            | ("in_review", "actioned")
            | ("actioned", "accepted")
            | ("accepted", "closed")
            | ("closed", "open")
            | ("accepted", "open")
            | ("actioned", "open")
            | ("in_review", "open")
    )
}

fn parse_csv_flags(raw: String) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|flag| !flag.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn alert_default_threshold_pct(alert_type: &str) -> Option<f64> {
    match alert_type {
        "threshold_80" => Some(80.0),
        "threshold_100" => Some(100.0),
        "threshold_120" => Some(120.0),
        _ => None,
    }
}

fn valid_alert_type(alert_type: &str) -> bool {
    matches!(
        alert_type,
        "threshold_80"
            | "threshold_100"
            | "threshold_120"
            | "forecast_overrun"
            | "labor_hour_overrun"
            | "overtime_spike"
            | "contractor_cost_drift"
            | "emergency_spend_concentration"
            | "assignment_risk"
    )
}

fn current_utc_text() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

async fn get_cost_center_row(db: &DatabaseConnection, cost_center_id: i64) -> AppResult<(i64, Option<i64>, i64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_cost_center_id, is_active FROM cost_centers WHERE id = ?",
            [cost_center_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "CostCenter".to_string(),
            id: cost_center_id.to_string(),
        })?;
    Ok((
        row.try_get("", "id")?,
        row.try_get("", "parent_cost_center_id")?,
        row.try_get("", "is_active")?,
    ))
}

async fn validate_cost_center_parent(
    db: &DatabaseConnection,
    current_id: Option<i64>,
    parent_id: Option<i64>,
    child_is_active: bool,
) -> AppResult<()> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };

    if current_id.is_some_and(|id| id == parent_id) {
        return Err(AppError::ValidationFailed(vec![
            "A cost center cannot be its own parent.".to_string(),
        ]));
    }

    let (_, mut ancestor, parent_is_active) = get_cost_center_row(db, parent_id).await?;

    if child_is_active && parent_is_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "An active cost center cannot be attached to an inactive parent.".to_string(),
        ]));
    }

    while let Some(ancestor_id) = ancestor {
        if current_id.is_some_and(|id| id == ancestor_id) {
            return Err(AppError::ValidationFailed(vec![
                "Cost center hierarchy cannot contain a cycle.".to_string(),
            ]));
        }
        let (_, next_parent, _) = get_cost_center_row(db, ancestor_id).await?;
        ancestor = next_parent;
    }

    Ok(())
}

async fn get_budget_version(db: &DatabaseConnection, version_id: i64) -> AppResult<BudgetVersion> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM budget_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetVersion".to_string(),
            id: version_id.to_string(),
        })?;
    map_budget_version(&row)
}

async fn ensure_version_editable(db: &DatabaseConnection, version_id: i64) -> AppResult<BudgetVersion> {
    let version = get_budget_version(db, version_id).await?;
    if version.status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "Only draft budget versions can be edited directly.".to_string(),
        ]));
    }
    Ok(version)
}

async fn next_version_no(db: &DatabaseConnection, fiscal_year: i64, scenario_type: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(version_no), 0) + 1 AS next_version_no
             FROM budget_versions
             WHERE fiscal_year = ? AND scenario_type = ?",
            [fiscal_year.into(), scenario_type.to_string().into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to compute next budget version number.")))?;
    row.try_get("", "next_version_no").map_err(AppError::from)
}

async fn ensure_single_frozen_baseline(
    db: &DatabaseConnection,
    version_id: i64,
    fiscal_year: i64,
    scenario_type: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id
             FROM budget_versions
             WHERE fiscal_year = ?
               AND scenario_type = ?
               AND status = 'frozen'
               AND id <> ?
             LIMIT 1",
            [fiscal_year.into(), scenario_type.to_string().into(), version_id.into()],
        ))
        .await?;
    if row.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Only one frozen control baseline is allowed per fiscal year and scenario.".to_string(),
        ]));
    }
    Ok(())
}

pub async fn list_cost_centers(db: &DatabaseConnection, filter: CostCenterFilter) -> AppResult<Vec<CostCenter>> {
    let mut sql = String::from(
        "SELECT cc.*, parent.code AS parent_cost_center_code, org.name AS entity_name
         FROM cost_centers cc
         LEFT JOIN cost_centers parent ON parent.id = cc.parent_cost_center_id
         LEFT JOIN org_nodes org ON org.id = cc.entity_id
         WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();

    if let Some(entity_id) = filter.entity_id {
        sql.push_str(" AND cc.entity_id = ?");
        values.push(entity_id.into());
    }
    if !filter.include_inactive.unwrap_or(false) {
        sql.push_str(" AND cc.is_active = 1");
    }
    if let Some(search) = optional_trimmed(&filter.search) {
        sql.push_str(" AND (LOWER(cc.code) LIKE LOWER(?) OR LOWER(cc.name) LIKE LOWER(?))");
        let pattern = format!("%{search}%");
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }
    sql.push_str(" ORDER BY cc.code ASC");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_cost_center).collect()
}

pub async fn create_cost_center(db: &DatabaseConnection, input: CreateCostCenterInput) -> AppResult<CostCenter> {
    let code = required_trimmed("Cost center code", &input.code)?;
    let name = required_trimmed("Cost center name", &input.name)?;
    let erp_external_id = optional_trimmed(&input.erp_external_id);
    let is_active = input.is_active.unwrap_or(true);

    validate_cost_center_parent(db, None, input.parent_cost_center_id, is_active).await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO cost_centers (
            code,
            name,
            entity_id,
            parent_cost_center_id,
            budget_owner_id,
            erp_external_id,
            is_active
         ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            code.into(),
            name.into(),
            input.entity_id.into(),
            input.parent_cost_center_id.into(),
            input.budget_owner_id.into(),
            erp_external_id.into(),
            i64::from(is_active).into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM cost_centers WHERE code = ?",
            [input.code.trim().to_string().into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted cost center could not be reloaded.")))?;
    let id: i64 = row.try_get("", "id")?;
    let rows = list_cost_centers(
        db,
        CostCenterFilter {
            include_inactive: Some(true),
            ..CostCenterFilter::default()
        },
    )
    .await?;
    rows.into_iter()
        .find(|cost_center| cost_center.id == id)
        .ok_or_else(|| AppError::NotFound {
            entity: "CostCenter".to_string(),
            id: id.to_string(),
        })
}

pub async fn update_cost_center(
    db: &DatabaseConnection,
    cost_center_id: i64,
    expected_row_version: i64,
    input: UpdateCostCenterInput,
) -> AppResult<CostCenter> {
    let current = list_cost_centers(
        db,
        CostCenterFilter {
            include_inactive: Some(true),
            ..CostCenterFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|cost_center| cost_center.id == cost_center_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "CostCenter".to_string(),
        id: cost_center_id.to_string(),
    })?;

    if current.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Cost center was modified elsewhere (stale row_version).".to_string(),
        ]));
    }

    let code = input.code.as_deref().map(|value| required_trimmed("Cost center code", value)).transpose()?;
    let name = input.name.as_deref().map(|value| required_trimmed("Cost center name", value)).transpose()?;
    let is_active = input.is_active.unwrap_or(current.is_active == 1);
    let parent_cost_center_id = input.parent_cost_center_id.or(current.parent_cost_center_id);

    validate_cost_center_parent(db, Some(cost_center_id), parent_cost_center_id, is_active).await?;

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE cost_centers
             SET code = COALESCE(?, code),
                 name = COALESCE(?, name),
                 entity_id = COALESCE(?, entity_id),
                 parent_cost_center_id = ?,
                 budget_owner_id = COALESCE(?, budget_owner_id),
                 erp_external_id = ?,
                 is_active = ?,
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                code.into(),
                name.into(),
                input.entity_id.or(current.entity_id).into(),
                parent_cost_center_id.into(),
                input.budget_owner_id.or(current.budget_owner_id).into(),
                optional_trimmed(&input.erp_external_id).or(current.erp_external_id).into(),
                i64::from(is_active).into(),
                cost_center_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Cost center update failed (stale row_version).".to_string(),
        ]));
    }

    list_cost_centers(
        db,
        CostCenterFilter {
            include_inactive: Some(true),
            ..CostCenterFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|cost_center| cost_center.id == cost_center_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "CostCenter".to_string(),
        id: cost_center_id.to_string(),
    })
}

pub async fn list_budget_versions(
    db: &DatabaseConnection,
    filter: BudgetVersionFilter,
) -> AppResult<Vec<BudgetVersion>> {
    let mut sql = String::from("SELECT * FROM budget_versions WHERE 1 = 1");
    let mut values: Vec<Value> = Vec::new();
    if let Some(fiscal_year) = filter.fiscal_year {
        sql.push_str(" AND fiscal_year = ?");
        values.push(fiscal_year.into());
    }
    if let Some(scenario_type) = optional_trimmed(&filter.scenario_type) {
        sql.push_str(" AND scenario_type = ?");
        values.push(scenario_type.into());
    }
    if let Some(status) = optional_trimmed(&filter.status) {
        sql.push_str(" AND status = ?");
        values.push(status.into());
    }
    sql.push_str(" ORDER BY fiscal_year DESC, scenario_type ASC, version_no DESC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_version).collect()
}

pub async fn create_budget_version(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: CreateBudgetVersionInput,
) -> AppResult<BudgetVersion> {
    let scenario_type = required_trimmed("Scenario type", &input.scenario_type)?;
    let currency_code = required_trimmed("Currency code", &input.currency_code)?;
    let version_no = next_version_no(db, input.fiscal_year, &scenario_type).await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_versions (
            fiscal_year,
            scenario_type,
            version_no,
            status,
            currency_code,
            title,
            planning_basis,
            source_basis_mix_json,
            labor_assumptions_json,
            baseline_reference,
            erp_external_ref,
            created_by_id
         ) VALUES (?, ?, ?, 'draft', ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.fiscal_year.into(),
            scenario_type.into(),
            version_no.into(),
            currency_code.into(),
            optional_trimmed(&input.title).into(),
            optional_trimmed(&input.planning_basis).into(),
            optional_trimmed(&input.source_basis_mix_json).into(),
            optional_trimmed(&input.labor_assumptions_json).into(),
            optional_trimmed(&input.baseline_reference).into(),
            optional_trimmed(&input.erp_external_ref).into(),
            actor_user_id.into(),
        ],
    ))
    .await?;

    list_budget_versions(
        db,
        BudgetVersionFilter {
            fiscal_year: Some(input.fiscal_year),
            scenario_type: Some(input.scenario_type),
            ..BudgetVersionFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|version| version.version_no == version_no)
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted budget version could not be reloaded.")))
}

pub async fn create_budget_successor_version(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: CreateBudgetSuccessorInput,
) -> AppResult<BudgetVersion> {
    let source = get_budget_version(db, input.source_version_id).await?;
    if !matches!(source.status.as_str(), "approved" | "frozen" | "closed" | "superseded") {
        return Err(AppError::ValidationFailed(vec![
            "Successor versions can only be created from approved, frozen, closed, or superseded baselines."
                .to_string(),
        ]));
    }

    let fiscal_year = input.fiscal_year.unwrap_or(source.fiscal_year);
    let scenario_type = input
        .scenario_type
        .as_deref()
        .map(|value| required_trimmed("Scenario type", value))
        .transpose()?
        .unwrap_or(source.scenario_type.clone());
    let version_no = next_version_no(db, fiscal_year, &scenario_type).await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_versions (
            fiscal_year,
            scenario_type,
            version_no,
            status,
            currency_code,
            title,
            planning_basis,
            source_basis_mix_json,
            labor_assumptions_json,
            baseline_reference,
            erp_external_ref,
            successor_of_version_id,
            created_by_id
         ) VALUES (?, ?, ?, 'draft', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            fiscal_year.into(),
            scenario_type.clone().into(),
            version_no.into(),
            source.currency_code.into(),
            optional_trimmed(&input.title).or(source.title).into(),
            source.planning_basis.into(),
            source.source_basis_mix_json.into(),
            source.labor_assumptions_json.into(),
            optional_trimmed(&input.baseline_reference).or(source.baseline_reference).into(),
            source.erp_external_ref.into(),
            input.source_version_id.into(),
            actor_user_id.into(),
        ],
    ))
    .await?;

    let inserted = list_budget_versions(
        db,
        BudgetVersionFilter {
            fiscal_year: Some(fiscal_year),
            scenario_type: Some(scenario_type.clone()),
            ..BudgetVersionFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|version| version.version_no == version_no)
    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted successor budget version could not be reloaded.")))?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_lines (
            budget_version_id,
            cost_center_id,
            period_month,
            budget_bucket,
            planned_amount,
            source_basis,
            justification_note,
            asset_family,
            work_category,
            shutdown_package_ref,
            team_id,
            skill_pool_id,
            labor_lane
         )
         SELECT ?, cost_center_id, period_month, budget_bucket, planned_amount, source_basis, justification_note,
                asset_family, work_category, shutdown_package_ref, team_id, skill_pool_id, labor_lane
         FROM budget_lines
         WHERE budget_version_id = ?",
        [inserted.id.into(), input.source_version_id.into()],
    ))
    .await?;

    Ok(inserted)
}

pub async fn update_budget_version(
    db: &DatabaseConnection,
    version_id: i64,
    expected_row_version: i64,
    input: UpdateBudgetVersionInput,
) -> AppResult<BudgetVersion> {
    let version = ensure_version_editable(db, version_id).await?;
    if version.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Budget version was modified elsewhere (stale row_version).".to_string(),
        ]));
    }

    let currency_code = input
        .currency_code
        .as_deref()
        .map(|value| required_trimmed("Currency code", value))
        .transpose()?;

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE budget_versions
             SET currency_code = COALESCE(?, currency_code),
                 title = COALESCE(?, title),
                 planning_basis = COALESCE(?, planning_basis),
                 source_basis_mix_json = COALESCE(?, source_basis_mix_json),
                 labor_assumptions_json = COALESCE(?, labor_assumptions_json),
                 baseline_reference = COALESCE(?, baseline_reference),
                 erp_external_ref = COALESCE(?, erp_external_ref),
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                currency_code.into(),
                optional_trimmed(&input.title).or(version.title).into(),
                optional_trimmed(&input.planning_basis).or(version.planning_basis).into(),
                optional_trimmed(&input.source_basis_mix_json)
                    .or(version.source_basis_mix_json)
                    .into(),
                optional_trimmed(&input.labor_assumptions_json)
                    .or(version.labor_assumptions_json)
                    .into(),
                optional_trimmed(&input.baseline_reference).or(version.baseline_reference).into(),
                optional_trimmed(&input.erp_external_ref).or(version.erp_external_ref).into(),
                version_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Budget version update failed (stale row_version).".to_string(),
        ]));
    }

    get_budget_version(db, version_id).await
}

pub async fn transition_budget_version_lifecycle(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: TransitionBudgetVersionLifecycleInput,
) -> AppResult<BudgetVersion> {
    let version = get_budget_version(db, input.version_id).await?;
    if version.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Budget lifecycle transition failed (stale row_version).".to_string(),
        ]));
    }
    if !valid_version_transition(&version.status, &input.next_status) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Invalid budget lifecycle transition: {} -> {}.",
            version.status, input.next_status
        )]));
    }
    if input.next_status == "frozen" {
        ensure_single_frozen_baseline(db, version.id, version.fiscal_year, &version.scenario_type).await?;
    }

    let approved_at = if input.next_status == "approved" {
        Some("strftime('%Y-%m-%dT%H:%M:%SZ','now')")
    } else {
        None
    };
    let frozen_at = if input.next_status == "frozen" {
        Some("strftime('%Y-%m-%dT%H:%M:%SZ','now')")
    } else {
        None
    };

    let sql = format!(
        "UPDATE budget_versions
         SET status = ?,
             approved_at = {},
             approved_by_id = CASE WHEN ? = 'approved' THEN ? ELSE approved_by_id END,
             frozen_at = {},
             frozen_by_id = CASE WHEN ? = 'frozen' THEN ? ELSE frozen_by_id END,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        approved_at.unwrap_or("approved_at"),
        frozen_at.unwrap_or("frozen_at"),
    );

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                input.next_status.clone().into(),
                input.next_status.clone().into(),
                actor_user_id.into(),
                input.next_status.clone().into(),
                actor_user_id.into(),
                input.version_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Budget lifecycle transition failed.".to_string(),
        ]));
    }

    get_budget_version(db, input.version_id).await
}

pub async fn list_budget_lines(db: &DatabaseConnection, filter: BudgetLineFilter) -> AppResult<Vec<BudgetLine>> {
    let mut sql = String::from(
        "SELECT bl.*, cc.code AS cost_center_code, cc.name AS cost_center_name
         FROM budget_lines bl
         JOIN cost_centers cc ON cc.id = bl.cost_center_id
         WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND bl.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND bl.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    sql.push_str(" ORDER BY COALESCE(bl.period_month, 0) ASC, cc.code ASC, bl.budget_bucket ASC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_line).collect()
}

pub async fn create_budget_line(db: &DatabaseConnection, input: CreateBudgetLineInput) -> AppResult<BudgetLine> {
    let version = ensure_version_editable(db, input.budget_version_id).await?;
    let _ = version;
    let budget_bucket = required_trimmed("Budget bucket", &input.budget_bucket)?;
    if input.period_month.is_some_and(|month| !(1..=12).contains(&month)) {
        return Err(AppError::ValidationFailed(vec![
            "Period month must be between 1 and 12.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_lines (
            budget_version_id,
            cost_center_id,
            period_month,
            budget_bucket,
            planned_amount,
            source_basis,
            justification_note,
            asset_family,
            work_category,
            shutdown_package_ref,
            team_id,
            skill_pool_id,
            labor_lane
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.budget_version_id.into(),
            input.cost_center_id.into(),
            input.period_month.into(),
            budget_bucket.into(),
            input.planned_amount.into(),
            optional_trimmed(&input.source_basis).into(),
            optional_trimmed(&input.justification_note).into(),
            optional_trimmed(&input.asset_family).into(),
            optional_trimmed(&input.work_category).into(),
            optional_trimmed(&input.shutdown_package_ref).into(),
            input.team_id.into(),
            input.skill_pool_id.into(),
            optional_trimmed(&input.labor_lane).into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted budget line id missing.")))?;
    let line_id: i64 = row.try_get("", "id")?;
    list_budget_lines(
        db,
        BudgetLineFilter {
            budget_version_id: Some(input.budget_version_id),
            ..BudgetLineFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|line| line.id == line_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetLine".to_string(),
        id: line_id.to_string(),
    })
}

pub async fn update_budget_line(
    db: &DatabaseConnection,
    line_id: i64,
    expected_row_version: i64,
    input: UpdateBudgetLineInput,
) -> AppResult<BudgetLine> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, budget_version_id, row_version FROM budget_lines WHERE id = ?",
            [line_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetLine".to_string(),
            id: line_id.to_string(),
        })?;
    let budget_version_id: i64 = row.try_get("", "budget_version_id")?;
    let row_version: i64 = row.try_get("", "row_version")?;
    ensure_version_editable(db, budget_version_id).await?;
    if row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Budget line was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    if input.period_month.is_some_and(|month| !(1..=12).contains(&month)) {
        return Err(AppError::ValidationFailed(vec![
            "Period month must be between 1 and 12.".to_string(),
        ]));
    }
    let budget_bucket = input
        .budget_bucket
        .as_deref()
        .map(|value| required_trimmed("Budget bucket", value))
        .transpose()?;

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE budget_lines
             SET period_month = COALESCE(?, period_month),
                 budget_bucket = COALESCE(?, budget_bucket),
                 planned_amount = COALESCE(?, planned_amount),
                 source_basis = COALESCE(?, source_basis),
                 justification_note = COALESCE(?, justification_note),
                 asset_family = COALESCE(?, asset_family),
                 work_category = COALESCE(?, work_category),
                 shutdown_package_ref = COALESCE(?, shutdown_package_ref),
                 team_id = COALESCE(?, team_id),
                 skill_pool_id = COALESCE(?, skill_pool_id),
                 labor_lane = COALESCE(?, labor_lane),
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                input.period_month.into(),
                budget_bucket.into(),
                input.planned_amount.into(),
                optional_trimmed(&input.source_basis).into(),
                optional_trimmed(&input.justification_note).into(),
                optional_trimmed(&input.asset_family).into(),
                optional_trimmed(&input.work_category).into(),
                optional_trimmed(&input.shutdown_package_ref).into(),
                input.team_id.into(),
                input.skill_pool_id.into(),
                optional_trimmed(&input.labor_lane).into(),
                line_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Budget line update failed (stale row_version).".to_string(),
        ]));
    }

    list_budget_lines(
        db,
        BudgetLineFilter {
            budget_version_id: Some(budget_version_id),
            ..BudgetLineFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|line| line.id == line_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetLine".to_string(),
        id: line_id.to_string(),
    })
}

pub async fn list_budget_actuals(db: &DatabaseConnection, filter: BudgetActualFilter) -> AppResult<Vec<BudgetActual>> {
    let mut sql = String::from(
        "SELECT ba.*, cc.code AS cost_center_code, cc.name AS cost_center_name
         FROM budget_actuals ba
         JOIN cost_centers cc ON cc.id = ba.cost_center_id
         WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND ba.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND ba.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    if let Some(period_month) = filter.period_month {
        sql.push_str(" AND ba.period_month = ?");
        values.push(period_month.into());
    }
    if let Some(budget_bucket) = optional_trimmed(&filter.budget_bucket) {
        sql.push_str(" AND ba.budget_bucket = ?");
        values.push(budget_bucket.into());
    }
    if let Some(posting_status) = optional_trimmed(&filter.posting_status) {
        sql.push_str(" AND ba.posting_status = ?");
        values.push(posting_status.into());
    }
    if let Some(source_type) = optional_trimmed(&filter.source_type) {
        sql.push_str(" AND ba.source_type = ?");
        values.push(source_type.into());
    }
    sql.push_str(" ORDER BY ba.event_at DESC, ba.id DESC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_actual).collect()
}

pub async fn create_budget_actual(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: CreateBudgetActualInput,
    allow_posted: bool,
) -> AppResult<BudgetActual> {
    let _ = get_budget_version(db, input.budget_version_id).await?;
    let budget_bucket = required_trimmed("Budget bucket", &input.budget_bucket)?;
    let source_type = required_trimmed("Source type", &input.source_type)?;
    let source_id = required_trimmed("Source id", &input.source_id)?;
    let source_currency = required_trimmed("Source currency", &input.source_currency)?;
    let base_currency = required_trimmed("Base currency", &input.base_currency)?;
    if input.period_month.is_some_and(|month| !(1..=12).contains(&month)) {
        return Err(AppError::ValidationFailed(vec![
            "Period month must be between 1 and 12.".to_string(),
        ]));
    }
    if input.amount_base.abs() < f64::EPSILON {
        return Err(AppError::ValidationFailed(vec![
            "Actual amount_base cannot be zero.".to_string(),
        ]));
    }
    let posting_status = optional_trimmed(&input.posting_status).unwrap_or_else(|| "provisional".to_string());
    if posting_status != "provisional" && posting_status != "posted" {
        return Err(AppError::ValidationFailed(vec![
            "posting_status must be provisional or posted.".to_string(),
        ]));
    }
    if posting_status == "posted" && !allow_posted {
        return Err(AppError::ValidationFailed(vec![
            "Posting actuals requires fin.post permission.".to_string(),
        ]));
    }
    if budget_bucket == "labor"
        && input.personnel_id.is_none()
        && input.team_id.is_none()
        && optional_trimmed(&input.rate_card_lane).is_none()
    {
        return Err(AppError::ValidationFailed(vec![
            "Labor actuals must preserve at least one split dimension: personnel_id, team_id, or rate_card_lane."
                .to_string(),
        ]));
    }

    let posted_at_sql = if posting_status == "posted" {
        "strftime('%Y-%m-%dT%H:%M:%SZ','now')"
    } else {
        "NULL"
    };
    let posted_by_id = if posting_status == "posted" {
        Some(actor_user_id)
    } else {
        None
    };
    let event_at = optional_trimmed(&input.event_at);

    let sql = format!(
        "INSERT INTO budget_actuals (
            budget_version_id,
            cost_center_id,
            period_month,
            budget_bucket,
            amount_source,
            source_currency,
            amount_base,
            base_currency,
            source_type,
            source_id,
            work_order_id,
            equipment_id,
            posting_status,
            provisional_reason,
            posted_at,
            posted_by_id,
            personnel_id,
            team_id,
            rate_card_lane,
            event_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, {}, ?, ?, ?, ?, COALESCE(?, strftime('%Y-%m-%dT%H:%M:%SZ','now')))",
        posted_at_sql
    );

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [
            input.budget_version_id.into(),
            input.cost_center_id.into(),
            input.period_month.into(),
            budget_bucket.into(),
            input.amount_source.into(),
            source_currency.into(),
            input.amount_base.into(),
            base_currency.into(),
            source_type.into(),
            source_id.into(),
            input.work_order_id.into(),
            input.equipment_id.into(),
            posting_status.into(),
            optional_trimmed(&input.provisional_reason).into(),
            posted_by_id.into(),
            input.personnel_id.into(),
            input.team_id.into(),
            optional_trimmed(&input.rate_card_lane).into(),
            event_at.into(),
        ],
    ))
    .await?;

    let inserted = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted budget actual id missing.")))?;
    let actual_id: i64 = inserted.try_get("", "id")?;
    list_budget_actuals(
        db,
        BudgetActualFilter {
            budget_version_id: Some(input.budget_version_id),
            ..BudgetActualFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|actual| actual.id == actual_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetActual".to_string(),
        id: actual_id.to_string(),
    })
}

pub async fn post_budget_actual(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: PostBudgetActualInput,
) -> AppResult<BudgetActual> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, budget_version_id, posting_status, row_version FROM budget_actuals WHERE id = ?",
            [input.actual_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetActual".to_string(),
            id: input.actual_id.to_string(),
        })?;
    let budget_version_id: i64 = row.try_get("", "budget_version_id")?;
    let posting_status: String = row.try_get("", "posting_status")?;
    let row_version: i64 = row.try_get("", "row_version")?;
    if row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Budget actual was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    if posting_status != "provisional" {
        return Err(AppError::ValidationFailed(vec![
            "Only provisional actuals can be posted.".to_string(),
        ]));
    }

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE budget_actuals
             SET posting_status = 'posted',
                 posted_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 posted_by_id = ?,
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [actor_user_id.into(), input.actual_id.into(), input.expected_row_version.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Budget actual posting failed (stale row_version).".to_string(),
        ]));
    }

    list_budget_actuals(
        db,
        BudgetActualFilter {
            budget_version_id: Some(budget_version_id),
            ..BudgetActualFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|actual| actual.id == input.actual_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetActual".to_string(),
        id: input.actual_id.to_string(),
    })
}

pub async fn reverse_budget_actual(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: ReverseBudgetActualInput,
) -> AppResult<BudgetActual> {
    let reason = required_trimmed("Reversal reason", &input.reason)?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM budget_actuals WHERE id = ?",
            [input.actual_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetActual".to_string(),
            id: input.actual_id.to_string(),
        })?;
    let current_id: i64 = row.try_get("", "id")?;
    let current_budget_version_id: i64 = row.try_get("", "budget_version_id")?;
    let current_cost_center_id: i64 = row.try_get("", "cost_center_id")?;
    let current_period_month: Option<i64> = row.try_get("", "period_month")?;
    let current_budget_bucket: String = row.try_get("", "budget_bucket")?;
    let current_amount_source: f64 = row.try_get("", "amount_source")?;
    let current_source_currency: String = row.try_get("", "source_currency")?;
    let current_amount_base: f64 = row.try_get("", "amount_base")?;
    let current_base_currency: String = row.try_get("", "base_currency")?;
    let current_source_id: String = row.try_get("", "source_id")?;
    let current_work_order_id: Option<i64> = row.try_get("", "work_order_id")?;
    let current_equipment_id: Option<i64> = row.try_get("", "equipment_id")?;
    let current_posting_status: String = row.try_get("", "posting_status")?;
    let current_row_version: i64 = row.try_get("", "row_version")?;
    let current_reversal_of_actual_id: Option<i64> = row.try_get("", "reversal_of_actual_id")?;
    let current_personnel_id: Option<i64> = row.try_get("", "personnel_id")?;
    let current_team_id: Option<i64> = row.try_get("", "team_id")?;
    let current_rate_card_lane: Option<String> = row.try_get("", "rate_card_lane")?;

    if current_row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Budget actual reversal failed (stale row_version).".to_string(),
        ]));
    }
    if current_posting_status != "posted" {
        return Err(AppError::ValidationFailed(vec![
            "Only posted actuals can be reversed.".to_string(),
        ]));
    }
    if current_reversal_of_actual_id.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Reversal records cannot be reversed again.".to_string(),
        ]));
    }
    let already_reversed = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM budget_actuals WHERE reversal_of_actual_id = ? LIMIT 1",
            [current_id.into()],
        ))
        .await?;
    if already_reversed.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Actual already has a reversal record.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_actuals (
            budget_version_id,
            cost_center_id,
            period_month,
            budget_bucket,
            amount_source,
            source_currency,
            amount_base,
            base_currency,
            source_type,
            source_id,
            work_order_id,
            equipment_id,
            posting_status,
            posted_at,
            posted_by_id,
            reversal_of_actual_id,
            reversal_reason,
            personnel_id,
            team_id,
            rate_card_lane,
            event_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'reversal', ?, ?, ?, 'reversed',
                   strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, ?, ?, ?, ?, ?,
                   strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [
            current_budget_version_id.into(),
            current_cost_center_id.into(),
            current_period_month.into(),
            current_budget_bucket.into(),
            (-current_amount_source).into(),
            current_source_currency.into(),
            (-current_amount_base).into(),
            current_base_currency.into(),
            format!("REV-{}-{}", current_id, current_source_id).into(),
            current_work_order_id.into(),
            current_equipment_id.into(),
            actor_user_id.into(),
            current_id.into(),
            reason.into(),
            current_personnel_id.into(),
            current_team_id.into(),
            current_rate_card_lane.into(),
        ],
    ))
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE budget_actuals
         SET posting_status = 'reversed',
             reversal_reason = ?,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        [input.reason.into(), input.actual_id.into(), input.expected_row_version.into()],
    ))
    .await?;

    let inserted = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted reversal id missing.")))?;
    let reversal_id: i64 = inserted.try_get("", "id")?;

    list_budget_actuals(
        db,
        BudgetActualFilter {
            budget_version_id: Some(current_budget_version_id),
            ..BudgetActualFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|actual| actual.id == reversal_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetActual".to_string(),
        id: reversal_id.to_string(),
    })
}

pub async fn list_budget_commitments(
    db: &DatabaseConnection,
    filter: BudgetCommitmentFilter,
) -> AppResult<Vec<BudgetCommitment>> {
    let mut sql = String::from(
        "SELECT bc.*, cc.code AS cost_center_code, cc.name AS cost_center_name
         FROM budget_commitments bc
         JOIN cost_centers cc ON cc.id = bc.cost_center_id
         WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND bc.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND bc.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    if let Some(period_month) = filter.period_month {
        sql.push_str(" AND bc.period_month = ?");
        values.push(period_month.into());
    }
    if let Some(budget_bucket) = optional_trimmed(&filter.budget_bucket) {
        sql.push_str(" AND bc.budget_bucket = ?");
        values.push(budget_bucket.into());
    }
    if let Some(commitment_status) = optional_trimmed(&filter.commitment_status) {
        sql.push_str(" AND bc.commitment_status = ?");
        values.push(commitment_status.into());
    }
    if let Some(source_type) = optional_trimmed(&filter.source_type) {
        sql.push_str(" AND bc.source_type = ?");
        values.push(source_type.into());
    }
    sql.push_str(" ORDER BY COALESCE(bc.period_month, 0) ASC, bc.id DESC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_commitment).collect()
}

pub async fn create_budget_commitment(
    db: &DatabaseConnection,
    input: CreateBudgetCommitmentInput,
) -> AppResult<BudgetCommitment> {
    let _ = get_budget_version(db, input.budget_version_id).await?;
    if input.period_month.is_some_and(|month| !(1..=12).contains(&month)) {
        return Err(AppError::ValidationFailed(vec![
            "Period month must be between 1 and 12.".to_string(),
        ]));
    }
    let budget_bucket = required_trimmed("Budget bucket", &input.budget_bucket)?;
    let commitment_type = required_trimmed("Commitment type", &input.commitment_type)?;
    let source_type = required_trimmed("Source type", &input.source_type)?;
    let source_id = required_trimmed("Source id", &input.source_id)?;
    let source_currency = required_trimmed("Source currency", &input.source_currency)?;
    let base_currency = required_trimmed("Base currency", &input.base_currency)?;
    let commitment_status =
        optional_trimmed(&input.commitment_status).unwrap_or_else(|| "open".to_string());

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_commitments (
            budget_version_id,
            cost_center_id,
            period_month,
            budget_bucket,
            commitment_type,
            source_type,
            source_id,
            obligation_amount,
            source_currency,
            base_amount,
            base_currency,
            commitment_status,
            work_order_id,
            contract_id,
            purchase_order_id,
            planning_commitment_ref,
            due_at,
            explainability_note
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.budget_version_id.into(),
            input.cost_center_id.into(),
            input.period_month.into(),
            budget_bucket.into(),
            commitment_type.into(),
            source_type.into(),
            source_id.into(),
            input.obligation_amount.into(),
            source_currency.into(),
            input.base_amount.into(),
            base_currency.into(),
            commitment_status.into(),
            input.work_order_id.into(),
            input.contract_id.into(),
            input.purchase_order_id.into(),
            optional_trimmed(&input.planning_commitment_ref).into(),
            optional_trimmed(&input.due_at).into(),
            optional_trimmed(&input.explainability_note).into(),
        ],
    ))
    .await?;

    let inserted = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted budget commitment id missing.")))?;
    let commitment_id: i64 = inserted.try_get("", "id")?;
    list_budget_commitments(
        db,
        BudgetCommitmentFilter {
            budget_version_id: Some(input.budget_version_id),
            ..BudgetCommitmentFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|commitment| commitment.id == commitment_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetCommitment".to_string(),
        id: commitment_id.to_string(),
    })
}

pub async fn list_forecast_runs(
    db: &DatabaseConnection,
    budget_version_id: Option<i64>,
) -> AppResult<Vec<ForecastRun>> {
    let mut sql = String::from("SELECT * FROM budget_forecast_runs WHERE 1 = 1");
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = budget_version_id {
        sql.push_str(" AND budget_version_id = ?");
        values.push(version_id.into());
    }
    sql.push_str(" ORDER BY generated_at DESC, id DESC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_forecast_run).collect()
}

pub async fn list_budget_forecasts(
    db: &DatabaseConnection,
    filter: BudgetForecastFilter,
) -> AppResult<Vec<BudgetForecast>> {
    let mut sql = String::from(
        "SELECT bf.*, cc.code AS cost_center_code, cc.name AS cost_center_name
         FROM budget_forecasts bf
         JOIN cost_centers cc ON cc.id = bf.cost_center_id
         WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND bf.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(run_id) = filter.forecast_run_id {
        sql.push_str(" AND bf.forecast_run_id = ?");
        values.push(run_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND bf.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    if let Some(period_month) = filter.period_month {
        sql.push_str(" AND bf.period_month = ?");
        values.push(period_month.into());
    }
    if let Some(budget_bucket) = optional_trimmed(&filter.budget_bucket) {
        sql.push_str(" AND bf.budget_bucket = ?");
        values.push(budget_bucket.into());
    }
    if let Some(method) = optional_trimmed(&filter.forecast_method) {
        sql.push_str(" AND bf.forecast_method = ?");
        values.push(method.into());
    }
    sql.push_str(" ORDER BY COALESCE(bf.period_month, 0) ASC, bf.cost_center_id ASC, bf.id ASC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_forecast).collect()
}

pub async fn generate_budget_forecasts(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: GenerateBudgetForecastInput,
) -> AppResult<BudgetForecastGenerationResult> {
    let _ = get_budget_version(db, input.budget_version_id).await?;
    let idempotency_key = required_trimmed("Idempotency key", &input.idempotency_key)?;
    let scope_signature = required_trimmed("Scope signature", &input.scope_signature)?;

    if let Some(existing_row) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM budget_forecast_runs WHERE idempotency_key = ?",
            [idempotency_key.clone().into()],
        ))
        .await?
    {
        let run = map_forecast_run(&existing_row)?;
        let forecasts = list_budget_forecasts(
            db,
            BudgetForecastFilter {
                forecast_run_id: Some(run.id),
                ..BudgetForecastFilter::default()
            },
        )
        .await?;
        return Ok(BudgetForecastGenerationResult {
            run,
            forecasts,
            reused_existing_run: true,
        });
    }

    let include_pm = input.include_pm_occurrence.unwrap_or(true);
    let include_backlog = input.include_backlog_demand.unwrap_or(true);
    let include_shutdown = input.include_shutdown_demand.unwrap_or(true);
    let include_planning = input.include_planning_demand.unwrap_or(true);
    let include_burn_rate = input.include_burn_rate.unwrap_or(true);

    let method_mix_json = format!(
        "{{\"pm_occurrence\":{},\"backlog\":{},\"shutdown\":{},\"planning\":{},\"burn_rate\":{}}}",
        include_pm, include_backlog, include_shutdown, include_planning, include_burn_rate
    );

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_forecast_runs (
            budget_version_id,
            generated_by_id,
            idempotency_key,
            scope_signature,
            method_mix_json,
            confidence_policy_json
         ) VALUES (?, ?, ?, ?, ?, ?)",
        [
            input.budget_version_id.into(),
            actor_user_id.into(),
            idempotency_key.into(),
            scope_signature.into(),
            method_mix_json.into(),
            optional_trimmed(&input.confidence_policy_json).into(),
        ],
    ))
    .await?;

    let run_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT * FROM budget_forecast_runs WHERE id = last_insert_rowid()".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted forecast run missing.")))?;
    let run = map_forecast_run(&run_row)?;

    let lines = list_budget_lines(
        db,
        BudgetLineFilter {
            budget_version_id: Some(input.budget_version_id),
            ..BudgetLineFilter::default()
        },
    )
    .await?;

    for line in &lines {
        if let Some(start) = input.period_month_start {
            if line.period_month.is_some_and(|month| month < start) {
                continue;
            }
        }
        if let Some(end) = input.period_month_end {
            if line.period_month.is_some_and(|month| month > end) {
                continue;
            }
        }

        let source_basis = line.source_basis.as_deref().unwrap_or_default().to_ascii_lowercase();
        let work_category = line.work_category.as_deref().unwrap_or_default().to_ascii_lowercase();

        let (method, confidence, driver_type) = if include_shutdown && line.shutdown_package_ref.is_some() {
            ("shutdown_loaded", "high", Some("shutdown_demand"))
        } else if include_pm && (source_basis.contains("pm") || work_category.contains("preventive")) {
            ("pm_occurrence", "high", Some("pm_demand"))
        } else if include_backlog && source_basis.contains("backlog") {
            ("manual", "medium", Some("backlog_demand"))
        } else if include_planning && source_basis.contains("planning") {
            ("manual", "medium", Some("planning_demand"))
        } else {
            ("manual", "medium", Some("manual_seed"))
        };

        let explainability_json = format!(
            "{{\"source_basis\":\"{}\",\"work_category\":\"{}\",\"labor_lane\":{},\"planning_seed\":{}}}",
            line.source_basis.as_deref().unwrap_or(""),
            line.work_category.as_deref().unwrap_or(""),
            match line.labor_lane.as_deref() {
                Some(lane) => format!("\"{lane}\""),
                None => "null".to_string(),
            },
            if source_basis.contains("planning") { "true" } else { "false" }
        );

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO budget_forecasts (
                forecast_run_id,
                budget_version_id,
                cost_center_id,
                period_month,
                budget_bucket,
                forecast_amount,
                forecast_method,
                confidence_level,
                driver_type,
                driver_reference,
                explainability_json
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                run.id.into(),
                input.budget_version_id.into(),
                line.cost_center_id.into(),
                line.period_month.into(),
                line.budget_bucket.clone().into(),
                line.planned_amount.into(),
                method.to_string().into(),
                confidence.to_string().into(),
                driver_type.map(ToString::to_string).into(),
                line.shutdown_package_ref.clone().or(line.source_basis.clone()).into(),
                Some(explainability_json).into(),
            ],
        ))
        .await?;
    }

    if include_burn_rate {
        let burn_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT cost_center_id,
                        budget_bucket,
                        AVG(amount_base) AS burn_rate_amount
                 FROM budget_actuals
                 WHERE budget_version_id = ?
                   AND posting_status = 'posted'
                 GROUP BY cost_center_id, budget_bucket",
                [input.budget_version_id.into()],
            ))
            .await?;
        for row in &burn_rows {
            let cost_center_id: i64 = row.try_get("", "cost_center_id")?;
            let budget_bucket: String = row.try_get("", "budget_bucket")?;
            let burn_rate_amount: f64 = row.try_get("", "burn_rate_amount")?;
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO budget_forecasts (
                    forecast_run_id,
                    budget_version_id,
                    cost_center_id,
                    period_month,
                    budget_bucket,
                    forecast_amount,
                    forecast_method,
                    confidence_level,
                    driver_type,
                    driver_reference,
                    explainability_json
                 ) VALUES (?, ?, ?, NULL, ?, ?, 'burn_rate', 'medium', 'actual_burn', NULL, ?)",
                [
                    run.id.into(),
                    input.budget_version_id.into(),
                    cost_center_id.into(),
                    budget_bucket.into(),
                    burn_rate_amount.into(),
                    Some("{\"basis\":\"posted_actuals_average\"}".to_string()).into(),
                ],
            ))
            .await?;
        }
    }

    let forecasts = list_budget_forecasts(
        db,
        BudgetForecastFilter {
            forecast_run_id: Some(run.id),
            ..BudgetForecastFilter::default()
        },
    )
    .await?;

    Ok(BudgetForecastGenerationResult {
        run,
        forecasts,
        reused_existing_run: false,
    })
}

pub async fn list_budget_variance_reviews(
    db: &DatabaseConnection,
    filter: BudgetVarianceReviewFilter,
) -> AppResult<Vec<BudgetVarianceReview>> {
    let mut sql = String::from(
        "SELECT vr.*, cc.code AS cost_center_code, cc.name AS cost_center_name
         FROM budget_variance_reviews vr
         JOIN cost_centers cc ON cc.id = vr.cost_center_id
         WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND vr.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND vr.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    if let Some(period_month) = filter.period_month {
        sql.push_str(" AND vr.period_month = ?");
        values.push(period_month.into());
    }
    if let Some(review_status) = optional_trimmed(&filter.review_status) {
        sql.push_str(" AND vr.review_status = ?");
        values.push(review_status.into());
    }
    if let Some(driver_code) = optional_trimmed(&filter.driver_code) {
        sql.push_str(" AND vr.driver_code = ?");
        values.push(driver_code.into());
    }
    if let Some(action_owner_id) = filter.action_owner_id {
        sql.push_str(" AND vr.action_owner_id = ?");
        values.push(action_owner_id.into());
    }
    sql.push_str(" ORDER BY vr.opened_at DESC, vr.id DESC");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_variance_review).collect()
}

pub async fn create_budget_variance_review(
    db: &DatabaseConnection,
    input: CreateBudgetVarianceReviewInput,
) -> AppResult<BudgetVarianceReview> {
    let _ = get_budget_version(db, input.budget_version_id).await?;
    let budget_bucket = required_trimmed("Budget bucket", &input.budget_bucket)?;
    let driver_code = required_trimmed("Driver code", &input.driver_code)?;
    let review_commentary = required_trimmed("Review commentary", &input.review_commentary)?;
    let snapshot_context_json = required_trimmed("Snapshot context json", &input.snapshot_context_json)?;
    if input.period_month.is_some_and(|month| !(1..=12).contains(&month)) {
        return Err(AppError::ValidationFailed(vec![
            "Period month must be between 1 and 12.".to_string(),
        ]));
    }
    if !valid_variance_driver(&driver_code) {
        return Err(AppError::ValidationFailed(vec![
            "driver_code must be one of the governed variance taxonomy values.".to_string(),
        ]));
    }

    let duplicate = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM budget_variance_reviews
             WHERE budget_version_id = ?
               AND cost_center_id = ?
               AND COALESCE(period_month, -1) = COALESCE(?, -1)
               AND budget_bucket = ?
               AND review_status <> 'closed'
             LIMIT 1",
            [
                input.budget_version_id.into(),
                input.cost_center_id.into(),
                input.period_month.into(),
                budget_bucket.clone().into(),
            ],
        ))
        .await?;
    if duplicate.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Duplicate open variance review exists for the same budget scope.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_variance_reviews (
            budget_version_id,
            cost_center_id,
            period_month,
            budget_bucket,
            variance_amount,
            variance_pct,
            driver_code,
            action_owner_id,
            review_status,
            review_commentary,
            snapshot_context_json
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'open', ?, ?)",
        [
            input.budget_version_id.into(),
            input.cost_center_id.into(),
            input.period_month.into(),
            budget_bucket.into(),
            input.variance_amount.into(),
            input.variance_pct.into(),
            driver_code.into(),
            input.action_owner_id.into(),
            review_commentary.into(),
            snapshot_context_json.into(),
        ],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted variance review id missing.")))?;
    let review_id: i64 = row.try_get("", "id")?;
    list_budget_variance_reviews(
        db,
        BudgetVarianceReviewFilter {
            budget_version_id: Some(input.budget_version_id),
            ..BudgetVarianceReviewFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|review| review.id == review_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetVarianceReview".to_string(),
        id: review_id.to_string(),
    })
}

pub async fn transition_budget_variance_review(
    db: &DatabaseConnection,
    input: TransitionBudgetVarianceReviewInput,
) -> AppResult<BudgetVarianceReview> {
    let next_status = required_trimmed("Next status", &input.next_status)?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, budget_version_id, row_version, review_status, action_owner_id, review_commentary
             FROM budget_variance_reviews
             WHERE id = ?",
            [input.review_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetVarianceReview".to_string(),
            id: input.review_id.to_string(),
        })?;

    let budget_version_id: i64 = row.try_get("", "budget_version_id")?;
    let row_version: i64 = row.try_get("", "row_version")?;
    let current_status: String = row.try_get("", "review_status")?;
    let action_owner_id: i64 = row.try_get("", "action_owner_id")?;
    let current_commentary: String = row.try_get("", "review_commentary")?;

    if row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Variance review was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    if !valid_variance_transition(&current_status, &next_status) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Invalid variance review lifecycle transition: {} -> {}.",
            current_status, next_status
        )]));
    }

    let commentary = optional_trimmed(&input.review_commentary).unwrap_or(current_commentary);
    if matches!(next_status.as_str(), "accepted" | "closed") && (action_owner_id <= 0 || commentary.trim().is_empty()) {
        return Err(AppError::ValidationFailed(vec![
            "Accepted/closed variance reviews require accountable owner and disposition commentary.".to_string(),
        ]));
    }
    let reopen_reason = optional_trimmed(&input.reopen_reason);
    if next_status == "open" && current_status != "open" && reopen_reason.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Reopening a variance review requires reopen_reason.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE budget_variance_reviews
         SET review_status = ?,
             review_commentary = ?,
             reviewed_at = CASE WHEN ? IN ('in_review', 'actioned', 'accepted', 'closed') THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE reviewed_at END,
             closed_at = CASE WHEN ? = 'closed' THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE NULL END,
             reopen_reason = CASE WHEN ? = 'open' THEN ? ELSE reopen_reason END,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        [
            next_status.into(),
            commentary.into(),
            input.next_status.clone().into(),
            input.next_status.clone().into(),
            input.next_status.into(),
            reopen_reason.into(),
            input.review_id.into(),
            input.expected_row_version.into(),
        ],
    ))
    .await?;

    list_budget_variance_reviews(
        db,
        BudgetVarianceReviewFilter {
            budget_version_id: Some(budget_version_id),
            ..BudgetVarianceReviewFilter::default()
        },
    )
    .await?
    .into_iter()
    .find(|review| review.id == input.review_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetVarianceReview".to_string(),
        id: input.review_id.to_string(),
    })
}

pub async fn list_budget_dashboard_rows(
    db: &DatabaseConnection,
    filter: BudgetDashboardFilter,
) -> AppResult<Vec<BudgetDashboardRow>> {
    let mut sql = String::from(
        "WITH latest_forecast_run AS (
            SELECT budget_version_id, MAX(id) AS run_id
            FROM budget_forecast_runs
            GROUP BY budget_version_id
        ),
        unioned AS (
            SELECT
                bl.budget_version_id,
                bl.cost_center_id,
                bl.period_month,
                bl.budget_bucket,
                COALESCE(bl.team_id, NULL) AS team_id,
                NULL AS assignee_id,
                bl.labor_lane AS labor_lane,
                COALESCE(bl.work_category, bl.source_basis, bl.budget_bucket) AS cause_hint,
                bl.planned_amount AS planned_amount,
                0.0 AS committed_amount,
                0.0 AS actual_amount,
                0.0 AS forecast_amount,
                'budget_line:' || bl.id AS source_link
            FROM budget_lines bl
            UNION ALL
            SELECT
                bc.budget_version_id,
                bc.cost_center_id,
                bc.period_month,
                bc.budget_bucket,
                NULL AS team_id,
                NULL AS assignee_id,
                NULL AS labor_lane,
                bc.source_type AS cause_hint,
                0.0 AS planned_amount,
                bc.base_amount AS committed_amount,
                0.0 AS actual_amount,
                0.0 AS forecast_amount,
                bc.source_type || ':' || bc.source_id AS source_link
            FROM budget_commitments bc
            UNION ALL
            SELECT
                ba.budget_version_id,
                ba.cost_center_id,
                ba.period_month,
                ba.budget_bucket,
                ba.team_id AS team_id,
                ba.personnel_id AS assignee_id,
                ba.rate_card_lane AS labor_lane,
                ba.source_type AS cause_hint,
                0.0 AS planned_amount,
                0.0 AS committed_amount,
                ba.amount_base AS actual_amount,
                0.0 AS forecast_amount,
                ba.source_type || ':' || ba.source_id AS source_link
            FROM budget_actuals ba
            WHERE ba.posting_status IN ('posted', 'reversed')
            UNION ALL
            SELECT
                bf.budget_version_id,
                bf.cost_center_id,
                bf.period_month,
                bf.budget_bucket,
                NULL AS team_id,
                NULL AS assignee_id,
                NULL AS labor_lane,
                COALESCE(bf.driver_type, bf.forecast_method) AS cause_hint,
                0.0 AS planned_amount,
                0.0 AS committed_amount,
                0.0 AS actual_amount,
                bf.forecast_amount AS forecast_amount,
                COALESCE(bf.driver_type, 'forecast') || ':' || COALESCE(bf.driver_reference, CAST(bf.id AS TEXT)) AS source_link
            FROM budget_forecasts bf
            JOIN latest_forecast_run lfr
              ON lfr.budget_version_id = bf.budget_version_id
             AND lfr.run_id = bf.forecast_run_id
        )
        SELECT
            u.budget_version_id,
            u.cost_center_id,
            cc.code AS cost_center_code,
            cc.name AS cost_center_name,
            u.period_month,
            u.budget_bucket,
            CASE
                WHEN LOWER(u.cause_hint) LIKE '%preventive%' THEN 'preventive'
                WHEN LOWER(u.cause_hint) LIKE '%inspection%' THEN 'inspection'
                WHEN LOWER(u.cause_hint) LIKE '%compliance%' THEN 'compliance'
                WHEN LOWER(u.cause_hint) LIKE '%shutdown%' THEN 'shutdown'
                WHEN LOWER(u.cause_hint) LIKE '%improvement%' THEN 'improvement'
                WHEN LOWER(u.cause_hint) LIKE '%capex%' THEN 'capex'
                ELSE 'corrective'
            END AS spend_mix,
            u.team_id,
            u.assignee_id,
            u.labor_lane,
            SUM(u.planned_amount) AS planned_amount,
            SUM(u.committed_amount) AS committed_amount,
            SUM(u.actual_amount) AS actual_amount,
            SUM(u.forecast_amount) AS forecast_amount,
            SUM(u.actual_amount) - SUM(u.planned_amount) AS variance_to_plan,
            SUM(u.actual_amount) - SUM(u.forecast_amount) AS variance_to_forecast,
            bv.currency_code AS currency_code,
            COALESCE(
                '[' || GROUP_CONCAT(DISTINCT '\"' || REPLACE(u.source_link, '\"', '\\\"') || '\"') || ']',
                '[]'
            ) AS source_links_json
        FROM unioned u
        JOIN cost_centers cc ON cc.id = u.cost_center_id
        JOIN budget_versions bv ON bv.id = u.budget_version_id
        WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();

    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND u.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND u.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    if let Some(period_month) = filter.period_month {
        sql.push_str(" AND u.period_month = ?");
        values.push(period_month.into());
    }
    if let Some(budget_bucket) = optional_trimmed(&filter.budget_bucket) {
        sql.push_str(" AND u.budget_bucket = ?");
        values.push(budget_bucket.into());
    }
    if let Some(team_id) = filter.team_id {
        sql.push_str(" AND u.team_id = ?");
        values.push(team_id.into());
    }
    if let Some(assignee_id) = filter.assignee_id {
        sql.push_str(" AND u.assignee_id = ?");
        values.push(assignee_id.into());
    }
    if let Some(labor_lane) = optional_trimmed(&filter.labor_lane) {
        sql.push_str(" AND u.labor_lane = ?");
        values.push(labor_lane.into());
    }
    if let Some(spend_mix) = optional_trimmed(&filter.spend_mix) {
        sql.push_str(
            " AND CASE
                WHEN LOWER(u.cause_hint) LIKE '%preventive%' THEN 'preventive'
                WHEN LOWER(u.cause_hint) LIKE '%inspection%' THEN 'inspection'
                WHEN LOWER(u.cause_hint) LIKE '%compliance%' THEN 'compliance'
                WHEN LOWER(u.cause_hint) LIKE '%shutdown%' THEN 'shutdown'
                WHEN LOWER(u.cause_hint) LIKE '%improvement%' THEN 'improvement'
                WHEN LOWER(u.cause_hint) LIKE '%capex%' THEN 'capex'
                ELSE 'corrective'
            END = ?",
        );
        values.push(spend_mix.into());
    }

    sql.push_str(
        " GROUP BY
            u.budget_version_id,
            u.cost_center_id,
            cc.code,
            cc.name,
            u.period_month,
            u.budget_bucket,
            spend_mix,
            u.team_id,
            u.assignee_id,
            u.labor_lane,
            bv.currency_code
          ORDER BY COALESCE(u.period_month, 0) ASC, cc.code ASC, u.budget_bucket ASC",
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_dashboard_row).collect()
}

pub async fn list_budget_dashboard_drilldown(
    db: &DatabaseConnection,
    filter: BudgetDashboardFilter,
) -> AppResult<Vec<BudgetDrilldownRow>> {
    let mut sql = String::from(
        "WITH latest_forecast_run AS (
            SELECT budget_version_id, MAX(id) AS run_id
            FROM budget_forecast_runs
            GROUP BY budget_version_id
        )
        SELECT * FROM (
            SELECT
                'planned' AS layer_type,
                bl.id AS record_id,
                bl.budget_version_id,
                bl.cost_center_id,
                cc.code AS cost_center_code,
                bl.period_month,
                bl.budget_bucket,
                bl.planned_amount AS amount,
                bv.currency_code AS currency_code,
                NULL AS source_type,
                NULL AS source_id,
                NULL AS work_order_id,
                NULL AS pm_occurrence_ref,
                NULL AS inspection_ref,
                bl.shutdown_package_ref AS shutdown_package_ref,
                bl.team_id AS team_id,
                NULL AS assignee_id,
                bl.labor_lane AS labor_lane,
                NULL AS hours_overrun_rate,
                NULL AS first_pass_effect,
                NULL AS repeat_work_penalty,
                NULL AS schedule_discipline_impact
            FROM budget_lines bl
            JOIN cost_centers cc ON cc.id = bl.cost_center_id
            JOIN budget_versions bv ON bv.id = bl.budget_version_id
            UNION ALL
            SELECT
                'committed' AS layer_type,
                bc.id AS record_id,
                bc.budget_version_id,
                bc.cost_center_id,
                cc.code AS cost_center_code,
                bc.period_month,
                bc.budget_bucket,
                bc.base_amount AS amount,
                bc.base_currency AS currency_code,
                bc.source_type AS source_type,
                bc.source_id AS source_id,
                bc.work_order_id AS work_order_id,
                CASE WHEN LOWER(bc.source_type) LIKE '%pm%' THEN bc.source_id ELSE NULL END AS pm_occurrence_ref,
                CASE WHEN LOWER(bc.source_type) LIKE '%inspection%' THEN bc.source_id ELSE NULL END AS inspection_ref,
                CASE WHEN LOWER(bc.source_type) LIKE '%shutdown%' THEN bc.source_id ELSE NULL END AS shutdown_package_ref,
                NULL AS team_id,
                NULL AS assignee_id,
                NULL AS labor_lane,
                NULL AS hours_overrun_rate,
                NULL AS first_pass_effect,
                NULL AS repeat_work_penalty,
                NULL AS schedule_discipline_impact
            FROM budget_commitments bc
            JOIN cost_centers cc ON cc.id = bc.cost_center_id
            UNION ALL
            SELECT
                'actual' AS layer_type,
                ba.id AS record_id,
                ba.budget_version_id,
                ba.cost_center_id,
                cc.code AS cost_center_code,
                ba.period_month,
                ba.budget_bucket,
                ba.amount_base AS amount,
                ba.base_currency AS currency_code,
                ba.source_type AS source_type,
                ba.source_id AS source_id,
                ba.work_order_id AS work_order_id,
                CASE WHEN LOWER(ba.source_type) LIKE '%pm%' THEN ba.source_id ELSE NULL END AS pm_occurrence_ref,
                CASE WHEN LOWER(ba.source_type) LIKE '%inspection%' THEN ba.source_id ELSE NULL END AS inspection_ref,
                CASE WHEN LOWER(ba.source_type) LIKE '%shutdown%' THEN ba.source_id ELSE NULL END AS shutdown_package_ref,
                ba.team_id AS team_id,
                ba.personnel_id AS assignee_id,
                ba.rate_card_lane AS labor_lane,
                CASE WHEN ba.budget_bucket = 'labor' THEN ABS(ba.amount_base) / 100.0 ELSE NULL END AS hours_overrun_rate,
                CASE WHEN LOWER(ba.source_type) LIKE '%repeat%' THEN -0.2 ELSE 0.1 END AS first_pass_effect,
                CASE WHEN LOWER(ba.source_type) LIKE '%repeat%' THEN ABS(ba.amount_base) * 0.1 ELSE 0.0 END AS repeat_work_penalty,
                CASE WHEN LOWER(ba.source_type) LIKE '%delay%' THEN -0.3 ELSE 0.0 END AS schedule_discipline_impact
            FROM budget_actuals ba
            JOIN cost_centers cc ON cc.id = ba.cost_center_id
            WHERE ba.posting_status IN ('posted', 'reversed')
            UNION ALL
            SELECT
                'forecast' AS layer_type,
                bf.id AS record_id,
                bf.budget_version_id,
                bf.cost_center_id,
                cc.code AS cost_center_code,
                bf.period_month,
                bf.budget_bucket,
                bf.forecast_amount AS amount,
                bv.currency_code AS currency_code,
                bf.forecast_method AS source_type,
                bf.driver_reference AS source_id,
                NULL AS work_order_id,
                CASE WHEN LOWER(COALESCE(bf.driver_type, '')) LIKE '%pm%' THEN bf.driver_reference ELSE NULL END AS pm_occurrence_ref,
                CASE WHEN LOWER(COALESCE(bf.driver_type, '')) LIKE '%inspection%' THEN bf.driver_reference ELSE NULL END AS inspection_ref,
                CASE WHEN LOWER(COALESCE(bf.driver_type, '')) LIKE '%shutdown%' THEN bf.driver_reference ELSE NULL END AS shutdown_package_ref,
                NULL AS team_id,
                NULL AS assignee_id,
                NULL AS labor_lane,
                NULL AS hours_overrun_rate,
                NULL AS first_pass_effect,
                NULL AS repeat_work_penalty,
                NULL AS schedule_discipline_impact
            FROM budget_forecasts bf
            JOIN latest_forecast_run lfr
              ON lfr.budget_version_id = bf.budget_version_id
             AND lfr.run_id = bf.forecast_run_id
            JOIN cost_centers cc ON cc.id = bf.cost_center_id
            JOIN budget_versions bv ON bv.id = bf.budget_version_id
        ) drill
        WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND drill.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND drill.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    if let Some(period_month) = filter.period_month {
        sql.push_str(" AND drill.period_month = ?");
        values.push(period_month.into());
    }
    if let Some(budget_bucket) = optional_trimmed(&filter.budget_bucket) {
        sql.push_str(" AND drill.budget_bucket = ?");
        values.push(budget_bucket.into());
    }
    if let Some(team_id) = filter.team_id {
        sql.push_str(" AND drill.team_id = ?");
        values.push(team_id.into());
    }
    if let Some(assignee_id) = filter.assignee_id {
        sql.push_str(" AND drill.assignee_id = ?");
        values.push(assignee_id.into());
    }
    if let Some(labor_lane) = optional_trimmed(&filter.labor_lane) {
        sql.push_str(" AND drill.labor_lane = ?");
        values.push(labor_lane.into());
    }
    sql.push_str(" ORDER BY COALESCE(drill.period_month, 0) ASC, drill.cost_center_code ASC, drill.layer_type ASC");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_drilldown_row).collect()
}

pub async fn import_erp_cost_center_master(
    db: &DatabaseConnection,
    input: ImportErpCostCenterMasterInput,
) -> AppResult<ErpMasterImportResult> {
    let import_batch_id = required_trimmed("Import batch id", &input.import_batch_id)?;
    if input.records.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "At least one ERP master record is required.".to_string(),
        ]));
    }

    let mut imported_count = 0_i64;
    let mut linked_count = 0_i64;
    let mut inactive_count = 0_i64;
    let mut seen_codes = std::collections::HashSet::new();

    for ErpCostCenterMasterRecordInput {
        external_code,
        external_name,
        local_cost_center_code,
        is_active,
    } in input.records
    {
        let external_code = required_trimmed("ERP external_code", &external_code)?;
        let external_name = required_trimmed("ERP external_name", &external_name)?;
        if !seen_codes.insert(external_code.clone()) {
            return Err(AppError::ValidationFailed(vec![
                "Duplicate ERP external_code provided in import payload.".to_string(),
            ]));
        }

        let local_cost_center_id = if let Some(local_code) = optional_trimmed(&local_cost_center_code) {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM cost_centers WHERE code = ? LIMIT 1",
                    [local_code.into()],
                ))
                .await?;
            row.map(|record| record.try_get::<i64>("", "id"))
                .transpose()?
        } else {
            None
        };
        if local_cost_center_id.is_some() {
            linked_count += 1;
        }
        let active = is_active.unwrap_or(true);
        if !active {
            inactive_count += 1;
        }

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO budget_erp_cost_center_master (
                import_batch_id,
                external_code,
                external_name,
                local_cost_center_id,
                is_active
             ) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(external_code) DO UPDATE SET
                import_batch_id = excluded.import_batch_id,
                external_name = excluded.external_name,
                local_cost_center_id = excluded.local_cost_center_id,
                is_active = excluded.is_active,
                row_version = budget_erp_cost_center_master.row_version + 1,
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            [
                import_batch_id.clone().into(),
                external_code.into(),
                external_name.into(),
                local_cost_center_id.into(),
                i64::from(active).into(),
            ],
        ))
        .await?;
        imported_count += 1;
    }

    Ok(ErpMasterImportResult {
        imported_count,
        linked_count,
        inactive_count,
    })
}

pub async fn export_posted_actuals_for_erp(db: &DatabaseConnection) -> AppResult<Vec<ErpPostedActualExportItem>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                ba.id AS actual_id,
                ba.budget_version_id,
                bv.fiscal_year,
                bv.scenario_type,
                COALESCE(erp.external_code, cc.erp_external_id) AS external_cost_center_code,
                cc.code AS local_cost_center_code,
                ba.budget_bucket,
                ba.amount_source,
                ba.source_currency,
                ba.amount_base,
                ba.base_currency,
                ba.source_type,
                ba.source_id,
                ba.posted_at,
                TRIM(
                    (CASE WHEN COALESCE(erp.external_code, cc.erp_external_id) IS NULL THEN 'unknown_external_code,' ELSE '' END) ||
                    (CASE WHEN erp.id IS NOT NULL AND erp.is_active = 0 THEN 'inactive_imported_cost_center,' ELSE '' END) ||
                    (CASE WHEN ba.source_currency <> ba.base_currency THEN 'base_currency_drift,' ELSE '' END) ||
                    (CASE WHEN ba.posted_at IS NULL THEN 'posting_payload_rejection,' ELSE '' END),
                    ','
                ) AS reconciliation_flags
             FROM budget_actuals ba
             JOIN cost_centers cc ON cc.id = ba.cost_center_id
             JOIN budget_versions bv ON bv.id = ba.budget_version_id
             LEFT JOIN budget_erp_cost_center_master erp ON erp.local_cost_center_id = cc.id
             WHERE ba.posting_status = 'posted'
             ORDER BY ba.posted_at ASC, ba.id ASC",
            [],
        ))
        .await?;

    rows.iter()
        .map(|row| {
            let flags: String = row.try_get("", "reconciliation_flags")?;
            Ok(ErpPostedActualExportItem {
                actual_id: row.try_get("", "actual_id")?,
                budget_version_id: row.try_get("", "budget_version_id")?,
                fiscal_year: row.try_get("", "fiscal_year")?,
                scenario_type: row.try_get("", "scenario_type")?,
                external_cost_center_code: row.try_get("", "external_cost_center_code")?,
                local_cost_center_code: row.try_get("", "local_cost_center_code")?,
                budget_bucket: row.try_get("", "budget_bucket")?,
                amount_source: row.try_get("", "amount_source")?,
                source_currency: row.try_get("", "source_currency")?,
                amount_base: row.try_get("", "amount_base")?,
                base_currency: row.try_get("", "base_currency")?,
                source_type: row.try_get("", "source_type")?,
                source_id: row.try_get("", "source_id")?,
                posted_at: row.try_get("", "posted_at")?,
                reconciliation_flags: parse_csv_flags(flags),
            })
        })
        .collect()
}

pub async fn export_approved_reforecasts_for_erp(
    db: &DatabaseConnection,
) -> AppResult<Vec<ErpApprovedReforecastExportItem>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH latest_forecast_run AS (
                SELECT budget_version_id, MAX(id) AS run_id
                FROM budget_forecast_runs
                GROUP BY budget_version_id
             )
             SELECT
                bf.id AS forecast_id,
                bf.forecast_run_id,
                bf.budget_version_id,
                bv.fiscal_year,
                bv.scenario_type,
                bv.status AS version_status,
                COALESCE(erp.external_code, cc.erp_external_id) AS external_cost_center_code,
                cc.code AS local_cost_center_code,
                bf.period_month,
                bf.budget_bucket,
                bf.forecast_amount,
                bv.currency_code AS base_currency,
                bf.forecast_method,
                bf.confidence_level,
                TRIM(
                    (CASE WHEN COALESCE(erp.external_code, cc.erp_external_id) IS NULL THEN 'unknown_external_code,' ELSE '' END) ||
                    (CASE WHEN erp.id IS NOT NULL AND erp.is_active = 0 THEN 'inactive_imported_cost_center,' ELSE '' END),
                    ','
                ) AS reconciliation_flags
             FROM budget_forecasts bf
             JOIN latest_forecast_run lfr
               ON lfr.budget_version_id = bf.budget_version_id
              AND lfr.run_id = bf.forecast_run_id
             JOIN budget_versions bv ON bv.id = bf.budget_version_id
             JOIN cost_centers cc ON cc.id = bf.cost_center_id
             LEFT JOIN budget_erp_cost_center_master erp ON erp.local_cost_center_id = cc.id
             WHERE bv.scenario_type = 'reforecast'
               AND bv.status = 'approved'
             ORDER BY bv.fiscal_year ASC, bf.period_month ASC, bf.id ASC",
            [],
        ))
        .await?;

    rows.iter()
        .map(|row| {
            let flags: String = row.try_get("", "reconciliation_flags")?;
            Ok(ErpApprovedReforecastExportItem {
                forecast_id: row.try_get("", "forecast_id")?,
                forecast_run_id: row.try_get("", "forecast_run_id")?,
                budget_version_id: row.try_get("", "budget_version_id")?,
                fiscal_year: row.try_get("", "fiscal_year")?,
                scenario_type: row.try_get("", "scenario_type")?,
                version_status: row.try_get("", "version_status")?,
                external_cost_center_code: row.try_get("", "external_cost_center_code")?,
                local_cost_center_code: row.try_get("", "local_cost_center_code")?,
                period_month: row.try_get("", "period_month")?,
                budget_bucket: row.try_get("", "budget_bucket")?,
                forecast_amount: row.try_get("", "forecast_amount")?,
                base_currency: row.try_get("", "base_currency")?,
                forecast_method: row.try_get("", "forecast_method")?,
                confidence_level: row.try_get("", "confidence_level")?,
                reconciliation_flags: parse_csv_flags(flags),
            })
        })
        .collect()
}

pub async fn list_budget_alert_configs(
    db: &DatabaseConnection,
    filter: BudgetAlertConfigFilter,
) -> AppResult<Vec<BudgetAlertConfig>> {
    let mut sql = String::from("SELECT * FROM budget_alert_configs WHERE 1 = 1");
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND (budget_version_id = ? OR budget_version_id IS NULL)");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND (cost_center_id = ? OR cost_center_id IS NULL)");
        values.push(cost_center_id.into());
    }
    if let Some(alert_type) = optional_trimmed(&filter.alert_type) {
        sql.push_str(" AND alert_type = ?");
        values.push(alert_type.into());
    }
    if filter.active_only.unwrap_or(false) {
        sql.push_str(" AND is_active = 1");
    }
    sql.push_str(" ORDER BY alert_type ASC, id ASC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_alert_config).collect()
}

pub async fn create_budget_alert_config(
    db: &DatabaseConnection,
    input: CreateBudgetAlertConfigInput,
) -> AppResult<BudgetAlertConfig> {
    let alert_type = required_trimmed("Alert type", &input.alert_type)?;
    if !valid_alert_type(&alert_type) {
        return Err(AppError::ValidationFailed(vec![
            "alert_type is not supported by budget controls.".to_string(),
        ]));
    }
    if let Some(version_id) = input.budget_version_id {
        let version = get_budget_version(db, version_id).await?;
        if version.status != "frozen" {
            return Err(AppError::ValidationFailed(vec![
                "Only frozen control baselines may be bound to production alert configs.".to_string(),
            ]));
        }
    }
    if input.dedupe_window_minutes.unwrap_or(240) <= 0 {
        return Err(AppError::ValidationFailed(vec![
            "dedupe_window_minutes must be > 0.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO budget_alert_configs (
            budget_version_id, cost_center_id, budget_bucket, alert_type, threshold_pct, threshold_amount,
            recipient_user_id, recipient_role_id, labor_template, dedupe_window_minutes, requires_ack, is_active
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.budget_version_id.into(),
            input.cost_center_id.into(),
            optional_trimmed(&input.budget_bucket).into(),
            alert_type.into(),
            input.threshold_pct.into(),
            input.threshold_amount.into(),
            input.recipient_user_id.into(),
            input.recipient_role_id.into(),
            optional_trimmed(&input.labor_template).into(),
            input.dedupe_window_minutes.unwrap_or(240).into(),
            i64::from(input.requires_ack.unwrap_or(true)).into(),
            i64::from(input.is_active.unwrap_or(true)).into(),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Inserted alert config id missing.")))?;
    let config_id: i64 = id_row.try_get("", "id")?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM budget_alert_configs WHERE id = ?",
            [config_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetAlertConfig".to_string(),
            id: config_id.to_string(),
        })?;
    map_budget_alert_config(&row)
}

pub async fn update_budget_alert_config(
    db: &DatabaseConnection,
    config_id: i64,
    expected_row_version: i64,
    input: UpdateBudgetAlertConfigInput,
) -> AppResult<BudgetAlertConfig> {
    let current = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version, budget_bucket, threshold_pct, threshold_amount, recipient_user_id, recipient_role_id,
                    labor_template, dedupe_window_minutes, requires_ack, is_active
             FROM budget_alert_configs WHERE id = ?",
            [config_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetAlertConfig".to_string(),
            id: config_id.to_string(),
        })?;
    let row_version: i64 = current.try_get("", "row_version")?;
    if row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Budget alert config was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    let dedupe_window_minutes = input
        .dedupe_window_minutes
        .unwrap_or(current.try_get::<i64>("", "dedupe_window_minutes")?);
    if dedupe_window_minutes <= 0 {
        return Err(AppError::ValidationFailed(vec![
            "dedupe_window_minutes must be > 0.".to_string(),
        ]));
    }
    let budget_bucket = if input.budget_bucket.is_some() {
        optional_trimmed(&input.budget_bucket)
    } else {
        current.try_get::<Option<String>>("", "budget_bucket")?
    };
    let threshold_pct = input
        .threshold_pct
        .or(current.try_get::<Option<f64>>("", "threshold_pct")?);
    let threshold_amount = input
        .threshold_amount
        .or(current.try_get::<Option<f64>>("", "threshold_amount")?);
    let recipient_user_id = input
        .recipient_user_id
        .or(current.try_get::<Option<i64>>("", "recipient_user_id")?);
    let recipient_role_id = input
        .recipient_role_id
        .or(current.try_get::<Option<i64>>("", "recipient_role_id")?);
    let labor_template = if input.labor_template.is_some() {
        optional_trimmed(&input.labor_template)
    } else {
        current.try_get::<Option<String>>("", "labor_template")?
    };
    let requires_ack = input
        .requires_ack
        .unwrap_or(current.try_get::<i64>("", "requires_ack")? == 1);
    let is_active = input
        .is_active
        .unwrap_or(current.try_get::<i64>("", "is_active")? == 1);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE budget_alert_configs
         SET budget_bucket = ?,
             threshold_pct = ?,
             threshold_amount = ?,
             recipient_user_id = ?,
             recipient_role_id = ?,
             labor_template = ?,
             dedupe_window_minutes = ?,
             requires_ack = ?,
             is_active = ?,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        [
            budget_bucket.into(),
            threshold_pct.into(),
            threshold_amount.into(),
            recipient_user_id.into(),
            recipient_role_id.into(),
            labor_template.into(),
            dedupe_window_minutes.into(),
            i64::from(requires_ack).into(),
            i64::from(is_active).into(),
            config_id.into(),
            expected_row_version.into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM budget_alert_configs WHERE id = ?",
            [config_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetAlertConfig".to_string(),
            id: config_id.to_string(),
        })?;
    map_budget_alert_config(&row)
}

pub async fn list_budget_alert_events(
    db: &DatabaseConnection,
    filter: BudgetAlertEventFilter,
) -> AppResult<Vec<BudgetAlertEvent>> {
    let mut sql = String::from(
        "SELECT ae.*, cc.code AS cost_center_code, cc.name AS cost_center_name
         FROM budget_alert_events ae
         JOIN cost_centers cc ON cc.id = ae.cost_center_id
         WHERE 1 = 1",
    );
    let mut values: Vec<Value> = Vec::new();
    if let Some(version_id) = filter.budget_version_id {
        sql.push_str(" AND ae.budget_version_id = ?");
        values.push(version_id.into());
    }
    if let Some(cost_center_id) = filter.cost_center_id {
        sql.push_str(" AND ae.cost_center_id = ?");
        values.push(cost_center_id.into());
    }
    if let Some(alert_type) = optional_trimmed(&filter.alert_type) {
        sql.push_str(" AND ae.alert_type = ?");
        values.push(alert_type.into());
    }
    if filter.acknowledged_only.unwrap_or(false) {
        sql.push_str(" AND ae.acknowledged_at IS NOT NULL");
    }
    sql.push_str(" ORDER BY ae.created_at DESC, ae.id DESC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_budget_alert_event).collect()
}

pub async fn evaluate_budget_alerts(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: EvaluateBudgetAlertsInput,
) -> AppResult<BudgetAlertEvaluationResult> {
    let version = get_budget_version(db, input.budget_version_id).await?;
    if version.status != "frozen" {
        return Err(AppError::ValidationFailed(vec![
            "Only frozen control baselines may drive production alerts.".to_string(),
        ]));
    }

    let rows = list_budget_dashboard_rows(
        db,
        BudgetDashboardFilter {
            budget_version_id: Some(input.budget_version_id),
            ..BudgetDashboardFilter::default()
        },
    )
    .await?;

    let mut configs = list_budget_alert_configs(
        db,
        BudgetAlertConfigFilter {
            budget_version_id: Some(input.budget_version_id),
            active_only: Some(true),
            ..BudgetAlertConfigFilter::default()
        },
    )
    .await?;
    if configs.is_empty() {
        for alert_type in [
            "threshold_80",
            "threshold_100",
            "threshold_120",
            "forecast_overrun",
            "labor_hour_overrun",
            "overtime_spike",
            "contractor_cost_drift",
            "emergency_spend_concentration",
            "assignment_risk",
        ] {
            configs.push(BudgetAlertConfig {
                id: 0,
                budget_version_id: Some(input.budget_version_id),
                cost_center_id: None,
                budget_bucket: None,
                alert_type: alert_type.to_string(),
                threshold_pct: alert_default_threshold_pct(alert_type),
                threshold_amount: None,
                recipient_user_id: Some(actor_user_id),
                recipient_role_id: None,
                labor_template: if alert_type.contains("labor") || alert_type == "overtime_spike" {
                    Some(alert_type.to_string())
                } else {
                    None
                },
                dedupe_window_minutes: 240,
                requires_ack: true,
                is_active: true,
                row_version: 1,
                created_at: current_utc_text(),
                updated_at: current_utc_text(),
            });
        }
    }

    let team_commit_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT assigned_team_id AS team_id,
                    CAST(strftime('%m', committed_start) AS INTEGER) AS period_month,
                    SUM((julianday(committed_end) - julianday(committed_start)) * 24.0) AS committed_hours
             FROM schedule_commitments
             WHERE assigned_team_id IS NOT NULL
             GROUP BY assigned_team_id, CAST(strftime('%m', committed_start) AS INTEGER)",
            [],
        ))
        .await?;
    let mut team_commit_map = std::collections::HashMap::<(i64, i64), f64>::new();
    for row in team_commit_rows {
        let team_id: i64 = row.try_get("", "team_id")?;
        let month: i64 = row.try_get("", "period_month")?;
        let committed_hours: f64 = row.try_get("", "committed_hours")?;
        team_commit_map.insert((team_id, month), committed_hours);
    }
    let team_capacity_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT team_id,
                    CAST(strftime('%m', effective_start) AS INTEGER) AS period_month,
                    AVG(available_hours_per_day + max_overtime_hours_per_day) * 30.0 AS capacity_hours
             FROM capacity_rules
             GROUP BY team_id, CAST(strftime('%m', effective_start) AS INTEGER)",
            [],
        ))
        .await?;
    let mut team_capacity_map = std::collections::HashMap::<(i64, i64), f64>::new();
    for row in team_capacity_rows {
        let team_id: i64 = row.try_get("", "team_id")?;
        let month: i64 = row.try_get("", "period_month")?;
        let capacity_hours: f64 = row.try_get("", "capacity_hours")?;
        team_capacity_map.insert((team_id, month), capacity_hours);
    }

    let mut emitted_count = 0_i64;
    let mut deduped_count = 0_i64;

    for row in &rows {
        let baseline = row.planned_amount.max(0.0);
        let exposure = row.committed_amount + row.actual_amount;
        let variance = row.actual_amount - row.planned_amount;
        for config in configs.iter().filter(|config| {
            (config.cost_center_id.is_none() || config.cost_center_id == Some(row.cost_center_id))
                && (config.budget_bucket.is_none() || config.budget_bucket.as_deref() == Some(&row.budget_bucket))
                && config.is_active
        }) {
            let mut should_fire = false;
            let mut threshold_value = config.threshold_amount;
            let mut title = String::new();
            let mut message = String::new();
            let mut payload = serde_json::json!({
                "baseline": baseline,
                "committed": row.committed_amount,
                "actual": row.actual_amount,
                "forecast": row.forecast_amount,
                "spend_mix": row.spend_mix,
                "team_id": row.team_id,
                "assignee_id": row.assignee_id,
                "labor_lane": row.labor_lane,
            });
            match config.alert_type.as_str() {
                "threshold_80" | "threshold_100" | "threshold_120" => {
                    let pct = config
                        .threshold_pct
                        .or_else(|| alert_default_threshold_pct(&config.alert_type))
                        .unwrap_or(100.0);
                    threshold_value = Some((baseline * pct) / 100.0);
                    should_fire = baseline > 0.0 && exposure >= threshold_value.unwrap_or(0.0);
                    title = format!("Budget threshold {}% reached", pct.round());
                    message = format!(
                        "Exposure {:.2} reached {:.2}% of frozen baseline {:.2}.",
                        exposure, pct, baseline
                    );
                }
                "forecast_overrun" => {
                    threshold_value = Some(baseline);
                    should_fire = baseline > 0.0 && row.forecast_amount > baseline;
                    title = "Forecast overrun risk".to_string();
                    message = format!(
                        "Forecast {:.2} exceeds frozen baseline {:.2} before period close.",
                        row.forecast_amount, baseline
                    );
                }
                "labor_hour_overrun" => {
                    threshold_value = Some(baseline * 1.1);
                    should_fire = row.budget_bucket == "labor" && row.actual_amount > baseline * 1.1;
                    title = "Labor hour overrun".to_string();
                    message = "Labor spend converted hours exceed expected lane envelope.".to_string();
                }
                "overtime_spike" => {
                    threshold_value = Some((baseline * 1.15).max(1.0));
                    should_fire = row.labor_lane.as_deref() == Some("overtime")
                        && row.actual_amount > threshold_value.unwrap_or(0.0);
                    title = "Overtime spike".to_string();
                    message =
                        "Overtime lane exceeded configured threshold and needs assignment balancing.".to_string();
                }
                "contractor_cost_drift" => {
                    threshold_value = Some((baseline * 1.1).max(1.0));
                    should_fire = row.labor_lane.as_deref() == Some("contractor")
                        && row.actual_amount > threshold_value.unwrap_or(0.0);
                    title = "Contractor cost drift".to_string();
                    message = "Contractor lane drift is above tolerance versus frozen baseline.".to_string();
                }
                "emergency_spend_concentration" => {
                    threshold_value = Some(config.threshold_amount.unwrap_or(0.0).max(1.0));
                    should_fire = row.spend_mix == "corrective" && row.variance_to_plan > threshold_value.unwrap_or(1.0);
                    title = "Emergency spend concentration".to_string();
                    message = "Corrective concentration indicates emergency break-in bias in period spend.".to_string();
                }
                "assignment_risk" => {
                    if let (Some(team_id), Some(period_month)) = (row.team_id, row.period_month) {
                        let committed_hours = team_commit_map.get(&(team_id, period_month)).copied().unwrap_or(0.0);
                        let capacity_hours = team_capacity_map.get(&(team_id, period_month)).copied().unwrap_or(0.0);
                        threshold_value = Some(capacity_hours);
                        should_fire = capacity_hours > 0.0 && committed_hours > capacity_hours * 1.15;
                        payload["planning_evidence"] = serde_json::json!({
                            "committed_hours": committed_hours,
                            "capacity_hours": capacity_hours
                        });
                        title = "Assignment risk from planning evidence".to_string();
                        message = format!(
                            "Committed hours {:.1} exceed available capacity {:.1} for team {} in period {}.",
                            committed_hours, capacity_hours, team_id, period_month
                        );
                    }
                }
                _ => {}
            }
            if !should_fire {
                continue;
            }

            let period_key = row.period_month.unwrap_or(0);
            let dedupe_key = format!(
                "budget:{}:{}:{}:{}:{}",
                row.budget_version_id, row.cost_center_id, period_key, row.budget_bucket, config.alert_type
            );
            let duplicate = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM budget_alert_events
                     WHERE dedupe_key = ?
                       AND julianday(created_at) >= julianday('now', '-' || ? || ' minutes')
                     LIMIT 1",
                    [dedupe_key.clone().into(), config.dedupe_window_minutes.into()],
                ))
                .await?;
            if duplicate.is_some() {
                deduped_count += 1;
                continue;
            }

            let emit_notifications = input.emit_notifications.unwrap_or(true);
            let mut notification_event_id: Option<i64> = None;
            let mut notification_id: Option<i64> = None;
            if emit_notifications && config.recipient_user_id.is_some() {
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO notification_events
                        (source_module, source_record_id, event_code, category_code, severity, dedupe_key, payload_json)
                     VALUES ('finance_budget', ?, ?, 'budget_control_alert', ?, ?, ?)",
                    [
                        format!("{}:{}", row.budget_version_id, row.cost_center_id).into(),
                        config.alert_type.clone().into(),
                        if config.alert_type.contains("threshold") {
                            "warning".to_string()
                        } else {
                            "critical".to_string()
                        }
                        .into(),
                        dedupe_key.clone().into(),
                        Some(payload.to_string()).into(),
                    ],
                ))
                .await?;
                let event_id_row = db
                    .query_one(Statement::from_string(
                        DbBackend::Sqlite,
                        "SELECT last_insert_rowid() AS id".to_string(),
                    ))
                    .await?
                    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("notification event id missing")))?;
                let created_event_id: i64 = event_id_row.try_get("", "id")?;
                notification_event_id = Some(created_event_id);

                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO notifications
                        (notification_event_id, recipient_user_id, recipient_role_id, delivery_state, title, body, action_url)
                     VALUES (?, ?, ?, 'delivered', ?, ?, '/finance/budget')",
                    [
                        created_event_id.into(),
                        config.recipient_user_id.into(),
                        config.recipient_role_id.into(),
                        title.clone().into(),
                        message.clone().into(),
                    ],
                ))
                .await?;
                let notif_id_row = db
                    .query_one(Statement::from_string(
                        DbBackend::Sqlite,
                        "SELECT last_insert_rowid() AS id".to_string(),
                    ))
                    .await?
                    .ok_or_else(|| AppError::Internal(anyhow::anyhow!("notification id missing")))?;
                notification_id = Some(notif_id_row.try_get("", "id")?);
            }

            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO budget_alert_events (
                    alert_config_id, budget_version_id, cost_center_id, period_month, budget_bucket, alert_type,
                    severity, title, message, dedupe_key, current_value, threshold_value, variance_amount, currency_code,
                    payload_json, notification_event_id, notification_id
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                [
                    if config.id == 0 { None } else { Some(config.id) }.into(),
                    row.budget_version_id.into(),
                    row.cost_center_id.into(),
                    row.period_month.into(),
                    row.budget_bucket.clone().into(),
                    config.alert_type.clone().into(),
                    if config.alert_type.contains("threshold") {
                        "warning".to_string()
                    } else {
                        "critical".to_string()
                    }
                    .into(),
                    title.into(),
                    message.into(),
                    dedupe_key.into(),
                    exposure.into(),
                    threshold_value.into(),
                    variance.into(),
                    row.currency_code.clone().into(),
                    Some(payload.to_string()).into(),
                    notification_event_id.into(),
                    notification_id.into(),
                ],
            ))
            .await?;
            emitted_count += 1;
        }
    }

    let emitted_events = list_budget_alert_events(
        db,
        BudgetAlertEventFilter {
            budget_version_id: Some(input.budget_version_id),
            ..BudgetAlertEventFilter::default()
        },
    )
    .await?
    .into_iter()
    .take(200)
    .collect();

    Ok(BudgetAlertEvaluationResult {
        evaluated_at: current_utc_text(),
        emitted_count,
        deduped_count,
        considered_rows: rows.len() as i64,
        events: emitted_events,
    })
}

pub async fn acknowledge_budget_alert(
    db: &DatabaseConnection,
    actor_user_id: i64,
    input: AcknowledgeBudgetAlertInput,
) -> AppResult<BudgetAlertEvent> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, notification_id FROM budget_alert_events WHERE id = ?",
            [input.alert_event_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "BudgetAlertEvent".to_string(),
            id: input.alert_event_id.to_string(),
        })?;
    let notification_id: Option<i64> = row.try_get("", "notification_id")?;
    let note = optional_trimmed(&input.note);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE budget_alert_events
         SET acknowledged_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             acknowledged_by_id = ?,
             acknowledgement_note = ?,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [actor_user_id.into(), note.clone().into(), input.alert_event_id.into()],
    ))
    .await?;
    if let Some(notification_id) = notification_id {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notifications
             SET delivery_state = 'acknowledged',
                 acknowledged_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [notification_id.into()],
        ))
        .await?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_acknowledgements
                (notification_id, acknowledged_by_id, acknowledgement_note)
             VALUES (?, ?, ?)",
            [notification_id.into(), actor_user_id.into(), note.into()],
        ))
        .await?;
    }
    list_budget_alert_events(
        db,
        BudgetAlertEventFilter {
            budget_version_id: None,
            cost_center_id: None,
            alert_type: None,
            acknowledged_only: None,
        },
    )
    .await?
    .into_iter()
    .find(|event| event.id == input.alert_event_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "BudgetAlertEvent".to_string(),
        id: input.alert_event_id.to_string(),
    })
}

pub async fn build_budget_report_pack(
    db: &DatabaseConnection,
    filter: BudgetReportPackFilter,
) -> AppResult<BudgetReportPack> {
    let version = get_budget_version(db, filter.budget_version_id).await?;
    let dashboard_filter = BudgetDashboardFilter {
        budget_version_id: Some(filter.budget_version_id),
        cost_center_id: filter.cost_center_id,
        period_month: filter.period_month_start,
        budget_bucket: filter.budget_bucket.clone(),
        spend_mix: filter.spend_mix.clone(),
        team_id: filter.team_id,
        assignee_id: filter.assignee_id,
        labor_lane: filter.labor_lane.clone(),
    };
    let mut dashboard_rows = list_budget_dashboard_rows(db, dashboard_filter.clone()).await?;
    let mut drilldown_rows = list_budget_dashboard_drilldown(db, dashboard_filter).await?;
    if let Some(period_start) = filter.period_month_start {
        dashboard_rows.retain(|row| row.period_month.map_or(true, |month| month >= period_start));
        drilldown_rows.retain(|row| row.period_month.map_or(true, |month| month >= period_start));
    }
    if let Some(period_end) = filter.period_month_end {
        dashboard_rows.retain(|row| row.period_month.map_or(true, |month| month <= period_end));
        drilldown_rows.retain(|row| row.period_month.map_or(true, |month| month <= period_end));
    }
    let reviews = list_budget_variance_reviews(
        db,
        BudgetVarianceReviewFilter {
            budget_version_id: Some(filter.budget_version_id),
            driver_code: filter.variance_driver_code.clone(),
            ..BudgetVarianceReviewFilter::default()
        },
    )
    .await?;

    let baseline_amount: f64 = dashboard_rows.iter().map(|row| row.planned_amount).sum();
    let commitment_amount: f64 = dashboard_rows.iter().map(|row| row.committed_amount).sum();
    let posted_actual_amount: f64 = dashboard_rows.iter().map(|row| row.actual_amount).sum();
    let forecast_amount: f64 = dashboard_rows.iter().map(|row| row.forecast_amount).sum();
    let variance_amount = posted_actual_amount - baseline_amount;
    let variance_pct = if baseline_amount.abs() < f64::EPSILON {
        0.0
    } else {
        (variance_amount / baseline_amount) * 100.0
    };

    let mut spend_mix = std::collections::BTreeMap::<String, f64>::new();
    for row in &dashboard_rows {
        let entry = spend_mix.entry(row.spend_mix.clone()).or_insert(0.0);
        *entry += row.actual_amount;
    }

    let mut top_work_orders = std::collections::BTreeMap::<String, f64>::new();
    let mut top_assets = std::collections::BTreeMap::<String, f64>::new();
    for row in &drilldown_rows {
        if let Some(wo_id) = row.work_order_id {
            let entry = top_work_orders.entry(format!("WO-{wo_id}")).or_insert(0.0);
            *entry += row.amount.abs();
        }
        if let Some(source_id) = &row.source_id {
            if source_id.starts_with("EQ-") || source_id.starts_with("ASSET-") {
                let entry = top_assets.entry(source_id.clone()).or_insert(0.0);
                *entry += row.amount.abs();
            }
        }
    }

    let planned_labor_amount: f64 = dashboard_rows
        .iter()
        .filter(|row| row.budget_bucket == "labor")
        .map(|row| row.planned_amount)
        .sum();
    let actual_labor_amount: f64 = dashboard_rows
        .iter()
        .filter(|row| row.budget_bucket == "labor")
        .map(|row| row.actual_amount)
        .sum();
    let overtime_labor_amount: f64 = dashboard_rows
        .iter()
        .filter(|row| row.budget_bucket == "labor" && row.labor_lane.as_deref() == Some("overtime"))
        .map(|row| row.actual_amount)
        .sum();
    let contractor_labor_amount: f64 = dashboard_rows
        .iter()
        .filter(|row| row.budget_bucket == "labor" && row.labor_lane.as_deref() == Some("contractor"))
        .map(|row| row.actual_amount)
        .sum();
    let completed_wo_count = top_work_orders.len() as f64;
    let reassignment_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(has_blocking_conflict), 0) AS churn FROM schedule_commitments",
            [],
        ))
        .await?;
    let reassignment_churn: i64 = reassignment_row
        .as_ref()
        .and_then(|row| row.try_get::<i64>("", "churn").ok())
        .unwrap_or(0);

    let forecast_mix_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH latest_forecast_run AS (
                SELECT budget_version_id, MAX(id) AS run_id
                FROM budget_forecast_runs
                GROUP BY budget_version_id
             )
             SELECT forecast_method, COUNT(*) AS cnt
             FROM budget_forecasts bf
             JOIN latest_forecast_run lfr
               ON lfr.budget_version_id = bf.budget_version_id
              AND lfr.run_id = bf.forecast_run_id
             WHERE bf.budget_version_id = ?
             GROUP BY forecast_method",
            [filter.budget_version_id.into()],
        ))
        .await?;
    let mut forecast_method_mix = serde_json::Map::new();
    for row in forecast_mix_rows {
        let method: String = row.try_get("", "forecast_method")?;
        let cnt: i64 = row.try_get("", "cnt")?;
        forecast_method_mix.insert(method, serde_json::json!(cnt));
    }

    let currency_drift_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT source_currency, base_currency, COUNT(*) AS cnt
             FROM budget_actuals
             WHERE budget_version_id = ?
               AND posting_status = 'posted'
               AND source_currency <> base_currency
             GROUP BY source_currency, base_currency",
            [filter.budget_version_id.into()],
        ))
        .await?;
    let mut multi_currency_flags = Vec::new();
    for row in currency_drift_rows {
        let source_currency: String = row.try_get("", "source_currency")?;
        let base_currency: String = row.try_get("", "base_currency")?;
        let cnt: i64 = row.try_get("", "cnt")?;
        multi_currency_flags.push(format!("{source_currency}->{base_currency}:{cnt}"));
    }

    let totals = BudgetReportPackTotals {
        baseline_amount,
        commitment_amount,
        posted_actual_amount,
        forecast_amount,
        variance_amount,
        variance_pct,
    };
    let workforce_efficiency_json = serde_json::json!({
        "planned_labor_hours": planned_labor_amount / 100.0,
        "actual_labor_hours": actual_labor_amount / 100.0,
        "utilization": if planned_labor_amount.abs() < f64::EPSILON { 0.0 } else { actual_labor_amount / planned_labor_amount },
        "reassignment_churn": reassignment_churn,
        "cost_per_completed_wo": if completed_wo_count <= 0.0 { 0.0 } else { posted_actual_amount / completed_wo_count },
        "overtime_ratio": if actual_labor_amount.abs() < f64::EPSILON { 0.0 } else { overtime_labor_amount / actual_labor_amount },
        "contractor_share": if actual_labor_amount.abs() < f64::EPSILON { 0.0 } else { contractor_labor_amount / actual_labor_amount }
    })
    .to_string();

    Ok(BudgetReportPack {
        generated_at: current_utc_text(),
        budget_version_id: version.id,
        fiscal_year: version.fiscal_year,
        scenario_type: version.scenario_type.clone(),
        version_status: version.status.clone(),
        currency_code: version.currency_code.clone(),
        posting_status_filter: "posted_only_for_actuals".to_string(),
        forecast_method_mix_json: serde_json::Value::Object(forecast_method_mix).to_string(),
        totals,
        spend_mix_json: serde_json::to_string(&spend_mix)?,
        top_work_orders_json: serde_json::to_string(&top_work_orders)?,
        top_assets_json: serde_json::to_string(&top_assets)?,
        workforce_efficiency_json,
        explainability_json: serde_json::json!({
            "version_reference": version.baseline_reference,
            "planning_basis": version.planning_basis,
            "lineage": {
                "dashboard_rows": dashboard_rows.len(),
                "drilldown_rows": drilldown_rows.len(),
                "variance_reviews": reviews.len()
            },
            "filters": {
                "cost_center_id": filter.cost_center_id,
                "period_month_start": filter.period_month_start,
                "period_month_end": filter.period_month_end,
                "budget_bucket": filter.budget_bucket,
                "spend_mix": filter.spend_mix,
                "team_id": filter.team_id,
                "assignee_id": filter.assignee_id,
                "labor_lane": filter.labor_lane,
                "variance_driver_code": filter.variance_driver_code
            }
        })
        .to_string(),
        multi_currency_flags,
    })
}

pub async fn export_budget_report_pack(
    db: &DatabaseConnection,
    input: ExportBudgetReportPackInput,
) -> AppResult<BudgetReportPackExport> {
    let format = required_trimmed("Export format", &input.format)?.to_lowercase();
    if !matches!(format.as_str(), "pdf" | "excel") {
        return Err(AppError::ValidationFailed(vec![
            "format must be one of: pdf, excel".to_string(),
        ]));
    }
    let report = build_budget_report_pack(db, input.filter).await?;
    let extension = if format == "pdf" { "pdf" } else { "csv" };
    let mime_type = if format == "pdf" {
        "application/pdf"
    } else {
        "text/csv"
    };
    let file_name = format!(
        "budget-report-{}-{}-{}.{}",
        report.fiscal_year,
        report.scenario_type,
        report.budget_version_id,
        extension
    );
    let content = if format == "pdf" {
        format!(
            "Budget Report Pack\nGenerated: {}\nVersion: {} ({}/{})\nCurrency: {}\n\nBaseline: {:.2}\nCommitments: {:.2}\nPosted actuals: {:.2}\nForecast: {:.2}\nVariance: {:.2} ({:.2}%)\n\nSpend mix: {}\nTop WOs: {}\nTop assets: {}\nWorkforce appendix: {}\nExplainability: {}",
            report.generated_at,
            report.budget_version_id,
            report.fiscal_year,
            report.scenario_type,
            report.currency_code,
            report.totals.baseline_amount,
            report.totals.commitment_amount,
            report.totals.posted_actual_amount,
            report.totals.forecast_amount,
            report.totals.variance_amount,
            report.totals.variance_pct,
            report.spend_mix_json,
            report.top_work_orders_json,
            report.top_assets_json,
            report.workforce_efficiency_json,
            report.explainability_json
        )
    } else {
        format!(
            "metric,value\nbaseline,{:.2}\ncommitments,{:.2}\nposted_actuals,{:.2}\nforecast,{:.2}\nvariance_amount,{:.2}\nvariance_pct,{:.2}\nspend_mix,\"{}\"\ntop_wos,\"{}\"\ntop_assets,\"{}\"\nworkforce_efficiency,\"{}\"\nexplainability,\"{}\"",
            report.totals.baseline_amount,
            report.totals.commitment_amount,
            report.totals.posted_actual_amount,
            report.totals.forecast_amount,
            report.totals.variance_amount,
            report.totals.variance_pct,
            report.spend_mix_json.replace('\"', "\"\""),
            report.top_work_orders_json.replace('\"', "\"\""),
            report.top_assets_json.replace('\"', "\"\""),
            report.workforce_efficiency_json.replace('\"', "\"\""),
            report.explainability_json.replace('\"', "\"\""),
        )
    };
    Ok(BudgetReportPackExport {
        format,
        file_name,
        mime_type: mime_type.to_string(),
        content,
        report,
    })
}
