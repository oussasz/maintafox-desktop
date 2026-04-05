//! Backup and restore preflight module.
//!
//! Responsibilities:
//! 1. Manual backup: copy the live SQLite DB to a target path via VACUUM INTO (WAL-safe)
//! 2. Backup run logging: insert a row in `backup_runs` for every execution
//! 3. Restore test mode: integrity check on a backup file without touching live DB
//! 4. Backup run listing: return run history for the Settings UI
//!
//! Architecture notes:
//! - All database access goes through `sea_orm::DatabaseConnection`, consistent with
//!   the rest of the codebase. No direct `sqlx::SqlitePool` usage.
//! - `VACUUM INTO` is the WAL-safe approach for hot-copying a SQLite database.
//!   It produces a point-in-time consistent copy without locking other readers.
//! - Phase 1: `encryption_mode` is always `"plaintext"`. AES-256 requires the
//!   key management infrastructure delivered in Phase 2 (VPS).

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[cfg(test)]
mod tests;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRunRecord {
    pub id: i64,
    pub trigger: String,
    pub status: String,
    pub output_path: String,
    pub file_size_bytes: Option<i64>,
    pub sha256_checksum: Option<String>,
    pub encryption_mode: String,
    pub db_schema_version: Option<i64>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub initiated_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRunResult {
    pub run_id: i64,
    pub output_path: String,
    pub file_size_bytes: u64,
    pub sha256_checksum: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreTestResult {
    pub backup_path: String,
    pub integrity_ok: bool,
    pub stored_checksum: Option<String>,
    pub computed_checksum: String,
    pub checksum_match: bool,
    pub integrity_check_output: String,
    pub warnings: Vec<String>,
}

// ─── Checksum ─────────────────────────────────────────────────────────────────

/// Compute SHA-256 hex digest of a file's entire contents.
fn compute_file_sha256(path: &Path) -> AppResult<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ─── Row mapper ───────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "backup_runs row decode failed for column '{}': {}",
        column,
        e
    ))
}

fn map_backup_run(row: sea_orm::QueryResult) -> AppResult<BackupRunRecord> {
    Ok(BackupRunRecord {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        trigger: row
            .try_get::<String>("", "trigger")
            .map_err(|e| decode_err("trigger", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        output_path: row
            .try_get::<String>("", "output_path")
            .map_err(|e| decode_err("output_path", e))?,
        file_size_bytes: row
            .try_get::<Option<i64>>("", "file_size_bytes")
            .map_err(|e| decode_err("file_size_bytes", e))?,
        sha256_checksum: row
            .try_get::<Option<String>>("", "sha256_checksum")
            .map_err(|e| decode_err("sha256_checksum", e))?,
        encryption_mode: row
            .try_get::<String>("", "encryption_mode")
            .map_err(|e| decode_err("encryption_mode", e))?,
        db_schema_version: row
            .try_get::<Option<i64>>("", "db_schema_version")
            .map_err(|e| decode_err("db_schema_version", e))?,
        started_at: row
            .try_get::<String>("", "started_at")
            .map_err(|e| decode_err("started_at", e))?,
        completed_at: row
            .try_get::<Option<String>>("", "completed_at")
            .map_err(|e| decode_err("completed_at", e))?,
        error_message: row
            .try_get::<Option<String>>("", "error_message")
            .map_err(|e| decode_err("error_message", e))?,
        initiated_by_id: row
            .try_get::<Option<i64>>("", "initiated_by_id")
            .map_err(|e| decode_err("initiated_by_id", e))?,
    })
}

// ─── Manual Backup ────────────────────────────────────────────────────────────

/// Execute a manual backup of the SQLite database.
///
/// Algorithm:
/// 1. Execute `VACUUM INTO '<target_path>'` on the live connection (WAL-safe)
/// 2. Compute SHA-256 of the output file
/// 3. Insert a `backup_runs` record with status, checksum, and file size
///
/// Phase 1: `encryption_mode` is always `"plaintext"`. Phase 2 will add AES-256.
#[allow(clippy::cast_possible_truncation)]
pub async fn run_manual_backup(
    db: &DatabaseConnection,
    target_path: &str,
    initiated_by_id: i32,
) -> AppResult<BackupRunResult> {
    use std::fs;

    let started_at = chrono::Utc::now().to_rfc3339();
    tracing::info!(
        target_path = target_path,
        actor = initiated_by_id,
        "manual backup started"
    );

    let target = Path::new(target_path);

    // Ensure the target directory exists
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    // VACUUM INTO is the recommended WAL-safe approach for hot-copying a SQLite
    // database. It creates a point-in-time consistent copy without locking readers.
    // Note: VACUUM INTO does not support parameter binding — the path must be
    // interpolated as a string literal. Single quotes are escaped to prevent injection.
    let target_path_sql = target_path.replace('\'', "''");
    db.execute_unprepared(&format!("VACUUM INTO '{target_path_sql}'"))
        .await?;

    // Compute checksum and file size
    let metadata = fs::metadata(target)?;
    let file_size = metadata.len();
    let checksum = compute_file_sha256(target)?;

    // Get current schema version (number of applied SeaORM migrations)
    let db_schema_version: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM seaql_migrations".to_string(),
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "cnt").unwrap_or(0))
        .unwrap_or(0);

    let completed_at = chrono::Utc::now().to_rfc3339();
    let file_size_i64 = file_size as i64;

    // Insert immutable backup_runs audit record
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"INSERT INTO backup_runs
               (trigger, status, output_path, file_size_bytes, sha256_checksum,
                encryption_mode, db_schema_version, started_at, completed_at,
                initiated_by_id)
               VALUES ('manual', 'success', ?, ?, ?, 'plaintext', ?, ?, ?, ?)"#,
            [
                target_path.into(),
                file_size_i64.into(),
                checksum.clone().into(),
                db_schema_version.into(),
                started_at.into(),
                completed_at.into(),
                i64::from(initiated_by_id).into(),
            ],
        ))
        .await?;

    let run_id = result.last_insert_id() as i64;

    tracing::info!(
        run_id = run_id,
        checksum = %checksum,
        file_size_bytes = file_size,
        "manual backup completed successfully"
    );

    Ok(BackupRunResult {
        run_id,
        output_path: target_path.to_string(),
        file_size_bytes: file_size,
        sha256_checksum: checksum,
        status: "success".to_string(),
    })
}

