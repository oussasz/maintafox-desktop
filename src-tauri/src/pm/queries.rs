use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};
use crate::pm::domain::{
    CreatePmPlanInput, CreatePmPlanVersionInput, ExecutePmOccurrenceInput, ExecutePmOccurrenceResult,
    GeneratePmOccurrencesInput, GeneratePmOccurrencesResult, PmDueMetrics, PmEffortVarianceKpi, PmExecution,
    PmExecutionFilter, PmExecutionFindingInput, PmFinding, PmGovernanceKpiInput, PmGovernanceKpiReport,
    PmOccurrence, PmOccurrenceFilter, PmPlan, PmPlanFilter, PmPlanVersion, PmPlanningCandidate,
    PmPlanningReadinessBlocker, PmPlanningReadinessInput, PmPlanningReadinessProjection, PmRateKpi,
    PmRecurringFinding, PmRecurringFindingsInput, PublishPmPlanVersionInput, TransitionPmOccurrenceInput,
    TransitionPmPlanLifecycleInput, UpdatePmPlanInput, UpdatePmPlanVersionInput,
};
use crate::activity::emitter::{emit_activity_event, ActivityEventInput};
use crate::di::queries::{create_intervention_request, DiCreateInput};
use crate::notifications::emitter::{emit_event as emit_notification_event, NotificationEventInput};
use crate::wo::domain::WoCreateInput;
use crate::wo::queries as wo_queries;

const STRATEGY_TYPES: &[&str] = &["fixed", "floating", "meter", "event", "condition"];
const ASSET_SCOPE_TYPES: &[&str] = &["equipment", "family", "location", "criticality_group"];
const PLAN_LIFECYCLE: &[&str] = &["draft", "proposed", "approved", "active", "suspended", "retired"];

fn parse_bool_to_i64(value: Option<bool>, default_true: bool) -> i64 {
    match value {
        Some(true) => 1,
        Some(false) => 0,
        None => i64::from(default_true),
    }
}

fn normalize_strategy_type(value: &str) -> AppResult<String> {
    let normalized = value.trim().to_lowercase();
    if STRATEGY_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Unsupported PM strategy type '{value}'."
        )]))
    }
}

fn normalize_asset_scope_type(value: &str) -> AppResult<String> {
    let normalized = value.trim().to_lowercase();
    if ASSET_SCOPE_TYPES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Unsupported PM asset scope type '{value}'."
        )]))
    }
}

fn normalize_lifecycle_status(value: &str) -> AppResult<String> {
    let normalized = value.trim().to_lowercase();
    if PLAN_LIFECYCLE.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Unsupported PM lifecycle status '{value}'."
        )]))
    }
}

fn lifecycle_transition_allowed(current: &str, next: &str) -> bool {
    matches!(
        (current, next),
        ("draft", "proposed")
            | ("draft", "retired")
            | ("proposed", "draft")
            | ("proposed", "approved")
            | ("proposed", "retired")
            | ("approved", "active")
            | ("approved", "suspended")
            | ("approved", "retired")
            | ("active", "suspended")
            | ("active", "retired")
            | ("suspended", "active")
            | ("suspended", "retired")
    )
}

fn validate_rfc3339(ts: &str, field: &str) -> AppResult<()> {
    DateTime::parse_from_rfc3339(ts).map_err(|_| {
        AppError::ValidationFailed(vec![format!("{field} must be a valid RFC3339 timestamp.")])
    })?;
    Ok(())
}

fn validate_effective_window(effective_from: &str, effective_to: Option<&str>) -> AppResult<()> {
    validate_rfc3339(effective_from, "effective_from")?;
    if let Some(effective_to) = effective_to {
        validate_rfc3339(effective_to, "effective_to")?;
        if DateTime::parse_from_rfc3339(effective_to)
            .map(|v| v.with_timezone(&Utc))
            .map_err(|_| AppError::ValidationFailed(vec!["effective_to must be a valid RFC3339 timestamp.".to_string()]))?
            < DateTime::parse_from_rfc3339(effective_from)
                .map(|v| v.with_timezone(&Utc))
                .map_err(|_| AppError::ValidationFailed(vec!["effective_from must be a valid RFC3339 timestamp.".to_string()]))?
        {
            return Err(AppError::ValidationFailed(vec![
                "effective_to must be >= effective_from.".to_string(),
            ]));
        }
    }
    Ok(())
}

async fn ensure_lookup_value_in_domain(
    db: &DatabaseConnection,
    value_id: i64,
    expected_domain_key: &str,
    field_name: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ld.domain_key
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE lv.id = ? AND lv.deleted_at IS NULL",
            [value_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::ValidationFailed(vec![format!("{field_name} does not exist.")]));
    };
    let domain_key: String = row.try_get("", "domain_key")?;
    if domain_key != expected_domain_key {
        return Err(AppError::ValidationFailed(vec![format!(
            "{field_name} must reference domain '{expected_domain_key}'."
        )]));
    }
    Ok(())
}

async fn validate_required_skills_json(db: &DatabaseConnection, input: Option<&str>) -> AppResult<()> {
    let Some(raw) = input else { return Ok(()); };
    let parsed = serde_json::from_str::<Value>(raw)
        .map_err(|_| AppError::ValidationFailed(vec!["required_skills_json must be valid JSON.".to_string()]))?;
    let Some(codes) = parsed.as_array() else {
        return Err(AppError::ValidationFailed(vec![
            "required_skills_json must be an array of skill codes.".to_string(),
        ]));
    };
    for code in codes {
        let Some(code) = code.as_str() else {
            return Err(AppError::ValidationFailed(vec![
                "required_skills_json must contain strings only.".to_string(),
            ]));
        };
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt
                 FROM reference_values rv
                 JOIN reference_sets rs ON rs.id = rv.set_id
                 JOIN reference_domains rd ON rd.id = rs.domain_id
                 WHERE rd.code = 'PERSONNEL.SKILLS' AND rs.status = 'published' AND rv.is_active = 1 AND rv.code = ?",
                [code.to_string().into()],
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("skills validation row missing")))?;
        let cnt: i64 = row.try_get("", "cnt")?;
        if cnt == 0 {
            return Err(AppError::ValidationFailed(vec![format!(
                "Skill code '{code}' is not published in PERSONNEL.SKILLS."
            )]));
        }
    }
    Ok(())
}

async fn validate_trigger_definition(
    db: &DatabaseConnection,
    strategy_type: &str,
    trigger_definition_json: &str,
) -> AppResult<()> {
    let parsed = serde_json::from_str::<Value>(trigger_definition_json)
        .map_err(|_| AppError::ValidationFailed(vec!["trigger_definition_json must be valid JSON.".to_string()]))?;
    let Some(obj) = parsed.as_object() else {
        return Err(AppError::ValidationFailed(vec![
            "trigger_definition_json must be an object.".to_string(),
        ]));
    };
    match strategy_type {
        "fixed" | "floating" => {
            let unit = obj.get("interval_unit").and_then(Value::as_str).unwrap_or_default();
            let value = obj.get("interval_value").and_then(Value::as_f64).unwrap_or(0.0);
            if unit.trim().is_empty() || value <= 0.0 {
                return Err(AppError::ValidationFailed(vec![
                    "Calendar/floating trigger requires interval_unit and interval_value > 0.".to_string(),
                ]));
            }
        }
        "meter" => {
            let meter_id = obj.get("asset_meter_id").and_then(Value::as_i64).unwrap_or(0);
            let threshold_value = obj.get("threshold_value").and_then(Value::as_f64).unwrap_or(0.0);
            if meter_id > 0 && threshold_value > 0.0 {
                let exists = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT id FROM equipment_meters WHERE id = ? AND is_active = 1",
                        [meter_id.into()],
                    ))
                    .await?;
                if exists.is_none() {
                    return Err(AppError::ValidationFailed(vec![
                        "Meter trigger references unknown or inactive asset meter.".to_string(),
                    ]));
                }
            } else {
                let meter_source = obj
                    .get("meter_source")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase();
                let interval_value = obj.get("interval_value").and_then(Value::as_f64).unwrap_or(0.0);
                if !matches!(meter_source.as_str(), "odometer" | "operating_hours") || interval_value <= 0.0 {
                    return Err(AppError::ValidationFailed(vec![
                        "Meter trigger requires either (asset_meter_id + threshold_value) or (meter_source + interval_value > 0).".to_string(),
                    ]));
                }
                // Constructor mode: meter resolution happens at generation time from plan asset scope.
                return Ok(());
            }
        }
        "event" => {
            let code = obj.get("event_code").and_then(Value::as_str).unwrap_or_default();
            if code.trim().is_empty() {
                return Err(AppError::ValidationFailed(vec![
                    "Event trigger requires event_code.".to_string(),
                ]));
            }
        }
        "condition" => {
            let code = obj.get("condition_code").and_then(Value::as_str).unwrap_or_default();
            if code.trim().is_empty() {
                return Err(AppError::ValidationFailed(vec![
                    "Condition trigger requires condition_code.".to_string(),
                ]));
            }
        }
        _ => {}
    }
    Ok(())
}

fn decode_pm_plan_row(row: &sea_orm::QueryResult) -> AppResult<PmPlan> { Ok(PmPlan {
    id: row.try_get("", "id")?, code: row.try_get("", "code")?, title: row.try_get("", "title")?,
    description: row.try_get("", "description")?, asset_scope_type: row.try_get("", "asset_scope_type")?,
    asset_scope_id: row.try_get("", "asset_scope_id")?, strategy_type: row.try_get("", "strategy_type")?,
    criticality_value_id: row.try_get("", "criticality_value_id")?, criticality_code: row.try_get("", "criticality_code")?,
    criticality_label: row.try_get("", "criticality_label")?, assigned_group_id: row.try_get("", "assigned_group_id")?,
    requires_shutdown: row.try_get("", "requires_shutdown")?, requires_permit: row.try_get("", "requires_permit")?,
    is_active: row.try_get("", "is_active")?, lifecycle_status: row.try_get("", "lifecycle_status")?,
    current_version_id: row.try_get("", "current_version_id")?, row_version: row.try_get("", "row_version")?,
    created_at: row.try_get("", "created_at")?, updated_at: row.try_get("", "updated_at")?,
})}

fn decode_pm_plan_version_row(row: &sea_orm::QueryResult) -> AppResult<PmPlanVersion> { Ok(PmPlanVersion {
    id: row.try_get("", "id")?, pm_plan_id: row.try_get("", "pm_plan_id")?, version_no: row.try_get("", "version_no")?,
    status: row.try_get("", "status")?, effective_from: row.try_get("", "effective_from")?,
    effective_to: row.try_get("", "effective_to")?, trigger_definition_json: row.try_get("", "trigger_definition_json")?,
    task_package_json: row.try_get("", "task_package_json")?, required_parts_json: row.try_get("", "required_parts_json")?,
    required_skills_json: row.try_get("", "required_skills_json")?, required_tools_json: row.try_get("", "required_tools_json")?,
    estimated_duration_hours: row.try_get("", "estimated_duration_hours")?, estimated_labor_cost: row.try_get("", "estimated_labor_cost")?,
    estimated_parts_cost: row.try_get("", "estimated_parts_cost")?, estimated_service_cost: row.try_get("", "estimated_service_cost")?,
    change_reason: row.try_get("", "change_reason")?, row_version: row.try_get("", "row_version")?,
    created_at: row.try_get("", "created_at")?, updated_at: row.try_get("", "updated_at")?,
})}

