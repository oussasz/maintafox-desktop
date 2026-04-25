use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use std::collections::HashMap;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::settings;
use crate::reliability::advanced_rams::domain::{
    CreateFmecaAnalysisInput, CreateRcmStudyInput, FmecaAnalysesFilter, FmecaAnalysis, FmecaItem,
    FmecaItemWithContext, FmecaItemsEquipmentFilter, FmecaSeverityOccurrenceMatrix, FmecaSoCell,
    RamIshikawaDiagram, RamIshikawaDiagramsFilter, ReliabilityRulIndicator, RcmDecision, RcmStudiesFilter,
    RcmStudy, UpdateFmecaAnalysisInput, UpdateRcmStudyInput, UpsertFmecaItemInput, UpsertRamIshikawaDiagramInput,
    UpsertRcmDecisionInput, WeibullFitRecord, WeibullFitRunInput,
};
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;
use crate::reliability::queries::evaluate_reliability_analysis_input;
use crate::reliability::weibull_fit::fit_weibull_with_ci;

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("advanced_rams decode '{field}': {err}"))
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

fn parse_ts(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::ValidationFailed(vec![format!("invalid timestamp: {e}")]))
}

pub fn inter_arrival_hours_from_events(
    timestamps: &[DateTime<Utc>],
) -> Vec<f64> {
    if timestamps.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(timestamps.len() - 1);
    for w in timestamps.windows(2) {
        let h = (w[1] - w[0]).num_milliseconds() as f64 / 3_600_000.0;
        if h > 0.0 && h.is_finite() {
            out.push(h);
        }
    }
    out
}

pub async fn run_and_store_weibull_fit(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    input: WeibullFitRunInput,
) -> AppResult<WeibullFitRecord> {
    let mut sql = String::from(
        "SELECT COALESCE(failed_at, detected_at, created_at) AS ts
         FROM failure_events WHERE equipment_id = ?",
    );
    let mut vals: Vec<sea_orm::Value> = vec![input.equipment_id.into()];
    if let Some(ref ps) = input.period_start {
        sql.push_str(" AND COALESCE(failed_at, detected_at, created_at) >= ?");
        vals.push(ps.clone().into());
    }
    if let Some(ref pe) = input.period_end {
        sql.push_str(" AND COALESCE(failed_at, detected_at, created_at) <= ?");
        vals.push(pe.clone().into());
    }
    sql.push_str(" ORDER BY ts ASC");
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    let mut ts: Vec<DateTime<Utc>> = Vec::new();
    for r in &rows {
        let s: String = r.try_get("", "ts").map_err(|e| decode_err("ts", e))?;
        if let Ok(t) = parse_ts(&s) {
            ts.push(t);
        }
    }
    let gaps = inter_arrival_hours_from_events(&ts);
    let fit = fit_weibull_with_ci(&gaps);
    let inter_json = serde_json::to_string(&gaps).unwrap_or_else(|_| "[]".to_string());
    let now = Utc::now().to_rfc3339();
    let uid = user_id.map(i64::from);
    let adequate = if fit.adequate_sample { 1 } else { 0 };
    let eid = format!("weibull_fit:{}", Uuid::new_v4());
    let (beta, eta, bl, bh, el, eh) = if fit.adequate_sample {
        (
            Some(fit.beta),
            Some(fit.eta),
            Some(fit.beta_ci_low),
            Some(fit.beta_ci_high),
            Some(fit.eta_ci_low),
            Some(fit.eta_ci_high),
        )
    } else {
        (None, None, None, None, None, None)
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO weibull_fit_results (
            entity_sync_id, equipment_id, period_start, period_end, n_points,
            inter_arrival_hours_json, beta, eta, beta_ci_low, beta_ci_high,
            eta_ci_low, eta_ci_high, adequate_sample, message, row_version, created_at, created_by_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.period_start.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
            input.period_end.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
            (gaps.len() as i64).into(),
            inter_json.into(),
            beta.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<f64>)),
            eta.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<f64>)),
            bl.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<f64>)),
            bh.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<f64>)),
            el.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<f64>)),
            eh.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<f64>)),
            adequate.into(),
            fit.message.clone().into(),
            now.clone().into(),
            uid.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    get_weibull_fit(db, id).await?
        .ok_or_else(|| AppError::SyncError("weibull_fit_results insert missing.".into()))
}

