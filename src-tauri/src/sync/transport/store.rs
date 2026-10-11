//! Persisted worker state (`sync_transport_state`, `sync_media_uploads`, migration 161).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::SyncMediaManifestItem;

pub(crate) fn now_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn decode(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("sync transport field '{field}': {err}"))
}

/// An exchange request stored before it is sent: a resend after a crash or a failure uses the same
/// key and the same body (contract §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingExchange {
    pub idempotency_key: String,
    pub body: String,
    pub outbox_ids: Vec<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct TransportState {
    pub pending: Option<PendingExchange>,
    pub paused_until: Option<String>,
    pub last_success_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub last_error_at: Option<String>,
    pub consecutive_failures: i64,
    pub refused_token_sha256: Option<String>,
    pub refused_reason: Option<String>,
}

fn opt(row: &QueryResult, field: &str) -> AppResult<Option<String>> {
    row.try_get::<Option<String>>("", field).map_err(|e| decode(field, e))
}

pub async fn load_state(db: &impl ConnectionTrait) -> AppResult<TransportState> {
    let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT pending_idempotency_key, pending_request_body, pending_outbox_ids_json, paused_until,
                    last_success_at, last_attempt_at, last_error_code, last_error_message, last_error_at,
                    consecutive_failures, refused_token_sha256, refused_reason
             FROM sync_transport_state WHERE id = 1",
            [],
        ))
        .await?
    else {
        return Err(AppError::SyncError(
            "sync_transport_state is missing; database migrations are incomplete.".into(),
        ));
    };
    let pending = match (
        opt(&row, "pending_idempotency_key")?,
        opt(&row, "pending_request_body")?,
        opt(&row, "pending_outbox_ids_json")?,
    ) {
        (Some(key), Some(body), ids) => Some(PendingExchange {
            idempotency_key: key,
            body,
            outbox_ids: match ids {
                Some(json) => serde_json::from_str(&json).map_err(|e| decode("pending_outbox_ids_json", e))?,
                None => Vec::new(),
            },
        }),
        _ => None,
    };
    Ok(TransportState {
        pending,
        paused_until: opt(&row, "paused_until")?,
        last_success_at: opt(&row, "last_success_at")?,
        last_attempt_at: opt(&row, "last_attempt_at")?,
        last_error_code: opt(&row, "last_error_code")?,
        last_error_message: opt(&row, "last_error_message")?,
        last_error_at: opt(&row, "last_error_at")?,
        consecutive_failures: row
            .try_get("", "consecutive_failures")
            .map_err(|e| decode("consecutive_failures", e))?,
        refused_token_sha256: opt(&row, "refused_token_sha256")?,
        refused_reason: opt(&row, "refused_reason")?,
    })
}

async fn update(db: &impl ConnectionTrait, set_sql: &str, values: Vec<sea_orm::Value>) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        format!(
            "UPDATE sync_transport_state SET {set_sql}, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = 1"
        ),
        values,
    ))
    .await?;
    Ok(())
}

pub async fn save_pending(db: &impl ConnectionTrait, pending: &PendingExchange) -> AppResult<()> {
    update(
        db,
        "pending_idempotency_key = ?, pending_request_body = ?, pending_outbox_ids_json = ?,
         pending_created_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        vec![
            pending.idempotency_key.clone().into(),
            pending.body.clone().into(),
            serde_json::to_string(&pending.outbox_ids)?.into(),
        ],
    )
    .await
}

/// After a backup was restored: the database's sync position (checkpoint and unanswered
/// exchange) is older than the server's, and the server never sends a device its own changes
/// back, so rows this device wrote after the backup would never return. Forgetting the position
/// makes the worker bootstrap from a full snapshot. Unsent changes of the backup stay queued and
/// are sent again (the server acknowledges the ones it already has as duplicates).
pub async fn forget_server_position(db: &impl ConnectionTrait) -> AppResult<()> {
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "UPDATE sync_checkpoint SET checkpoint_token = NULL WHERE id = 1".to_string(),
    ))
    .await?;
    clear_pending(db).await
}

pub async fn clear_pending(db: &impl ConnectionTrait) -> AppResult<()> {
    update(
        db,
        "pending_idempotency_key = NULL, pending_request_body = NULL, pending_outbox_ids_json = NULL,
         pending_created_at = NULL",
        vec![],
    )
    .await
}