pub async fn list_pm_plans(db: &DatabaseConnection, _filter: PmPlanFilter) -> AppResult<Vec<PmPlan>> {
    let rows = db.query_all(Statement::from_string(DbBackend::Sqlite,
        "SELECT p.id,p.code,p.title,p.description,p.asset_scope_type,p.asset_scope_id,p.strategy_type,
                p.criticality_value_id,lv.code AS criticality_code,lv.label AS criticality_label,p.assigned_group_id,
                p.requires_shutdown,p.requires_permit,p.is_active,p.lifecycle_status,p.current_version_id,p.row_version,p.created_at,p.updated_at
         FROM pm_plans p LEFT JOIN lookup_values lv ON lv.id = p.criticality_value_id ORDER BY p.code".to_string())).await?;
    rows.iter().map(decode_pm_plan_row).collect()
}

pub async fn get_pm_plan(db: &DatabaseConnection, plan_id: i64) -> AppResult<PmPlan> {
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite,
        "SELECT p.id,p.code,p.title,p.description,p.asset_scope_type,p.asset_scope_id,p.strategy_type,
                p.criticality_value_id,lv.code AS criticality_code,lv.label AS criticality_label,p.assigned_group_id,
                p.requires_shutdown,p.requires_permit,p.is_active,p.lifecycle_status,p.current_version_id,p.row_version,p.created_at,p.updated_at
         FROM pm_plans p LEFT JOIN lookup_values lv ON lv.id = p.criticality_value_id WHERE p.id = ?", [plan_id.into()])).await?
        .ok_or_else(|| AppError::NotFound{entity:"pm_plan".to_string(), id:plan_id.to_string()})?;
    decode_pm_plan_row(&row)
}

pub async fn create_pm_plan(db: &DatabaseConnection, input: CreatePmPlanInput) -> AppResult<PmPlan> {
    let code = input.code.trim().to_uppercase();
    let title = input.title.trim().to_string();
    if code.is_empty() || title.is_empty() { return Err(AppError::ValidationFailed(vec!["PM plan code and title are required.".to_string()])); }
    let strategy_type = normalize_strategy_type(&input.strategy_type)?;
    let asset_scope_type = normalize_asset_scope_type(&input.asset_scope_type)?;
    if let Some(v) = input.criticality_value_id { ensure_lookup_value_in_domain(db, v, "equipment.criticality", "criticality_value_id").await?; }
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
        "INSERT INTO pm_plans (code,title,description,asset_scope_type,asset_scope_id,strategy_type,criticality_value_id,assigned_group_id,requires_shutdown,requires_permit,is_active,lifecycle_status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'draft')",
        [code.clone().into(),title.into(),input.description.into(),asset_scope_type.into(),input.asset_scope_id.into(),strategy_type.into(),input.criticality_value_id.into(),input.assigned_group_id.into(),i64::from(input.requires_shutdown).into(),i64::from(input.requires_permit).into(),parse_bool_to_i64(input.is_active,true).into()])).await?;
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id FROM pm_plans WHERE code = ?", [code.into()])).await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created pm plan not found")))?;
    get_pm_plan(db, row.try_get("", "id")?).await
}

pub async fn update_pm_plan(db: &DatabaseConnection, plan_id: i64, expected_row_version: i64, input: UpdatePmPlanInput) -> AppResult<PmPlan> {
    let current = get_pm_plan(db, plan_id).await?;
    if current.row_version != expected_row_version { return Err(AppError::ValidationFailed(vec!["PM plan was modified elsewhere (stale row_version).".to_string()])); }
    let title = input.title.unwrap_or(current.title);
    let strategy_type = normalize_strategy_type(&input.strategy_type.unwrap_or(current.strategy_type.clone()))?;
    let asset_scope_type = normalize_asset_scope_type(&input.asset_scope_type.unwrap_or(current.asset_scope_type.clone()))?;
    let criticality_value_id = input.criticality_value_id.or(current.criticality_value_id);
    if let Some(v) = criticality_value_id { ensure_lookup_value_in_domain(db, v, "equipment.criticality", "criticality_value_id").await?; }
    let result = db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
        "UPDATE pm_plans SET title=?,description=?,asset_scope_type=?,asset_scope_id=?,strategy_type=?,criticality_value_id=?,assigned_group_id=?,requires_shutdown=?,requires_permit=?,is_active=?,row_version=row_version+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id=? AND row_version=?",
        [title.into(),input.description.or(current.description).into(),asset_scope_type.into(),input.asset_scope_id.or(current.asset_scope_id).into(),strategy_type.into(),criticality_value_id.into(),input.assigned_group_id.or(current.assigned_group_id).into(),i64::from(input.requires_shutdown.unwrap_or(current.requires_shutdown==1)).into(),i64::from(input.requires_permit.unwrap_or(current.requires_permit==1)).into(),i64::from(input.is_active.unwrap_or(current.is_active==1)).into(),plan_id.into(),expected_row_version.into()])).await?;
    if result.rows_affected()==0 { return Err(AppError::ValidationFailed(vec!["PM plan update failed.".to_string()])); }
    get_pm_plan(db, plan_id).await
}

pub async fn transition_pm_plan_lifecycle(db: &DatabaseConnection, input: TransitionPmPlanLifecycleInput) -> AppResult<PmPlan> {
    let current = get_pm_plan(db, input.plan_id).await?;
    if current.row_version != input.expected_row_version { return Err(AppError::ValidationFailed(vec!["PM lifecycle transition failed (stale row_version).".to_string()])); }
    let next = normalize_lifecycle_status(&input.next_status)?;
    if !lifecycle_transition_allowed(&current.lifecycle_status, &next) { return Err(AppError::ValidationFailed(vec![format!("Invalid PM lifecycle transition: {} -> {}.", current.lifecycle_status, next)])); }
    if next=="active" && current.current_version_id.is_none() { return Err(AppError::ValidationFailed(vec!["Cannot activate PM plan without a published current version.".to_string()])); }
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
        "UPDATE pm_plans SET lifecycle_status=?,row_version=row_version+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND row_version=?",
        [next.into(), input.plan_id.into(), input.expected_row_version.into()])).await?;
    get_pm_plan(db, input.plan_id).await
}

pub async fn list_pm_plan_versions(db: &DatabaseConnection, pm_plan_id: i64) -> AppResult<Vec<PmPlanVersion>> {
    let rows = db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite,
        "SELECT id,pm_plan_id,version_no,status,effective_from,effective_to,trigger_definition_json,task_package_json,required_parts_json,required_skills_json,required_tools_json,estimated_duration_hours,estimated_labor_cost,estimated_parts_cost,estimated_service_cost,change_reason,row_version,created_at,updated_at
         FROM pm_plan_versions WHERE pm_plan_id = ? ORDER BY version_no DESC", [pm_plan_id.into()])).await?;
    rows.iter().map(decode_pm_plan_version_row).collect()
}

pub async fn create_pm_plan_version(db: &DatabaseConnection, pm_plan_id: i64, input: CreatePmPlanVersionInput) -> AppResult<PmPlanVersion> {
    let plan = get_pm_plan(db, pm_plan_id).await?;
    validate_effective_window(&input.effective_from, input.effective_to.as_deref())?;
    validate_trigger_definition(db, &plan.strategy_type, &input.trigger_definition_json).await?;
    validate_required_skills_json(db, input.required_skills_json.as_deref()).await?;
    let version_no_row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT COALESCE(MAX(version_no),0)+1 AS next_no FROM pm_plan_versions WHERE pm_plan_id = ?", [pm_plan_id.into()])).await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("next version not found")))?;
    let next_no: i64 = version_no_row.try_get("", "next_no")?;
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
        "INSERT INTO pm_plan_versions (pm_plan_id,version_no,status,effective_from,effective_to,trigger_definition_json,task_package_json,required_parts_json,required_skills_json,required_tools_json,estimated_duration_hours,estimated_labor_cost,estimated_parts_cost,estimated_service_cost,change_reason)
         VALUES (?, ?, 'draft', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [pm_plan_id.into(),next_no.into(),input.effective_from.into(),input.effective_to.into(),input.trigger_definition_json.into(),input.task_package_json.into(),input.required_parts_json.into(),input.required_skills_json.into(),input.required_tools_json.into(),input.estimated_duration_hours.into(),input.estimated_labor_cost.into(),input.estimated_parts_cost.into(),input.estimated_service_cost.into(),input.change_reason.into()])).await?;
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id FROM pm_plan_versions WHERE pm_plan_id = ? AND version_no = ?", [pm_plan_id.into(), next_no.into()])).await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created version not found")))?;
    let id: i64 = row.try_get("", "id")?;
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id,pm_plan_id,version_no,status,effective_from,effective_to,trigger_definition_json,task_package_json,required_parts_json,required_skills_json,required_tools_json,estimated_duration_hours,estimated_labor_cost,estimated_parts_cost,estimated_service_cost,change_reason,row_version,created_at,updated_at FROM pm_plan_versions WHERE id = ?", [id.into()])).await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created version missing")))?;
    decode_pm_plan_version_row(&row)
}

pub async fn update_pm_plan_version(db: &DatabaseConnection, version_id: i64, expected_row_version: i64, input: UpdatePmPlanVersionInput) -> AppResult<PmPlanVersion> {
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT * FROM pm_plan_versions WHERE id = ?", [version_id.into()])).await?
        .ok_or_else(|| AppError::NotFound{entity:"pm_plan_version".to_string(), id:version_id.to_string()})?;
    let status: String = row.try_get("", "status")?;
    if status != "draft" { return Err(AppError::ValidationFailed(vec!["Only draft PM plan versions can be edited.".to_string()])); }
    let rv: i64 = row.try_get("", "row_version")?;
    if rv != expected_row_version { return Err(AppError::ValidationFailed(vec!["PM plan version was modified elsewhere (stale row_version).".to_string()])); }
    let plan_id: i64 = row.try_get("", "pm_plan_id")?;
    let plan = get_pm_plan(db, plan_id).await?;
    let effective_from = input.effective_from.unwrap_or(row.try_get("", "effective_from")?);
    let effective_to = input.effective_to.or(row.try_get("", "effective_to")?);
    let trigger_definition_json = input.trigger_definition_json.unwrap_or(row.try_get("", "trigger_definition_json")?);
    validate_effective_window(&effective_from, effective_to.as_deref())?;
    validate_trigger_definition(db, &plan.strategy_type, &trigger_definition_json).await?;
    let required_skills_json = input.required_skills_json.or(row.try_get("", "required_skills_json")?);
    validate_required_skills_json(db, required_skills_json.as_deref()).await?;
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
        "UPDATE pm_plan_versions SET effective_from=?,effective_to=?,trigger_definition_json=?,task_package_json=?,required_parts_json=?,required_skills_json=?,required_tools_json=?,estimated_duration_hours=?,estimated_labor_cost=?,estimated_parts_cost=?,estimated_service_cost=?,change_reason=?,row_version=row_version+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND row_version=?",
        [effective_from.into(),effective_to.into(),trigger_definition_json.into(),input.task_package_json.or(row.try_get("", "task_package_json")?).into(),input.required_parts_json.or(row.try_get("", "required_parts_json")?).into(),required_skills_json.into(),input.required_tools_json.or(row.try_get("", "required_tools_json")?).into(),input.estimated_duration_hours.or(row.try_get("", "estimated_duration_hours")?).into(),input.estimated_labor_cost.or(row.try_get("", "estimated_labor_cost")?).into(),input.estimated_parts_cost.or(row.try_get("", "estimated_parts_cost")?).into(),input.estimated_service_cost.or(row.try_get("", "estimated_service_cost")?).into(),input.change_reason.or(row.try_get("", "change_reason")?).into(),version_id.into(),expected_row_version.into()])).await?;
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id,pm_plan_id,version_no,status,effective_from,effective_to,trigger_definition_json,task_package_json,required_parts_json,required_skills_json,required_tools_json,estimated_duration_hours,estimated_labor_cost,estimated_parts_cost,estimated_service_cost,change_reason,row_version,created_at,updated_at FROM pm_plan_versions WHERE id = ?", [version_id.into()])).await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("updated version missing")))?;
    decode_pm_plan_version_row(&row)
}

