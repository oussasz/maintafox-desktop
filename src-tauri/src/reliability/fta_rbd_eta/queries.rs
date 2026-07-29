use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::reliability::fta_rbd_eta::domain::{
    CreateEventTreeModelInput, CreateFtaModelInput, CreateRbdModelInput, EventTreeModel,
    EventTreeModelsFilter, FtaModel, FtaModelsFilter, RbdModel, RbdModelsFilter, UpdateEventTreeModelInput,
    UpdateFtaModelInput, UpdateRbdModelInput,
};
use crate::reliability::fta_rbd_eta::eta_eval::{evaluate_eta, EtaGraph};
use crate::reliability::fta_rbd_eta::fta_eval::{evaluate_fta, FtaGraph};
use crate::reliability::fta_rbd_eta::rbd_eval::{evaluate_rbd, RbdGraph};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("fta_rbd_eta decode '{field}': {err}"))
}

fn as_i64(v: Option<&Value>) -> Option<i64> {
    v.and_then(Value::as_i64)
}

fn as_f64(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn as_str<'a>(v: Option<&'a Value>) -> Option<&'a str> {
    v.and_then(Value::as_str)
}

fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

type EquipmentStats = (i64, f64, f64, f64);

async fn latest_rams_data_change_at(
    db: &DatabaseConnection,
    equipment_id: i64,
) -> AppResult<Option<String>> {
    let ev_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT MAX(COALESCE(updated_at, created_at)) AS ts
             FROM failure_events WHERE equipment_id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("failure_events max(updated_at) missing.".into()))?;
    let exp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT MAX(recorded_at) AS ts
             FROM runtime_exposure_logs WHERE equipment_id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("runtime_exposure_logs max(recorded_at) missing.".into()))?;
    let ev_ts = ev_row
        .try_get::<Option<String>>("", "ts")
        .map_err(|e| decode_err("failure_events.ts", e))?;
    let exp_ts = exp_row
        .try_get::<Option<String>>("", "ts")
        .map_err(|e| decode_err("runtime_exposure_logs.ts", e))?;
    Ok(match (ev_ts, exp_ts) {
        (Some(a), Some(b)) => Some(if a >= b { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    })
}

#[derive(Clone, Debug)]
struct NodeBindingConfig {
    equipment_id: i64,
    lookback_days: i64,
    mission_hours: f64,
    failure_mode_id: Option<i64>,
    source_metric: Option<String>,
}

fn parse_node_binding(
    node_id: &str,
    node_spec: &Value,
    default_equipment_id: i64,
) -> Result<Option<NodeBindingConfig>, String> {
    let Some(spec_obj) = node_spec.as_object() else {
        return Ok(None);
    };
    let Some(binding) = spec_obj.get("binding") else {
        return Ok(None);
    };
    let Some(bind_obj) = binding.as_object() else {
        return Err(format!("node '{node_id}': binding must be an object"));
    };

    let equipment_id = as_i64(bind_obj.get("equipment_id")).unwrap_or(default_equipment_id);
    if equipment_id <= 0 {
        return Err(format!("node '{node_id}': binding.equipment_id must be > 0"));
    }
    let lookback_days = as_i64(bind_obj.get("lookback_days"))
        .ok_or_else(|| format!("node '{node_id}': binding.lookback_days is required"))?;
    if !(1..=3650).contains(&lookback_days) {
        return Err(format!(
            "node '{node_id}': binding.lookback_days must be between 1 and 3650"
        ));
    }
    let mission_hours = as_f64(bind_obj.get("mission_hours"))
        .ok_or_else(|| format!("node '{node_id}': binding.mission_hours is required"))?;
    if !(mission_hours.is_finite() && mission_hours > 0.0) {
        return Err(format!(
            "node '{node_id}': binding.mission_hours must be a positive finite number"
        ));
    }
    let failure_mode_id = as_i64(bind_obj.get("failure_mode_id"));
    if let Some(fm) = failure_mode_id {
        if fm <= 0 {
            return Err(format!(
                "node '{node_id}': binding.failure_mode_id must be > 0 when provided"
            ));
        }
    }

    let source_metric = as_str(bind_obj.get("source_metric")).map(str::to_string);
    if let Some(sm) = &source_metric {
        if sm != "availability" && sm != "reliability" {
            return Err(format!(
                "node '{node_id}': binding.source_metric must be 'availability' or 'reliability'"
            ));
        }
    }

    Ok(Some(NodeBindingConfig {
        equipment_id,
        lookback_days,
        mission_hours,
        failure_mode_id,
        source_metric,
    }))
}

async fn fetch_equipment_stats(
    db: &DatabaseConnection,
    equipment_id: i64,
    lookback_days: i64,
    failure_mode_id: Option<i64>,
) -> AppResult<EquipmentStats> {
    let days = lookback_days.clamp(1, 3650);
    let since = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();

    let mut ev_sql = String::from(
        "SELECT
            COUNT(*) AS c,
            COALESCE(SUM(downtime_duration_hours), 0.0) AS downtime_h,
            COALESCE(SUM(active_repair_hours), 0.0) AS repair_h
         FROM failure_events
         WHERE equipment_id = ?
           AND COALESCE(failed_at, detected_at, created_at) >= ?",
    );
    let mut ev_vals: Vec<sea_orm::Value> = vec![equipment_id.into(), since.clone().into()];
    if let Some(fm) = failure_mode_id {
        ev_sql.push_str(" AND failure_mode_id = ?");
        ev_vals.push(fm.into());
    }
    let ev_row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, ev_sql, ev_vals))
        .await?
        .ok_or_else(|| AppError::SyncError("failure_events stats row missing.".into()))?;
    let count: i64 = ev_row.try_get("", "c").map_err(|e| decode_err("c", e))?;
    let downtime_h: f64 = ev_row
        .try_get("", "downtime_h")
        .map_err(|e| decode_err("downtime_h", e))?;
    let repair_h: f64 = ev_row
        .try_get("", "repair_h")
        .map_err(|e| decode_err("repair_h", e))?;

    let exp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(value), 0.0) AS exposure_h
             FROM runtime_exposure_logs
             WHERE equipment_id = ?
               AND recorded_at >= ?",
            [equipment_id.into(), since.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("runtime_exposure_logs stats row missing.".into()))?;
    let exposure_h: f64 = exp_row
        .try_get("", "exposure_h")
        .map_err(|e| decode_err("exposure_h", e))?;
    Ok((count, downtime_h, repair_h, exposure_h))
}

