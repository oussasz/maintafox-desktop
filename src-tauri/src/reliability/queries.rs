use chrono::{Duration, SecondsFormat, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::reliability::analysis_input::{
    build_input_spec_json, dataset_hash_sha256, ExposurePart, FailurePart,
};
use crate::reliability::compute::{compute_reliability_kpis, KpiFailureEvent, ReliabilityKpiComputeInput};
use crate::reliability::domain::{
    CostOfFailureFilter, CostOfFailureRow, DeactivateFailureCodeInput, FailureCode, FailureCodeUpsertInput,
    FailureCodesFilter, FailureEvent, FailureEventsFilter, FailureHierarchy, FailureHierarchyUpsertInput,
    DismissRamDataQualityIssueInput, EquipmentMissingExposureRow, Iso14224DatasetCompleteness, RamDataQualityIssue,
    RamDataQualityIssuesFilter, RamEquipmentQualityBadge, RefreshReliabilityKpiSnapshotInput,
    ReliabilityAnalysisInputEvaluation, ReliabilityKpiSnapshot, ReliabilityKpiSnapshotsFilter, RuntimeExposureLog,
    RuntimeExposureLogsFilter, UpsertFailureEventInput, UpsertRuntimeExposureLogInput, UserDismissal,
    WoMissingFailureModeRow,
};
use crate::reliability::sync_stage::{
    stage_failure_code, stage_failure_event, stage_failure_hierarchy, stage_reliability_kpi_snapshot,
    stage_runtime_exposure_log, stage_user_dismissal,
};

const CODE_TYPES: &[&str] = &["class", "mode", "mechanism", "cause", "effect", "remedy"];

const WO_FAILURE_SOURCE: &str = "work_order";
const INGEST_WO_TYPE_CODES: &[&str] = &["corrective", "emergency"];

const EXPOSURE_TYPES: &[&str] = &["hours", "cycles", "output_distance", "production_output"];
const RUNTIME_SOURCE_TYPES: &[&str] = &["meter_reading", "iot_counter", "manual", "calendar_operating_schedule"];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode reliability field '{field}': {err}"))
}

fn i64_to_bool(v: i64) -> bool {
    v != 0
}

fn bool_to_i64(b: bool) -> i64 {
    i64::from(b)
}

fn map_hierarchy(row: &sea_orm::QueryResult) -> AppResult<FailureHierarchy> {
    Ok(FailureHierarchy {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        name: row.try_get("", "name").map_err(|e| decode_err("name", e))?,
        asset_scope_json: row.try_get("", "asset_scope").map_err(|e| decode_err("asset_scope", e))?,
        version_no: row.try_get("", "version_no").map_err(|e| decode_err("version_no", e))?,
        is_active: i64_to_bool(
            row.try_get::<i64>("", "is_active")
                .map_err(|e| decode_err("is_active", e))?,
        ),
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_code(row: &sea_orm::QueryResult) -> AppResult<FailureCode> {
    Ok(FailureCode {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        hierarchy_id: row.try_get("", "hierarchy_id").map_err(|e| decode_err("hierarchy_id", e))?,
        parent_id: row
            .try_get::<Option<i64>>("", "parent_id")
            .map_err(|e| decode_err("parent_id", e))?,
        code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        label: row.try_get("", "label").map_err(|e| decode_err("label", e))?,
        code_type: row.try_get("", "code_type").map_err(|e| decode_err("code_type", e))?,
        iso_14224_annex_ref: row
            .try_get::<Option<String>>("", "iso_14224_annex_ref")
            .map_err(|e| decode_err("iso_14224_annex_ref", e))?,
        is_active: i64_to_bool(
            row.try_get::<i64>("", "is_active")
                .map_err(|e| decode_err("is_active", e))?,
        ),
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

fn opt_string(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn validate_code_type(t: &str) -> AppResult<()> {
    if CODE_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "code_type must be one of: {CODE_TYPES:?}"
        )]))
    }
}

fn validate_asset_scope_json(raw: &str) -> AppResult<()> {
    serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|e| AppError::ValidationFailed(vec![format!("asset_scope JSON: {e}")]))?;
    Ok(())
}

async fn last_insert_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("last_insert_rowid missing.".into()))?;
    Ok(row.try_get("", "id").map_err(|e| decode_err("id", e))?)
}

pub async fn list_failure_hierarchies(db: &DatabaseConnection) -> AppResult<Vec<FailureHierarchy>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, name, asset_scope, version_no, is_active, row_version
             FROM failure_hierarchies ORDER BY id ASC"
                .to_string(),
        ))
        .await?;
    rows.iter().map(map_hierarchy).collect()
}

pub async fn list_failure_codes(db: &DatabaseConnection, filter: FailureCodesFilter) -> AppResult<Vec<FailureCode>> {
    let inc = i64::from(filter.include_inactive.unwrap_or(false));
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, hierarchy_id, parent_id, code, label, code_type,
                    iso_14224_annex_ref, is_active, row_version
             FROM failure_codes
             WHERE hierarchy_id = ? AND (is_active = 1 OR ? = 1)
             ORDER BY code ASC",
            [filter.hierarchy_id.into(), inc.into()],
        ))
        .await?;
    rows.iter().map(map_code).collect()
}

pub async fn upsert_failure_hierarchy(
    db: &DatabaseConnection,
    input: FailureHierarchyUpsertInput,
) -> AppResult<FailureHierarchy> {
    validate_asset_scope_json(&input.asset_scope_json)?;
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::ValidationFailed(vec!["name required.".into()]));
    }

    let row = if let Some(id) = input.id {
        let exp = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required.".into()]))?;
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE failure_hierarchies SET name = ?, asset_scope = ?, version_no = ?, is_active = ?,
                 row_version = row_version + 1
                 WHERE id = ? AND row_version = ?",
                [
                    name.clone().into(),
                    input.asset_scope_json.clone().into(),
                    input.version_no.into(),
                    bool_to_i64(input.is_active).into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec![
                "failure_hierarchies update conflict or not found.".into(),
            ]));
        }
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, name, asset_scope, version_no, is_active, row_version
             FROM failure_hierarchies WHERE id = ?",
            [id.into()],
        ))
        .await?
    } else {
        let eid = format!("failure_hierarchy:{}", Uuid::new_v4());
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO failure_hierarchies (entity_sync_id, name, asset_scope, version_no, is_active, row_version)
             VALUES (?, ?, ?, ?, ?, 1)",
            [
                eid.into(),
                name.into(),
                input.asset_scope_json.into(),
                input.version_no.into(),
                bool_to_i64(input.is_active).into(),
            ],
        ))
        .await?;
        let new_id = last_insert_id(db).await?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, name, asset_scope, version_no, is_active, row_version
             FROM failure_hierarchies WHERE id = ?",
            [new_id.into()],
        ))
        .await?
    }
    .ok_or_else(|| AppError::SyncError("failure_hierarchies row missing after upsert.".into()))?;

    let mapped = map_hierarchy(&row)?;
    stage_failure_hierarchy(db, &mapped).await?;
    Ok(mapped)
}