pub async fn publish_pm_plan_version(db: &DatabaseConnection, input: PublishPmPlanVersionInput) -> AppResult<PmPlanVersion> {
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT * FROM pm_plan_versions WHERE id = ?", [input.version_id.into()])).await?
        .ok_or_else(|| AppError::NotFound{entity:"pm_plan_version".to_string(), id:input.version_id.to_string()})?;
    let rv: i64 = row.try_get("", "row_version")?;
    if rv != input.expected_row_version { return Err(AppError::ValidationFailed(vec!["PM version publish failed (stale row_version).".to_string()])); }
    let plan_id: i64 = row.try_get("", "pm_plan_id")?;
    let plan = get_pm_plan(db, plan_id).await?;
    if !matches!(plan.lifecycle_status.as_str(), "approved" | "active" | "suspended") {
        return Err(AppError::ValidationFailed(vec!["Plan must be approved, active, or suspended before publishing a version.".to_string()]));
    }
    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE pm_plan_versions SET status='superseded',row_version=row_version+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE pm_plan_id=? AND status='published' AND id != ?", [plan_id.into(), input.version_id.into()])).await?;
    tx.execute(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE pm_plan_versions SET status='published',row_version=row_version+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=? AND row_version=?", [input.version_id.into(),input.expected_row_version.into()])).await?;
    let next_plan_lifecycle = if plan.lifecycle_status == "approved" { "active".to_string() } else { plan.lifecycle_status };
    tx.execute(Statement::from_sql_and_values(DbBackend::Sqlite, "UPDATE pm_plans SET current_version_id=?,lifecycle_status=?,row_version=row_version+1,updated_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?", [input.version_id.into(), next_plan_lifecycle.into(), plan_id.into()])).await?;
    tx.commit().await?;
    let row = db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, "SELECT id,pm_plan_id,version_no,status,effective_from,effective_to,trigger_definition_json,task_package_json,required_parts_json,required_skills_json,required_tools_json,estimated_duration_hours,estimated_labor_cost,estimated_parts_cost,estimated_service_cost,change_reason,row_version,created_at,updated_at FROM pm_plan_versions WHERE id = ?", [input.version_id.into()])).await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("published version missing")))?;
    decode_pm_plan_version_row(&row)
}


const OCCURRENCE_STATUSES: &[&str] = &[
    "forecasted",
    "generated",
    "ready_for_scheduling",
    "scheduled",
    "in_progress",
    "completed",
    "deferred",
    "missed",
    "cancelled",
];

fn normalize_occurrence_status(value: &str) -> AppResult<String> {
    let normalized = value.trim().to_lowercase();
    if OCCURRENCE_STATUSES.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Unsupported PM occurrence status '{value}'."
        )]))
    }
}

fn occurrence_transition_allowed(current: &str, next: &str) -> bool {
    matches!(
        (current, next),
        ("forecasted", "generated")
            | ("forecasted", "cancelled")
            | ("forecasted", "missed")
            | ("generated", "ready_for_scheduling")
            | ("generated", "deferred")
            | ("generated", "cancelled")
            | ("generated", "missed")
            | ("ready_for_scheduling", "scheduled")
            | ("ready_for_scheduling", "deferred")
            | ("ready_for_scheduling", "cancelled")
            | ("scheduled", "in_progress")
            | ("scheduled", "deferred")
            | ("scheduled", "cancelled")
            | ("scheduled", "missed")
            | ("in_progress", "completed")
            | ("in_progress", "deferred")
            | ("in_progress", "cancelled")
            | ("deferred", "ready_for_scheduling")
            | ("deferred", "scheduled")
            | ("deferred", "missed")
            | ("deferred", "cancelled")
            | ("missed", "generated")
            | ("missed", "cancelled")
    )
}

fn now_rfc3339() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_as_of_or_now(as_of: Option<&str>) -> AppResult<DateTime<Utc>> {
    match as_of {
        Some(raw) => {
            let parsed = DateTime::parse_from_rfc3339(raw).map_err(|_| {
                AppError::ValidationFailed(vec!["as_of must be a valid RFC3339 timestamp.".to_string()])
            })?;
            Ok(parsed.with_timezone(&Utc))
        }
        None => Ok(Utc::now()),
    }
}

fn parse_interval(trigger_obj: &serde_json::Map<String, Value>) -> AppResult<Duration> {
    let unit = trigger_obj
        .get("interval_unit")
        .and_then(Value::as_str)
        .unwrap_or("day")
        .trim()
        .to_lowercase();
    let value = trigger_obj
        .get("interval_value")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if value <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "interval_value must be > 0 for fixed/floating strategies.".to_string(),
        ]));
    }

    let amount = value.round() as i64;
    match unit.as_str() {
        "hour" | "hours" => Ok(Duration::hours(amount)),
        "day" | "days" => Ok(Duration::days(amount)),
        "week" | "weeks" => Ok(Duration::weeks(amount)),
        "month" | "months" => Ok(Duration::days(amount * 30)),
        "year" | "years" => Ok(Duration::days(amount * 365)),
        _ => Err(AppError::ValidationFailed(vec![
            "interval_unit must be one of: hour, day, week, month, year.".to_string(),
        ])),
    }
}

fn decode_pm_occurrence_row(row: &sea_orm::QueryResult) -> AppResult<PmOccurrence> {
    Ok(PmOccurrence {
        id: row.try_get("", "id")?,
        pm_plan_id: row.try_get("", "pm_plan_id")?,
        plan_version_id: row.try_get("", "plan_version_id")?,
        due_basis: row.try_get("", "due_basis")?,
        due_at: row.try_get("", "due_at")?,
        due_meter_value: row.try_get("", "due_meter_value")?,
        generated_at: row.try_get("", "generated_at")?,
        status: row.try_get("", "status")?,
        linked_work_order_id: row.try_get("", "linked_work_order_id")?,
        linked_work_order_code: row.try_get("", "linked_work_order_code")?,
        deferral_reason: row.try_get("", "deferral_reason")?,
        missed_reason: row.try_get("", "missed_reason")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
        plan_code: row.try_get("", "plan_code")?,
        plan_title: row.try_get("", "plan_title")?,
        strategy_type: row.try_get("", "strategy_type")?,
    })
}

async fn get_pm_occurrence_by_id(db: &DatabaseConnection, occurrence_id: i64) -> AppResult<PmOccurrence> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.id, po.pm_plan_id, po.plan_version_id, po.due_basis, po.due_at, po.due_meter_value, po.generated_at, po.status,
                    po.linked_work_order_id, wo.code AS linked_work_order_code, po.deferral_reason, po.missed_reason, po.row_version,
                    po.created_at, po.updated_at, pp.code AS plan_code, pp.title AS plan_title, pp.strategy_type
             FROM pm_occurrences po
             JOIN pm_plans pp ON pp.id = po.pm_plan_id
             LEFT JOIN work_orders wo ON wo.id = po.linked_work_order_id
             WHERE po.id = ?",
            [occurrence_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_occurrence".to_string(),
            id: occurrence_id.to_string(),
        })?;
    decode_pm_occurrence_row(&row)
}

async fn occurrence_exists(
    db: &DatabaseConnection,
    pm_plan_id: i64,
    plan_version_id: i64,
    due_basis: &str,
    due_at: Option<&str>,
    due_meter_value: Option<f64>,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt
             FROM pm_occurrences
             WHERE pm_plan_id = ?
               AND plan_version_id = ?
               AND due_basis = ?
               AND COALESCE(due_at, '') = COALESCE(?, '')
               AND COALESCE(due_meter_value, -1) = COALESCE(?, -1)",
            [
                pm_plan_id.into(),
                plan_version_id.into(),
                due_basis.to_string().into(),
                due_at.map(|v| v.to_string()).into(),
                due_meter_value.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("occurrence existence check row missing")))?;

    let count: i64 = row.try_get("", "cnt")?;
    Ok(count > 0)
}

async fn insert_trigger_event(
    db: &DatabaseConnection,
    pm_plan_id: i64,
    plan_version_id: i64,
    trigger_type: &str,
    source_reference: Option<String>,
    measured_value: Option<f64>,
    threshold_value: Option<f64>,
    was_generated: bool,
    generated_occurrence_id: Option<i64>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO pm_trigger_events
         (pm_plan_id, plan_version_id, trigger_type, source_reference, measured_value, threshold_value, was_generated, generated_occurrence_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            pm_plan_id.into(),
            plan_version_id.into(),
            trigger_type.to_string().into(),
            source_reference.into(),
            measured_value.into(),
            threshold_value.into(),
            i64::from(was_generated).into(),
            generated_occurrence_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

fn is_occurrence_idempotency_conflict(err: &AppError) -> bool {
    matches!(err, AppError::Database(db_err) if db_err.to_string().contains("idx_pm_occurrence_idempotency"))
}

async fn insert_occurrence(
    db: &DatabaseConnection,
    pm_plan_id: i64,
    plan_version_id: i64,
    due_basis: &str,
    due_at: Option<String>,
    due_meter_value: Option<f64>,
) -> AppResult<Option<i64>> {
    let insert_result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO pm_occurrences (pm_plan_id, plan_version_id, due_basis, due_at, due_meter_value, status)
             VALUES (?, ?, ?, ?, ?, 'forecasted')",
            [
                pm_plan_id.into(),
                plan_version_id.into(),
                due_basis.to_string().into(),
                due_at.into(),
                due_meter_value.into(),
            ],
        ))
        .await;

    if let Err(db_err) = insert_result {
        let app_err = AppError::Database(db_err);
        if is_occurrence_idempotency_conflict(&app_err) {
            return Ok(None);
        }
        return Err(app_err);
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM pm_occurrences WHERE rowid = last_insert_rowid()",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to read inserted PM occurrence")))?;
    let id: i64 = row.try_get("", "id")?;
    Ok(Some(id))
}

pub async fn list_pm_occurrences(db: &DatabaseConnection, filter: PmOccurrenceFilter) -> AppResult<Vec<PmOccurrence>> {
    let mut sql = String::from(
        "SELECT po.id, po.pm_plan_id, po.plan_version_id, po.due_basis, po.due_at, po.due_meter_value, po.generated_at, po.status,
                po.linked_work_order_id, wo.code AS linked_work_order_code, po.deferral_reason, po.missed_reason, po.row_version,
                po.created_at, po.updated_at, pp.code AS plan_code, pp.title AS plan_title, pp.strategy_type
         FROM pm_occurrences po
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN work_orders wo ON wo.id = po.linked_work_order_id
         WHERE 1 = 1",
    );
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(pm_plan_id) = filter.pm_plan_id {
        sql.push_str(" AND po.pm_plan_id = ?");
        binds.push(pm_plan_id.into());
    }
    if let Some(status) = filter.status {
        sql.push_str(" AND po.status = ?");
        binds.push(status.into());
    }
    if let Some(due_from) = filter.due_from {
        sql.push_str(" AND po.due_at >= ?");
        binds.push(due_from.into());
    }
    if let Some(due_to) = filter.due_to {
        sql.push_str(" AND po.due_at <= ?");
        binds.push(due_to.into());
    }
    if filter.include_completed != Some(true) {
        sql.push_str(" AND po.status NOT IN ('completed','cancelled')");
    }

    sql.push_str(" ORDER BY CASE WHEN po.due_at IS NULL THEN 1 ELSE 0 END, po.due_at ASC, po.id DESC");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;
    rows.iter().map(decode_pm_occurrence_row).collect()
}