async fn resolve_fta_graph_from_real_data(
    db: &DatabaseConnection,
    equipment_id: i64,
    graph_json: &str,
) -> AppResult<(String, Value)> {
    let mut root: Value = serde_json::from_str(graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("FTA graph_json: {e}")]))?;
    let Some(nodes) = root.get_mut("nodes").and_then(Value::as_object_mut) else {
        return Ok((graph_json.to_string(), json!({ "bindings": [] })));
    };

    let mut bindings: Vec<Value> = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    for (node_id, node_spec) in nodes.iter_mut() {
        let is_basic = node_spec
            .as_object()
            .and_then(|o| as_str(o.get("kind")))
            .map(|k| k == "basic")
            .unwrap_or(false);
        if !is_basic {
            continue;
        }

        let p_manual = node_spec
            .as_object()
            .and_then(|o| as_f64(o.get("p")))
            .ok_or_else(|| {
                AppError::ValidationFailed(vec![format!(
                    "node '{node_id}': basic event requires numeric field 'p' for manual mode"
                )])
            })?;
        let binding_cfg = match parse_node_binding(node_id, node_spec, equipment_id) {
            Ok(v) => v,
            Err(msg) => {
                issues.push(msg);
                continue;
            }
        };

        let Some(spec) = node_spec.as_object_mut() else {
            continue;
        };
        let Some(cfg) = binding_cfg else {
            bindings.push(json!({
                "node_id": node_id,
                "metric": "failure_probability",
                "resolved_value": clamp01(p_manual),
                "mode": "manual_static"
            }));
            continue;
        };

        let (event_count, downtime_h, _repair_h, exposure_h) =
            fetch_equipment_stats(db, cfg.equipment_id, cfg.lookback_days, cfg.failure_mode_id).await?;

        if exposure_h <= 0.0 {
            issues.push(format!(
                "node '{node_id}': no runtime exposure in lookback window (equipment_id={}, lookback_days={})",
                cfg.equipment_id, cfg.lookback_days
            ));
            continue;
        }

        let lambda = (event_count as f64 / exposure_h).max(0.0);
        let p_real = clamp01(1.0 - (-lambda * cfg.mission_hours).exp());
        spec.insert("p".into(), json!(p_real));

        bindings.push(json!({
            "node_id": node_id,
            "metric": "failure_probability",
            "resolved_value": p_real,
            "manual_value": clamp01(p_manual),
            "equipment_id": cfg.equipment_id,
            "failure_mode_id": cfg.failure_mode_id,
            "lookback_days": cfg.lookback_days,
            "mission_hours": cfg.mission_hours,
            "event_count": event_count,
            "downtime_hours": downtime_h,
            "exposure_hours": exposure_h,
            "method": "poisson_from_failure_rate",
            "mode": "data_bound"
        }));
    }

    if !issues.is_empty() {
        return Err(AppError::ValidationFailed(issues));
    }

    Ok((
        serde_json::to_string(&root).unwrap_or_else(|_| graph_json.to_string()),
        json!({ "bindings": bindings }),
    ))
}