pub async fn get_weibull_fit(db: &DatabaseConnection, id: i64) -> AppResult<Option<WeibullFitRecord>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, period_start, period_end, n_points,
                    inter_arrival_hours_json, beta, eta, beta_ci_low, beta_ci_high,
                    eta_ci_low, eta_ci_high, adequate_sample, message, row_version, created_at, created_by_id
             FROM weibull_fit_results WHERE id = ?",
            [id.into()],
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => map_weibull(&r).map(Some),
    }
}

pub async fn get_latest_weibull_fit_for_equipment(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<Option<WeibullFitRecord>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, period_start, period_end, n_points,
                    inter_arrival_hours_json, beta, eta, beta_ci_low, beta_ci_high,
                    eta_ci_low, eta_ci_high, adequate_sample, message, row_version, created_at, created_by_id
             FROM weibull_fit_results WHERE equipment_id = ? ORDER BY id DESC LIMIT 1",
            [equipment_id.into()],
        ))
        .await?;
    match row {
        None => Ok(None),
        Some(r) => map_weibull(&r).map(Some),
    }
}

fn map_weibull(row: &sea_orm::QueryResult) -> AppResult<WeibullFitRecord> {
    Ok(WeibullFitRecord {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        period_start: row.try_get::<Option<String>>("", "period_start").map_err(|e| decode_err("period_start", e))?,
        period_end: row.try_get::<Option<String>>("", "period_end").map_err(|e| decode_err("period_end", e))?,
        n_points: row.try_get("", "n_points").map_err(|e| decode_err("n_points", e))?,
        inter_arrival_hours_json: row
            .try_get("", "inter_arrival_hours_json")
            .map_err(|e| decode_err("inter_arrival_hours_json", e))?,
        beta: row.try_get::<Option<f64>>("", "beta").map_err(|e| decode_err("beta", e))?,
        eta: row.try_get::<Option<f64>>("", "eta").map_err(|e| decode_err("eta", e))?,
        beta_ci_low: row.try_get::<Option<f64>>("", "beta_ci_low").map_err(|e| decode_err("beta_ci_low", e))?,
        beta_ci_high: row.try_get::<Option<f64>>("", "beta_ci_high").map_err(|e| decode_err("beta_ci_high", e))?,
        eta_ci_low: row.try_get::<Option<f64>>("", "eta_ci_low").map_err(|e| decode_err("eta_ci_low", e))?,
        eta_ci_high: row.try_get::<Option<f64>>("", "eta_ci_high").map_err(|e| decode_err("eta_ci_high", e))?,
        adequate_sample: row.try_get::<i64>("", "adequate_sample").map_err(|e| decode_err("adequate_sample", e))? != 0,
        message: row.try_get("", "message").map_err(|e| decode_err("message", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id").map_err(|e| decode_err("created_by_id", e))?,
    })
}

fn validate_sod(s: i64, o: i64, d: i64) -> AppResult<()> {
    for (name, v) in [("severity", s), ("occurrence", o), ("detectability", d)] {
        if !(1..=10).contains(&v) {
            return Err(AppError::ValidationFailed(vec![format!(
                "{name} must be 1-10, got {v}"
            )]));
        }
    }
    Ok(())
}

fn map_fmeca_analysis(row: &sea_orm::QueryResult) -> AppResult<FmecaAnalysis> {
    Ok(FmecaAnalysis {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        boundary_definition: row.try_get("", "boundary_definition").map_err(|e| decode_err("boundary_definition", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id").map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_fmeca_analyses(
    db: &DatabaseConnection,
    filter: FmecaAnalysesFilter,
) -> AppResult<Vec<FmecaAnalysis>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, title, boundary_definition, status,
                row_version, created_at, created_by_id, updated_at
         FROM fmeca_analyses WHERE 1=1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_fmeca_analysis).collect()
}

pub async fn create_fmeca_analysis(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    input: CreateFmecaAnalysisInput,
) -> AppResult<FmecaAnalysis> {
    let now = Utc::now().to_rfc3339();
    let eid = format!("fmeca_analysis:{}", Uuid::new_v4());
    let status = input.status.unwrap_or_else(|| "draft".into());
    let boundary = input.boundary_definition.unwrap_or_default();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO fmeca_analyses (
            entity_sync_id, equipment_id, title, boundary_definition, status, row_version, created_at, created_by_id, updated_at
        ) VALUES (?, ?, ?, ?, ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.title.into(),
            boundary.into(),
            status.into(),
            now.clone().into(),
            user_id.map(i64::from).map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, boundary_definition, status,
                    row_version, created_at, created_by_id, updated_at
             FROM fmeca_analyses WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("fmeca_analyses row missing.".into()))?;
    map_fmeca_analysis(&row)
}

pub async fn update_fmeca_analysis(
    db: &DatabaseConnection,
    input: UpdateFmecaAnalysisInput,
) -> AppResult<FmecaAnalysis> {
    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE fmeca_analyses SET
                title = COALESCE(?, title),
                boundary_definition = COALESCE(?, boundary_definition),
                status = COALESCE(?, status),
                row_version = row_version + 1,
                updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                input.title.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                input.boundary_definition.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                input.status.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                Utc::now().to_rfc3339().into(),
                input.id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["fmeca_analyses update conflict.".into()]));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, boundary_definition, status,
                    row_version, created_at, created_by_id, updated_at
             FROM fmeca_analyses WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "FmecaAnalysis".into(),
            id: input.id.to_string(),
        })?;
    map_fmeca_analysis(&row)
}