pub async fn get_pm_due_metrics(db: &DatabaseConnection) -> AppResult<PmDueMetrics> {
    let as_of = now_rfc3339();
    let overdue_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences
             WHERE status NOT IN ('completed','cancelled','missed')
               AND due_at IS NOT NULL
               AND due_at < ?",
            [as_of.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm overdue metric row missing")))?;
    let overdue_count: i64 = overdue_row.try_get("", "cnt")?;

    let due_today_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences
             WHERE status NOT IN ('completed','cancelled','missed')
               AND due_at IS NOT NULL
               AND date(due_at) = date('now')",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm due today metric row missing")))?;
    let due_today_count: i64 = due_today_row.try_get("", "cnt")?;

    let due_next_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences
             WHERE status NOT IN ('completed','cancelled','missed')
               AND due_at IS NOT NULL
               AND due_at >= ?
               AND due_at <= datetime(?, '+7 days')",
            [as_of.clone().into(), as_of.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm due next metric row missing")))?;
    let due_next_7d_count: i64 = due_next_row.try_get("", "cnt")?;

    let ready_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences WHERE status = 'ready_for_scheduling'",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm ready metric row missing")))?;
    let ready_for_scheduling_count: i64 = ready_row.try_get("", "cnt")?;

    Ok(PmDueMetrics {
        as_of,
        overdue_count,
        due_today_count,
        due_next_7d_count,
        ready_for_scheduling_count,
    })
}

pub async fn list_pm_planning_readiness(
    db: &DatabaseConnection,
    input: PmPlanningReadinessInput,
) -> AppResult<PmPlanningReadinessProjection> {
    if let Some(due_from) = input.due_from.as_deref() {
        validate_rfc3339(due_from, "due_from")?;
    }
    if let Some(due_to) = input.due_to.as_deref() {
        validate_rfc3339(due_to, "due_to")?;
    }

    let include_linked_work_orders = input.include_linked_work_orders.unwrap_or(false);
    let limit = input.limit.unwrap_or(200).clamp(1, 1000);

    let mut sql = String::from(
        "SELECT po.id, po.pm_plan_id, po.plan_version_id, po.due_basis, po.due_at, po.due_meter_value, po.generated_at, po.status,
                po.linked_work_order_id, wo.code AS linked_work_order_code, po.deferral_reason, po.missed_reason, po.row_version,
                po.created_at, po.updated_at, pp.code AS plan_code, pp.title AS plan_title, pp.strategy_type,
                pv.required_parts_json, pv.required_skills_json, pp.requires_permit, pp.requires_shutdown
         FROM pm_occurrences po
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         JOIN pm_plan_versions pv ON pv.id = po.plan_version_id
         LEFT JOIN work_orders wo ON wo.id = po.linked_work_order_id
         WHERE po.status IN ('generated','ready_for_scheduling','deferred')",
    );
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(pm_plan_id) = input.pm_plan_id {
        sql.push_str(" AND po.pm_plan_id = ?");
        binds.push(pm_plan_id.into());
    }
    if let Some(due_from) = input.due_from {
        sql.push_str(" AND po.due_at >= ?");
        binds.push(due_from.into());
    }
    if let Some(due_to) = input.due_to {
        sql.push_str(" AND po.due_at <= ?");
        binds.push(due_to.into());
    }
    if !include_linked_work_orders {
        sql.push_str(" AND po.linked_work_order_id IS NULL");
    }

    sql.push_str(" ORDER BY CASE WHEN po.due_at IS NULL THEN 1 ELSE 0 END, po.due_at ASC, po.id DESC LIMIT ?");
    binds.push(limit.into());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut candidates = Vec::new();
    let mut ready_count = 0_i64;
    let mut blocked_count = 0_i64;

    for row in rows {
        let occurrence = decode_pm_occurrence_row(&row)?;
        let required_parts_json: Option<String> = row.try_get("", "required_parts_json")?;
        let required_skills_json: Option<String> = row.try_get("", "required_skills_json")?;
        let requires_permit: i64 = row.try_get("", "requires_permit")?;
        let requires_shutdown: i64 = row.try_get("", "requires_shutdown")?;

        let mut blockers: Vec<PmPlanningReadinessBlocker> = Vec::new();

        if json_array_has_items(required_parts_json.as_deref()) {
            blockers.push(PmPlanningReadinessBlocker {
                code: "missing_parts".to_string(),
                message: "PM version defines required_parts_json and no planning reservation contract exists yet.".to_string(),
                source: "pm_plan_versions.required_parts_json".to_string(),
            });
        }
        if json_array_has_items(required_skills_json.as_deref()) {
            blockers.push(PmPlanningReadinessBlocker {
                code: "missing_qualification".to_string(),
                message: "PM version defines required_skills_json and no assignment qualification has been committed yet.".to_string(),
                source: "pm_plan_versions.required_skills_json".to_string(),
            });
        }
        if requires_permit == 1 {
            blockers.push(PmPlanningReadinessBlocker {
                code: "permit_not_ready".to_string(),
                message: "Plan requires permit and permit readiness is not committed by planning workflow yet.".to_string(),
                source: "pm_plans.requires_permit".to_string(),
            });
        }
        if requires_shutdown == 1 {
            blockers.push(PmPlanningReadinessBlocker {
                code: "locked_window".to_string(),
                message: "Plan requires shutdown window that is not yet reserved in planning.".to_string(),
                source: "pm_plans.requires_shutdown".to_string(),
            });
        }
        if occurrence.status == "deferred" {
            blockers.push(PmPlanningReadinessBlocker {
                code: "prerequisite_incomplete".to_string(),
                message: format!(
                    "Occurrence is deferred{}.",
                    occurrence
                        .deferral_reason
                        .as_ref()
                        .map(|reason| format!(" (reason: {})", reason))
                        .unwrap_or_default()
                ),
                source: "pm_occurrences.status".to_string(),
            });
        }
        if occurrence.linked_work_order_id.is_some() {
            blockers.push(PmPlanningReadinessBlocker {
                code: "prerequisite_incomplete".to_string(),
                message: "Occurrence already linked to a work order and should not be re-committed by planning.".to_string(),
                source: "pm_occurrences.linked_work_order_id".to_string(),
            });
        }

        let ready_for_scheduling = blockers.is_empty();
        if ready_for_scheduling {
            ready_count += 1;
        } else {
            blocked_count += 1;
        }

        candidates.push(PmPlanningCandidate {
            occurrence,
            ready_for_scheduling,
            blockers,
        });
    }

    Ok(PmPlanningReadinessProjection {
        as_of: now_rfc3339(),
        candidate_count: candidates.len() as i64,
        ready_count,
        blocked_count,
        derivation_rules: vec![
            "Occurrences are source records; readiness output is projection-only and never mutates scheduling commitments.".to_string(),
            "Blocker taxonomy is currently derived from PM plan/version governance fields plus occurrence state.".to_string(),
            "missing_parts/missing_qualification indicate declared requirements without a committed planning assignment/reservation.".to_string(),
        ],
        candidates,
    })
}