async fn verify_parent_hierarchy(
    db: &impl ConnectionTrait,
    hierarchy_id: i64,
    parent_id: Option<i64>,
) -> AppResult<()> {
    if let Some(pid) = parent_id {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT hierarchy_id FROM failure_codes WHERE id = ?",
                [pid.into()],
            ))
            .await?
            .ok_or_else(|| AppError::ValidationFailed(vec!["parent_id not found.".into()]))?;
        let hid: i64 = row.try_get("", "hierarchy_id").map_err(|e| decode_err("hierarchy_id", e))?;
        if hid != hierarchy_id {
            return Err(AppError::ValidationFailed(vec![
                "parent_id must belong to the same hierarchy.".into(),
            ]));
        }
    }
    Ok(())
}

pub async fn upsert_failure_code(db: &DatabaseConnection, input: FailureCodeUpsertInput) -> AppResult<FailureCode> {
    let ct = input.code_type.trim().to_string();
    validate_code_type(&ct)?;
    verify_parent_hierarchy(db, input.hierarchy_id, input.parent_id).await?;

    let code = input.code.trim().to_string();
    let label = input.label.trim().to_string();
    if code.is_empty() || label.is_empty() {
        return Err(AppError::ValidationFailed(vec!["code and label required.".into()]));
    }

    let row = if let Some(id) = input.id {
        let exp = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required.".into()]))?;
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE failure_codes SET hierarchy_id = ?, parent_id = ?, code = ?, label = ?, code_type = ?,
                 iso_14224_annex_ref = ?, is_active = ?, row_version = row_version + 1
                 WHERE id = ? AND row_version = ?",
                [
                    input.hierarchy_id.into(),
                    opt_i64(input.parent_id),
                    code.clone().into(),
                    label.into(),
                    ct.clone().into(),
                    opt_string(input.iso_14224_annex_ref.clone()),
                    bool_to_i64(input.is_active).into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec![
                "failure_codes update conflict or not found.".into(),
            ]));
        }
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, hierarchy_id, parent_id, code, label, code_type,
                    iso_14224_annex_ref, is_active, row_version
             FROM failure_codes WHERE id = ?",
            [id.into()],
        ))
        .await?
    } else {
        let eid = format!("failure_code:{}", Uuid::new_v4());
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO failure_codes (
                entity_sync_id, hierarchy_id, parent_id, code, label, code_type, iso_14224_annex_ref, is_active, row_version
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                eid.into(),
                input.hierarchy_id.into(),
                opt_i64(input.parent_id),
                code.into(),
                label.into(),
                ct.into(),
                opt_string(input.iso_14224_annex_ref),
                bool_to_i64(input.is_active).into(),
            ],
        ))
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE") {
                AppError::ValidationFailed(vec!["duplicate (hierarchy_id, code).".into()])
            } else {
                AppError::SyncError(msg)
            }
        })?;
        let new_id = last_insert_id(db).await?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, hierarchy_id, parent_id, code, label, code_type,
                    iso_14224_annex_ref, is_active, row_version
             FROM failure_codes WHERE id = ?",
            [new_id.into()],
        ))
        .await?
    }
    .ok_or_else(|| AppError::SyncError("failure_codes row missing after upsert.".into()))?;

    let mapped = map_code(&row)?;
    stage_failure_code(db, &mapped).await?;
    Ok(mapped)
}

pub async fn deactivate_failure_code(db: &DatabaseConnection, input: DeactivateFailureCodeInput) -> AppResult<FailureCode> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, hierarchy_id, parent_id, code, label, code_type,
                    iso_14224_annex_ref, is_active, row_version
             FROM failure_codes WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "FailureCode".into(),
            id: input.id.to_string(),
        })?;

    let current: FailureCode = map_code(&row)?;
    if current.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec!["expected_row_version mismatch.".into()]));
    }

    if !current.is_active {
        return Ok(current);
    }

    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE failure_codes SET is_active = 0, row_version = row_version + 1
             WHERE id = ? AND row_version = ?",
            [input.id.into(), input.expected_row_version.into()],
        ))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["deactivate conflict.".into()]));
    }

    let updated = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, hierarchy_id, parent_id, code, label, code_type,
                    iso_14224_annex_ref, is_active, row_version
             FROM failure_codes WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("failure_codes missing after deactivate.".into()))?;
    let mapped = map_code(&updated)?;
    stage_failure_code(db, &mapped).await?;
    Ok(mapped)
}