pub async fn delete_fmeca_analysis(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM fmeca_analyses WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

fn map_fmeca_item(row: &sea_orm::QueryResult) -> AppResult<FmecaItem> {
    Ok(FmecaItem {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        analysis_id: row.try_get("", "analysis_id").map_err(|e| decode_err("analysis_id", e))?,
        component_id: row.try_get::<Option<i64>>("", "component_id").map_err(|e| decode_err("component_id", e))?,
        functional_failure: row.try_get("", "functional_failure").map_err(|e| decode_err("functional_failure", e))?,
        failure_mode_id: row.try_get::<Option<i64>>("", "failure_mode_id").map_err(|e| decode_err("failure_mode_id", e))?,
        failure_effect: row.try_get("", "failure_effect").map_err(|e| decode_err("failure_effect", e))?,
        severity: row.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
        occurrence: row.try_get("", "occurrence").map_err(|e| decode_err("occurrence", e))?,
        detectability: row.try_get("", "detectability").map_err(|e| decode_err("detectability", e))?,
        rpn: row.try_get("", "rpn").map_err(|e| decode_err("rpn", e))?,
        recommended_action: row.try_get("", "recommended_action").map_err(|e| decode_err("recommended_action", e))?,
        current_control: row.try_get("", "current_control").map_err(|e| decode_err("current_control", e))?,
        linked_pm_plan_id: row.try_get::<Option<i64>>("", "linked_pm_plan_id").map_err(|e| decode_err("linked_pm_plan_id", e))?,
        linked_work_order_id: row.try_get::<Option<i64>>("", "linked_work_order_id").map_err(|e| decode_err("linked_work_order_id", e))?,
        revised_rpn: row.try_get::<Option<i64>>("", "revised_rpn").map_err(|e| decode_err("revised_rpn", e))?,
        source_ram_ishikawa_diagram_id: row
            .try_get::<Option<i64>>("", "source_ram_ishikawa_diagram_id")
            .map_err(|e| decode_err("source_ram_ishikawa_diagram_id", e))?,
        source_ishikawa_flow_node_id: row
            .try_get::<Option<String>>("", "source_ishikawa_flow_node_id")
            .map_err(|e| decode_err("source_ishikawa_flow_node_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_fmeca_items(db: &DatabaseConnection, analysis_id: i64) -> AppResult<Vec<FmecaItem>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                    failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                    linked_pm_plan_id, linked_work_order_id, revised_rpn,
                    source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                    row_version, updated_at
             FROM fmeca_items WHERE analysis_id = ? ORDER BY id ASC",
            [analysis_id.into()],
        ))
        .await?;
    rows.iter().map(map_fmeca_item).collect()
}

pub async fn upsert_fmeca_item(db: &DatabaseConnection, input: UpsertFmecaItemInput) -> AppResult<FmecaItem> {
    validate_sod(input.severity, input.occurrence, input.detectability)?;
    let rpn = input.severity * input.occurrence * input.detectability;
    let now = Utc::now().to_rfc3339();
    let ff = input.functional_failure.clone().unwrap_or_default();
    let fe = input.failure_effect.clone().unwrap_or_default();
    let ra = input.recommended_action.clone().unwrap_or_default();
    let cc = input.current_control.clone().unwrap_or_default();
    if let Some(id) = input.id {
        let exp = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required.".into()]))?;
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE fmeca_items SET
                    component_id = ?, functional_failure = ?, failure_mode_id = ?, failure_effect = ?,
                    severity = ?, occurrence = ?, detectability = ?, rpn = ?,
                    recommended_action = ?, current_control = ?,
                    linked_pm_plan_id = ?, linked_work_order_id = ?, revised_rpn = ?,
                    row_version = row_version + 1, updated_at = ?
                 WHERE id = ? AND row_version = ?",
                [
                    input.component_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    ff.into(),
                    input.failure_mode_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    fe.into(),
                    input.severity.into(),
                    input.occurrence.into(),
                    input.detectability.into(),
                    rpn.into(),
                    ra.into(),
                    cc.into(),
                    input.linked_pm_plan_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input.linked_work_order_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    input.revised_rpn.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    now.clone().into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec!["fmeca_items update conflict.".into()]));
        }
    } else {
        if let Some(did) = input.source_ram_ishikawa_diagram_id {
            let d_exists = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT id FROM ram_ishikawa_diagrams WHERE id = ?",
                    [did.into()],
                ))
                .await?;
            if d_exists.is_none() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Ishikawa diagram not found (source_ram_ishikawa_diagram_id={did})."
                )]));
            }
        }
        let eid = format!("fmeca_item:{}", Uuid::new_v4());
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO fmeca_items (
                entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                failure_effect, severity, occurrence, detectability, rpn,
                recommended_action, current_control, linked_pm_plan_id, linked_work_order_id, revised_rpn,
                source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                row_version, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
            [
                eid.into(),
                input.analysis_id.into(),
                input.component_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                ff.into(),
                input.failure_mode_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                fe.into(),
                input.severity.into(),
                input.occurrence.into(),
                input.detectability.into(),
                rpn.into(),
                ra.into(),
                cc.into(),
                input.linked_pm_plan_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                input.linked_work_order_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                input.revised_rpn.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                input
                    .source_ram_ishikawa_diagram_id
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                input
                    .source_ishikawa_flow_node_id
                    .clone()
                    .map(sea_orm::Value::from)
                    .unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                now.into(),
            ],
        ))
        .await?;
    }
    let nid = if let Some(id) = input.id {
        id
    } else {
        last_insert_id(db).await?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                    failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                    linked_pm_plan_id, linked_work_order_id, revised_rpn,
                    source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                    row_version, updated_at
             FROM fmeca_items WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("fmeca_items row missing.".into()))?;
    map_fmeca_item(&row)
}