pub async fn get_pm_governance_kpi_report(
    db: &DatabaseConnection,
    input: PmGovernanceKpiInput,
) -> AppResult<PmGovernanceKpiReport> {
    let as_of = now_rfc3339();
    let (from, to) = bounded_period(input.from.as_deref(), input.to.as_deref())?;
    let criticality_code = input
        .criticality_code
        .as_ref()
        .map(|value| value.trim().to_uppercase())
        .filter(|value| !value.is_empty());

    let mut compliance_sql = String::from(
        "SELECT
            COALESCE(SUM(CASE WHEN po.status = 'completed' THEN 1 ELSE 0 END), 0) AS numerator,
            COALESCE(SUM(CASE WHEN po.status != 'cancelled' THEN 1 ELSE 0 END), 0) AS denominator
         FROM pm_occurrences po
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN lookup_values lv ON lv.id = pp.criticality_value_id
         WHERE po.due_at IS NOT NULL
           AND po.due_at >= ?
           AND po.due_at <= ?",
    );
    let mut compliance_binds: Vec<sea_orm::Value> = vec![from.clone().into(), to.clone().into()];
    if let Some(pm_plan_id) = input.pm_plan_id {
        compliance_sql.push_str(" AND po.pm_plan_id = ?");
        compliance_binds.push(pm_plan_id.into());
    }
    if let Some(code) = criticality_code.as_ref() {
        compliance_sql.push_str(" AND UPPER(COALESCE(lv.code, '')) = ?");
        compliance_binds.push(code.clone().into());
    }
    let compliance_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &compliance_sql,
            compliance_binds,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm compliance KPI row missing")))?;
    let compliance_numerator: i64 = compliance_row.try_get("", "numerator")?;
    let compliance_denominator: i64 = compliance_row.try_get("", "denominator")?;

    let mut overdue_sql = String::from(
        "SELECT
            COALESCE(SUM(CASE WHEN po.status NOT IN ('completed','cancelled','missed') AND po.due_at < ? THEN 1 ELSE 0 END), 0) AS numerator,
            COALESCE(SUM(CASE WHEN po.status NOT IN ('completed','cancelled','missed') THEN 1 ELSE 0 END), 0) AS denominator
         FROM pm_occurrences po
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN lookup_values lv ON lv.id = pp.criticality_value_id
         WHERE po.due_at IS NOT NULL
           AND po.due_at <= ?",
    );
    let mut overdue_binds: Vec<sea_orm::Value> = vec![as_of.clone().into(), to.clone().into()];
    if let Some(pm_plan_id) = input.pm_plan_id {
        overdue_sql.push_str(" AND po.pm_plan_id = ?");
        overdue_binds.push(pm_plan_id.into());
    }
    if let Some(code) = criticality_code.as_ref() {
        overdue_sql.push_str(" AND UPPER(COALESCE(lv.code, '')) = ?");
        overdue_binds.push(code.clone().into());
    }
    let overdue_row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &overdue_sql, overdue_binds))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm overdue KPI row missing")))?;
    let overdue_numerator: i64 = overdue_row.try_get("", "numerator")?;
    let overdue_denominator: i64 = overdue_row.try_get("", "denominator")?;

    let mut first_pass_sql = String::from(
        "SELECT
            COALESCE(SUM(CASE WHEN pe.execution_result = 'completed_no_findings' THEN 1 ELSE 0 END), 0) AS numerator,
            COALESCE(SUM(CASE WHEN pe.execution_result IN ('completed_no_findings','completed_with_findings') THEN 1 ELSE 0 END), 0) AS denominator
         FROM pm_executions pe
         JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN lookup_values lv ON lv.id = pp.criticality_value_id
         WHERE pe.executed_at >= ?
           AND pe.executed_at <= ?",
    );
    let mut first_pass_binds: Vec<sea_orm::Value> = vec![from.clone().into(), to.clone().into()];
    if let Some(pm_plan_id) = input.pm_plan_id {
        first_pass_sql.push_str(" AND po.pm_plan_id = ?");
        first_pass_binds.push(pm_plan_id.into());
    }
    if let Some(code) = criticality_code.as_ref() {
        first_pass_sql.push_str(" AND UPPER(COALESCE(lv.code, '')) = ?");
        first_pass_binds.push(code.clone().into());
    }
    let first_pass_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &first_pass_sql,
            first_pass_binds,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm first-pass KPI row missing")))?;
    let first_pass_numerator: i64 = first_pass_row.try_get("", "numerator")?;
    let first_pass_denominator: i64 = first_pass_row.try_get("", "denominator")?;

    let mut follow_up_sql = String::from(
        "SELECT
            COALESCE(SUM(CASE WHEN pf.follow_up_di_id IS NOT NULL OR pf.follow_up_work_order_id IS NOT NULL THEN 1 ELSE 0 END), 0) AS numerator,
            COALESCE(COUNT(*), 0) AS denominator
         FROM pm_findings pf
         JOIN pm_executions pe ON pe.id = pf.pm_execution_id
         JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN lookup_values lv ON lv.id = pp.criticality_value_id
         WHERE pe.executed_at >= ?
           AND pe.executed_at <= ?",
    );
    let mut follow_up_binds: Vec<sea_orm::Value> = vec![from.clone().into(), to.clone().into()];
    if let Some(pm_plan_id) = input.pm_plan_id {
        follow_up_sql.push_str(" AND po.pm_plan_id = ?");
        follow_up_binds.push(pm_plan_id.into());
    }
    if let Some(code) = criticality_code.as_ref() {
        follow_up_sql.push_str(" AND UPPER(COALESCE(lv.code, '')) = ?");
        follow_up_binds.push(code.clone().into());
    }
    let follow_up_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &follow_up_sql,
            follow_up_binds,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm follow-up KPI row missing")))?;
    let follow_up_numerator: i64 = follow_up_row.try_get("", "numerator")?;
    let follow_up_denominator: i64 = follow_up_row.try_get("", "denominator")?;

    let mut effort_sql = String::from(
        "SELECT
            COALESCE(COUNT(*), 0) AS sample_size,
            CAST(COALESCE(SUM(COALESCE(pv.estimated_duration_hours, 0.0)), 0.0) AS REAL) AS estimated_hours,
            CAST(COALESCE(SUM(COALESCE(pe.actual_labor_hours, pe.actual_duration_hours, 0.0)), 0.0) AS REAL) AS actual_hours
         FROM pm_executions pe
         JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id
         JOIN pm_plan_versions pv ON pv.id = po.plan_version_id
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN lookup_values lv ON lv.id = pp.criticality_value_id
         WHERE pe.executed_at >= ?
           AND pe.executed_at <= ?
           AND pe.execution_result IN ('completed_no_findings','completed_with_findings')
           AND pv.estimated_duration_hours IS NOT NULL
           AND (pe.actual_labor_hours IS NOT NULL OR pe.actual_duration_hours IS NOT NULL)",
    );
    let mut effort_binds: Vec<sea_orm::Value> = vec![from.clone().into(), to.clone().into()];
    if let Some(pm_plan_id) = input.pm_plan_id {
        effort_sql.push_str(" AND po.pm_plan_id = ?");
        effort_binds.push(pm_plan_id.into());
    }
    if let Some(code) = criticality_code.as_ref() {
        effort_sql.push_str(" AND UPPER(COALESCE(lv.code, '')) = ?");
        effort_binds.push(code.clone().into());
    }
    let effort_row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &effort_sql, effort_binds))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm effort KPI row missing")))?;

    let sample_size: i64 = effort_row.try_get("", "sample_size")?;
    let estimated_hours: f64 = effort_row.try_get("", "estimated_hours")?;
    let actual_hours: f64 = effort_row.try_get("", "actual_hours")?;
    let variance_hours = actual_hours - estimated_hours;
    let variance_pct = if estimated_hours <= 0.0 {
        None
    } else {
        Some((variance_hours * 100.0) / estimated_hours)
    };

    Ok(PmGovernanceKpiReport {
        as_of,
        from,
        to,
        pm_plan_id: input.pm_plan_id,
        criticality_code,
        compliance: PmRateKpi {
            numerator: compliance_numerator,
            denominator: compliance_denominator,
            value_pct: rate_pct(compliance_numerator, compliance_denominator),
            derivation: "completed occurrences / all non-cancelled due occurrences in period".to_string(),
        },
        overdue_risk: PmRateKpi {
            numerator: overdue_numerator,
            denominator: overdue_denominator,
            value_pct: rate_pct(overdue_numerator, overdue_denominator),
            derivation: "open overdue occurrences / all open occurrences with due_at <= period end".to_string(),
        },
        first_pass_completion: PmRateKpi {
            numerator: first_pass_numerator,
            denominator: first_pass_denominator,
            value_pct: rate_pct(first_pass_numerator, first_pass_denominator),
            derivation: "completed_no_findings executions / all completed executions".to_string(),
        },
        follow_up_ratio: PmRateKpi {
            numerator: follow_up_numerator,
            denominator: follow_up_denominator,
            value_pct: rate_pct(follow_up_numerator, follow_up_denominator),
            derivation: "findings with DI/WO follow-up / all findings on executions in period".to_string(),
        },
        effort_variance: PmEffortVarianceKpi {
            sample_size,
            estimated_hours,
            actual_hours,
            variance_hours,
            variance_pct,
            derivation: "sum(actual labor|duration hours) - sum(estimated_duration_hours) for completed executions with estimates".to_string(),
        },
        derivation_rules: vec![
            "Period filter applies to due_at for compliance/overdue and executed_at for execution/finding KPIs.".to_string(),
            "criticality_code uses pm_plans.criticality_value_id -> lookup_values.code.".to_string(),
            "Effort variance uses execution.actual_labor_hours first, then execution.actual_duration_hours fallback.".to_string(),
        ],
    })
}
pub async fn generate_pm_occurrences(
    db: &DatabaseConnection,
    input: GeneratePmOccurrencesInput,
) -> AppResult<GeneratePmOccurrencesResult> {
    let as_of_dt = parse_as_of_or_now(input.as_of.as_deref())?;
    let as_of = as_of_dt.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let horizon_days = input.horizon_days.unwrap_or(30).clamp(1, 365);
    let horizon_end = as_of_dt + Duration::days(horizon_days);

    let event_codes: std::collections::HashSet<String> = input
        .event_codes
        .unwrap_or_default()
        .into_iter()
        .map(|v| v.trim().to_uppercase())
        .collect();
    let condition_codes: std::collections::HashSet<String> = input
        .condition_codes
        .unwrap_or_default()
        .into_iter()
        .map(|v| v.trim().to_uppercase())
        .collect();

    let mut sql = String::from(
        "SELECT p.id AS pm_plan_id, p.code AS plan_code, p.title AS plan_title, p.strategy_type, p.asset_scope_type, p.asset_scope_id,
                pv.id AS version_id, pv.effective_from, pv.effective_to, pv.trigger_definition_json
         FROM pm_plans p
         JOIN pm_plan_versions pv ON pv.id = p.current_version_id
         WHERE p.is_active = 1
           AND p.lifecycle_status IN ('active', 'suspended')
           AND pv.status = 'published'",
    );
    let mut binds: Vec<sea_orm::Value> = Vec::new();
    if let Some(plan_id) = input.pm_plan_id {
        sql.push_str(" AND p.id = ?");
        binds.push(plan_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut generated_count = 0_i64;
    let mut skipped_count = 0_i64;
    let mut trigger_events_recorded = 0_i64;
    let mut occurrence_ids: Vec<i64> = Vec::new();

    for row in rows {
        let pm_plan_id: i64 = row.try_get("", "pm_plan_id")?;
        let version_id: i64 = row.try_get("", "version_id")?;
        let strategy_type: String = row.try_get("", "strategy_type")?;
        let effective_from: String = row.try_get("", "effective_from")?;
        let trigger_definition_json: String = row.try_get("", "trigger_definition_json")?;
        let trigger_value = serde_json::from_str::<Value>(&trigger_definition_json)
            .map_err(|_| AppError::ValidationFailed(vec!["trigger_definition_json must be valid JSON.".to_string()]))?;
        let Some(trigger_obj) = trigger_value.as_object() else {
            return Err(AppError::ValidationFailed(vec![
                "trigger_definition_json must be a JSON object.".to_string(),
            ]));
        };

        match strategy_type.as_str() {
            "fixed" => {
                let interval = parse_interval(trigger_obj)?;
                let mut due = DateTime::parse_from_rfc3339(&effective_from)
                    .map_err(|_| AppError::ValidationFailed(vec!["effective_from must be RFC3339.".to_string()]))?
                    .with_timezone(&Utc);
                while due < as_of_dt {
                    due += interval;
                }
                while due <= horizon_end {
                    let due_at = due.format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    let due_basis = format!("fixed:{}", due_at);
                    let exists = occurrence_exists(db, pm_plan_id, version_id, &due_basis, Some(&due_at), None).await?;
                    if exists {
                        skipped_count += 1;
                        insert_trigger_event(
                            db,
                            pm_plan_id,
                            version_id,
                            "fixed",
                            Some(due_basis),
                            None,
                            None,
                            false,
                            None,
                        )
                        .await?;
                    } else {
                        if let Some(occurrence_id) =
                            insert_occurrence(db, pm_plan_id, version_id, &due_basis, Some(due_at.clone()), None).await?
                        {
                            generated_count += 1;
                            occurrence_ids.push(occurrence_id);
                            insert_trigger_event(
                                db,
                                pm_plan_id,
                                version_id,
                                "fixed",
                                Some(due_basis),
                                None,
                                None,
                                true,
                                Some(occurrence_id),
                            )
                            .await?;
                        } else {
                            skipped_count += 1;
                            insert_trigger_event(
                                db,
                                pm_plan_id,
                                version_id,
                                "fixed",
                                Some(due_basis),
                                None,
                                None,
                                false,
                                None,
                            )
                            .await?;
                        }
                    }
                    trigger_events_recorded += 1;
                    due += interval;
                }
            }
            "floating" => {
                let interval = parse_interval(trigger_obj)?;
                let base_row = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT due_at, updated_at
                         FROM pm_occurrences
                         WHERE pm_plan_id = ? AND plan_version_id = ? AND status = 'completed'
                         ORDER BY updated_at DESC
                         LIMIT 1",
                        [pm_plan_id.into(), version_id.into()],
                    ))
                    .await?;
                let base_ts = if let Some(base_row) = base_row {
                    let completed_due_at: Option<String> = base_row.try_get("", "due_at")?;
                    completed_due_at.unwrap_or(base_row.try_get("", "updated_at")?)
                } else {
                    effective_from.clone()
                };
                let mut due = DateTime::parse_from_rfc3339(&base_ts)
                    .map_err(|_| AppError::ValidationFailed(vec!["floating trigger base timestamp is invalid.".to_string()]))?
                    .with_timezone(&Utc)
                    + interval;
                while due < as_of_dt {
                    due += interval;
                }
                if due <= horizon_end {
                    let due_at = due.format("%Y-%m-%dT%H:%M:%SZ").to_string();
                    let due_basis = format!("floating:{}", due_at);
                    let exists = occurrence_exists(db, pm_plan_id, version_id, &due_basis, Some(&due_at), None).await?;
                    if exists {
                        skipped_count += 1;
                        insert_trigger_event(
                            db,
                            pm_plan_id,
                            version_id,
                            "floating",
                            Some(due_basis),
                            None,
                            None,
                            false,
                            None,
                        )
                        .await?;
                    } else {
                        if let Some(occurrence_id) =
                            insert_occurrence(db, pm_plan_id, version_id, &due_basis, Some(due_at.clone()), None).await?
                        {
                            generated_count += 1;
                            occurrence_ids.push(occurrence_id);
                            insert_trigger_event(
                                db,
                                pm_plan_id,
                                version_id,
                                "floating",
                                Some(due_basis),
                                None,
                                None,
                                true,
                                Some(occurrence_id),
                            )
                            .await?;
                        } else {
                            skipped_count += 1;
                            insert_trigger_event(
                                db,
                                pm_plan_id,
                                version_id,
                                "floating",
                                Some(due_basis),
                                None,
                                None,
                                false,
                                None,
                            )
                            .await?;
                        }
                    }
                    trigger_events_recorded += 1;
                }
            }
        "meter" => {
                let legacy_meter_id = trigger_obj
                    .get("asset_meter_id")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let legacy_threshold = trigger_obj
                    .get("threshold_value")
                    .and_then(Value::as_f64)
                    .unwrap_or_default();

                let mut meter_id = legacy_meter_id;
                let mut threshold_value = legacy_threshold;
                let current_reading: f64;

                if meter_id > 0 && threshold_value > 0.0 {
                    let meter_row = db
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT current_reading FROM equipment_meters WHERE id = ? AND is_active = 1",
                            [meter_id.into()],
                        ))
                        .await?
                        .ok_or_else(|| {
                            AppError::ValidationFailed(vec![
                                "Meter trigger references unknown or inactive asset meter.".to_string(),
                            ])
                        })?;
                    current_reading = meter_row.try_get("", "current_reading")?;
                } else {
                    let meter_source = trigger_obj
                        .get("meter_source")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_lowercase();
                    let interval_value = trigger_obj
                        .get("interval_value")
                        .and_then(Value::as_f64)
                        .unwrap_or_default();
                    if !matches!(meter_source.as_str(), "odometer" | "operating_hours") || interval_value <= 0.0 {
                        return Err(AppError::ValidationFailed(vec![
                            "meter trigger requires either asset_meter_id/threshold_value or meter_source/interval_value > 0.".to_string(),
                        ]));
                    }

                    let meter_type_code = if meter_source == "odometer" { "DISTANCE" } else { "HOURS" };
                    let plan_scope_row = db
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT asset_scope_type, asset_scope_id FROM pm_plans WHERE id = ?",
                            [pm_plan_id.into()],
                        ))
                        .await?
                        .ok_or_else(|| AppError::NotFound {
                            entity: "pm_plan".to_string(),
                            id: pm_plan_id.to_string(),
                        })?;
                    let asset_scope_type: String = plan_scope_row.try_get("", "asset_scope_type")?;
                    let asset_scope_id: Option<i64> = plan_scope_row.try_get("", "asset_scope_id")?;
                    if asset_scope_type != "equipment" || asset_scope_id.unwrap_or_default() <= 0 {
                        return Err(AppError::ValidationFailed(vec![
                            "meter_source constructor requires PM plan asset scope type 'equipment' with a valid asset_scope_id.".to_string(),
                        ]));
                    }
                    let equipment_id = asset_scope_id.unwrap_or_default();
                    let meter_row = db
                        .query_one(Statement::from_sql_and_values(
                            DbBackend::Sqlite,
                            "SELECT id, current_reading
                             FROM equipment_meters
                             WHERE equipment_id = ? AND is_active = 1 AND UPPER(meter_type) = ?
                             ORDER BY is_primary DESC, id ASC
                             LIMIT 1",
                            [equipment_id.into(), meter_type_code.to_string().into()],
                        ))
                        .await?
                        .ok_or_else(|| {
                            AppError::ValidationFailed(vec![
                                format!(
                                    "No active {} meter found on equipment {} for meter-based PM trigger.",
                                    meter_type_code,
                                    equipment_id
                                ),
                            ])
                        })?;
                    meter_id = meter_row.try_get("", "id")?;
                    current_reading = meter_row.try_get("", "current_reading")?;
                    threshold_value = current_reading + interval_value;
                }

                let due_basis = format!("meter:{}:{:.4}", meter_id, threshold_value);
                let should_generate = current_reading >= threshold_value;
                let mut generated_occurrence_id: Option<i64> = None;
                if should_generate {
                    let exists = occurrence_exists(
                        db,
                        pm_plan_id,
                        version_id,
                        &due_basis,
                        None,
                        Some(threshold_value),
                    )
                    .await?;
                    if exists {
                        skipped_count += 1;
                    } else {
                        if let Some(occurrence_id) = insert_occurrence(
                            db,
                            pm_plan_id,
                            version_id,
                            &due_basis,
                            Some(as_of.clone()),
                            Some(threshold_value),
                        )
                        .await?
                        {
                            generated_count += 1;
                            occurrence_ids.push(occurrence_id);
                            generated_occurrence_id = Some(occurrence_id);
                        } else {
                            skipped_count += 1;
                        }
                    }
                }
                insert_trigger_event(
                    db,
                    pm_plan_id,
                    version_id,
                    "meter",
                    Some(format!("asset_meter:{}", meter_id)),
                    Some(current_reading),
                    Some(threshold_value),
                    generated_occurrence_id.is_some(),
                    generated_occurrence_id,
                )
                .await?;
                trigger_events_recorded += 1;
            }
            "event" => {
                let event_code = trigger_obj
                    .get("event_code")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_uppercase();
                if event_code.is_empty() {
                    return Err(AppError::ValidationFailed(vec![
                        "event trigger requires event_code.".to_string(),
                    ]));
                }
                let due_basis = format!("event:{}:{}", event_code, as_of_dt.format("%Y-%m-%d"));
                let mut generated_occurrence_id: Option<i64> = None;
                if event_codes.contains(&event_code) {
                    let exists = occurrence_exists(db, pm_plan_id, version_id, &due_basis, Some(&as_of), None).await?;
                    if exists {
                        skipped_count += 1;
                    } else {
                        if let Some(occurrence_id) =
                            insert_occurrence(db, pm_plan_id, version_id, &due_basis, Some(as_of.clone()), None).await?
                        {
                            generated_count += 1;
                            occurrence_ids.push(occurrence_id);
                            generated_occurrence_id = Some(occurrence_id);
                        } else {
                            skipped_count += 1;
                        }
                    }
                }
                insert_trigger_event(
                    db,
                    pm_plan_id,
                    version_id,
                    "event",
                    Some(event_code),
                    None,
                    None,
                    generated_occurrence_id.is_some(),
                    generated_occurrence_id,
                )
                .await?;
                trigger_events_recorded += 1;
            }
            "condition" => {
                let condition_code = trigger_obj
                    .get("condition_code")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_uppercase();
                if condition_code.is_empty() {
                    return Err(AppError::ValidationFailed(vec![
                        "condition trigger requires condition_code.".to_string(),
                    ]));
                }
                let due_basis = format!("condition:{}:{}", condition_code, as_of_dt.format("%Y-%m-%d"));
                let mut generated_occurrence_id: Option<i64> = None;
                if condition_codes.contains(&condition_code) {
                    let exists = occurrence_exists(db, pm_plan_id, version_id, &due_basis, Some(&as_of), None).await?;
                    if exists {
                        skipped_count += 1;
                    } else {
                        if let Some(occurrence_id) =
                            insert_occurrence(db, pm_plan_id, version_id, &due_basis, Some(as_of.clone()), None).await?
                        {
                            generated_count += 1;
                            occurrence_ids.push(occurrence_id);
                            generated_occurrence_id = Some(occurrence_id);
                        } else {
                            skipped_count += 1;
                        }
                    }
                }
                insert_trigger_event(
                    db,
                    pm_plan_id,
                    version_id,
                    "condition",
                    Some(condition_code),
                    None,
                    None,
                    generated_occurrence_id.is_some(),
                    generated_occurrence_id,
                )
                .await?;
                trigger_events_recorded += 1;
            }
            _ => {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Unsupported PM strategy type '{}'.",
                    strategy_type
                )]));
            }
        }
    }
    for occurrence_id in &occurrence_ids {
        let _ = emit_pm_due_event(db, *occurrence_id).await;
    }

    Ok(GeneratePmOccurrencesResult {
        generated_count,
        skipped_count,
        trigger_events_recorded,
        occurrence_ids,
    })
}