fn utc_now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn map_failure_event(row: &sea_orm::QueryResult) -> AppResult<FailureEvent> {
    Ok(FailureEvent {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        source_type: row.try_get("", "source_type").map_err(|e| decode_err("source_type", e))?,
        source_id: row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        component_id: row
            .try_get::<Option<i64>>("", "component_id")
            .map_err(|e| decode_err("component_id", e))?,
        detected_at: row
            .try_get::<Option<String>>("", "detected_at")
            .map_err(|e| decode_err("detected_at", e))?,
        failed_at: row
            .try_get::<Option<String>>("", "failed_at")
            .map_err(|e| decode_err("failed_at", e))?,
        restored_at: row
            .try_get::<Option<String>>("", "restored_at")
            .map_err(|e| decode_err("restored_at", e))?,
        downtime_duration_hours: row
            .try_get("", "downtime_duration_hours")
            .map_err(|e| decode_err("downtime_duration_hours", e))?,
        active_repair_hours: row
            .try_get("", "active_repair_hours")
            .map_err(|e| decode_err("active_repair_hours", e))?,
        waiting_hours: row.try_get("", "waiting_hours").map_err(|e| decode_err("waiting_hours", e))?,
        is_planned: i64_to_bool(
            row.try_get::<i64>("", "is_planned")
                .map_err(|e| decode_err("is_planned", e))?,
        ),
        failure_class_id: row
            .try_get::<Option<i64>>("", "failure_class_id")
            .map_err(|e| decode_err("failure_class_id", e))?,
        failure_mode_id: row
            .try_get::<Option<i64>>("", "failure_mode_id")
            .map_err(|e| decode_err("failure_mode_id", e))?,
        failure_cause_id: row
            .try_get::<Option<i64>>("", "failure_cause_id")
            .map_err(|e| decode_err("failure_cause_id", e))?,
        failure_effect_id: row
            .try_get::<Option<i64>>("", "failure_effect_id")
            .map_err(|e| decode_err("failure_effect_id", e))?,
        failure_mechanism_id: row
            .try_get::<Option<i64>>("", "failure_mechanism_id")
            .map_err(|e| decode_err("failure_mechanism_id", e))?,
        cause_not_determined: i64_to_bool(
            row.try_get::<i64>("", "cause_not_determined")
                .map_err(|e| decode_err("cause_not_determined", e))?,
        ),
        production_impact_level: row
            .try_get::<Option<i64>>("", "production_impact_level")
            .map_err(|e| decode_err("production_impact_level", e))?,
        safety_impact_level: row
            .try_get::<Option<i64>>("", "safety_impact_level")
            .map_err(|e| decode_err("safety_impact_level", e))?,
        recorded_by_id: row
            .try_get::<Option<i64>>("", "recorded_by_id")
            .map_err(|e| decode_err("recorded_by_id", e))?,
        verification_status: row
            .try_get("", "verification_status")
            .map_err(|e| decode_err("verification_status", e))?,
        eligible_flags_json: row
            .try_get("", "eligible_flags_json")
            .map_err(|e| decode_err("eligible_flags_json", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_cost_of_failure(row: &sea_orm::QueryResult) -> AppResult<CostOfFailureRow> {
    Ok(CostOfFailureRow {
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        period: row.try_get("", "period").map_err(|e| decode_err("period", e))?,
        total_downtime_cost: row
            .try_get("", "total_downtime_cost")
            .map_err(|e| decode_err("total_downtime_cost", e))?,
        total_corrective_cost: row
            .try_get("", "total_corrective_cost")
            .map_err(|e| decode_err("total_corrective_cost", e))?,
        currency_code: row.try_get("", "currency_code").map_err(|e| decode_err("currency_code", e))?,
    })
}

async fn assert_equipment_exists(db: &DatabaseConnection, equipment_id: i64) -> AppResult<()> {
    let n: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM equipment WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("equipment count missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["equipment_id not found.".into()]));
    }
    Ok(())
}

pub async fn list_cost_of_failure(db: &DatabaseConnection, filter: CostOfFailureFilter) -> AppResult<Vec<CostOfFailureRow>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 500);
    let mut sql =
        String::from("SELECT equipment_id, period, total_downtime_cost, total_corrective_cost, currency_code FROM v_cost_of_failure WHERE 1 = 1");
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        values.push(eid.into());
    }
    if let Some(ref p) = filter.period {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            sql.push_str(" AND period = ?");
            values.push(trimmed.to_string().into());
        }
    }
    sql.push_str(" ORDER BY period DESC, equipment_id ASC LIMIT ?");
    values.push(limit.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_cost_of_failure).collect()
}

pub async fn upsert_failure_event(db: &DatabaseConnection, input: UpsertFailureEventInput) -> AppResult<FailureEvent> {
    serde_json::from_str::<serde_json::Value>(&input.eligible_flags_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("eligible_flags_json: {e}")]))?;
    let st = input.source_type.trim().to_string();
    if st.is_empty() {
        return Err(AppError::ValidationFailed(vec!["source_type required.".into()]));
    }
    let mut vs = input.verification_status.trim().to_string();
    if vs.is_empty() {
        vs = "recorded".to_string();
    }
    assert_equipment_exists(db, input.equipment_id).await?;

    let row = if let Some(id) = input.id {
        let exp = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required.".into()]))?;
        let now = utc_now_rfc3339();
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE failure_events SET source_type = ?, source_id = ?, equipment_id = ?, component_id = ?,
                 detected_at = ?, failed_at = ?, restored_at = ?,
                 downtime_duration_hours = ?, active_repair_hours = ?, waiting_hours = ?,
                 is_planned = ?,
                 failure_class_id = ?, failure_mode_id = ?, failure_cause_id = ?, failure_effect_id = ?, failure_mechanism_id = ?,
                 cause_not_determined = ?, production_impact_level = ?, safety_impact_level = ?,
                 recorded_by_id = ?, verification_status = ?, eligible_flags_json = ?,
                 row_version = row_version + 1, updated_at = ?
                 WHERE id = ? AND row_version = ?",
                [
                    st.clone().into(),
                    input.source_id.into(),
                    input.equipment_id.into(),
                    opt_i64(input.component_id),
                    opt_string(input.detected_at.clone()),
                    opt_string(input.failed_at.clone()),
                    opt_string(input.restored_at.clone()),
                    input.downtime_duration_hours.into(),
                    input.active_repair_hours.into(),
                    input.waiting_hours.into(),
                    bool_to_i64(input.is_planned).into(),
                    opt_i64(input.failure_class_id),
                    opt_i64(input.failure_mode_id),
                    opt_i64(input.failure_cause_id),
                    opt_i64(input.failure_effect_id),
                    opt_i64(input.failure_mechanism_id),
                    bool_to_i64(input.cause_not_determined).into(),
                    opt_i64(input.production_impact_level),
                    opt_i64(input.safety_impact_level),
                    opt_i64(input.recorded_by_id),
                    vs.clone().into(),
                    input.eligible_flags_json.clone().into(),
                    now.into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec![
                "failure_events update conflict or not found.".into(),
            ]));
        }
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, source_type, source_id, equipment_id, component_id,
                    detected_at, failed_at, restored_at,
                    downtime_duration_hours, active_repair_hours, waiting_hours,
                    is_planned, failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, failure_mechanism_id,
                    cause_not_determined, production_impact_level, safety_impact_level,
                    recorded_by_id, verification_status, eligible_flags_json,
                    row_version, created_at, updated_at
             FROM failure_events WHERE id = ?",
            [id.into()],
        ))
        .await?
    } else {
        let eid = format!("failure_event:{}", Uuid::new_v4());
        let ts = utc_now_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO failure_events (
                entity_sync_id, source_type, source_id, equipment_id, component_id,
                detected_at, failed_at, restored_at,
                downtime_duration_hours, active_repair_hours, waiting_hours,
                is_planned,
                failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, failure_mechanism_id,
                cause_not_determined, production_impact_level, safety_impact_level,
                recorded_by_id, verification_status, eligible_flags_json,
                row_version, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
            [
                eid.into(),
                st.into(),
                input.source_id.into(),
                input.equipment_id.into(),
                opt_i64(input.component_id),
                opt_string(input.detected_at),
                opt_string(input.failed_at),
                opt_string(input.restored_at),
                input.downtime_duration_hours.into(),
                input.active_repair_hours.into(),
                input.waiting_hours.into(),
                bool_to_i64(input.is_planned).into(),
                opt_i64(input.failure_class_id),
                opt_i64(input.failure_mode_id),
                opt_i64(input.failure_cause_id),
                opt_i64(input.failure_effect_id),
                opt_i64(input.failure_mechanism_id),
                bool_to_i64(input.cause_not_determined).into(),
                opt_i64(input.production_impact_level),
                opt_i64(input.safety_impact_level),
                opt_i64(input.recorded_by_id),
                vs.into(),
                input.eligible_flags_json.into(),
                ts.clone().into(),
                ts.into(),
            ],
        ))
        .await?;
        let new_id = last_insert_id(db).await?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, source_type, source_id, equipment_id, component_id,
                    detected_at, failed_at, restored_at,
                    downtime_duration_hours, active_repair_hours, waiting_hours,
                    is_planned, failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, failure_mechanism_id,
                    cause_not_determined, production_impact_level, safety_impact_level,
                    recorded_by_id, verification_status, eligible_flags_json,
                    row_version, created_at, updated_at
             FROM failure_events WHERE id = ?",
            [new_id.into()],
        ))
        .await?
    }
    .ok_or_else(|| AppError::SyncError("failure_events row missing after upsert.".into()))?;

    let mapped = map_failure_event(&row)?;
    stage_failure_event(db, &mapped).await?;
    Ok(mapped)
}

async fn is_failure_mode_active_mode(db: &DatabaseConnection, mode_id: Option<i64>) -> AppResult<bool> {
    let Some(mid) = mode_id else {
        return Ok(false);
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM failure_codes WHERE id = ? AND code_type = 'mode' AND is_active = 1",
            [mid.into()],
        ))
        .await?;
    Ok(row.is_some())
}