pub async fn delete_fmeca_item(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM fmeca_items WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

fn map_rcm_study(row: &sea_orm::QueryResult) -> AppResult<RcmStudy> {
    Ok(RcmStudy {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id").map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_rcm_studies(db: &DatabaseConnection, filter: RcmStudiesFilter) -> AppResult<Vec<RcmStudy>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
         FROM rcm_studies WHERE 1=1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_rcm_study).collect()
}

pub async fn create_rcm_study(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    input: CreateRcmStudyInput,
) -> AppResult<RcmStudy> {
    let now = Utc::now().to_rfc3339();
    let eid = format!("rcm_study:{}", Uuid::new_v4());
    let status = input.status.unwrap_or_else(|| "draft".into());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO rcm_studies (
            entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
        ) VALUES (?, ?, ?, ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.title.into(),
            status.into(),
            now.clone().into(),
            user_id.map(i64::from).map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
             FROM rcm_studies WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("rcm_studies row missing.".into()))?;
    map_rcm_study(&row)
}

pub async fn update_rcm_study(db: &DatabaseConnection, input: UpdateRcmStudyInput) -> AppResult<RcmStudy> {
    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE rcm_studies SET
                title = COALESCE(?, title),
                status = COALESCE(?, status),
                row_version = row_version + 1,
                updated_at = ?
             WHERE id = ? AND row_version = ?",
            [
                input.title.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                input.status.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                Utc::now().to_rfc3339().into(),
                input.id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["rcm_studies update conflict.".into()]));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, status, row_version, created_at, created_by_id, updated_at
             FROM rcm_studies WHERE id = ?",
            [input.id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "RcmStudy".into(),
            id: input.id.to_string(),
        })?;
    map_rcm_study(&row)
}