async fn resolve_preventive_work_order_type_id(
    db: &DatabaseConnection,
    explicit_type_id: Option<i64>,
) -> AppResult<i64> {
    if let Some(type_id) = explicit_type_id {
        return Ok(type_id);
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_types WHERE lower(code) = 'preventive' LIMIT 1",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "Unable to resolve work_order_types.preventive for PM work order creation.".to_string(),
            ])
        })?;
    let type_id: i64 = row.try_get("", "id")?;
    Ok(type_id)
}

pub async fn transition_pm_occurrence(
    db: &DatabaseConnection,
    input: TransitionPmOccurrenceInput,
) -> AppResult<PmOccurrence> {
    let current = get_pm_occurrence_by_id(db, input.occurrence_id).await?;
    if current.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "PM occurrence was modified elsewhere (stale row_version).".to_string(),
        ]));
    }

    let next_status = normalize_occurrence_status(&input.next_status)?;
    if !occurrence_transition_allowed(&current.status, &next_status) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Invalid PM occurrence transition: {} -> {}.",
            current.status, next_status
        )]));
    }

    if matches!(next_status.as_str(), "deferred" | "missed" | "cancelled")
        && input
            .reason_code
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
    {
        return Err(AppError::ValidationFailed(vec![
            "reason_code is required for deferred, missed, and cancelled transitions.".to_string(),
        ]));
    }

    let mut linked_work_order_id = current.linked_work_order_id;
    if input.generate_work_order == Some(true) && linked_work_order_id.is_none() {
        let wo_type_id = resolve_preventive_work_order_type_id(db, input.work_order_type_id).await?;
        let wo_title = format!(
            "PM {} occurrence #{}",
            current.plan_code.clone().unwrap_or_else(|| current.pm_plan_id.to_string()),
            current.id
        );
        let note_text = if let Some(note) = input.note.clone() {
            format!("PM occurrence {} | {}", current.id, note)
        } else {
            format!("PM occurrence {}", current.id)
        };
        let wo = wo_queries::create_work_order(
            db,
            WoCreateInput {
                type_id: wo_type_id,
                equipment_id: None,
                location_id: None,
                source_di_id: None,
                source_inspection_anomaly_id: None,
                source_ram_ishikawa_diagram_id: None,
                source_ishikawa_flow_node_id: None,
                source_rca_cause_text: None,
                entity_id: None,
                planner_id: None,
                urgency_id: None,
                title: wo_title,
                description: current.plan_title.clone(),
                notes: Some(note_text),
                planned_start: current.due_at.clone(),
                planned_end: None,
                shift: None,
                expected_duration_hours: None,
                creator_id: input.actor_id.unwrap_or(1),
                requires_permit: None,
            },
        )
        .await?;
        linked_work_order_id = Some(wo.id);
    }

    let deferral_reason = if next_status == "deferred" {
        input.reason_code.clone()
    } else {
        None
    };
    let missed_reason = if next_status == "missed" {
        input.reason_code.clone()
    } else {
        None
    };

    let update = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE pm_occurrences
             SET status = ?,
                 linked_work_order_id = ?,
                 deferral_reason = ?,
                 missed_reason = ?,
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                next_status.clone().into(),
                linked_work_order_id.into(),
                deferral_reason.into(),
                missed_reason.into(),
                input.occurrence_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;

    if update.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "PM occurrence transition failed due to stale row_version.".to_string(),
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO pm_occurrence_transitions (pm_occurrence_id, from_status, to_status, reason_code, note, actor_id)
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            input.occurrence_id.into(),
            current.status.into(),
            next_status.into(),
            input.reason_code.clone().into(),
            input.note.into(),
            input.actor_id.into(),
        ],
    ))
    .await?;

    let updated = get_pm_occurrence_by_id(db, input.occurrence_id).await?;
    if updated.status == "missed" {
        let _ = emit_pm_missed_event(db, &updated, input.reason_code.clone()).await;
    } else if updated.status == "deferred" {
        let _ = emit_pm_deferred_event(db, &updated, input.reason_code.clone()).await;
    }
    let _ = emit_pm_occurrence_activity(db, &updated, "pm.occurrence.transition", input.actor_id, input.reason_code).await;
    Ok(updated)
}