pub async fn ingest_failure_event_from_closed_wo(
    db: &DatabaseConnection,
    wo_id: i64,
    actor_id: i64,
) -> AppResult<()> {
    let wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.equipment_id, wo.closed_at, wo.actual_start, wo.created_at, wot.code AS type_code \
             FROM work_orders wo \
             INNER JOIN work_order_types wot ON wot.id = wo.type_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;

    let type_code: String = wo_row.try_get("", "type_code").map_err(|e| decode_err("type_code", e))?;
    if !INGEST_WO_TYPE_CODES.contains(&type_code.as_str()) {
        return Ok(());
    }

    let equipment_id: Option<i64> = wo_row
        .try_get::<Option<i64>>("", "equipment_id")
        .map_err(|e| decode_err("equipment_id", e))?;
    let Some(equipment_id) = equipment_id else {
        return Ok(());
    };

    let closed_at: Option<String> = wo_row
        .try_get::<Option<String>>("", "closed_at")
        .map_err(|e| decode_err("closed_at", e))?;
    let actual_start: Option<String> = wo_row
        .try_get::<Option<String>>("", "actual_start")
        .map_err(|e| decode_err("actual_start", e))?;
    let created_at: String = wo_row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?;

    let t_detect = actual_start.clone().or_else(|| Some(created_at.clone()));

    let fd_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT failure_mode_id, failure_cause_id, failure_effect_id, cause_not_determined \
             FROM work_order_failure_details WHERE work_order_id = ? ORDER BY id DESC LIMIT 1",
            [wo_id.into()],
        ))
        .await?;

    let (failure_mode_id, failure_cause_id, failure_effect_id, cause_not_determined_i) =
        if let Some(r) = fd_row {
            (
                r.try_get::<Option<i64>>("", "failure_mode_id").ok().flatten(),
                r.try_get::<Option<i64>>("", "failure_cause_id").ok().flatten(),
                r.try_get::<Option<i64>>("", "failure_effect_id").ok().flatten(),
                r.try_get::<i64>("", "cause_not_determined").unwrap_or(0),
            )
        } else {
            (None, None, None, 0)
        };

    let cause_nd = cause_not_determined_i != 0;

    let dt_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM( \
                 CASE WHEN ended_at IS NOT NULL \
                 THEN (JULIANDAY(ended_at) - JULIANDAY(started_at)) * 24.0 \
                 ELSE 0 END \
               ), 0.0) AS h \
             FROM work_order_downtime_segments WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("downtime sum missing.".into()))?;
    let downtime_h: f64 = dt_row.try_get("", "h").map_err(|e| decode_err("h", e))?;

    let wh_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM( \
                 CASE WHEN ended_at IS NOT NULL \
                 THEN (JULIANDAY(ended_at) - JULIANDAY(started_at)) * 24.0 \
                 ELSE 0 END \
               ), 0.0) AS h \
             FROM work_order_delay_segments WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("waiting sum missing.".into()))?;
    let waiting_h: f64 = wh_row.try_get("", "h").map_err(|e| decode_err("h", e))?;

    let lab_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(hours_worked), 0.0) AS h FROM work_order_interveners WHERE work_order_id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("labor sum missing.".into()))?;
    let labor_h: f64 = lab_row.try_get("", "h").map_err(|e| decode_err("h", e))?;

    let ver_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT result FROM work_order_verifications WHERE work_order_id = ? ORDER BY verified_at DESC LIMIT 1",
            [wo_id.into()],
        ))
        .await?;
    let verification_status = ver_row
        .and_then(|r| r.try_get::<String>("", "result").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "recorded".to_string());

    let mode_ok = is_failure_mode_active_mode(db, failure_mode_id).await?;
    let is_planned = false;
    let eligible_unplanned = !is_planned && mode_ok && !cause_nd;
    let eligible_json = serde_json::json!({
        "eligible_unplanned_mtbf": eligible_unplanned,
        "eligible_for_strict_mtbf": eligible_unplanned,
    })
    .to_string();

    let entity_sync_id = format!("failure_event:{}", Uuid::new_v4());
    let ts = utc_now_rfc3339();

    let ins = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO failure_events (
                entity_sync_id, source_type, source_id, equipment_id, component_id,
                detected_at, failed_at, restored_at,
                downtime_duration_hours, active_repair_hours, waiting_hours,
                is_planned,
                failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, failure_mechanism_id,
                cause_not_determined, production_impact_level, safety_impact_level,
                recorded_by_id, verification_status, eligible_flags_json,
                row_version, created_at, updated_at
            ) VALUES (
                ?, ?, ?, ?, NULL,
                ?, ?, ?,
                ?, ?, ?,
                ?,
                NULL, ?, ?, ?, NULL,
                ?, NULL, NULL,
                ?, ?, ?,
                1, ?, ?
            )",
            [
                entity_sync_id.into(),
                WO_FAILURE_SOURCE.into(),
                wo_id.into(),
                equipment_id.into(),
                opt_string(t_detect),
                opt_string(actual_start.clone()),
                opt_string(closed_at.clone()),
                downtime_h.into(),
                labor_h.into(),
                waiting_h.into(),
                bool_to_i64(is_planned).into(),
                opt_i64(failure_mode_id),
                opt_i64(failure_cause_id),
                opt_i64(failure_effect_id),
                bool_to_i64(cause_nd).into(),
                actor_id.into(),
                verification_status.into(),
                eligible_json.into(),
                ts.clone().into(),
                ts.into(),
            ],
        ))
        .await?;
    if ins.rows_affected() == 0 {
        return Ok(());
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, source_type, source_id, equipment_id, component_id,
                    detected_at, failed_at, restored_at,
                    downtime_duration_hours, active_repair_hours, waiting_hours,
                    is_planned, failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, failure_mechanism_id,
                    cause_not_determined, production_impact_level, safety_impact_level,
                    recorded_by_id, verification_status, eligible_flags_json,
                    row_version, created_at, updated_at
             FROM failure_events WHERE source_type = ? AND source_id = ?",
            [WO_FAILURE_SOURCE.into(), wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("failure_events row missing after ingest.".into()))?;

    let mapped = map_failure_event(&row)?;
    stage_failure_event(db, &mapped).await?;
    Ok(())
}

pub async fn list_failure_events(db: &DatabaseConnection, filter: FailureEventsFilter) -> AppResult<Vec<FailureEvent>> {
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, source_type, source_id, equipment_id, component_id,
                detected_at, failed_at, restored_at,
                downtime_duration_hours, active_repair_hours, waiting_hours,
                is_planned, failure_class_id, failure_mode_id, failure_cause_id, failure_effect_id, failure_mechanism_id,
                cause_not_determined, production_impact_level, safety_impact_level,
                recorded_by_id, verification_status, eligible_flags_json,
                row_version, created_at, updated_at
         FROM failure_events WHERE 1 = 1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(limit.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_failure_event).collect()
}

fn parse_dt_utc(s: &str) -> AppResult<chrono::DateTime<Utc>> {
    let t = s.trim();
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(t) {
        return Ok(d.with_timezone(&Utc));
    }
    if let Ok(n) = chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S") {
        return Ok(chrono::DateTime::from_naive_utc_and_offset(n, Utc));
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        let n = d.and_hms_opt(0, 0, 0).unwrap();
        return Ok(chrono::DateTime::from_naive_utc_and_offset(n, Utc));
    }
    Err(AppError::ValidationFailed(vec![format!("invalid datetime: {s}")]))
}

fn validate_exposure_type(t: &str) -> AppResult<()> {
    if EXPOSURE_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "exposure_type must be one of: {EXPOSURE_TYPES:?}"
        )]))
    }
}