pub async fn delete_rcm_study(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM rcm_studies WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

fn map_rcm_decision(row: &sea_orm::QueryResult) -> AppResult<RcmDecision> {
    Ok(RcmDecision {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        study_id: row.try_get("", "study_id").map_err(|e| decode_err("study_id", e))?,
        function_description: row.try_get("", "function_description").map_err(|e| decode_err("function_description", e))?,
        functional_failure: row.try_get("", "functional_failure").map_err(|e| decode_err("functional_failure", e))?,
        failure_mode_id: row.try_get::<Option<i64>>("", "failure_mode_id").map_err(|e| decode_err("failure_mode_id", e))?,
        consequence_category: row.try_get("", "consequence_category").map_err(|e| decode_err("consequence_category", e))?,
        selected_tactic: row.try_get("", "selected_tactic").map_err(|e| decode_err("selected_tactic", e))?,
        justification: row.try_get("", "justification").map_err(|e| decode_err("justification", e))?,
        review_due_at: row.try_get::<Option<String>>("", "review_due_at").map_err(|e| decode_err("review_due_at", e))?,
        linked_pm_plan_id: row.try_get::<Option<i64>>("", "linked_pm_plan_id").map_err(|e| decode_err("linked_pm_plan_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_rcm_decisions(db: &DatabaseConnection, study_id: i64) -> AppResult<Vec<RcmDecision>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                    consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                    row_version, updated_at
             FROM rcm_decisions WHERE study_id = ? ORDER BY id ASC",
            [study_id.into()],
        ))
        .await?;
    rows.iter().map(map_rcm_decision).collect()
}

const RCM_TACTICS: &[&str] = &[
    "condition_based",
    "time_based",
    "failure_finding",
    "run_to_failure",
    "redesign",
];

fn validate_tactic(t: &str) -> AppResult<()> {
    if RCM_TACTICS.contains(&t) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "selected_tactic must be one of {:?}",
            RCM_TACTICS
        )]))
    }
}

pub async fn upsert_rcm_decision(db: &DatabaseConnection, input: UpsertRcmDecisionInput) -> AppResult<RcmDecision> {
    validate_tactic(&input.selected_tactic)?;
    let now = Utc::now().to_rfc3339();
    let fd = input.function_description.clone().unwrap_or_default();
    let ff = input.functional_failure.clone().unwrap_or_default();
    let cc = input.consequence_category.clone().unwrap_or_default();
    let jus = input.justification.clone().unwrap_or_default();
    if let Some(id) = input.id {
        let exp = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required.".into()]))?;
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE rcm_decisions SET
                    function_description = ?, functional_failure = ?, failure_mode_id = ?,
                    consequence_category = ?, selected_tactic = ?, justification = ?,
                    review_due_at = ?, linked_pm_plan_id = ?,
                    row_version = row_version + 1, updated_at = ?
                 WHERE id = ? AND row_version = ?",
                [
                    fd.into(),
                    ff.into(),
                    input.failure_mode_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    cc.into(),
                    input.selected_tactic.into(),
                    jus.into(),
                    input.review_due_at.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                    input.linked_pm_plan_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                    now.clone().into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec!["rcm_decisions update conflict.".into()]));
        }
    } else {
        let eid = format!("rcm_decision:{}", Uuid::new_v4());
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO rcm_decisions (
                entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                row_version, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
            [
                eid.into(),
                input.study_id.into(),
                fd.into(),
                ff.into(),
                input.failure_mode_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                cc.into(),
                input.selected_tactic.into(),
                jus.into(),
                input.review_due_at.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
                input.linked_pm_plan_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
                now.into(),
            ],
        ))
        .await?;
    }
    let nid = if let Some(id) = input.id {
        id
    } else {
        last_insert_id(db).await?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                    consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                    row_version, updated_at
             FROM rcm_decisions WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("rcm_decisions row missing.".into()))?;
    map_rcm_decision(&row)
}

