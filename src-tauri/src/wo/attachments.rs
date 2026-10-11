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
//!   - Attachments are blocked on closed/cancelled WOs (strict post-close lock).
//!   - `relative_path` column is unique to prevent collision.

use crate::errors::{AppError, AppResult};
use crate::wo::time::now_utc_z;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
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
    pub phase: Option<String>,
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
    pub phase: Option<String>,
}

const VALID_PHASES: &[&str] = &["before", "during", "after", "evidence"];

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
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
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
        phase: row.try_get::<Option<String>>("", "phase").unwrap_or(None),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) save_wo_attachment — write to disk + insert row
// ═══════════════════════════════════════════════════════════════════════════════

/// Save a WO attachment: write bytes to disk, then insert the DB record.
///
/// WO must remain mutable (not `closed` / `cancelled`) to accept attachments.
///
/// # Errors
/// - WO does not exist or is closed/cancelled
/// - File exceeds 25 MB
/// - Invalid file name
/// - Disk write failure
pub async fn save_wo_attachment(
    db: &DatabaseConnection,
    app_data_dir: &Path,
    input: WoAttachmentInput,
) -> AppResult<WoAttachment> {
    // ── Validate phase ────────────────────────────────────────────────────
    if let Some(ref phase) = input.phase {
        if !VALID_PHASES.contains(&phase.as_str()) {
            return Err(AppError::ValidationFailed(vec![format!(
                "phase invalide : '{phase}'. Valeurs autorisées : before, during, after, evidence."
            )]));
        }
    }

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
        return Err(AppError::ValidationFailed(vec!["Nom de fichier invalide.".into()]));
    }

    // Resolved before the transaction: the helper reads through the pool connection.
    let env = crate::sync::media::resolve_media_deployment_env();
    let tenant_id = crate::sync::media::resolve_media_tenant_id(db).await;

    // The attachment row, its media metadata row and the media outbox entry commit together.
    let txn = db.begin().await?;

    // ── Verify WO exists and is mutable ───────────────────────────────────
    let wo_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.id, wo.entity_sync_id, wos.code AS status_code \
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

    if status == "cancelled" || status == "closed" {
        return Err(AppError::ValidationFailed(vec![
            "Impossible d'ajouter une piece jointe a un OT cloture/annule.".into(),
        ]));
    }

    let parent_entity_sync_id = crate::sync::identity::require_entity_sync_id(
        "work_orders",
        input.wo_id,
        wo_row
            .try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
    )?;

    // ── Generate unique relative path ─────────────────────────────────────
    let uuid = Uuid::new_v4();
    let relative_path = format!("wo_attachments/{}/{}-{}", input.wo_id, uuid, sanitized_name);

    // ── Write to disk ─────────────────────────────────────────────────────
    let absolute_path = app_data_dir.join(&relative_path);
    if let Some(parent) = absolute_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&absolute_path, &input.file_bytes)?;

    // ── Insert DB record ──────────────────────────────────────────────────
    let now = now_utc_z();
    let size_bytes = input.file_bytes.len() as i64;
    let mime_type = input.mime_type.clone();
    let file_name = sanitized_name.to_string();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_attachments \
            (work_order_id, file_name, relative_path, mime_type, size_bytes, \
             uploaded_by_id, uploaded_at, notes, phase) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            file_name.clone().into(),
            relative_path.clone().into(),
            mime_type.clone().into(),
            size_bytes.into(),
            input.uploaded_by_id.into(),
            now.into(),
            input
                .notes
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input
                .phase
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    let payload = crate::sync::media::build_media_attachment(
        env,
        &tenant_id,
        "work_orders",
        &parent_entity_sync_id,
        &file_name,
        &mime_type,
        &input.file_bytes,
        Some(relative_path.clone()),
        None,
    )?;
    crate::sync::media::upsert_media_attachment_row(&txn, &payload).await?;

    // ── Re-fetch the inserted row ─────────────────────────────────────────
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, file_name, relative_path, mime_type, size_bytes, \
                    uploaded_by_id, uploaded_at, notes, phase \
               FROM work_order_attachments WHERE relative_path = ?",
            [relative_path.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-fetch inserted wo_attachment")))?;
    let attachment = map_attachment(&row)?;

    txn.commit().await?;
    Ok(attachment)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) list_wo_attachments
// ═══════════════════════════════════════════════════════════════════════════════

/// List all attachments for a given WO, ordered by upload date descending.
pub async fn list_wo_attachments(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Vec<WoAttachment>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, file_name, relative_path, mime_type, size_bytes, \
                    uploaded_by_id, uploaded_at, notes, phase \
               FROM work_order_attachments \
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
pub async fn delete_wo_attachment_record(db: &DatabaseConnection, attachment_id: i64) -> AppResult<()> {
    let txn = db.begin().await?;

    let state_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code, wa.relative_path, wo.entity_sync_id AS parent_entity_sync_id \
             FROM work_order_attachments wa \
             JOIN work_orders wo ON wo.id = wa.work_order_id \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             WHERE wa.id = ?",
            [attachment_id.into()],
        ))
        .await?;
    if let Some(row) = state_row {
        let status: String = row
            .try_get("", "status_code")
            .map_err(|e| decode_err("status_code", e))?;
        if status == "closed" || status == "cancelled" {
            return Err(AppError::ValidationFailed(vec![
                "Suppression interdite: OT cloture/annule (audit trail verrouille).".into(),
            ]));
        }
        let relative_path: String = row.try_get("", "relative_path").unwrap_or_default();
        let parent_esid: String = row.try_get("", "parent_entity_sync_id").unwrap_or_default();
        crate::sync::media::delete_media_attachment_if_found(
            &txn,
            Some("work_orders"),
            if parent_esid.is_empty() {
                None
            } else {
                Some(parent_esid.as_str())
            },
            if relative_path.is_empty() {
                None
            } else {
                Some(relative_path.as_str())
            },
            None,
        )
        .await?;
    }

    let result = txn
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

    txn.commit().await?;
    Ok(())
}