fn validate_runtime_source_type(t: &str) -> AppResult<()> {
    if RUNTIME_SOURCE_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "source_type must be one of: {RUNTIME_SOURCE_TYPES:?}"
        )]))
    }
}

fn map_runtime_exposure_log(row: &sea_orm::QueryResult) -> AppResult<RuntimeExposureLog> {
    Ok(RuntimeExposureLog {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        exposure_type: row.try_get("", "exposure_type").map_err(|e| decode_err("exposure_type", e))?,
        value: row.try_get("", "value").map_err(|e| decode_err("value", e))?,
        recorded_at: row.try_get("", "recorded_at").map_err(|e| decode_err("recorded_at", e))?,
        source_type: row.try_get("", "source_type").map_err(|e| decode_err("source_type", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_kpi_snapshot(row: &sea_orm::QueryResult) -> AppResult<ReliabilityKpiSnapshot> {
    Ok(ReliabilityKpiSnapshot {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row
            .try_get::<Option<i64>>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        asset_group_id: row
            .try_get::<Option<i64>>("", "asset_group_id")
            .map_err(|e| decode_err("asset_group_id", e))?,
        period_start: row.try_get("", "period_start").map_err(|e| decode_err("period_start", e))?,
        period_end: row.try_get("", "period_end").map_err(|e| decode_err("period_end", e))?,
        mtbf: row.try_get::<Option<f64>>("", "mtbf").map_err(|e| decode_err("mtbf", e))?,
        mttr: row.try_get::<Option<f64>>("", "mttr").map_err(|e| decode_err("mttr", e))?,
        availability: row.try_get::<Option<f64>>("", "availability").map_err(|e| decode_err("availability", e))?,
        failure_rate: row
            .try_get::<Option<f64>>("", "failure_rate")
            .map_err(|e| decode_err("failure_rate", e))?,
        repeat_failure_rate: row
            .try_get::<Option<f64>>("", "repeat_failure_rate")
            .map_err(|e| decode_err("repeat_failure_rate", e))?,
        event_count: row.try_get("", "event_count").map_err(|e| decode_err("event_count", e))?,
        data_quality_score: row
            .try_get("", "data_quality_score")
            .map_err(|e| decode_err("data_quality_score", e))?,
        inspection_signal_json: row
            .try_get::<Option<String>>("", "inspection_signal_json")
            .map_err(|e| decode_err("inspection_signal_json", e))?,
        analysis_dataset_hash_sha256: row
            .try_get("", "analysis_dataset_hash_sha256")
            .map_err(|e| decode_err("analysis_dataset_hash_sha256", e))?,
        analysis_input_spec_json: row
            .try_get("", "analysis_input_spec_json")
            .map_err(|e| decode_err("analysis_input_spec_json", e))?,
        plot_payload_json: row
            .try_get("", "plot_payload_json")
            .map_err(|e| decode_err("plot_payload_json", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

pub async fn upsert_runtime_exposure_log(
    db: &DatabaseConnection,
    input: UpsertRuntimeExposureLogInput,
) -> AppResult<RuntimeExposureLog> {
    validate_exposure_type(&input.exposure_type)?;
    validate_runtime_source_type(&input.source_type)?;
    if !input.value.is_finite() {
        return Err(AppError::ValidationFailed(vec!["value must be finite.".into()]));
    }

    let row = if let Some(id) = input.id {
        let exp = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required.".into()]))?;
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE runtime_exposure_logs SET equipment_id = ?, exposure_type = ?, value = ?,
                 recorded_at = ?, source_type = ?, row_version = row_version + 1
                 WHERE id = ? AND row_version = ?",
                [
                    input.equipment_id.into(),
                    input.exposure_type.clone().into(),
                    input.value.into(),
                    input.recorded_at.clone().into(),
                    input.source_type.clone().into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec![
                "runtime_exposure_logs update conflict or not found.".into(),
            ]));
        }
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, exposure_type, value, recorded_at, source_type, row_version
             FROM runtime_exposure_logs WHERE id = ?",
            [id.into()],
        ))
        .await?
    } else {
        let eid = format!("runtime_exposure:{}", Uuid::new_v4());
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO runtime_exposure_logs (entity_sync_id, equipment_id, exposure_type, value, recorded_at, source_type, row_version)
             VALUES (?, ?, ?, ?, ?, ?, 1)",
            [
                eid.into(),
                input.equipment_id.into(),
                input.exposure_type.into(),
                input.value.into(),
                input.recorded_at.into(),
                input.source_type.into(),
            ],
        ))
        .await?;
        let new_id = last_insert_id(db).await?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, exposure_type, value, recorded_at, source_type, row_version
             FROM runtime_exposure_logs WHERE id = ?",
            [new_id.into()],
        ))
        .await?
    }
    .ok_or_else(|| AppError::SyncError("runtime_exposure_logs row missing after upsert.".into()))?;

    let mapped = map_runtime_exposure_log(&row)?;
    stage_runtime_exposure_log(db, &mapped).await?;
    Ok(mapped)
}

pub async fn list_runtime_exposure_logs(
    db: &DatabaseConnection,
    filter: RuntimeExposureLogsFilter,
) -> AppResult<Vec<RuntimeExposureLog>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, exposure_type, value, recorded_at, source_type, row_version
         FROM runtime_exposure_logs WHERE 1 = 1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    if let Some(ref ps) = filter.period_start {
        sql.push_str(" AND recorded_at >= ?");
        vals.push(ps.clone().into());
    }
    if let Some(ref pe) = filter.period_end {
        sql.push_str(" AND recorded_at <= ?");
        vals.push(pe.clone().into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(limit.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_runtime_exposure_log).collect()
}

struct KpiRefreshCore {
    p0s: String,
    p1s: String,
    t_exp: f64,
    mtbf: Option<f64>,
    mttr: Option<f64>,
    availability: Option<f64>,
    failure_rate: Option<f64>,
    repeat_failure_rate: Option<f64>,
    f: i64,
    dq: f64,
    dataset_hash: String,
    input_spec_json: String,
    plot_payload_json: String,
}

async fn kpi_refresh_core(
    db: &DatabaseConnection,
    input: &RefreshReliabilityKpiSnapshotInput,
) -> AppResult<KpiRefreshCore> {
    let p0 = parse_dt_utc(&input.period_start)?;
    let p1 = parse_dt_utc(&input.period_end)?;
    if p1 < p0 {
        return Err(AppError::ValidationFailed(vec!["period_end before period_start.".into()]));
    }
    let lookback = input.repeat_lookback_days.unwrap_or(30).max(1);
    let min_n = input.min_sample_n.unwrap_or(5).max(1);
    let lb = Duration::days(lookback);
    let ext_start = p0 - lb;
    let ext_start_s = ext_start.to_rfc3339_opts(SecondsFormat::Secs, true);
    let p0s = p0.to_rfc3339_opts(SecondsFormat::Secs, true);
    let p1s = p1.to_rfc3339_opts(SecondsFormat::Secs, true);

    let t_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(value), 0.0) AS texp FROM runtime_exposure_logs
             WHERE equipment_id = ? AND exposure_type = 'hours'
               AND recorded_at >= ? AND recorded_at <= ?",
            [input.equipment_id.into(), p0s.clone().into(), p1s.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("exposure sum missing.".into()))?;
    let t_exp: f64 = t_row.try_get("", "texp").map_err(|e| decode_err("texp", e))?;
    let t_exp = t_exp.max(0.0);

    let exp_log_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, exposure_type, value, recorded_at, source_type FROM runtime_exposure_logs
             WHERE equipment_id = ? AND exposure_type = 'hours'
               AND recorded_at >= ? AND recorded_at <= ?
             ORDER BY id ASC",
            [input.equipment_id.into(), p0s.clone().into(), p1s.clone().into()],
        ))
        .await?;

    let mut exposure_parts: Vec<ExposurePart> = Vec::new();
    for r in &exp_log_rows {
        exposure_parts.push(ExposurePart {
            id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            exposure_type: r.try_get("", "exposure_type").map_err(|e| decode_err("exposure_type", e))?,
            value: r.try_get("", "value").map_err(|e| decode_err("value", e))?,
            recorded_at: r.try_get("", "recorded_at").map_err(|e| decode_err("recorded_at", e))?,
            source_type: r.try_get("", "source_type").map_err(|e| decode_err("source_type", e))?,
        });
    }

    let fe_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, eligible_flags_json, downtime_duration_hours, active_repair_hours, failure_mode_id,
                    COALESCE(failed_at, detected_at, created_at) AS ev_ts
             FROM failure_events
             WHERE equipment_id = ?
               AND COALESCE(failed_at, detected_at, created_at) >= ?
               AND COALESCE(failed_at, detected_at, created_at) <= ?
             ORDER BY ev_ts ASC",
            [input.equipment_id.into(), ext_start_s.into(), p1s.clone().into()],
        ))
        .await?;

    let mut failure_parts: Vec<FailurePart> = Vec::new();
    let mut events: Vec<KpiFailureEvent> = Vec::new();
    for r in &fe_rows {
        let ts_s: String = r.try_get("", "ev_ts").map_err(|e| decode_err("ev_ts", e))?;
        let ts = parse_dt_utc(&ts_s).unwrap_or(p0);
        let flags: String = r.try_get("", "eligible_flags_json").map_err(|e| decode_err("eligible_flags_json", e))?;
        let id: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
        failure_parts.push(FailurePart {
            id,
            ev_ts: ts_s,
            eligible_flags_json: flags.clone(),
            downtime_duration_hours: r.try_get("", "downtime_duration_hours").map_err(|e| decode_err("downtime_duration_hours", e))?,
            active_repair_hours: r.try_get("", "active_repair_hours").map_err(|e| decode_err("active_repair_hours", e))?,
            failure_mode_id: r.try_get::<Option<i64>>("", "failure_mode_id").map_err(|e| decode_err("failure_mode_id", e))?,
        });
        events.push(KpiFailureEvent {
            id,
            event_ts: ts,
            eligible_flags_json: flags,
            downtime_duration_hours: r.try_get("", "downtime_duration_hours").map_err(|e| decode_err("downtime_duration_hours", e))?,
            active_repair_hours: r.try_get("", "active_repair_hours").map_err(|e| decode_err("active_repair_hours", e))?,
            failure_mode_id: r.try_get::<Option<i64>>("", "failure_mode_id").map_err(|e| decode_err("failure_mode_id", e))?,
        });
    }

    let computed = compute_reliability_kpis(&ReliabilityKpiComputeInput {
        period_start: p0,
        period_end: p1,
        t_exp_hours: t_exp,
        repeat_lookback_days: lookback,
        min_sample_n: min_n,
        events,
    });

    let dataset_hash = dataset_hash_sha256(
        input.equipment_id,
        &p0s,
        &p1s,
        lookback,
        min_n,
        t_exp,
        exposure_parts,
        failure_parts,
    );
    let input_spec_json = build_input_spec_json(t_exp, computed.event_count, min_n);
    let plot_payload_json = crate::reliability::plot_payload::build_kpi_plot_payload_json(
        input.equipment_id,
        &p0s,
        &p1s,
        &dataset_hash,
        t_exp,
        computed.event_count,
        computed.mtbf,
        computed.mttr,
        computed.availability,
        computed.failure_rate,
        computed.repeat_failure_rate,
    );

    Ok(KpiRefreshCore {
        p0s,
        p1s,
        t_exp,
        mtbf: computed.mtbf,
        mttr: computed.mttr,
        availability: computed.availability,
        failure_rate: computed.failure_rate,
        repeat_failure_rate: computed.repeat_failure_rate,
        f: computed.event_count,
        dq: computed.data_quality_score,
        dataset_hash,
        input_spec_json,
        plot_payload_json,
    })
}