fn decode_pm_execution_row(row: &sea_orm::QueryResult) -> AppResult<PmExecution> {
    Ok(PmExecution {
        id: row.try_get("", "id")?,
        pm_occurrence_id: row.try_get("", "pm_occurrence_id")?,
        work_order_id: row.try_get("", "work_order_id")?,
        work_order_code: row.try_get("", "work_order_code")?,
        execution_result: row.try_get("", "execution_result")?,
        executed_at: row.try_get("", "executed_at")?,
        notes: row.try_get("", "notes")?,
        actor_id: row.try_get("", "actor_id")?,
        actual_duration_hours: row.try_get("", "actual_duration_hours")?,
        actual_labor_hours: row.try_get("", "actual_labor_hours")?,
        created_at: row.try_get("", "created_at")?,
    })
}

fn decode_pm_finding_row(row: &sea_orm::QueryResult) -> AppResult<PmFinding> {
    Ok(PmFinding {
        id: row.try_get("", "id")?,
        pm_execution_id: row.try_get("", "pm_execution_id")?,
        finding_type: row.try_get("", "finding_type")?,
        severity: row.try_get("", "severity")?,
        description: row.try_get("", "description")?,
        follow_up_di_id: row.try_get("", "follow_up_di_id")?,
        follow_up_work_order_id: row.try_get("", "follow_up_work_order_id")?,
        follow_up_di_code: row.try_get("", "follow_up_di_code")?,
        follow_up_work_order_code: row.try_get("", "follow_up_work_order_code")?,
        created_at: row.try_get("", "created_at")?,
    })
}

fn normalize_execution_result(value: &str) -> AppResult<String> {
    let normalized = value.trim().to_lowercase();
    match normalized.as_str() {
        "completed_no_findings" | "completed_with_findings" | "deferred" | "missed" | "cancelled" => {
            Ok(normalized)
        }
        _ => Err(AppError::ValidationFailed(vec![format!(
            "Unsupported PM execution result '{}'.",
            value
        )])),
    }
}

fn normalize_optional_reason(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_uppercase()).filter(|v| !v.is_empty())
}

fn json_array_has_items(raw: Option<&str>) -> bool {
    let Some(raw) = raw else { return false; };
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return true;
    };
    parsed
        .as_array()
        .map(|arr| !arr.is_empty())
        .unwrap_or(true)
}

fn rate_pct(numerator: i64, denominator: i64) -> Option<f64> {
    if denominator <= 0 {
        None
    } else {
        Some(((numerator as f64) * 100.0) / (denominator as f64))
    }
}

fn bounded_period(input_from: Option<&str>, input_to: Option<&str>) -> AppResult<(String, String)> {
    let now = Utc::now();
    let from = match input_from {
        Some(value) => DateTime::parse_from_rfc3339(value)
            .map_err(|_| AppError::ValidationFailed(vec!["from must be a valid RFC3339 timestamp.".to_string()]))?
            .with_timezone(&Utc),
        None => now - Duration::days(30),
    };
    let to = match input_to {
        Some(value) => DateTime::parse_from_rfc3339(value)
            .map_err(|_| AppError::ValidationFailed(vec!["to must be a valid RFC3339 timestamp.".to_string()]))?
            .with_timezone(&Utc),
        None => now,
    };
    if to < from {
        return Err(AppError::ValidationFailed(vec!["to must be greater than or equal to from.".to_string()]));
    }
    Ok((
        from.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        to.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    ))
}
async fn load_pm_routing_context(
    db: &DatabaseConnection,
    occurrence_id: i64,
) -> AppResult<(Option<i64>, Option<i64>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pp.assigned_group_id, e.installed_at_node_id
             FROM pm_occurrences po
             JOIN pm_plans pp ON pp.id = po.pm_plan_id
             LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment'
             WHERE po.id = ?",
            [occurrence_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_occurrence".to_string(),
            id: occurrence_id.to_string(),
        })?;

    let assigned_group_id: Option<i64> = row.try_get("", "assigned_group_id")?;
    let installed_at_node_id: Option<i64> = row.try_get("", "installed_at_node_id")?;
    Ok((assigned_group_id, installed_at_node_id))
}

async fn emit_pm_occurrence_activity(
    db: &DatabaseConnection,
    occurrence: &PmOccurrence,
    event_code: &str,
    actor_id: Option<i64>,
    reason_code: Option<String>,
) -> AppResult<()> {
    let summary_json = json!({
        "pm_occurrence_id": occurrence.id,
        "pm_plan_id": occurrence.pm_plan_id,
        "status": occurrence.status,
        "reason_code": reason_code,
        "linked_work_order_id": occurrence.linked_work_order_id,
    });

    let _ = emit_activity_event(
        db,
        ActivityEventInput {
            event_class: "operational".to_string(),
            event_code: event_code.to_string(),
            source_module: "pm".to_string(),
            source_record_type: Some("pm_occurrence".to_string()),
            source_record_id: Some(occurrence.id.to_string()),
            entity_scope_id: None,
            actor_id,
            severity: if occurrence.status == "missed" { "warning".to_string() } else { "info".to_string() },
            summary_json: Some(summary_json),
            correlation_id: Some(format!("pm-occ-{}", occurrence.id)),
            visibility_scope: "entity".to_string(),
        },
    )
    .await;

    Ok(())
}

async fn emit_pm_notification(
    db: &DatabaseConnection,
    occurrence: &PmOccurrence,
    event_code: &str,
    category_code: &str,
    severity: &str,
    dedupe_key: Option<String>,
    reason_code: Option<String>,
) -> AppResult<()> {
    let (assigned_group_id, org_node_id) = load_pm_routing_context(db, occurrence.id).await?;
    let payload = json!({
        "source_module": "pm",
        "source_record_id": occurrence.id,
        "pm_occurrence_id": occurrence.id,
        "pm_plan_id": occurrence.pm_plan_id,
        "pm_plan_code": occurrence.plan_code,
        "pm_plan_title": occurrence.plan_title,
        "pm_status": occurrence.status,
        "due_at": occurrence.due_at,
        "assigned_group_id": assigned_group_id,
        "org_node_id": org_node_id,
        "reason_code": reason_code,
    });

    let _ = emit_notification_event(
        db,
        NotificationEventInput {
            source_module: "pm".to_string(),
            source_record_id: Some(occurrence.id.to_string()),
            event_code: event_code.to_string(),
            category_code: category_code.to_string(),
            severity: severity.to_string(),
            dedupe_key,
            payload_json: Some(payload.to_string()),
            title: format!("PM {}", event_code),
            body: Some(format!(
                "PM occurrence {} for plan {} is {}.",
                occurrence.id,
                occurrence.plan_code.clone().unwrap_or_else(|| occurrence.pm_plan_id.to_string()),
                occurrence.status
            )),
            action_url: Some(format!("/pm?occurrence={}", occurrence.id)),
        },
    )
    .await;

    Ok(())
}

async fn emit_pm_due_event(db: &DatabaseConnection, occurrence_id: i64) -> AppResult<()> {
    let occurrence = get_pm_occurrence_by_id(db, occurrence_id).await?;
    emit_pm_notification(
        db,
        &occurrence,
        "pm.due",
        "pm_due",
        "info",
        Some(format!("pm-due-{}-{}", occurrence.id, occurrence.due_basis)),
        None,
    )
    .await?;
    emit_pm_occurrence_activity(db, &occurrence, "pm.occurrence.generated", None, None).await?;
    Ok(())
}

async fn emit_pm_missed_event(
    db: &DatabaseConnection,
    occurrence: &PmOccurrence,
    reason_code: Option<String>,
) -> AppResult<()> {
    emit_pm_notification(
        db,
        occurrence,
        "pm.missed",
        "pm_missed",
        "warning",
        Some(format!("pm-missed-open-{}", occurrence.id)),
        reason_code.clone(),
    )
    .await?;
    emit_pm_occurrence_activity(db, occurrence, "pm.occurrence.missed", None, reason_code).await?;
    Ok(())
}

async fn emit_pm_deferred_event(
    db: &DatabaseConnection,
    occurrence: &PmOccurrence,
    reason_code: Option<String>,
) -> AppResult<()> {
    emit_pm_notification(
        db,
        occurrence,
        "pm.deferred",
        "pm_deferred",
        "warning",
        Some(format!("pm-deferred-open-{}", occurrence.id)),
        reason_code.clone(),
    )
    .await?;
    emit_pm_occurrence_activity(db, occurrence, "pm.occurrence.deferred", None, reason_code).await?;
    Ok(())
}

async fn create_follow_up_di_from_finding(
    db: &DatabaseConnection,
    occurrence: &PmOccurrence,
    finding: &PmExecutionFindingInput,
    actor_id: i64,
) -> AppResult<i64> {
    let plan_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT asset_scope_type, asset_scope_id
             FROM pm_plans
             WHERE id = ?",
            [occurrence.pm_plan_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan".to_string(),
            id: occurrence.pm_plan_id.to_string(),
        })?;

    let asset_scope_type: String = plan_row.try_get("", "asset_scope_type")?;
    let asset_scope_id: Option<i64> = plan_row.try_get("", "asset_scope_id")?;
    if asset_scope_type != "equipment" || asset_scope_id.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Follow-up DI creation requires PM plan asset_scope_type = 'equipment' with asset_scope_id.".to_string(),
        ]));
    }

    let equipment_id = asset_scope_id.expect("validated equipment id");
    let node_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["PM plan equipment not found for follow-up DI.".to_string()]))?;
    let org_node_id: Option<i64> = node_row.try_get("", "installed_at_node_id")?;
    let Some(org_node_id) = org_node_id else {
        return Err(AppError::ValidationFailed(vec![
            "PM plan equipment must have installed_at_node_id for follow-up DI routing.".to_string(),
        ]));
    };

    let urgency = match finding.severity.as_deref().unwrap_or("medium").to_lowercase().as_str() {
        "critical" => "critical",
        "high" => "high",
        "low" => "low",
        _ => "medium",
    };

    let impact = match urgency {
        "critical" => "critical",
        "high" => "major",
        "low" => "minor",
        _ => "minor",
    };

    let di = create_intervention_request(
        db,
        DiCreateInput {
            asset_id: equipment_id,
            org_node_id,
            title: format!(
                "PM finding follow-up [{}] {}",
                occurrence.plan_code.clone().unwrap_or_else(|| occurrence.pm_plan_id.to_string()),
                finding.finding_type
            ),
            description: finding.description.clone(),
            origin_type: "pm".to_string(),
            symptom_code_id: None,
            impact_level: impact.to_string(),
            production_impact: false,
            safety_flag: urgency == "critical",
            environmental_flag: false,
            quality_flag: false,
            reported_urgency: urgency.to_string(),
            observed_at: Some(now_rfc3339()),
            source_inspection_anomaly_id: None,
            submitter_id: actor_id,
        },
    )
    .await?;

    Ok(di.id)
}

