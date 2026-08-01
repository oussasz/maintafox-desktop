use crate::errors::AppResult;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DataIntegrityFindingRow {
    pub id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
    pub severity: String,
    pub domain: String,
    pub record_class: String,
    pub record_id: i64,
    pub finding_code: String,
    pub details_json: String,
    pub detected_at: String,
    pub cleared_at: Option<String>,
    pub status: String,
    pub waiver_reason: Option<String>,
    pub waiver_approver_id: Option<i64>,
}

fn map_row(row: &sea_orm::QueryResult) -> AppResult<DataIntegrityFindingRow> {
    Ok(DataIntegrityFindingRow {
        id: row.try_get("", "id").map_err(|e| map_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| map_err("entity_sync_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| map_err("row_version", e))?,
        severity: row.try_get("", "severity").map_err(|e| map_err("severity", e))?,
        domain: row.try_get("", "domain").map_err(|e| map_err("domain", e))?,
        record_class: row.try_get("", "record_class").map_err(|e| map_err("record_class", e))?,
        record_id: row.try_get("", "record_id").map_err(|e| map_err("record_id", e))?,
        finding_code: row.try_get("", "finding_code").map_err(|e| map_err("finding_code", e))?,
        details_json: row
            .try_get("", "details_json")
            .map_err(|e| map_err("details_json", e))?,
        detected_at: row.try_get("", "detected_at").map_err(|e| map_err("detected_at", e))?,
        cleared_at: row.try_get("", "cleared_at").ok().flatten(),
        status: row.try_get("", "status").map_err(|e| map_err("status", e))?,
        waiver_reason: row.try_get("", "waiver_reason").ok().flatten(),
        waiver_approver_id: row.try_get("", "waiver_approver_id").ok().flatten(),
    })
}

fn map_err(col: &str, e: sea_orm::DbErr) -> crate::errors::AppError {
    crate::errors::AppError::Internal(anyhow::anyhow!("data_integrity row {col}: {e}"))
}

pub async fn list_open_findings(
    db: &DatabaseConnection,
    limit: i64,
) -> AppResult<Vec<DataIntegrityFindingRow>> {
    let lim = limit.clamp(1, 500);
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT id, entity_sync_id, row_version, severity, domain, record_class, record_id, \
                 finding_code, details_json, detected_at, cleared_at, status, waiver_reason, waiver_approver_id \
                 FROM data_integrity_findings WHERE status = 'open' ORDER BY detected_at DESC LIMIT {lim}"
            ),
        ))
        .await?;
    rows.iter().map(map_row).collect()
}

pub async fn get_finding(
    db: &impl ConnectionTrait,
    id: i64,
) -> AppResult<Option<DataIntegrityFindingRow>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, row_version, severity, domain, record_class, record_id, \
             finding_code, details_json, detected_at, cleared_at, status, waiver_reason, waiver_approver_id \
             FROM data_integrity_findings WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.map(|r| map_row(&r)).transpose()
}

pub async fn last_insert_rowid(db: &impl ConnectionTrait) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| {
            crate::errors::AppError::Internal(anyhow::anyhow!("last_insert_rowid"))
        })?;
    row.try_get("", "id").map_err(|e| map_err("id", e))
}