pub async fn evaluate_reliability_analysis_input(
    db: &DatabaseConnection,
    input: RefreshReliabilityKpiSnapshotInput,
) -> AppResult<ReliabilityAnalysisInputEvaluation> {
    let core = kpi_refresh_core(db, &input).await?;
    Ok(ReliabilityAnalysisInputEvaluation {
        equipment_id: input.equipment_id,
        period_start: input.period_start,
        period_end: input.period_end,
        exposure_hours: core.t_exp,
        eligible_event_count: core.f,
        min_sample_n: input.min_sample_n.unwrap_or(5).max(1),
        analysis_dataset_hash_sha256: core.dataset_hash,
        analysis_input_spec_json: core.input_spec_json,
    })
}

pub async fn refresh_reliability_kpi_snapshot(
    db: &DatabaseConnection,
    input: RefreshReliabilityKpiSnapshotInput,
) -> AppResult<ReliabilityKpiSnapshot> {
    let core = kpi_refresh_core(db, &input).await?;
    let p0s = core.p0s.clone();
    let p1s = core.p1s.clone();
    let mtbf = core.mtbf;
    let mttr = core.mttr;
    let availability = core.availability;
    let failure_rate = core.failure_rate;
    let repeat_failure_rate = core.repeat_failure_rate;
    let f = core.f;
    let dq = core.dq;
    let dataset_hash = core.dataset_hash.clone();
    let input_spec_json = core.input_spec_json.clone();
    let plot_payload_json = core.plot_payload_json.clone();

    let sig_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT warning_count, fail_count, anomaly_open_count, checkpoint_coverage_ratio
             FROM inspection_reliability_signals
             WHERE equipment_id = ? AND period_start = ? AND period_end = ?",
            [input.equipment_id.into(), p0s.clone().into(), p1s.clone().into()],
        ))
        .await?;

    let inspection_signal_json = if let Some(sr) = sig_row {
        let w: i64 = sr.try_get("", "warning_count").map_err(|e| decode_err("warning_count", e))?;
        let fc: i64 = sr.try_get("", "fail_count").map_err(|e| decode_err("fail_count", e))?;
        let ao: i64 = sr.try_get("", "anomaly_open_count").map_err(|e| decode_err("anomaly_open_count", e))?;
        let cc: f64 = sr
            .try_get("", "checkpoint_coverage_ratio")
            .map_err(|e| decode_err("checkpoint_coverage_ratio", e))?;
        Some(
            serde_json::json!({
                "warning_count": w,
                "fail_count": fc,
                "anomaly_open_count": ao,
                "checkpoint_coverage_ratio": cc,
            })
            .to_string(),
        )
    } else {
        None
    };

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, row_version FROM reliability_kpi_snapshots
             WHERE equipment_id = ? AND period_start = ? AND period_end = ?",
            [input.equipment_id.into(), p0s.clone().into(), p1s.clone().into()],
        ))
        .await?;

    let out = if let Some(ex) = existing {
        let ex_id: i64 = ex.try_get("", "id").map_err(|e| decode_err("id", e))?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reliability_kpi_snapshots SET
                mtbf = ?, mttr = ?, availability = ?, failure_rate = ?, repeat_failure_rate = ?,
                event_count = ?, data_quality_score = ?, inspection_signal_json = ?,
                analysis_dataset_hash_sha256 = ?, analysis_input_spec_json = ?, plot_payload_json = ?,
                row_version = row_version + 1
             WHERE id = ?",
            [
                opt_f64(mtbf),
                opt_f64(mttr),
                opt_f64(availability),
                opt_f64(failure_rate),
                opt_f64(repeat_failure_rate),
                f.into(),
                dq.into(),
                opt_string(inspection_signal_json.clone()),
                dataset_hash.clone().into(),
                input_spec_json.clone().into(),
                plot_payload_json.clone().into(),
                ex_id.into(),
            ],
        ))
        .await?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, asset_group_id, period_start, period_end,
                    mtbf, mttr, availability, failure_rate, repeat_failure_rate,
                    event_count, data_quality_score, inspection_signal_json,
                    analysis_dataset_hash_sha256, analysis_input_spec_json, plot_payload_json, row_version
             FROM reliability_kpi_snapshots WHERE id = ?",
            [ex_id.into()],
        ))
        .await?
    } else {
        let eid = format!("kpi_snapshot:{}", Uuid::new_v4());
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reliability_kpi_snapshots (
                entity_sync_id, equipment_id, asset_group_id, period_start, period_end,
                mtbf, mttr, availability, failure_rate, repeat_failure_rate,
                event_count, data_quality_score, inspection_signal_json,
                analysis_dataset_hash_sha256, analysis_input_spec_json, plot_payload_json, row_version
            ) VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                eid.into(),
                input.equipment_id.into(),
                p0s.clone().into(),
                p1s.clone().into(),
                opt_f64(mtbf),
                opt_f64(mttr),
                opt_f64(availability),
                opt_f64(failure_rate),
                opt_f64(repeat_failure_rate),
                f.into(),
                dq.into(),
                opt_string(inspection_signal_json.clone()),
                dataset_hash.into(),
                input_spec_json.into(),
                plot_payload_json.into(),
            ],
        ))
        .await?;
        let nid = last_insert_id(db).await?;
        db.query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, asset_group_id, period_start, period_end,
                    mtbf, mttr, availability, failure_rate, repeat_failure_rate,
                    event_count, data_quality_score, inspection_signal_json,
                    analysis_dataset_hash_sha256, analysis_input_spec_json, plot_payload_json, row_version
             FROM reliability_kpi_snapshots WHERE id = ?",
            [nid.into()],
        ))
        .await?
    }
    .ok_or_else(|| AppError::SyncError("reliability_kpi_snapshots row missing after refresh.".into()))?;

    let mapped = map_kpi_snapshot(&out)?;
    stage_reliability_kpi_snapshot(db, &mapped).await?;
    Ok(mapped)
}