pub async fn set_paused_until(db: &impl ConnectionTrait, until: Option<&str>) -> AppResult<()> {
    update(db, "paused_until = ?", vec![until.map(str::to_string).into()]).await
}

pub async fn record_attempt(db: &impl ConnectionTrait) -> AppResult<()> {
    update(db, "last_attempt_at = ?", vec![now_utc().into()]).await
}

pub async fn record_success(db: &impl ConnectionTrait) -> AppResult<()> {
    update(
        db,
        "last_success_at = ?, consecutive_failures = 0, last_error_code = NULL, last_error_message = NULL,
         last_error_at = NULL",
        vec![now_utc().into()],
    )
    .await
}

pub async fn record_failure(db: &impl ConnectionTrait, code: &str, message: &str) -> AppResult<()> {
    update(
        db,
        "consecutive_failures = consecutive_failures + 1, last_error_code = ?, last_error_message = ?,
         last_error_at = ?",
        vec![code.into(), message.into(), now_utc().into()],
    )
    .await
}

/// Forget the failure streak (an operator asked for a sync now): the next failure starts the
/// backoff from the shortest delay again.
pub async fn reset_failures(db: &impl ConnectionTrait) -> AppResult<()> {
    update(db, "consecutive_failures = 0", vec![]).await
}

/// The server refused this activation token (401/403) or this tenant; sync waits for another token.
pub async fn set_refused(db: &impl ConnectionTrait, token_sha256: &str, reason: &str) -> AppResult<()> {
    update(
        db,
        "refused_token_sha256 = ?, refused_reason = ?",
        vec![token_sha256.into(), reason.into()],
    )
    .await
}

pub async fn clear_refused(db: &impl ConnectionTrait) -> AppResult<()> {
    update(db, "refused_token_sha256 = NULL, refused_reason = NULL", vec![]).await
}

// ── Media uploads ────────────────────────────────────────────────────────────

/// A media object announced in a manifest whose bytes are not on object storage yet.
#[derive(Debug, Clone)]
pub struct PendingMediaUpload {
    pub object_key: String,
    pub entity_sync_id: String,
    pub content_sha256: String,
    pub attempt_count: i64,
    /// From `media_attachments`; `None` when the local row is gone.
    pub parent_entity_type: Option<String>,
    pub parent_entity_sync_id: Option<String>,
    pub relative_path: Option<String>,
}

pub async fn record_media_manifest(db: &impl ConnectionTrait, items: &[SyncMediaManifestItem]) -> AppResult<()> {
    for item in items {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT OR IGNORE INTO sync_media_uploads (object_key, entity_sync_id, content_sha256, byte_size)
             VALUES (?, ?, ?, ?)",
            [
                item.object_key.clone().into(),
                item.entity_sync_id.clone().into(),
                item.content_sha256.clone().into(),
                item.byte_size.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

pub async fn list_pending_media(db: &DatabaseConnection, limit: u32) -> AppResult<Vec<PendingMediaUpload>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT u.object_key, u.entity_sync_id, u.content_sha256, u.attempt_count,
                    m.parent_entity_type, m.parent_entity_sync_id, m.relative_path
             FROM sync_media_uploads u
             LEFT JOIN media_attachments m ON m.entity_sync_id = u.entity_sync_id
             WHERE u.status = 'pending'
             ORDER BY u.updated_at ASC, u.object_key ASC
             LIMIT ?",
            [i64::from(limit).into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(PendingMediaUpload {
                object_key: r.try_get("", "object_key").map_err(|e| decode("object_key", e))?,
                entity_sync_id: r
                    .try_get("", "entity_sync_id")
                    .map_err(|e| decode("entity_sync_id", e))?,
                content_sha256: r
                    .try_get("", "content_sha256")
                    .map_err(|e| decode("content_sha256", e))?,
                attempt_count: r.try_get("", "attempt_count").map_err(|e| decode("attempt_count", e))?,
                parent_entity_type: opt(r, "parent_entity_type")?,
                parent_entity_sync_id: opt(r, "parent_entity_sync_id")?,
                relative_path: opt(r, "relative_path")?,
            })
        })
        .collect()
}

pub async fn mark_media_uploaded(db: &impl ConnectionTrait, object_key: &str) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_media_uploads
         SET status = 'uploaded', last_error = NULL, attempt_count = attempt_count + 1,
             uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE object_key = ?",
        [object_key.into()],
    ))
    .await?;
    Ok(())
}