pub async fn delete_rcm_decision(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM rcm_decisions WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

const DEFAULT_FMECA_RPN_CRITICAL: i64 = 150;
const SETTING_KEY_FMECA_RPN_CRITICAL: &str = "ram.fmeca_rpn_critical_threshold";

fn parse_json_int_setting(json: &str, default: i64) -> i64 {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return default;
    }
    if let Ok(v) = serde_json::from_str::<i64>(trimmed) {
        return v.clamp(1, 1_000_000);
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        match val {
            serde_json::Value::Number(n) => {
                return n
                    .as_i64()
                    .or_else(|| n.as_f64().map(|f| f.round() as i64))
                    .unwrap_or(default)
                    .clamp(1, 1_000_000);
            }
            serde_json::Value::Object(o) => {
                if let Some(v) = o.get("value") {
                    if let Some(i) = v.as_i64() {
                        return i.clamp(1, 1_000_000);
                    }
                    if let Some(f) = v.as_f64() {
                        return (f.round() as i64).clamp(1, 1_000_000);
                    }
                }
            }
            _ => {}
        }
    }
    default
}

/// Tenant-configurable FMECA RPN threshold for spare-line / inventory checks (`app_settings`).
pub async fn fmeca_rpn_critical_threshold_i64(db: &DatabaseConnection) -> AppResult<i64> {
    let row = settings::get_setting(db, SETTING_KEY_FMECA_RPN_CRITICAL, "tenant").await?;
    let Some(s) = row else {
        return Ok(DEFAULT_FMECA_RPN_CRITICAL);
    };
    Ok(parse_json_int_setting(&s.setting_value_json, DEFAULT_FMECA_RPN_CRITICAL))
}

fn weibull_r(beta: f64, eta: f64, t: f64) -> Option<f64> {
    if beta <= 0.0 || eta <= 0.0 || t < 0.0 || !t.is_finite() {
        return None;
    }
    Some((-(t / eta).powf(beta)).exp())
}

fn weibull_residual_median_hours(beta: f64, eta: f64, t: f64) -> Option<f64> {
    if beta <= 0.0 || eta <= 0.0 || !t.is_finite() {
        return None;
    }
    let t0 = t.max(0.0);
    let tb = t0.powf(beta);
    let inner = tb + eta.powf(beta) * std::f64::consts::LN_2;
    if inner <= 0.0 {
        return Some(0.0);
    }
    let x = inner.powf(1.0 / beta) - t0;
    Some(x.max(0.0))
}

async fn sum_spare_stock_for_work_order(db: &DatabaseConnection, wo_id: i64) -> AppResult<f64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(sb.available_qty), 0) AS q
             FROM work_order_parts wop
             LEFT JOIN stock_balances sb ON sb.article_id = wop.article_id
             WHERE wop.work_order_id = ? AND wop.article_id IS NOT NULL",
            [wo_id.into()],
        ))
        .await?;
    let r = row.ok_or_else(|| AppError::SyncError("spare stock aggregate missing.".into()))?;
    r.try_get::<f64>("", "q").map_err(|e| decode_err("q", e))
}

async fn work_order_has_spare_lines(db: &DatabaseConnection, wo_id: i64) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_order_parts WHERE work_order_id = ? AND article_id IS NOT NULL",
            [wo_id.into()],
        ))
        .await?;
    let c: i64 = row
        .ok_or_else(|| AppError::SyncError("work_order_parts count missing.".into()))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))?;
    Ok(c > 0)
}

pub async fn get_fmeca_severity_occurrence_matrix(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<FmecaSeverityOccurrenceMatrix> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT fi.severity, fi.occurrence, COUNT(*) AS c
             FROM fmeca_items fi
             INNER JOIN fmeca_analyses fa ON fa.id = fi.analysis_id
             WHERE fa.equipment_id = ?
             GROUP BY fi.severity, fi.occurrence",
            [equipment_id.into()],
        ))
        .await?;
    let mut map: HashMap<(i64, i64), i64> = HashMap::new();
    for r in &rows {
        let s: i64 = r.try_get("", "severity").map_err(|e| decode_err("severity", e))?;
        let o: i64 = r.try_get("", "occurrence").map_err(|e| decode_err("occurrence", e))?;
        let c: i64 = r.try_get("", "c").map_err(|e| decode_err("c", e))?;
        map.insert((s, o), c);
    }
    let mut cells: Vec<FmecaSoCell> = Vec::with_capacity(100);
    for s in 1_i64..=10 {
        for o in 1_i64..=10 {
            let count = map.get(&(s, o)).copied().unwrap_or(0);
            cells.push(FmecaSoCell {
                severity: s,
                occurrence: o,
                count,
            });
        }
    }
    Ok(FmecaSeverityOccurrenceMatrix {
        equipment_id,
        cells,
    })
}