fn opt_f64(v: Option<f64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<f64>))
}

pub async fn list_reliability_kpi_snapshots(
    db: &DatabaseConnection,
    filter: ReliabilityKpiSnapshotsFilter,
) -> AppResult<Vec<ReliabilityKpiSnapshot>> {
    let limit = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, asset_group_id, period_start, period_end,
                mtbf, mttr, availability, failure_rate, repeat_failure_rate,
                event_count, data_quality_score, inspection_signal_json,
                analysis_dataset_hash_sha256, analysis_input_spec_json, plot_payload_json, row_version
         FROM reliability_kpi_snapshots WHERE 1 = 1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(limit.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_kpi_snapshot).collect()
}

pub async fn get_reliability_kpi_snapshot(db: &DatabaseConnection, id: i64) -> AppResult<ReliabilityKpiSnapshot> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, asset_group_id, period_start, period_end,
                    mtbf, mttr, availability, failure_rate, repeat_failure_rate,
                    event_count, data_quality_score, inspection_signal_json,
                    analysis_dataset_hash_sha256, analysis_input_spec_json, plot_payload_json, row_version
             FROM reliability_kpi_snapshots WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ReliabilityKpiSnapshot".into(),
            id: id.to_string(),
        })?;
    map_kpi_snapshot(&row)
}

fn ram_scope_key(issue_code: &str, equipment_id: i64) -> String {
    format!("{issue_code}:{equipment_id}")
}

fn map_ram_issue(row: &sea_orm::QueryResult) -> AppResult<RamDataQualityIssue> {
    Ok(RamDataQualityIssue {
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        issue_code: row.try_get("", "issue_code").map_err(|e| decode_err("issue_code", e))?,
        severity: row.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
        remediation_url: row.try_get("", "remediation_url").map_err(|e| decode_err("remediation_url", e))?,
    })
}