async fn resolve_rbd_graph_from_real_data(
    db: &DatabaseConnection,
    equipment_id: i64,
    graph_json: &str,
) -> AppResult<(String, Value)> {
    let mut root: Value = serde_json::from_str(graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("RBD graph_json: {e}")]))?;
    let Some(nodes) = root.get_mut("nodes").and_then(Value::as_object_mut) else {
        return Ok((graph_json.to_string(), json!({ "bindings": [] })));
    };

    let mut bindings: Vec<Value> = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    for (node_id, node_spec) in nodes.iter_mut() {
        let is_block = node_spec
            .as_object()
            .and_then(|o| as_str(o.get("kind")))
            .map(|k| k == "block")
            .unwrap_or(false);
        if !is_block {
            continue;
        }

        let r_manual = node_spec
            .as_object()
            .and_then(|o| as_f64(o.get("r")))
            .ok_or_else(|| {
                AppError::ValidationFailed(vec![format!(
                    "node '{node_id}': block requires numeric field 'r' for manual mode"
                )])
            })?;

        let binding_cfg = match parse_node_binding(node_id, node_spec, equipment_id) {
            Ok(v) => v,
            Err(msg) => {
                issues.push(msg);
                continue;
            }
        };

        let Some(spec) = node_spec.as_object_mut() else {
            continue;
        };
        let Some(cfg) = binding_cfg else {
            bindings.push(json!({
                "node_id": node_id,
                "metric": "reliability",
                "resolved_value": clamp01(r_manual),
                "mode": "manual_static"
            }));
            continue;
        };

        let (event_count, downtime_h, _repair_h, exposure_h) =
            fetch_equipment_stats(db, cfg.equipment_id, cfg.lookback_days, cfg.failure_mode_id).await?;

        if exposure_h <= 0.0 {
            issues.push(format!(
                "node '{node_id}': no runtime exposure in lookback window (equipment_id={}, lookback_days={})",
                cfg.equipment_id, cfg.lookback_days
            ));
            continue;
        }

        let lambda = (event_count as f64 / exposure_h).max(0.0);
        let availability = clamp01(1.0 - downtime_h / exposure_h);
        let reliability = clamp01((-lambda * cfg.mission_hours).exp());
        let metric = cfg
            .source_metric
            .clone()
            .unwrap_or_else(|| "availability".to_string());
        let r_real = if metric == "reliability" {
            reliability
        } else {
            availability
        };
        spec.insert("r".into(), json!(r_real));

        bindings.push(json!({
            "node_id": node_id,
            "metric": "reliability",
            "resolved_value": r_real,
            "manual_value": clamp01(r_manual),
            "source_metric": metric,
            "equipment_id": cfg.equipment_id,
            "failure_mode_id": cfg.failure_mode_id,
            "lookback_days": cfg.lookback_days,
            "mission_hours": cfg.mission_hours,
            "event_count": event_count,
            "downtime_hours": downtime_h,
            "exposure_hours": exposure_h,
            "method": if metric == "reliability" { "exp_minus_lambda_t" } else { "1_minus_downtime_over_exposure" },
            "mode": "data_bound"
        }));
    }

    if !issues.is_empty() {
        return Err(AppError::ValidationFailed(issues));
    }

    Ok((
        serde_json::to_string(&root).unwrap_or_else(|_| graph_json.to_string()),
        json!({ "bindings": bindings }),
    ))
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

