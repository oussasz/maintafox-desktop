use crate::errors::AppResult;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsContractVersionRow {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub contract_id: String,
    pub version_semver: String,
    pub content_sha256: String,
    pub activated_at: String,
}

fn map_row(row: &sea_orm::QueryResult) -> AppResult<AnalyticsContractVersionRow> {
    Ok(AnalyticsContractVersionRow {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| map_err("entity_sync_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| map_err("row_version", e))?,
        contract_id: row.try_get("", "contract_id").map_err(|e| map_err("contract_id", e))?,
        version_semver: row.try_get("", "version_semver").map_err(|e| map_err("version_semver", e))?,
        content_sha256: row.try_get("", "content_sha256").map_err(|e| map_err("content_sha256", e))?,
        activated_at: row.try_get("", "activated_at").map_err(|e| map_err("activated_at", e))?,
    })
}

fn map_err(col: &str, e: sea_orm::DbErr) -> crate::errors::AppError {
    crate::errors::AppError::Internal(anyhow::anyhow!("analytics_contract {col}: {e}"))
}

pub async fn list_contract_versions(db: &DatabaseConnection) -> AppResult<Vec<AnalyticsContractVersionRow>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, row_version, contract_id, version_semver, content_sha256, activated_at \
             FROM analytics_contract_versions ORDER BY activated_at DESC"
                .to_string(),
        ))
        .await?;
    rows.iter().map(map_row).collect()
}

pub async fn insert_contract_version(
    db: &impl ConnectionTrait,
    entity_sync_id: &str,
    contract_id: &str,
    version_semver: &str,
    content_sha256: &str,
) -> AppResult<i64> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO analytics_contract_versions \
         (entity_sync_id, row_version, contract_id, version_semver, content_sha256) \
         VALUES (?, 1, ?, ?, ?)",
        [
            entity_sync_id.into(),
            contract_id.into(),
            version_semver.into(),
            content_sha256.into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    row.try_get("", "id").map_err(|e| map_err("id", e))
}

pub async fn get_contract_version_by_id(
    db: &impl ConnectionTrait,
    id: i64,
) -> AppResult<Option<AnalyticsContractVersionRow>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, row_version, contract_id, version_semver, content_sha256, activated_at \
             FROM analytics_contract_versions WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.map(|r| map_row(&r)).transpose()
}