fn map_fmeca_item_with_ctx(row: &sea_orm::QueryResult) -> AppResult<FmecaItemWithContext> {
    let item = map_fmeca_item(row)?;
    let analysis_title: String = row
        .try_get("", "analysis_title")
        .map_err(|e| decode_err("analysis_title", e))?;
    let equipment_ctx: i64 = row
        .try_get("", "equipment_id")
        .map_err(|e| decode_err("equipment_id", e))?;
    Ok(FmecaItemWithContext {
        item,
        analysis_title,
        equipment_id: equipment_ctx,
        spare_stock_total: None,
        inventory_status: "not_applicable".into(),
    })
}

pub async fn list_fmeca_items_for_equipment(
    db: &DatabaseConnection,
    filter: FmecaItemsEquipmentFilter,
) -> AppResult<Vec<FmecaItemWithContext>> {
    let lim = filter.limit.unwrap_or(500).clamp(1, 2000);
    let mut sql = String::from(
        "SELECT fi.id AS id, fi.entity_sync_id AS entity_sync_id, fi.analysis_id AS analysis_id, \
         fi.component_id AS component_id, fi.functional_failure AS functional_failure, \
         fi.failure_mode_id AS failure_mode_id, fi.failure_effect AS failure_effect, \
         fi.severity AS severity, fi.occurrence AS occurrence, fi.detectability AS detectability, \
         fi.rpn AS rpn, fi.recommended_action AS recommended_action, fi.current_control AS current_control, \
         fi.linked_pm_plan_id AS linked_pm_plan_id, fi.linked_work_order_id AS linked_work_order_id, \
         fi.revised_rpn AS revised_rpn, \
         fi.source_ram_ishikawa_diagram_id AS source_ram_ishikawa_diagram_id, \
         fi.source_ishikawa_flow_node_id AS source_ishikawa_flow_node_id, \
         fi.row_version AS row_version, fi.updated_at AS updated_at, \
         fa.title AS analysis_title, fa.equipment_id AS equipment_id \
         FROM fmeca_items fi \
         INNER JOIN fmeca_analyses fa ON fa.id = fi.analysis_id \
         WHERE fa.equipment_id = ?",
    );
    let mut vals: Vec<sea_orm::Value> = vec![filter.equipment_id.into()];
    if let Some(sv) = filter.severity {
        sql.push_str(" AND fi.severity = ?");
        vals.push(sv.into());
    }
    if let Some(ov) = filter.occurrence {
        sql.push_str(" AND fi.occurrence = ?");
        vals.push(ov.into());
    }
    sql.push_str(" ORDER BY fi.rpn DESC, fi.id ASC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    let mut out: Vec<FmecaItemWithContext> = rows.iter().map(map_fmeca_item_with_ctx).collect::<Result<Vec<_>, _>>()?;
    let rpn_critical = fmeca_rpn_critical_threshold_i64(db).await?;
    for row in &mut out {
        if row.item.rpn <= rpn_critical {
            row.inventory_status = "not_applicable".into();
            continue;
        }
        let Some(wo_id) = row.item.linked_work_order_id else {
            row.inventory_status = "no_wo_link".into();
            continue;
        };
        if !work_order_has_spare_lines(db, wo_id).await? {
            row.inventory_status = "no_spare_lines".into();
            continue;
        }
        let qty = sum_spare_stock_for_work_order(db, wo_id).await?;
        row.spare_stock_total = Some(qty);
        row.inventory_status = if qty <= 0.0 {
            "critical_shortage".into()
        } else {
            "ok".into()
        };
    }
    Ok(out)
}

pub async fn get_reliability_rul_indicator(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<ReliabilityRulIndicator> {
    let end = Utc::now();
    let start = end - Duration::days(365);
    let ev = evaluate_reliability_analysis_input(
        db,
        RefreshReliabilityKpiSnapshotInput {
            equipment_id,
            period_start: start.to_rfc3339(),
            period_end: end.to_rfc3339(),
            min_sample_n: Some(1),
            repeat_lookback_days: None,
        },
    )
    .await?;
    let exposure_t = ev.exposure_hours.max(0.0);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT beta, eta FROM weibull_fit_results
             WHERE equipment_id = ? AND beta IS NOT NULL AND eta IS NOT NULL AND beta > 0 AND eta > 0
             ORDER BY id DESC LIMIT 1",
            [equipment_id.into()],
        ))
        .await?;

    let (beta, eta) = match row {
        Some(r) => {
            let b: f64 = r.try_get("", "beta").map_err(|e| decode_err("beta", e))?;
            let e: f64 = r.try_get("", "eta").map_err(|e| decode_err("eta", e))?;
            (b, e)
        }
        None => {
            return Ok(ReliabilityRulIndicator {
                equipment_id,
                weibull_beta: None,
                weibull_eta_hours: None,
                reliability_at_t: None,
                predicted_rul_hours: None,
                t_hours: Some(exposure_t),
                message: "No valid Weibull fit in database for this equipment.".into(),
            });
        }
    };

    let t_eff = exposure_t;
    let r_now = weibull_r(beta, eta, t_eff);
    let rul = weibull_residual_median_hours(beta, eta, t_eff);

    Ok(ReliabilityRulIndicator {
        equipment_id,
        weibull_beta: Some(beta),
        weibull_eta_hours: Some(eta),
        reliability_at_t: r_now,
        predicted_rul_hours: rul,
        t_hours: Some(t_eff),
        message: format!(
            "t = {:.1} h (exposure hours from maintenance / operating profile); R(t) and RUL from Weibull fit (β={:.4}, η={:.1} h).",
            t_eff, beta, eta
        ),
    })
}

