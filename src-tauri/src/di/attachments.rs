//! DI attachment subsystem — upload, list, and delete-record functions.
//!
//! Phase 2 - Sub-phase 04 - File 03 - Sprint S1.
//!
//! Files are stored on the local filesystem under the app data directory.
//! The `di_attachments` table stores relative paths only; absolute paths
//! are resolved at runtime via `AppHandle::path().app_data_dir()`.
//!
//! Architecture rules:
//!   - Files are never deleted from disk when the attachment record is removed;
//!     they are orphaned and can be purged by an admin cleanup task.
//!   - Attachments are allowed on all non-archived DI states.
//!   - The `relative_path` column is unique to prevent collision.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Row from `di_attachments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiAttachment {
    pub id: i64,
    pub di_id: i64,
    pub file_name: String,
    pub relative_path: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub attachment_type: String,
    pub uploaded_by_id: Option<i64>,
    pub uploaded_at: String,
    pub notes: Option<String>,
}

/// Input for saving a new DI attachment.
#[derive(Debug, Clone, Deserialize)]
pub struct DiAttachmentInput {
    pub di_id: i64,
    pub file_name: String,
    pub file_bytes: Vec<u8>,
    pub mime_type: String,
    pub attachment_type: String,
    pub notes: Option<String>,
    pub uploaded_by_id: i64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum file size: 20 MB. Enforced at the domain layer as a safety net
/// (frontend also validates before sending bytes over IPC).
const MAX_FILE_SIZE_BYTES: usize = 20 * 1024 * 1024;

/// Allowed attachment types (validated on input).
const VALID_ATTACHMENT_TYPES: &[&str] = &["photo", "sensor_snapshot", "pdf", "other"];

// ═══════════════════════════════════════════════════════════════════════════════
// Row mapping
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "di_attachments row decode failed for column '{column}': {e}"
    ))
}

fn map_attachment(row: &QueryResult) -> AppResult<DiAttachment> {
    Ok(DiAttachment {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        di_id: row
            .try_get::<i64>("", "di_id")
            .map_err(|e| decode_err("di_id", e))?,
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
        attachment_type: row
            .try_get::<String>("", "attachment_type")
            .map_err(|e| decode_err("attachment_type", e))?,
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
// A) save_di_attachment — write to disk + insert row
// ═══════════════════════════════════════════════════════════════════════════════

/// Save a DI attachment: write bytes to disk, then insert the DB record.
///
/// # Errors
/// - DI does not exist
/// - DI is in `archived` status (attachments disallowed)
/// - File exceeds 20 MB
/// - Invalid attachment type
/// - Disk write failure
pub async fn save_di_attachment(
    db: &impl ConnectionTrait,
    app_data_dir: &Path,
    input: DiAttachmentInput,
) -> AppResult<DiAttachment> {
    // ── Validate file size ────────────────────────────────────────────────
    if input.file_bytes.len() > MAX_FILE_SIZE_BYTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "La taille du fichier dépasse la limite de {} Mo.",
            MAX_FILE_SIZE_BYTES / (1024 * 1024)
        )]));
    }

    // ── Validate attachment type ──────────────────────────────────────────
    if !VALID_ATTACHMENT_TYPES.contains(&input.attachment_type.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type de pièce jointe invalide : '{}'. Valeurs autorisées : photo, sensor_snapshot, pdf, other.",
            input.attachment_type
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

    // ── Verify DI exists and is not archived ──────────────────────────────
    let di_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, status FROM intervention_requests WHERE id = ?",
            [input.di_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;

    let status: String = di_row
        .try_get("", "status")
        .map_err(|e| decode_err("status", e))?;

    if status == "archived" {
        return Err(AppError::ValidationFailed(vec![
            "Impossible d'ajouter une pièce jointe à une DI archivée.".into(),
        ]));
    }

    // ── Generate unique relative path ─────────────────────────────────────
    let uuid = Uuid::new_v4();
    let relative_path = format!(
        "di_attachments/{}/{}-{}",
        input.di_id, uuid, sanitized_name
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
        "INSERT INTO di_attachments \
            (di_id, file_name, relative_path, mime_type, size_bytes, \
             attachment_type, uploaded_by_id, uploaded_at, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.di_id.into(),
            sanitized_name.to_string().into(),
            relative_path.clone().into(),
            input.mime_type.into(),
            size_bytes.into(),
            input.attachment_type.into(),
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
            "SELECT * FROM di_attachments WHERE relative_path = ?",
            [relative_path.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Failed to re-fetch inserted di_attachment"
            ))
        })?;

    map_attachment(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) list_di_attachments
// ═══════════════════════════════════════════════════════════════════════════════

/// List all attachments for a given DI, ordered by upload date descending.
pub async fn list_di_attachments(
    db: &impl ConnectionTrait,
    di_id: i64,
) -> AppResult<Vec<DiAttachment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT * FROM di_attachments WHERE di_id = ? ORDER BY uploaded_at DESC",
            [di_id.into()],
        ))
        .await?;

    rows.iter().map(map_attachment).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) delete_di_attachment_record — DB record only, not the file
// ═══════════════════════════════════════════════════════════════════════════════

/// Delete the attachment DB record. Does NOT delete the file from disk.
/// The caller (command layer) must hold `di.admin` to invoke this.
pub async fn delete_di_attachment_record(
    db: &impl ConnectionTrait,
    attachment_id: i64,
) -> AppResult<()> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM di_attachments WHERE id = ?",
            [attachment_id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "DiAttachment".into(),
            id: attachment_id.to_string(),
        });
    }

    Ok(())
}