fn map_user_dismissal_row(row: &sea_orm::QueryResult) -> AppResult<UserDismissal> {
    Ok(UserDismissal {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        user_id: row.try_get("", "user_id").map_err(|e| decode_err("user_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        issue_code: row.try_get("", "issue_code").map_err(|e| decode_err("issue_code", e))?,
        scope_key: row.try_get("", "scope_key").map_err(|e| decode_err("scope_key", e))?,
        dismissed_at: row.try_get("", "dismissed_at").map_err(|e| decode_err("dismissed_at", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

pub async fn list_ram_data_quality_issues(
    db: &DatabaseConnection,
    filter: RamDataQualityIssuesFilter,
    user_id: i32,
) -> AppResult<Vec<RamDataQualityIssue>> {
    let mut sql = String::from(
        "SELECT v.equipment_id, v.issue_code, v.severity, v.remediation_url
         FROM v_ram_data_quality_issues v
         WHERE NOT EXISTS (
           SELECT 1 FROM user_dismissals ud
           WHERE ud.user_id = ? AND ud.scope_key = (v.issue_code || ':' || CAST(v.equipment_id AS TEXT))
         )",
    );
    let mut vals: Vec<sea_orm::Value> = vec![user_id.into()];
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND v.equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(
        " ORDER BY CASE v.severity WHEN 'blocking' THEN 0 ELSE 1 END, v.equipment_id, v.issue_code",
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_ram_issue).collect()
}

pub async fn list_wos_missing_failure_mode(
    db: &DatabaseConnection,
    equipment_id: Option<i64>,
    limit: Option<i64>,
) -> AppResult<Vec<WoMissingFailureModeRow>> {
    let lim = limit.unwrap_or(200).clamp(1, 500);
    let mut sql = String::from(
        "SELECT wo.id AS work_order_id, wo.equipment_id, wo.closed_at, wot.code AS type_code
         FROM work_orders wo
         INNER JOIN work_order_failure_details wfd ON wfd.work_order_id = wo.id
         INNER JOIN work_order_types wot ON wot.id = wo.type_id
         WHERE wo.closed_at IS NOT NULL
           AND wo.equipment_id IS NOT NULL
           AND wfd.failure_mode_id IS NULL
           AND wot.code IN ('corrective', 'emergency')",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = equipment_id {
        sql.push_str(" AND wo.equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY wo.closed_at DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(WoMissingFailureModeRow {
                work_order_id: r.try_get("", "work_order_id").map_err(|e| decode_err("work_order_id", e))?,
                equipment_id: r.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
                closed_at: r
                    .try_get::<Option<String>>("", "closed_at")
                    .map_err(|e| decode_err("closed_at", e))?,
                type_code: r.try_get("", "type_code").map_err(|e| decode_err("type_code", e))?,
            })
        })
        .collect()
}

pub async fn list_equipment_missing_exposure_90d(
    db: &DatabaseConnection,
    limit: Option<i64>,
) -> AppResult<Vec<EquipmentMissingExposureRow>> {
    let lim = limit.unwrap_or(200).clamp(1, 500);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id AS equipment_id, e.name AS equipment_name
             FROM equipment e
             WHERE NOT EXISTS (
               SELECT 1 FROM runtime_exposure_logs rel
               WHERE rel.equipment_id = e.id
                 AND rel.recorded_at >= datetime('now', '-90 days')
             )
             ORDER BY e.id ASC
             LIMIT ?",
            [lim.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(EquipmentMissingExposureRow {
                equipment_id: r.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
                equipment_name: r.try_get("", "equipment_name").map_err(|e| decode_err("equipment_name", e))?,
            })
        })
        .collect()
}

pub async fn get_ram_equipment_quality_badge(
    db: &DatabaseConnection,
    equipment_id: i64,
    user_id: i32,
) -> AppResult<RamEquipmentQualityBadge> {
    let score_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT data_quality_score FROM reliability_kpi_snapshots
             WHERE equipment_id = ? ORDER BY id DESC LIMIT 1",
            [equipment_id.into()],
        ))
        .await?;
    let data_quality_score: Option<f64> = score_row
        .and_then(|r| r.try_get::<Option<f64>>("", "data_quality_score").ok().flatten());

    let issue_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT v.issue_code, v.severity FROM v_ram_data_quality_issues v
             WHERE v.equipment_id = ?
               AND NOT EXISTS (
                 SELECT 1 FROM user_dismissals ud
                 WHERE ud.user_id = ? AND ud.scope_key = (v.issue_code || ':' || CAST(v.equipment_id AS TEXT))
               )",
            [equipment_id.into(), i64::from(user_id).into()],
        ))
        .await?;

    let mut blocking_issue_codes: Vec<String> = Vec::new();
    for r in &issue_rows {
        let sev: String = r.try_get("", "severity").map_err(|e| decode_err("severity", e))?;
        if sev == "blocking" {
            let code: String = r.try_get("", "issue_code").map_err(|e| decode_err("issue_code", e))?;
            if !blocking_issue_codes.contains(&code) {
                blocking_issue_codes.push(code);
            }
        }
    }

    let badge = if !blocking_issue_codes.is_empty() {
        "red"
    } else if data_quality_score.map(|s| s >= 0.85).unwrap_or(false) {
        "green"
    } else {
        "yellow"
    }
    .to_string();

    Ok(RamEquipmentQualityBadge {
        equipment_id,
        data_quality_score,
        badge,
        blocking_issue_codes,
    })
}

pub async fn iso_14224_failure_dataset_completeness(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<Iso14224DatasetCompleteness> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT COUNT(*) AS n,
                      AVG(CASE WHEN fe.equipment_id IS NOT NULL THEN 1.0 ELSE 0.0 END) AS d_eq,
                      AVG(CASE WHEN (fe.failed_at IS NOT NULL OR fe.detected_at IS NOT NULL)
                                    AND fe.restored_at IS NOT NULL THEN 1.0 ELSE 0.0 END) AS d_win,
                      AVG(CASE WHEN fe.failure_mode_id IS NOT NULL THEN 1.0 ELSE 0.0 END) AS d_mode,
                      AVG(CASE WHEN fe.restored_at IS NOT NULL
                                    OR (fe.source_type = 'work_order' AND wo.closed_at IS NOT NULL)
                               THEN 1.0 ELSE 0.0 END) AS d_corr
               FROM failure_events fe
               LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND fe.source_id = wo.id
               WHERE fe.equipment_id = ?"#,
            [equipment_id.into()],
        ))
        .await?;
    let Some(r) = row else {
        return Ok(Iso14224DatasetCompleteness {
            equipment_id,
            event_count: 0,
            completeness_percent: 0.0,
            dim_equipment_id_pct: 0.0,
            dim_failure_interval_pct: 0.0,
            dim_failure_mode_pct: 0.0,
            dim_corrective_closure_pct: 0.0,
        });
    };
    let n: i64 = r.try_get("", "n").map_err(|e| decode_err("n", e))?;
    if n == 0 {
        return Ok(Iso14224DatasetCompleteness {
            equipment_id,
            event_count: 0,
            completeness_percent: 0.0,
            dim_equipment_id_pct: 0.0,
            dim_failure_interval_pct: 0.0,
            dim_failure_mode_pct: 0.0,
            dim_corrective_closure_pct: 0.0,
        });
    }
    let d_eq: f64 = r.try_get("", "d_eq").map_err(|e| decode_err("d_eq", e))?;
    let d_win: f64 = r.try_get("", "d_win").map_err(|e| decode_err("d_win", e))?;
    let d_mode: f64 = r.try_get("", "d_mode").map_err(|e| decode_err("d_mode", e))?;
    let d_corr: f64 = r.try_get("", "d_corr").map_err(|e| decode_err("d_corr", e))?;
    let completeness_percent = ((d_eq + d_win + d_mode + d_corr) / 4.0) * 100.0;
    Ok(Iso14224DatasetCompleteness {
        equipment_id,
        event_count: n,
        completeness_percent,
        dim_equipment_id_pct: d_eq * 100.0,
        dim_failure_interval_pct: d_win * 100.0,
        dim_failure_mode_pct: d_mode * 100.0,
        dim_corrective_closure_pct: d_corr * 100.0,
    })
}

pub async fn dismiss_ram_data_quality_issue(
    db: &DatabaseConnection,
    user_id: i32,
    input: DismissRamDataQualityIssueInput,
) -> AppResult<UserDismissal> {
    let issue_code = input.issue_code.trim();
    if issue_code.is_empty() {
        return Err(AppError::ValidationFailed(vec!["issue_code required.".into()]));
    }
    let scope_key = ram_scope_key(issue_code, input.equipment_id);
    let eid = format!("user_dismissal:{}", Uuid::new_v4());
    let ts = utc_now_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_dismissals (entity_sync_id, user_id, equipment_id, issue_code, scope_key, dismissed_at, row_version)
         VALUES (?, ?, ?, ?, ?, ?, 1)
         ON CONFLICT(user_id, scope_key) DO UPDATE SET
           dismissed_at = excluded.dismissed_at,
           entity_sync_id = excluded.entity_sync_id,
           row_version = user_dismissals.row_version + 1",
        [
            eid.clone().into(),
            i64::from(user_id).into(),
            input.equipment_id.into(),
            issue_code.to_string().into(),
            scope_key.clone().into(),
            ts.clone().into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, user_id, equipment_id, issue_code, scope_key, dismissed_at, row_version
             FROM user_dismissals WHERE user_id = ? AND scope_key = ?",
            [i64::from(user_id).into(), scope_key.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("user_dismissals row missing after upsert.".into()))?;
    let mapped = map_user_dismissal_row(&row)?;
    stage_user_dismissal(db, &mapped).await?;
    Ok(mapped)
}
