//! WO attachment subsystem — upload, list, and delete-record functions.
//!
//! Phase 2 - Sub-phase 05 - File 03 - Sprint S1.
//!
//! Follows the DI attachment pattern (SP04). Files are stored on the local
//! filesystem under the app data directory. The `work_order_attachments` table
//! stores relative paths only; absolute paths are resolved at runtime.
//!
//! Architecture rules:
//!   - Files are never deleted from disk when the record is removed.
//!   - Attachments are allowed on all non-cancelled WO states (even after closure).
//!   - `relative_path` column is unique to prevent collision.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Row from `work_order_attachments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoAttachment {
    pub id: i64,
    pub work_order_id: i64,
    pub file_name: String,
    pub relative_path: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uploaded_by_id: Option<i64>,
    pub uploaded_at: String,
    pub notes: Option<String>,
}

/// Input for saving a new WO attachment.
#[derive(Debug, Clone, Deserialize)]
pub struct WoAttachmentInput {
    pub wo_id: i64,
    pub file_name: String,
    pub file_bytes: Vec<u8>,
    pub mime_type: String,
    pub notes: Option<String>,
    pub uploaded_by_id: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum file size: 25 MB (WOs may have larger PDFs than DI photos).
const MAX_FILE_SIZE_BYTES: usize = 25 * 1024 * 1024;

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "wo_attachments row decode failed for column '{column}': {e}"
    ))
}

fn map_attachment(row: &QueryResult) -> AppResult<WoAttachment> {
    Ok(WoAttachment {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        file_name: row
            .try_get::<String>("", "file_name")
            .map_err(|e| decode_err("file_name", e))?,
        relative_path: row
            .try_get::<String>("", "relative_path")
            .map_err(|e| decode_err("relative_path", e))?,
        mime_type: row
            .try_get::<String>("", "mime_type")
            .map_err(|e| decode_err("mime_type", e))?,
        size_bytes: row
            .try_get::<i64>("", "size_bytes")
            .map_err(|e| decode_err("size_bytes", e))?,
        uploaded_by_id: row
            .try_get::<Option<i64>>("", "uploaded_by_id")
            .map_err(|e| decode_err("uploaded_by_id", e))?,
        uploaded_at: row
            .try_get::<String>("", "uploaded_at")
            .map_err(|e| decode_err("uploaded_at", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) save_wo_attachment — write to disk + insert row
// ═══════════════════════════════════════════════════════════════════════════════

/// Save a WO attachment: write bytes to disk, then insert the DB record.
///
/// WO must not be in `cancelled` state (attachments allowed even after closure).
///
/// # Errors
/// - WO does not exist or is cancelled
/// - File exceeds 25 MB
/// - Invalid file name
/// - Disk write failure
pub async fn save_wo_attachment(
    db: &impl ConnectionTrait,
    app_data_dir: &Path,
    input: WoAttachmentInput,
) -> AppResult<WoAttachment> {
    // ── Validate file size ────────────────────────────────────────────────
    if input.file_bytes.len() > MAX_FILE_SIZE_BYTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "La taille du fichier depasse la limite de {} Mo.",
            MAX_FILE_SIZE_BYTES / (1024 * 1024)
        )]));
    }

    // ── Validate file name (no path traversal) ────────────────────────────
    let sanitized_name = Path::new(&input.file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment");
    if sanitized_name.is_empty() || sanitized_name.contains("..") {
        return Err(AppError::ValidationFailed(vec![
            "Nom de fichier invalide.".into(),
        ]));
    }

    // ── Verify WO exists and is not cancelled ─────────────────────────────
    let wo_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.id, wos.code AS status_code \
             FROM work_orders wo \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             WHERE wo.id = ?",
            [input.wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;

    let status: String = wo_row
        .try_get("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;

    if status == "cancelled" {
        return Err(AppError::ValidationFailed(vec![
            "Impossible d'ajouter une piece jointe a un OT annule.".into(),
        ]));
    }

    // ── Generate unique relative path ─────────────────────────────────────
    let uuid = Uuid::new_v4();
    let relative_path = format!(
        "wo_attachments/{}/{}-{}",
        input.wo_id, uuid, sanitized_name
    );

    // ── Write to disk ─────────────────────────────────────────────────────
    let absolute_path = app_data_dir.join(&relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&absolute_path, &input.file_bytes)?;

    // ── Insert DB record ──────────────────────────────────────────────────
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let size_bytes = input.file_bytes.len() as i64;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_attachments \
            (work_order_id, file_name, relative_path, mime_type, size_bytes, \
             uploaded_by_id, uploaded_at, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            sanitized_name.to_string().into(),
            relative_path.clone().into(),
            input.mime_type.into(),
            size_bytes.into(),
            input.uploaded_by_id.into(),
            now.into(),
            input
                .notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    // ── Re-fetch the inserted row ─────────────────────────────────────────
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM work_order_attachments WHERE relative_path = ?",
            [relative_path.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Failed to re-fetch inserted wo_attachment"
            ))
        })?;

    map_attachment(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) list_wo_attachments
// ═══════════════════════════════════════════════════════════════════════════════

/// List all attachments for a given WO, ordered by upload date descending.
pub async fn list_wo_attachments(
    db: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<Vec<WoAttachment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM work_order_attachments \
             WHERE work_order_id = ? ORDER BY uploaded_at DESC",
            [wo_id.into()],
        ))
        .await?;

    rows.iter().map(map_attachment).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) delete_wo_attachment_record — DB record only, not the file
// ═══════════════════════════════════════════════════════════════════════════════

/// Delete the attachment DB record. Does NOT delete the file from disk.
/// The caller (command layer) must hold `ot.admin` to invoke this.
pub async fn delete_wo_attachment_record(
    db: &impl ConnectionTrait,
    attachment_id: i64,
) -> AppResult<()> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM work_order_attachments WHERE id = ?",
            [attachment_id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "WoAttachment".into(),
            id: attachment_id.to_string(),
        });
    }

    Ok(())
}
