use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
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
    let g: FtaGraph = serde_json::from_str(&m.graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("FTA graph_json: {e}")]))?;
    let ev = evaluate_fta(&g).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let result_json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
    let now = Utc::now().to_rfc3339();
    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE fta_models SET result_json = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
            [result_json.into(), now.clone().into(), id.into()],
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
    let g: RbdGraph = serde_json::from_str(&m.graph_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("RBD graph_json: {e}")]))?;
    let ev = evaluate_rbd(&g).map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let result_json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE rbd_models SET result_json = ?, row_version = row_version + 1, updated_at = ? WHERE id = ?",
        [result_json.into(), now.clone().into(), id.into()],
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