fn map_fta(row: &sea_orm::QueryResult) -> AppResult<FtaModel> {
    Ok(FtaModel {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        graph_json: row.try_get("", "graph_json").map_err(|e| decode_err("graph_json", e))?,
        result_json: row.try_get("", "result_json").map_err(|e| decode_err("result_json", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_rbd(row: &sea_orm::QueryResult) -> AppResult<RbdModel> {
    Ok(RbdModel {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        graph_json: row.try_get("", "graph_json").map_err(|e| decode_err("graph_json", e))?,
        result_json: row.try_get("", "result_json").map_err(|e| decode_err("result_json", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_eta(row: &sea_orm::QueryResult) -> AppResult<EventTreeModel> {
    Ok(EventTreeModel {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        graph_json: row.try_get("", "graph_json").map_err(|e| decode_err("graph_json", e))?,
        result_json: row.try_get("", "result_json").map_err(|e| decode_err("result_json", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_fta_models(db: &DatabaseConnection, filter: FtaModelsFilter) -> AppResult<Vec<FtaModel>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let (sql, vals) = if let Some(eid) = filter.equipment_id {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM fta_models WHERE equipment_id = ? ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![eid.into(), lim.into()],
        )
    } else {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM fta_models ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![lim.into()],
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    rows.iter().map(map_fta).collect()
}

pub async fn create_fta_model(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    input: CreateFtaModelInput,
) -> AppResult<FtaModel> {
    let now = Utc::now().to_rfc3339();
    let g = input.graph_json.unwrap_or_else(|| "{}".to_string());
    let st = input.status.unwrap_or_else(|| "draft".to_string());
    let eid = format!("fta_model:{}", Uuid::new_v4());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO fta_models (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at) VALUES (?, ?, ?, ?, '{}', ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.title.into(),
            g.into(),
            st.into(),
            now.clone().into(),
            user_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    load_fta_by_id(db, id).await
}

pub async fn update_fta_model(db: &DatabaseConnection, input: UpdateFtaModelInput) -> AppResult<FtaModel> {
    let now = Utc::now().to_rfc3339();
    let mut sets = Vec::new();
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(t) = input.title {
        sets.push("title = ?");
        vals.push(t.into());
    }
    if let Some(g) = input.graph_json {
        sets.push("graph_json = ?");
        vals.push(g.into());
    }
    if let Some(s) = input.status {
        sets.push("status = ?");
        vals.push(s.into());
    }
    if sets.is_empty() {
        return Err(AppError::ValidationFailed(vec!["no fields to update.".into()]));
    }
    sets.push("row_version = row_version + 1");
    sets.push("updated_at = ?");
    vals.push(now.into());
    vals.push(input.id.into());
    vals.push(input.expected_row_version.into());
    let sql = format!(
        "UPDATE fta_models SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    let n = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["fta_models update conflict.".into()]));
    }
    load_fta_by_id(db, input.id).await
}

async fn load_fta_by_id(db: &DatabaseConnection, id: i64) -> AppResult<FtaModel> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM fta_models WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("fta_model not found.".into()))?;
    map_fta(&row)
}

pub async fn delete_fta_model(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM fta_models WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn evaluate_fta_model(db: &DatabaseConnection, id: i64) -> AppResult<FtaModel> {
    let m = load_fta_by_id(db, id).await?;
    let (resolved_graph_json, binding_meta) =
        resolve_fta_graph_from_real_data(db, m.equipment_id, &m.graph_json).await?;
    let g: FtaGraph = serde_json::from_str(&resolved_graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("FTA graph_json: {e}")]))?;
    let ev = evaluate_fta(&g).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let latest_data_at = latest_rams_data_change_at(db, m.equipment_id).await?;
    let evaluated_at = Utc::now().to_rfc3339();
    let result_payload = json!({
        "evaluation": ev,
        "data_binding": binding_meta,
        "freshness": {
            "evaluated_at": evaluated_at,
            "latest_data_at": latest_data_at,
            "is_stale": false
        }
    });
    let result_json = serde_json::to_string(&result_payload).unwrap_or_else(|_| "{}".to_string());
    let now = Utc::now().to_rfc3339();
    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE fta_models SET graph_json = ?, result_json = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
            [resolved_graph_json.into(), result_json.into(), now.clone().into(), id.into()],
        ))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::SyncError("fta evaluate failed.".into()));
    }
    load_fta_by_id(db, id).await
}

pub async fn list_rbd_models(db: &DatabaseConnection, filter: RbdModelsFilter) -> AppResult<Vec<RbdModel>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let (sql, vals) = if let Some(eid) = filter.equipment_id {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM rbd_models WHERE equipment_id = ? ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![eid.into(), lim.into()],
        )
    } else {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM rbd_models ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![lim.into()],
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    rows.iter().map(map_rbd).collect()
}

pub async fn create_rbd_model(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    input: CreateRbdModelInput,
) -> AppResult<RbdModel> {
    let now = Utc::now().to_rfc3339();
    let g = input.graph_json.unwrap_or_else(|| "{}".to_string());
    let st = input.status.unwrap_or_else(|| "draft".to_string());
    let eid = format!("rbd_model:{}", Uuid::new_v4());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO rbd_models (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at) VALUES (?, ?, ?, ?, '{}', ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.title.into(),
            g.into(),
            st.into(),
            now.clone().into(),
            user_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    load_rbd_by_id(db, id).await
}

pub async fn update_rbd_model(db: &DatabaseConnection, input: UpdateRbdModelInput) -> AppResult<RbdModel> {
    let now = Utc::now().to_rfc3339();
    let mut sets = Vec::new();
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(t) = input.title {
        sets.push("title = ?");
        vals.push(t.into());
    }
    if let Some(g) = input.graph_json {
        sets.push("graph_json = ?");
        vals.push(g.into());
    }
    if let Some(s) = input.status {
        sets.push("status = ?");
        vals.push(s.into());
    }
    if sets.is_empty() {
        return Err(AppError::ValidationFailed(vec!["no fields to update.".into()]));
    }
    sets.push("row_version = row_version + 1");
    sets.push("updated_at = ?");
    vals.push(now.into());
    vals.push(input.id.into());
    vals.push(input.expected_row_version.into());
    let sql = format!(
        "UPDATE rbd_models SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    let n = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["rbd_models update conflict.".into()]));
    }
    load_rbd_by_id(db, input.id).await
}

async fn load_rbd_by_id(db: &DatabaseConnection, id: i64) -> AppResult<RbdModel> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM rbd_models WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("rbd_model not found.".into()))?;
    map_rbd(&row)
}

pub async fn delete_rbd_model(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM rbd_models WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn evaluate_rbd_model(db: &DatabaseConnection, id: i64) -> AppResult<RbdModel> {
    let m = load_rbd_by_id(db, id).await?;
    let (resolved_graph_json, binding_meta) =
        resolve_rbd_graph_from_real_data(db, m.equipment_id, &m.graph_json).await?;
    let g: RbdGraph = serde_json::from_str(&resolved_graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("RBD graph_json: {e}")]))?;
    let ev = evaluate_rbd(&g).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let latest_data_at = latest_rams_data_change_at(db, m.equipment_id).await?;
    let evaluated_at = Utc::now().to_rfc3339();
    let result_payload = json!({
        "evaluation": ev,
        "data_binding": binding_meta,
        "freshness": {
            "evaluated_at": evaluated_at,
            "latest_data_at": latest_data_at,
            "is_stale": false
        }
    });
    let result_json = serde_json::to_string(&result_payload).unwrap_or_else(|_| "{}".to_string());
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE rbd_models SET graph_json = ?, result_json = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [resolved_graph_json.into(), result_json.into(), now.clone().into(), id.into()],
    ))
    .await?;
    load_rbd_by_id(db, id).await
}

pub async fn list_event_tree_models(
    db: &DatabaseConnection,
    filter: EventTreeModelsFilter,
) -> AppResult<Vec<EventTreeModel>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let (sql, vals) = if let Some(eid) = filter.equipment_id {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM event_tree_models WHERE equipment_id = ? ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![eid.into(), lim.into()],
        )
    } else {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM event_tree_models ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![lim.into()],
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    rows.iter().map(map_eta).collect()
}

pub async fn create_event_tree_model(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    input: CreateEventTreeModelInput,
) -> AppResult<EventTreeModel> {
    let now = Utc::now().to_rfc3339();
    let g = input.graph_json.unwrap_or_else(|| "{}".to_string());
    let st = input.status.unwrap_or_else(|| "draft".to_string());
    let eid = format!("event_tree_model:{}", Uuid::new_v4());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO event_tree_models (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at) VALUES (?, ?, ?, ?, '{}', ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.title.into(),
            g.into(),
            st.into(),
            now.clone().into(),
            user_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    load_eta_by_id(db, id).await
}

pub async fn update_event_tree_model(
    db: &DatabaseConnection,
    input: UpdateEventTreeModelInput,
) -> AppResult<EventTreeModel> {
    let now = Utc::now().to_rfc3339();
    let mut sets = Vec::new();
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(t) = input.title {
        sets.push("title = ?");
        vals.push(t.into());
    }
    if let Some(g) = input.graph_json {
        sets.push("graph_json = ?");
        vals.push(g.into());
    }
    if let Some(s) = input.status {
        sets.push("status = ?");
        vals.push(s.into());
    }
    if sets.is_empty() {
        return Err(AppError::ValidationFailed(vec!["no fields to update.".into()]));
    }
    sets.push("row_version = row_version + 1");
    sets.push("updated_at = ?");
    vals.push(now.into());
    vals.push(input.id.into());
    vals.push(input.expected_row_version.into());
    let sql = format!(
        "UPDATE event_tree_models SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    let n = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["event_tree_models update conflict.".into()]));
    }
    load_eta_by_id(db, input.id).await
}

async fn load_eta_by_id(db: &DatabaseConnection, id: i64) -> AppResult<EventTreeModel> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM event_tree_models WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("event_tree_model not found.".into()))?;
    map_eta(&row)
}

pub async fn delete_event_tree_model(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM event_tree_models WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn evaluate_event_tree_model(db: &DatabaseConnection, id: i64) -> AppResult<EventTreeModel> {
    let m = load_eta_by_id(db, id).await?;
    let g: EtaGraph = serde_json::from_str(&m.graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Event tree graph_json: {e}")]))?;
    let ev = evaluate_eta(&g).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let result_json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE event_tree_models SET result_json = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [result_json.into(), now.clone().into(), id.into()],
    ))
    .await?;
    load_eta_by_id(db, id).await
}
