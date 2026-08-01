use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::jobs::domain::ComputationJob;

fn dec(f: &str, e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("computation_jobs.{f}: {e}"))
}

async fn last_insert_id(db: &DatabaseConnection) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("last_insert_rowid missing.".into()))?;
    Ok(row.try_get("", "id").map_err(|e| dec("id", e))?)
}

pub async fn insert_computation_job(
    db: &DatabaseConnection,
    job_kind: &str,
    input_json: &str,
) -> AppResult<i64> {
    let eid = format!("computation_job:{}", Uuid::new_v4());
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO computation_jobs (
            entity_sync_id, job_kind, status, progress_pct, input_json, created_at, row_version
        ) VALUES (?, ?, 'pending', 0, ?, ?, 1)",
        [
            eid.into(),
            job_kind.into(),
            input_json.into(),
            now.into(),
        ],
    ))
    .await?;
    last_insert_id(db).await
}

pub async fn update_job_running(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE computation_jobs SET status = 'running', started_at = ?, progress_pct = 5,
         row_version = row_version + 1 WHERE id = ?",
        [now.into(), id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn update_job_progress(db: &DatabaseConnection, id: i64, progress_pct: f64) -> AppResult<()> {
    let p = progress_pct.clamp(0.0, 100.0);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE computation_jobs SET progress_pct = ?, row_version = row_version + 1 WHERE id = ?",
        [p.into(), id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn complete_job_success(db: &DatabaseConnection, id: i64, result_json: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE computation_jobs SET status = 'completed', progress_pct = 100, result_json = ?,
         finished_at = ?, error_message = NULL, row_version = row_version + 1 WHERE id = ?",
        [result_json.into(), now.into(), id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn complete_job_failed(db: &DatabaseConnection, id: i64, message: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE computation_jobs SET status = 'failed', error_message = ?, finished_at = ?,
         row_version = row_version + 1 WHERE id = ?",
        [message.into(), now.into(), id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn complete_job_cancelled(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE computation_jobs SET status = 'cancelled', finished_at = ?, error_message = 'cancelled',
         row_version = row_version + 1 WHERE id = ?",
        [now.into(), id.into()],
    ))
    .await?;
    Ok(())
}

fn map_job(row: &sea_orm::QueryResult) -> AppResult<ComputationJob> {
    Ok(ComputationJob {
        id: row.try_get("", "id").map_err(|e| dec("id", e))?,
        entity_sync_id: row.try_get("", "entity_sync_id").map_err(|e| dec("entity_sync_id", e))?,
        job_kind: row.try_get("", "job_kind").map_err(|e| dec("job_kind", e))?,
        status: row.try_get("", "status").map_err(|e| dec("status", e))?,
        progress_pct: row.try_get("", "progress_pct").map_err(|e| dec("progress_pct", e))?,
        input_json: row.try_get("", "input_json").map_err(|e| dec("input_json", e))?,
        result_json: row.try_get::<Option<String>>("", "result_json").map_err(|e| dec("result_json", e))?,
        error_message: row.try_get::<Option<String>>("", "error_message").map_err(|e| dec("error_message", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| dec("created_at", e))?,
        started_at: row.try_get::<Option<String>>("", "started_at").map_err(|e| dec("started_at", e))?,
        finished_at: row.try_get::<Option<String>>("", "finished_at").map_err(|e| dec("finished_at", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| dec("row_version", e))?,
    })
}

pub async fn get_computation_job(db: &DatabaseConnection, id: i64) -> AppResult<Option<ComputationJob>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, job_kind, status, progress_pct, input_json, result_json, error_message,
                    created_at, started_at, finished_at, row_version
             FROM computation_jobs WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.map(|r| map_job(&r)).transpose()
}

pub async fn list_computation_jobs(db: &DatabaseConnection, limit: i64) -> AppResult<Vec<ComputationJob>> {
    let lim = limit.clamp(1, 200);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, job_kind, status, progress_pct, input_json, result_json, error_message,
                    created_at, started_at, finished_at, row_version
             FROM computation_jobs ORDER BY id DESC LIMIT ?",
            [lim.into()],
        ))
        .await?;
    rows.iter().map(|r| map_job(r)).collect::<Result<Vec<_>, _>>()
}
