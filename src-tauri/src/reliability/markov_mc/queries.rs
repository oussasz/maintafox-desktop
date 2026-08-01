use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::reliability::markov_mc::domain::{
    CreateMarkovModelInput, CreateMcModelInput, MarkovModel, MarkovModelsFilter, McModel, McModelsFilter,
    UpdateMarkovModelInput, UpdateMcModelInput,
};
use crate::reliability::markov_mc::guardrails::{self, GuardrailFlags};
use crate::reliability::markov_mc::mc_eval::evaluate_mc;
use crate::reliability::markov_mc::markov_solve::{solve_dtmc_steady_state, MarkovSpec};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("markov_mc decode '{field}': {err}"))
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

fn map_mc(row: &sea_orm::QueryResult) -> AppResult<McModel> {
    Ok(McModel {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        graph_json: row.try_get("", "graph_json").map_err(|e| decode_err("graph_json", e))?,
        trials: row.try_get("", "trials").map_err(|e| decode_err("trials", e))?,
        seed: row.try_get::<Option<i64>>("", "seed").map_err(|e| decode_err("seed", e))?,
        result_json: row.try_get("", "result_json").map_err(|e| decode_err("result_json", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_markov(row: &sea_orm::QueryResult) -> AppResult<MarkovModel> {
    Ok(MarkovModel {
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

pub async fn get_ram_advanced_guardrails(db: &DatabaseConnection) -> AppResult<GuardrailFlags> {
    guardrails::load_guardrails(db).await
}

pub async fn set_ram_advanced_guardrails(db: &DatabaseConnection, flags: &GuardrailFlags) -> AppResult<()> {
    guardrails::save_guardrails(db, flags).await
}

pub async fn list_mc_models(db: &DatabaseConnection, filter: McModelsFilter) -> AppResult<Vec<McModel>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let (sql, vals) = if let Some(eid) = filter.equipment_id {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, trials, seed, result_json, status, row_version, created_at, created_by_id, updated_at FROM mc_models WHERE equipment_id = ? ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![eid.into(), lim.into()],
        )
    } else {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, trials, seed, result_json, status, row_version, created_at, created_by_id, updated_at FROM mc_models ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![lim.into()],
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    rows.iter().map(map_mc).collect()
}

pub async fn create_mc_model(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    input: CreateMcModelInput,
) -> AppResult<McModel> {
    let now = Utc::now().to_rfc3339();
    let g = input.graph_json.unwrap_or_else(|| "{}".to_string());
    let st = input.status.unwrap_or_else(|| "draft".to_string());
    let trials = input.trials.unwrap_or(10_000).max(1);
    let eid = format!("mc_model:{}", Uuid::new_v4());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO mc_models (entity_sync_id, equipment_id, title, graph_json, trials, seed, result_json, status, row_version, created_at, created_by_id, updated_at) VALUES (?, ?, ?, ?, ?, ?, '{}', ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.title.into(),
            g.into(),
            trials.into(),
            input.seed.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            st.into(),
            now.clone().into(),
            user_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    load_mc_by_id(db, id).await
}

pub async fn update_mc_model(db: &DatabaseConnection, input: UpdateMcModelInput) -> AppResult<McModel> {
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
    if let Some(tr) = input.trials {
        sets.push("trials = ?");
        vals.push(tr.max(1).into());
    }
    if let Some(s) = input.seed {
        sets.push("seed = ?");
        vals.push(s.into());
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
        "UPDATE mc_models SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    let n = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["mc_models update conflict.".into()]));
    }
    load_mc_by_id(db, input.id).await
}

async fn load_mc_by_id(db: &DatabaseConnection, id: i64) -> AppResult<McModel> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, trials, seed, result_json, status, row_version, created_at, created_by_id, updated_at FROM mc_models WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("mc_model not found.".into()))?;
    map_mc(&row)
}

pub async fn delete_mc_model(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM mc_models WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn evaluate_mc_model(db: &DatabaseConnection, id: i64) -> AppResult<McModel> {
    let g = guardrails::load_guardrails(db).await?;
    if !g.monte_carlo_enabled {
        return Err(AppError::ValidationFailed(vec!["Monte Carlo disabled by guardrails.".into()]));
    }
    let m = load_mc_by_id(db, id).await?;
    if m.trials > g.mc_max_trials {
        return Err(AppError::ValidationFailed(vec![format!(
            "trials {} exceeds mc_max_trials {}",
            m.trials, g.mc_max_trials
        )]));
    }
    let ev = evaluate_mc(&m.graph_json, m.trials, m.seed).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let result_json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE mc_models SET result_json = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [result_json.into(), now.clone().into(), id.into()],
    ))
    .await?;
    load_mc_by_id(db, id).await
}

pub async fn list_markov_models(db: &DatabaseConnection, filter: MarkovModelsFilter) -> AppResult<Vec<MarkovModel>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let (sql, vals) = if let Some(eid) = filter.equipment_id {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM markov_models WHERE equipment_id = ? ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![eid.into(), lim.into()],
        )
    } else {
        (
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM markov_models ORDER BY id DESC LIMIT ?"
                .to_string(),
            vec![lim.into()],
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    rows.iter().map(map_markov).collect()
}

pub async fn create_markov_model(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    input: CreateMarkovModelInput,
) -> AppResult<MarkovModel> {
    let now = Utc::now().to_rfc3339();
    let g = input.graph_json.unwrap_or_else(|| "{}".to_string());
    let st = input.status.unwrap_or_else(|| "draft".to_string());
    let eid = format!("markov_model:{}", Uuid::new_v4());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO markov_models (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at) VALUES (?, ?, ?, ?, '{}', ?, 1, ?, ?, ?)",
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
    load_markov_by_id(db, id).await
}

pub async fn update_markov_model(db: &DatabaseConnection, input: UpdateMarkovModelInput) -> AppResult<MarkovModel> {
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
        "UPDATE markov_models SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    let n = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["markov_models update conflict.".into()]));
    }
    load_markov_by_id(db, input.id).await
}

async fn load_markov_by_id(db: &DatabaseConnection, id: i64) -> AppResult<MarkovModel> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at FROM markov_models WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("markov_model not found.".into()))?;
    map_markov(&row)
}

pub async fn delete_markov_model(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM markov_models WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn evaluate_markov_model(db: &DatabaseConnection, id: i64) -> AppResult<MarkovModel> {
    let g = guardrails::load_guardrails(db).await?;
    if !g.markov_enabled {
        return Err(AppError::ValidationFailed(vec!["Markov disabled by guardrails.".into()]));
    }
    let m = load_markov_by_id(db, id).await?;
    let spec: MarkovSpec = serde_json::from_str(&m.graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Markov graph_json: {e}")]))?;
    let n = spec.states.len() as i64;
    if n > g.markov_max_states {
        return Err(AppError::ValidationFailed(vec![format!(
            "states {n} exceeds markov_max_states {}",
            g.markov_max_states
        )]));
    }
    let ev = solve_dtmc_steady_state(&spec, 100_000, 1e-12).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let result_json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE markov_models SET result_json = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [result_json.into(), now.clone().into(), id.into()],
    ))
    .await?;
    load_markov_by_id(db, id).await
}
