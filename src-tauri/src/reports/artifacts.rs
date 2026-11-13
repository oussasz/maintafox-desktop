//! Persist report binaries under the Tauri app data directory (not OS temp).

use std::path::{Path, PathBuf};

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};
use crate::planning::domain::ExportedBinaryDocument;
use crate::reports::domain::ReportArtifact;
use crate::reports::queries::finish_run_success;

const ARTIFACTS_SUBDIR: &str = "reports/artifacts";

pub fn reports_artifacts_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("reports").join("artifacts")
}

fn resolve_under_app_data(app_data_dir: &Path, relative: &str) -> PathBuf {
    let mut out = app_data_dir.to_path_buf();
    for part in relative.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            continue;
        }
        out.push(part);
    }
    out
}

fn mime_for_format(export_format: &str) -> &'static str {
    if export_format.eq_ignore_ascii_case("xlsx") {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    } else {
        "application/pdf"
    }
}

fn ext_for_format(export_format: &str) -> &'static str {
    if export_format.eq_ignore_ascii_case("xlsx") {
        "xlsx"
    } else {
        "pdf"
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Write bytes to app-data, insert `report_artifacts`, update `report_runs`.
pub async fn persist_run_artifact(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    run_id: i64,
    export_format: &str,
    file_name: &str,
    bytes: &[u8],
) -> AppResult<ReportArtifact> {
    let dir = reports_artifacts_dir(app_data_dir);
    std::fs::create_dir_all(&dir).map_err(AppError::Io)?;

    let digest = sha256_hex(bytes);
    let ext = ext_for_format(export_format);
    let short = digest.get(..12).unwrap_or(&digest);
    let stored_name = format!("run-{run_id}-{short}.{ext}");
    let absolute = dir.join(&stored_name);
    std::fs::write(&absolute, bytes).map_err(AppError::Io)?;

    let relative_path = format!("{ARTIFACTS_SUBDIR}/{stored_name}");
    let mime_type = mime_for_format(export_format).to_string();
    let byte_size = bytes.len() as i64;
    let created_at = Utc::now().to_rfc3339();

    let inserted = db
        .execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO report_artifacts (run_id, relative_path, file_name, mime_type, content_sha256, byte_size, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            run_id.into(),
            relative_path.clone().into(),
            file_name.into(),
            mime_type.clone().into(),
            digest.clone().into(),
            byte_size.into(),
            created_at.clone().into(),
        ],
    ))
    .await?;

    let id = crate::db::inserted_id(&inserted)?;

    finish_run_success(db, run_id, &relative_path, byte_size).await?;

    Ok(ReportArtifact {
        id,
        run_id,
        relative_path,
        file_name: file_name.to_string(),
        mime_type,
        content_sha256: digest,
        byte_size,
        created_at,
    })
}

pub async fn get_artifact_by_run_id(db: &DatabaseConnection, run_id: i64) -> AppResult<Option<ReportArtifact>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, run_id, relative_path, file_name, mime_type, content_sha256, byte_size, created_at \
             FROM report_artifacts WHERE run_id = ?",
            [run_id.into()],
        ))
        .await?;
    row.map(|r| map_artifact(&r)).transpose()
}

fn map_artifact(row: &sea_orm::QueryResult) -> AppResult<ReportArtifact> {
    Ok(ReportArtifact {
        id: row
            .try_get("", "id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?,
        run_id: row
            .try_get("", "run_id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("run_id: {e}"))))?,
        relative_path: row
            .try_get("", "relative_path")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("relative_path: {e}"))))?,
        file_name: row
            .try_get("", "file_name")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("file_name: {e}"))))?,
        mime_type: row
            .try_get("", "mime_type")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("mime_type: {e}"))))?,
        content_sha256: row
            .try_get("", "content_sha256")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("content_sha256: {e}"))))?,
        byte_size: row
            .try_get("", "byte_size")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("byte_size: {e}"))))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("created_at: {e}"))))?,
    })
}

/// Load a successful run's bytes from durable storage (or legacy absolute path).
pub async fn read_run_document(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    run_id: i64,
    user_id: i64,
) -> AppResult<ExportedBinaryDocument> {
    let run_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status, artifact_path, export_format FROM report_runs WHERE id = ? AND user_id = ?",
            [run_id.into(), user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "report_run".into(),
            id: run_id.to_string(),
        })?;

    let status: String = run_row
        .try_get("", "status")
        .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("status: {e}"))))?;
    if status != "success" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_REPORTS_RUN_NO_ARTIFACT: Report run has no downloadable artifact (not successful).".into(),
        ]));
    }

    if let Some(art) = get_artifact_by_run_id(db, run_id).await? {
        let abs = resolve_under_app_data(app_data_dir, &art.relative_path);
        let bytes = std::fs::read(&abs).map_err(|e| {
            AppError::Internal(anyhow::anyhow!("failed to read report artifact {}: {e}", abs.display()))
        })?;
        return Ok(ExportedBinaryDocument {
            file_name: art.file_name,
            mime_type: art.mime_type,
            bytes,
        });
    }

    // Legacy: absolute temp path stored only on report_runs.
    let artifact_path: Option<String> = run_row.try_get("", "artifact_path").ok();
    let path = artifact_path
        .filter(|p| !p.trim().is_empty())
        .ok_or_else(|| AppError::NotFound {
            entity: "report_artifact".into(),
            id: run_id.to_string(),
        })?;
    let abs = PathBuf::from(&path);
    let bytes = std::fs::read(&abs).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "failed to read legacy report artifact {}: {e}",
            abs.display()
        ))
    })?;
    let export_format: String = run_row.try_get("", "export_format").unwrap_or_else(|_| "pdf".into());
    let file_name = abs
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("report.bin")
        .to_string();
    Ok(ExportedBinaryDocument {
        file_name,
        mime_type: mime_for_format(&export_format).to_string(),
        bytes,
    })
}