// ─── Restore Test ─────────────────────────────────────────────────────────────

/// Validate a backup file without modifying the live database.
///
/// Steps:
/// 1. Confirm the file exists and compute its SHA-256 checksum
/// 2. Look up the stored checksum from `backup_runs` (if a record exists)
/// 3. Copy the backup file to a temporary path
/// 4. Open a read-only `sea_orm` connection to the temp copy
/// 5. Run `PRAGMA integrity_check`
/// 6. Return validation results — never replace the live DB
pub async fn validate_backup_file(db: &DatabaseConnection, backup_path: &str) -> AppResult<RestoreTestResult> {
    use std::fs;

    let path = Path::new(backup_path);
    if !path.exists() {
        return Err(AppError::NotFound {
            entity: "backup_file".to_string(),
            id: backup_path.to_string(),
        });
    }

    let computed_checksum = compute_file_sha256(path)?;

    // Look up stored checksum from the most recent backup_runs entry for this path
    let stored_checksum: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sha256_checksum FROM backup_runs WHERE output_path = ? ORDER BY id DESC LIMIT 1",
            [backup_path.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<String>>("", "sha256_checksum").ok())
        .flatten();

    let checksum_match = stored_checksum
        .as_deref()
        .map(|s| s == computed_checksum)
        .unwrap_or(false);

    // Copy to a temp path for isolated read-only integrity check.
    // This ensures the live database is never touched.
    let temp_dir = std::env::temp_dir();
    let temp_name = format!("maintafox_restore_test_{}.db", chrono::Utc::now().timestamp());
    let temp_path = temp_dir.join(&temp_name);
    fs::copy(path, &temp_path)?;

    // Open an isolated read-only sea_orm connection to the temp copy
    let integrity_output = {
        let url = format!("sqlite://{}?mode=ro", temp_path.to_string_lossy());
        let mut opts = ConnectOptions::new(url);
        opts.max_connections(1);

        match Database::connect(opts).await {
            Ok(test_db) => {
                let result = test_db
                    .query_one(Statement::from_string(
                        DbBackend::Sqlite,
                        "PRAGMA integrity_check".to_string(),
                    ))
                    .await;

                let output = match result {
                    Ok(Some(row)) => row
                        .try_get::<String>("", "integrity_check")
                        .unwrap_or_else(|e| format!("integrity_check decode failed: {e}")),
                    Ok(None) => "no result from integrity_check".to_string(),
                    Err(e) => format!("integrity_check failed: {e}"),
                };

                test_db.close().await?;
                output
            }
            Err(e) => format!("could not open backup file: {e}"),
        }
    };

    // Clean up temp file (non-fatal if removal fails)
    let _ = fs::remove_file(&temp_path);

    let integrity_ok = integrity_output.trim() == "ok";

    tracing::info!(
        backup_path = backup_path,
        integrity_ok = integrity_ok,
        checksum_match = checksum_match,
        "restore preflight completed"
    );

    Ok(RestoreTestResult {
        backup_path: backup_path.to_string(),
        integrity_ok,
        stored_checksum,
        computed_checksum,
        checksum_match,
        integrity_check_output: integrity_output,
        warnings: vec![],
    })
}

// ─── List backup runs ─────────────────────────────────────────────────────────

pub async fn list_backup_runs(db: &DatabaseConnection, limit: i64) -> AppResult<Vec<BackupRunRecord>> {
    let safe_limit = limit.clamp(1, 500);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT id, trigger, status, output_path, file_size_bytes, sha256_checksum,
                      encryption_mode, db_schema_version,
                      COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', started_at), started_at) AS started_at,
                      CASE
                          WHEN completed_at IS NULL THEN NULL
                          ELSE COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', completed_at), completed_at)
                      END AS completed_at,
                      error_message, initiated_by_id
               FROM backup_runs
               ORDER BY started_at DESC
               LIMIT ?"#,
            [safe_limit.into()],
        ))
        .await?;

    rows.into_iter().map(map_backup_run).collect()
}