async fn create_follow_up_wo_from_finding(
    db: &DatabaseConnection,
    occurrence: &PmOccurrence,
    finding: &PmExecutionFindingInput,
    actor_id: i64,
    source_di_id: Option<i64>,
) -> AppResult<i64> {
    let plan_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT asset_scope_type, asset_scope_id
             FROM pm_plans
             WHERE id = ?",
            [occurrence.pm_plan_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan".to_string(),
            id: occurrence.pm_plan_id.to_string(),
        })?;
    let asset_scope_type: String = plan_row.try_get("", "asset_scope_type")?;
    let asset_scope_id: Option<i64> = plan_row.try_get("", "asset_scope_id")?;
    let wo_type_id = resolve_preventive_work_order_type_id(db, finding.follow_up_work_order_type_id).await?;

    let wo = wo_queries::create_work_order(
        db,
        WoCreateInput {
            type_id: wo_type_id,
            equipment_id: if asset_scope_type == "equipment" { asset_scope_id } else { None },
            location_id: None,
            source_di_id,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_id: None,
            title: format!(
                "PM follow-up [{}] {}",
                occurrence.plan_code.clone().unwrap_or_else(|| occurrence.pm_plan_id.to_string()),
                finding.finding_type
            ),
            description: Some(finding.description.clone()),
            notes: Some(format!(
                "Origin PM occurrence {} / plan {}",
                occurrence.id,
                occurrence.plan_code.clone().unwrap_or_else(|| occurrence.pm_plan_id.to_string())
            )),
            planned_start: occurrence.due_at.clone(),
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor_id,
            requires_permit: None,
        },
    )
    .await?;

    Ok(wo.id)
}

pub async fn list_pm_executions(db: &DatabaseConnection, filter: PmExecutionFilter) -> AppResult<Vec<PmExecution>> {
    let mut sql = String::from(
        "SELECT pe.id, pe.pm_occurrence_id, pe.work_order_id, wo.code AS work_order_code, pe.execution_result,
                pe.executed_at, pe.notes, pe.actor_id, pe.actual_duration_hours, pe.actual_labor_hours, pe.created_at
         FROM pm_executions pe
         LEFT JOIN work_orders wo ON wo.id = pe.work_order_id
         LEFT JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id
         WHERE 1 = 1",
    );
    let mut binds: Vec<sea_orm::Value> = Vec::new();

    if let Some(occurrence_id) = filter.occurrence_id {
        sql.push_str(" AND pe.pm_occurrence_id = ?");
        binds.push(occurrence_id.into());
    }
    if let Some(pm_plan_id) = filter.pm_plan_id {
        sql.push_str(" AND po.pm_plan_id = ?");
        binds.push(pm_plan_id.into());
    }

    sql.push_str(" ORDER BY pe.executed_at DESC, pe.id DESC");

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;
    rows.iter().map(decode_pm_execution_row).collect()
}

pub async fn list_pm_findings(db: &DatabaseConnection, execution_id: i64) -> AppResult<Vec<PmFinding>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pf.id, pf.pm_execution_id, pf.finding_type, pf.severity, pf.description,
                    pf.follow_up_di_id, pf.follow_up_work_order_id,
                    di.code AS follow_up_di_code,
                    wo.code AS follow_up_work_order_code,
                    pf.created_at
             FROM pm_findings pf
             LEFT JOIN intervention_requests di ON di.id = pf.follow_up_di_id
             LEFT JOIN work_orders wo ON wo.id = pf.follow_up_work_order_id
             WHERE pf.pm_execution_id = ?
             ORDER BY pf.id ASC",
            [execution_id.into()],
        ))
        .await?;
    rows.iter().map(decode_pm_finding_row).collect()
}

pub async fn list_pm_recurring_findings(
    db: &DatabaseConnection,
    input: PmRecurringFindingsInput,
) -> AppResult<Vec<PmRecurringFinding>> {
    let days_window = input.days_window.unwrap_or(90).clamp(1, 3650);
    let min_occurrences = input.min_occurrences.unwrap_or(2).max(2);
    let threshold = (Utc::now() - Duration::days(days_window)).format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut sql = String::from(
        "SELECT po.pm_plan_id,
                pp.code AS plan_code,
                pf.finding_type,
                COUNT(*) AS occurrence_count,
                MIN(pe.executed_at) AS first_seen_at,
                MAX(pe.executed_at) AS last_seen_at,
                MAX(COALESCE(pf.severity, '')) AS latest_severity
         FROM pm_findings pf
         JOIN pm_executions pe ON pe.id = pf.pm_execution_id
         JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         WHERE pe.executed_at >= ?",
    );

    let mut binds: Vec<sea_orm::Value> = vec![threshold.into()];
    if let Some(pm_plan_id) = input.pm_plan_id {
        sql.push_str(" AND po.pm_plan_id = ?");
        binds.push(pm_plan_id.into());
    }

    sql.push_str(
        " GROUP BY po.pm_plan_id, pp.code, pf.finding_type
          HAVING COUNT(*) >= ?
          ORDER BY occurrence_count DESC, last_seen_at DESC",
    );
    binds.push(min_occurrences.into());

    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let mut out: Vec<PmRecurringFinding> = Vec::new();
    for row in rows {
        let latest_severity = row.try_get::<String>("", "latest_severity")?;
        out.push(PmRecurringFinding {
            pm_plan_id: row.try_get("", "pm_plan_id")?,
            plan_code: row.try_get("", "plan_code")?,
            finding_type: row.try_get("", "finding_type")?,
            occurrence_count: row.try_get("", "occurrence_count")?,
            first_seen_at: row.try_get("", "first_seen_at")?,
            last_seen_at: row.try_get("", "last_seen_at")?,
            latest_severity: if latest_severity.is_empty() { None } else { Some(latest_severity) },
        });
    }

    Ok(out)
}

pub async fn execute_pm_occurrence(
    db: &DatabaseConnection,
    input: ExecutePmOccurrenceInput,
) -> AppResult<ExecutePmOccurrenceResult> {
    let execution_result = normalize_execution_result(&input.execution_result)?;
    let findings_input = input.findings.clone().unwrap_or_default();

    if execution_result == "completed_no_findings" && !findings_input.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "completed_no_findings cannot include findings payload.".to_string(),
        ]));
    }
    if execution_result == "completed_with_findings" && findings_input.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "completed_with_findings requires at least one finding.".to_string(),
        ]));
    }

    let defer_reason = normalize_optional_reason(input.defer_reason_code.clone());
    let miss_reason = normalize_optional_reason(input.miss_reason_code.clone());

    if execution_result == "deferred" && defer_reason.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "defer_reason_code is required for deferred execution outcomes.".to_string(),
        ]));
    }
    if execution_result == "missed" && miss_reason.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "miss_reason_code is required for missed execution outcomes.".to_string(),
        ]));
    }

    let next_status = match execution_result.as_str() {
        "completed_no_findings" | "completed_with_findings" => "completed".to_string(),
        "deferred" => "deferred".to_string(),
        "missed" => "missed".to_string(),
        "cancelled" => "cancelled".to_string(),
        _ => unreachable!(),
    };

    let occurrence = transition_pm_occurrence(
        db,
        TransitionPmOccurrenceInput {
            occurrence_id: input.occurrence_id,
            expected_row_version: input.expected_occurrence_row_version,
            next_status,
            reason_code: defer_reason.clone().or(miss_reason.clone()),
            note: input.note.clone(),
            generate_work_order: Some(false),
            work_order_type_id: None,
            actor_id: input.actor_id,
        },
    )
    .await?;

    let actor_id = input.actor_id.unwrap_or(1);
    let mut work_order_id = input.work_order_id.or(occurrence.linked_work_order_id);
    if let (Some(linked), Some(provided)) = (occurrence.linked_work_order_id, input.work_order_id) {
        if linked != provided {
            return Err(AppError::ValidationFailed(vec![
                "work_order_id must match linked occurrence work order when one already exists.".to_string(),
            ]));
        }
    }

    let mut work_order_code: Option<String> = None;
    let mut actual_duration_hours: Option<f64> = None;
    let mut actual_labor_hours: Option<f64> = None;

    if let Some(wo_id) = work_order_id {
        let wo_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, code, actual_duration_hours, active_labor_hours
                 FROM work_orders
                 WHERE id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::ValidationFailed(vec!["work_order_id does not exist.".to_string()]))?;
        work_order_code = wo_row.try_get("", "code")?;
        actual_duration_hours = wo_row.try_get("", "actual_duration_hours")?;
        actual_labor_hours = wo_row.try_get("", "active_labor_hours")?;
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO pm_executions
            (pm_occurrence_id, work_order_id, execution_result, executed_at, completed_by_id, notes, actor_id, actual_duration_hours, actual_labor_hours, created_at)
         VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?)",
        [
            occurrence.id.into(),
            work_order_id.into(),
            execution_result.clone().into(),
            now_rfc3339().into(),
            input.note.clone().into(),
            actor_id.into(),
            actual_duration_hours.into(),
            actual_labor_hours.into(),
            now_rfc3339().into(),
        ],
    ))
    .await?;

    let execution_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pe.id, pe.pm_occurrence_id, pe.work_order_id, wo.code AS work_order_code, pe.execution_result,
                    pe.executed_at, pe.notes, pe.actor_id, pe.actual_duration_hours, pe.actual_labor_hours, pe.created_at
             FROM pm_executions pe
             LEFT JOIN work_orders wo ON wo.id = pe.work_order_id
             WHERE pe.rowid = last_insert_rowid()",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to read inserted PM execution")))?;
    let execution = decode_pm_execution_row(&execution_row)?;

    for finding in findings_input {
        let finding_type = finding.finding_type.trim().to_uppercase();
        if finding_type.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "finding_type is required for each PM finding.".to_string(),
            ]));
        }
        let description = finding.description.trim().to_string();
        if description.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "description is required for each PM finding.".to_string(),
            ]));
        }

        let mut follow_up_di_id: Option<i64> = None;
        let mut follow_up_wo_id: Option<i64> = None;

        if finding.create_follow_up_di == Some(true) {
            follow_up_di_id = Some(create_follow_up_di_from_finding(db, &occurrence, &finding, actor_id).await?);
        }

        if finding.create_follow_up_work_order == Some(true) {
            follow_up_wo_id = Some(
                create_follow_up_wo_from_finding(db, &occurrence, &finding, actor_id, follow_up_di_id).await?,
            );
        }

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO pm_findings
                (pm_execution_id, finding_type, severity, description, follow_up_di_id, follow_up_work_order_id)
             VALUES (?, ?, ?, ?, ?, ?)",
            [
                execution.id.into(),
                finding_type.into(),
                finding.severity.clone().map(|v| v.to_lowercase()).into(),
                description.into(),
                follow_up_di_id.into(),
                follow_up_wo_id.into(),
            ],
        ))
        .await?;

        if follow_up_di_id.is_some() || follow_up_wo_id.is_some() {
            let _ = emit_pm_notification(
                db,
                &occurrence,
                "pm.follow_up_created",
                "pm_follow_up_created",
                "info",
                None,
                None,
            )
            .await;
            let _ = emit_pm_occurrence_activity(
                db,
                &occurrence,
                "pm.finding.follow_up_created",
                Some(actor_id),
                None,
            )
            .await;
        }
    }

    if execution_result == "deferred" {
        let _ = emit_pm_deferred_event(db, &occurrence, defer_reason).await;
    } else if execution_result == "missed" {
        let _ = emit_pm_missed_event(db, &occurrence, miss_reason).await;
    }

    let findings = list_pm_findings(db, execution.id).await?;

    if work_order_code.is_some() && work_order_id.is_none() {
        work_order_id = occurrence.linked_work_order_id;
    }

    Ok(ExecutePmOccurrenceResult {
        occurrence,
        execution: PmExecution {
            work_order_id,
            work_order_code,
            actual_duration_hours,
            actual_labor_hours,
            ..execution
        },
        findings,
    })
}
