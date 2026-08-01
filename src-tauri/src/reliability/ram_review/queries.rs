use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::reliability::ram_review::domain::{
    CreateRamExpertSignOffInput, RamExpertSignOff, RamExpertSignOffsFilter, SignRamExpertReviewInput,
    UpdateRamExpertSignOffInput,
};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("ram_review decode '{field}': {err}"))
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

fn map_row(row: &sea_orm::QueryResult) -> AppResult<RamExpertSignOff> {
    Ok(RamExpertSignOff {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?,
        method_category: row.try_get("", "method_category").map_err(|e| decode_err("method_category", e))?,
        target_ref: row.try_get::<Option<String>>("", "target_ref").map_err(|e| decode_err("target_ref", e))?,
        title: row.try_get("", "title").map_err(|e| decode_err("title", e))?,
        reviewer_name: row.try_get("", "reviewer_name").map_err(|e| decode_err("reviewer_name", e))?,
        reviewer_role: row.try_get("", "reviewer_role").map_err(|e| decode_err("reviewer_role", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        signed_at: row.try_get::<Option<String>>("", "signed_at").map_err(|e| decode_err("signed_at", e))?,
        notes: row.try_get("", "notes").map_err(|e| decode_err("notes", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        created_by_id: row.try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub async fn list_ram_expert_sign_offs(
    db: &DatabaseConnection,
    filter: RamExpertSignOffsFilter,
) -> AppResult<Vec<RamExpertSignOff>> {
    let lim = filter.limit.unwrap_or(100).clamp(1, 500);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, equipment_id, method_category, target_ref, title, reviewer_name, reviewer_role, status, signed_at, notes, row_version, created_at, created_by_id, updated_at FROM ram_expert_sign_offs WHERE 1=1",
    );
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(eid) = filter.equipment_id {
        sql.push_str(" AND equipment_id = ?");
        vals.push(eid.into());
    }
    if let Some(ref mc) = filter.method_category {
        sql.push_str(" AND method_category = ?");
        vals.push(mc.clone().into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    vals.push(lim.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?;
    rows.iter().map(map_row).collect()
}

pub async fn create_ram_expert_sign_off(
    db: &DatabaseConnection,
    user_id: Option<i64>,
    input: CreateRamExpertSignOffInput,
) -> AppResult<RamExpertSignOff> {
    let now = Utc::now().to_rfc3339();
    let eid = format!("ram_signoff:{}", Uuid::new_v4());
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::ValidationFailed(vec!["title required.".into()]));
    }
    let rn = input.reviewer_name.unwrap_or_default();
    let rr = input.reviewer_role.unwrap_or_default();
    let notes = input.notes.unwrap_or_default();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO ram_expert_sign_offs (entity_sync_id, equipment_id, method_category, target_ref, title, reviewer_name, reviewer_role, status, signed_at, notes, row_version, created_at, created_by_id, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'draft', NULL, ?, 1, ?, ?, ?)",
        [
            eid.into(),
            input.equipment_id.into(),
            input.method_category.into(),
            input.target_ref.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<String>)),
            title.into(),
            rn.into(),
            rr.into(),
            notes.into(),
            now.clone().into(),
            user_id.map(sea_orm::Value::from).unwrap_or_else(|| sea_orm::Value::from(None::<i64>)),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_id(db).await?;
    load_by_id(db, id).await
}

pub async fn update_ram_expert_sign_off(
    db: &DatabaseConnection,
    input: UpdateRamExpertSignOffInput,
) -> AppResult<RamExpertSignOff> {
    let now = Utc::now().to_rfc3339();
    let mut sets = Vec::new();
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(t) = input.title {
        sets.push("title = ?");
        vals.push(t.into());
    }
    if let Some(n) = input.reviewer_name {
        sets.push("reviewer_name = ?");
        vals.push(n.into());
    }
    if let Some(r) = input.reviewer_role {
        sets.push("reviewer_role = ?");
        vals.push(r.into());
    }
    if let Some(n) = input.notes {
        sets.push("notes = ?");
        vals.push(n.into());
    }
    if let Some(tr) = input.target_ref {
        sets.push("target_ref = ?");
        vals.push(tr.into());
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
        "UPDATE ram_expert_sign_offs SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    let n = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, vals))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["ram_expert_sign_offs update conflict.".into()]));
    }
    load_by_id(db, input.id).await
}

pub async fn sign_ram_expert_review(
    db: &DatabaseConnection,
    input: SignRamExpertReviewInput,
) -> AppResult<RamExpertSignOff> {
    let now = Utc::now().to_rfc3339();
    let notes = input.notes.unwrap_or_default();
    let n = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE ram_expert_sign_offs SET status = 'signed', signed_at = ?, reviewer_name = ?, notes = ?, row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
            [
                now.clone().into(),
                input.reviewer_name.into(),
                notes.into(),
                now.clone().into(),
                input.id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::ValidationFailed(vec!["sign-off conflict.".into()]));
    }
    load_by_id(db, input.id).await
}

async fn load_by_id(db: &DatabaseConnection, id: i64) -> AppResult<RamExpertSignOff> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, equipment_id, method_category, target_ref, title, reviewer_name, reviewer_role, status, signed_at, notes, row_version, created_at, created_by_id, updated_at FROM ram_expert_sign_offs WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("ram_expert_sign_offs not found.".into()))?;
    map_row(&row)
}

pub async fn delete_ram_expert_sign_off(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM ram_expert_sign_offs WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}