fn map_ram_ishikawa(row: &sea_orm::QueryResult) -> AppResult<RamIshikawaDiagram> {
    Ok(RamIshikawaDiagram {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        flow_json: row.try_get("", "flow_json").map_err(|e| decode_err("flow_json", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_ram_ishikawa_diagrams(
    db: &DatabaseConnection,
    filter: RamIshikawaDiagramsFilter,
) -> AppResult<Vec<RamIshikawaDiagram>> {
    let lim = filter.limit.unwrap_or(50).clamp(1, 200);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, title, flow_json, row_version, created_at, updated_at \
         FROM ram_ishikawa_diagrams WHERE 1=1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    rows.iter().map(map_ram_ishikawa).collect()
}

pub async fn upsert_ram_ishikawa_diagram(
    db: &DatabaseConnection,
    _user_id: Option<i32>,
    input: UpsertRamIshikawaDiagramInput,
) -> AppResult<RamIshikawaDiagram> {
    let now = Utc::now().to_rfc3339();
    let _ = _user_id;
    if let Some(id) = input.id {
        let exp = input
            .expected_row_version
            .ok_or_else(|| AppError::ValidationFailed(vec!["expected_row_version required.".into()]))?;
        let title = input.title.clone().unwrap_or_default();
        let n = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE ram_ishikawa_diagrams SET
                    title = COALESCE(?, title),
                    flow_json = ?,
                    row_version = row_version + 1,
                    updated_at = ?
                 WHERE id = ? AND row_version = ?",
                [
                    title.into(),
                    input.flow_json.into(),
                    now.clone().into(),
                    id.into(),
                    exp.into(),
                ],
            ))
            .await?
            .rows_affected();
        if n == 0 {
            return Err(AppError::ValidationFailed(vec!["ram_ishikawa_diagrams update conflict.".into()]));
        }
    } else {
        let eid = format!("ram_ishikawa:{}", Uuid::new_v4());
        let title = input.title.clone().unwrap_or_default();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO ram_ishikawa_diagrams (
                entity_sync_id, equipment_id, title, flow_json, row_version, created_at, updated_at
            ) VALUES (?, ?, ?, ?, 1, ?, ?)",
            [
                eid.into(),
                input.equipment_id.into(),
                title.into(),
                input.flow_json.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;
    }

    let nid = if let Some(id) = input.id {
        id
    } else {
        last_insert_id(db).await?
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, flow_json, row_version, created_at, updated_at
             FROM ram_ishikawa_diagrams WHERE id = ?",
            [nid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("ram_ishikawa_diagrams row missing.".into()))?;
    map_ram_ishikawa(&row)
}

pub async fn delete_ram_ishikawa_diagram(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM ram_ishikawa_diagrams WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}