/// Record a failed upload. `give_up` moves the object to `failed` (shown to the user, never retried
/// automatically); otherwise it stays `pending` and is retried in a later cycle.
pub async fn mark_media_failed(
    db: &impl ConnectionTrait,
    object_key: &str,
    error: &str,
    give_up: bool,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_media_uploads
         SET status = CASE WHEN ? THEN 'failed' ELSE 'pending' END,
             last_error = ?, attempt_count = attempt_count + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE object_key = ?",
        [give_up.into(), error.into(), object_key.into()],
    ))
    .await?;
    Ok(())
}

/// `(pending, failed)` media uploads.
pub async fn media_counts(db: &impl ConnectionTrait) -> AppResult<(i64, i64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS pending,
                    COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed
             FROM sync_media_uploads",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("media upload counts unavailable".into()))?;
    Ok((
        row.try_get("", "pending").map_err(|e| decode("pending", e))?,
        row.try_get("", "failed").map_err(|e| decode("failed", e))?,
    ))
}

// ── Media downloads ──────────────────────────────────────────────────────────

/// A media file received from another device that this device has not stored yet
/// (`sync_media_downloads`, queued by the inbound apply of `media_attachments`).
#[derive(Debug, Clone)]
pub struct PendingMediaDownload {
    pub entity_sync_id: String,
    pub object_key: String,
    pub content_sha256: String,
    pub byte_size: i64,
    /// Where the file goes under the media root; `None` for a row sent by an app version that did
    /// not send the storage location.
    pub relative_path: Option<String>,
    pub attempt_count: i64,
}

/// Pending downloads whose next attempt is due, oldest first.
pub async fn list_due_media_downloads(db: &DatabaseConnection, limit: u32) -> AppResult<Vec<PendingMediaDownload>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, object_key, content_sha256, byte_size, relative_path, attempt_count
             FROM sync_media_downloads
             WHERE status = 'pending' AND (next_attempt_at IS NULL OR next_attempt_at <= ?)
             ORDER BY created_at ASC, entity_sync_id ASC
             LIMIT ?",
            [now_utc().into(), i64::from(limit).into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(PendingMediaDownload {
                entity_sync_id: r
                    .try_get("", "entity_sync_id")
                    .map_err(|e| decode("entity_sync_id", e))?,
                object_key: r.try_get("", "object_key").map_err(|e| decode("object_key", e))?,
                content_sha256: r
                    .try_get("", "content_sha256")
                    .map_err(|e| decode("content_sha256", e))?,
                byte_size: r.try_get("", "byte_size").map_err(|e| decode("byte_size", e))?,
                relative_path: opt(r, "relative_path")?,
                attempt_count: r.try_get("", "attempt_count").map_err(|e| decode("attempt_count", e))?,
            })
        })
        .collect()
}

pub async fn mark_media_downloaded(db: &impl ConnectionTrait, entity_sync_id: &str) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_media_downloads
         SET status = 'downloaded', last_error = NULL, next_attempt_at = NULL,
             downloaded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE entity_sync_id = ?",
        [entity_sync_id.into()],
    ))
    .await?;
    Ok(())
}

/// Record a failed download. `give_up` moves it to `failed` (shown with a retry action, never
/// retried automatically); otherwise it stays `pending` until `next_attempt_at`.
pub async fn mark_media_download_failed(
    db: &impl ConnectionTrait,
    entity_sync_id: &str,
    error: &str,
    give_up: bool,
    next_attempt_at: Option<&str>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_media_downloads
         SET status = CASE WHEN ? THEN 'failed' ELSE 'pending' END,
             last_error = ?, attempt_count = attempt_count + 1, next_attempt_at = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE entity_sync_id = ?",
        [
            give_up.into(),
            error.into(),
            next_attempt_at.map(str::to_string).into(),
            entity_sync_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// `(pending, failed)` media downloads.
pub async fn media_download_counts(db: &impl ConnectionTrait) -> AppResult<(i64, i64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0) AS pending,
                    COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed
             FROM sync_media_downloads",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("media download counts unavailable".into()))?;
    Ok((
        row.try_get("", "pending").map_err(|e| decode("pending", e))?,
        row.try_get("", "failed").map_err(|e| decode("failed", e))?,
    ))
}
