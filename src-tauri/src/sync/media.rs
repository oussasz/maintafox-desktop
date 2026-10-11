//! Tenant media sync helpers — metadata envelopes + hash-addressed object keys.
//!
//! JSON outbox MUST NOT carry file bytes. Binaries use `ObjectCategory::TenantMedia`.

use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::MediaAttachmentSyncPayload;
use crate::vps::object_storage::{build_tenant_media_object_key, DeploymentEnvironment, ObjectCategory};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

/// Compute SHA-256 hex of file bytes (lowercase, 64 chars).
pub fn content_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Resolve deployment environment for object keys (env var, else Dev).
pub fn resolve_media_deployment_env() -> DeploymentEnvironment {
    std::env::var("MAINTAFOX_DEPLOYMENT_ENV")
        .ok()
        .and_then(|s| DeploymentEnvironment::parse(&s))
        .unwrap_or(DeploymentEnvironment::Dev)
}

/// Tenant id from product license claim, else `"local-tenant"`.
pub async fn resolve_media_tenant_id(db: &DatabaseConnection) -> String {
    match crate::commands::product_license::get_activation_claim_tenant_id(db).await {
        Ok(Some(tid)) if !tid.trim().is_empty() => tid,
        _ => "local-tenant".to_string(),
    }
}

/// Persist the local media metadata row. The sync capture of `media_attachments` sends it (and the
/// worker uploads the bytes it names); nothing else needs to be staged.
pub async fn upsert_media_attachment_row(
    db: &impl ConnectionTrait,
    payload: &MediaAttachmentSyncPayload,
) -> AppResult<()> {
    let updated_at = payload
        .updated_at
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT OR IGNORE INTO media_attachments (
            entity_sync_id, parent_entity_type, parent_entity_sync_id, content_sha256,
            mime_type, byte_size, object_key, file_name, relative_path, capture_meta_json,
            row_version, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            payload.entity_sync_id.clone().into(),
            payload.parent_entity_type.clone().into(),
            payload.parent_entity_sync_id.clone().into(),
            payload.content_sha256.clone().into(),
            payload.mime_type.clone().into(),
            payload.byte_size.into(),
            payload.object_key.clone().into(),
            payload.file_name.clone().into(),
            payload.relative_path.clone().into(),
            payload.capture_meta_json.clone().into(),
            payload.row_version.into(),
            updated_at.clone().into(),
            updated_at.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Build the media metadata for file bytes stored under the app data dir (content hash and
/// tenant object key). The caller writes it with [`upsert_media_attachment_row`] in its
/// transaction.
pub fn build_media_attachment(
    env: DeploymentEnvironment,
    tenant_id: &str,
    parent_entity_type: &str,
    parent_entity_sync_id: &str,
    file_name: &str,
    mime_type: &str,
    bytes: &[u8],
    relative_path: Option<String>,
    capture_meta_json: Option<String>,
) -> AppResult<MediaAttachmentSyncPayload> {
    if bytes.is_empty() {
        return Err(AppError::ValidationFailed(vec!["media bytes must not be empty".into()]));
    }
    let content_sha256 = content_sha256_hex(bytes);
    let object_key = build_tenant_media_object_key(
        env,
        tenant_id,
        parent_entity_type,
        parent_entity_sync_id,
        &content_sha256,
    )
    .map_err(|e| AppError::ValidationFailed(vec![e.message]))?
    .full_key;

    let now = chrono::Utc::now().to_rfc3339();
    let payload = MediaAttachmentSyncPayload {
        entity_sync_id: uuid::Uuid::new_v4().to_string(),
        row_version: 1,
        parent_entity_type: parent_entity_type.to_string(),
        parent_entity_sync_id: parent_entity_sync_id.to_string(),
        content_sha256,
        mime_type: mime_type.to_string(),
        byte_size: bytes.len() as i64,
        object_key,
        file_name: file_name.to_string(),
        relative_path,
        capture_meta_json,
        updated_at: Some(now),
    };
    Ok(payload)
}

/// Delete the `media_attachments` row of a removed file when it can be resolved (the sync capture
/// sends the delete); otherwise warn and skip.
pub async fn delete_media_attachment_if_found(
    db: &impl ConnectionTrait,
    parent_entity_type: Option<&str>,
    parent_entity_sync_id: Option<&str>,
    relative_path: Option<&str>,
    content_sha256: Option<&str>,
) -> AppResult<()> {
    let row = if let (Some(ptype), Some(psync), Some(path)) = (parent_entity_type, parent_entity_sync_id, relative_path)
    {
        db.query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, row_version FROM media_attachments
             WHERE parent_entity_type = ? AND parent_entity_sync_id = ? AND relative_path = ?
             LIMIT 1",
            [ptype.into(), psync.into(), path.into()],
        ))
        .await?
    } else if let Some(sha) = content_sha256 {
        db.query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, row_version FROM media_attachments
             WHERE content_sha256 = ? LIMIT 1",
            [sha.into()],
        ))
        .await?
    } else {
        None
    };

    let Some(row) = row else {
        tracing::warn!(
            target: "maintafox",
            "sync stage media delete skipped: no media_attachments row for parent/path or hash"
        );
        return Ok(());
    };

    let esid: String = crate::sync::identity::require_parent_sync_id(&row, "entity_sync_id")?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "DELETE FROM media_attachments WHERE entity_sync_id = ?",
        [esid.into()],
    ))
    .await?;
    Ok(())
}

/// Verify local bytes match the envelope hash before upload/download accept.
pub fn verify_content_hash(bytes: &[u8], expected_sha256: &str) -> AppResult<()> {
    let actual = content_sha256_hex(bytes);
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if actual != expected {
        return Err(AppError::ValidationFailed(vec![format!(
            "media content hash mismatch: expected {expected}, got {actual}"
        )]));
    }
    Ok(())
}

pub fn media_object_category() -> ObjectCategory {
    ObjectCategory::TenantMedia
}

/// Library documents stored before the library synchronised have no media row, so their files
/// would never be uploaded. Called once per start (before the worker): builds the missing media
/// rows from the files on disk. A document whose file is missing is skipped with a warning (other
/// devices then show its file as missing). Returns how many media rows were added.
pub async fn backfill_library_document_media(
    db: &DatabaseConnection,
    media_root: &std::path::Path,
) -> AppResult<usize> {
    use sea_orm::TransactionTrait;
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT ld.entity_sync_id, ld.file_name, ld.relative_path, ld.mime_type
             FROM library_documents ld
             LEFT JOIN media_attachments m
               ON m.parent_entity_type = 'library_documents' AND m.parent_entity_sync_id = ld.entity_sync_id
             WHERE m.id IS NULL AND ld.entity_sync_id IS NOT NULL"
                .to_string(),
        ))
        .await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let env = resolve_media_deployment_env();
    let tenant_id = resolve_media_tenant_id(db).await;
    let mut added = 0;
    for row in rows {
        let text = |c: &str| {
            row.try_get::<String>("", c)
                .map_err(|e| AppError::SyncError(format!("library_documents.{c}: {e}")))
        };
        let (entity_sync_id, file_name, relative_path, mime_type) =
            (text("entity_sync_id")?, text("file_name")?, text("relative_path")?, text("mime_type")?);
        let Some(path) = media_storage_path(&relative_path) else {
            tracing::warn!(target: "maintafox::sync", relative_path, "library document path is not inside the media directories; its file is not synchronised");
            continue;
        };
        let bytes = match tokio::fs::read(media_root.join(path)).await {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) | Err(_) => {
                tracing::warn!(target: "maintafox::sync", relative_path, "library document file is missing or empty; its file is not synchronised");
                continue;
            }
        };
        let media = build_media_attachment(
            env,
            &tenant_id,
            "library_documents",
            &entity_sync_id,
            &file_name,
            &mime_type,
            &bytes,
            Some(relative_path.clone()),
            None,
        )?;
        let txn = db.begin().await?;
        upsert_media_attachment_row(&txn, &media).await?;
        txn.commit().await?;
        added += 1;
    }
    Ok(added)
}

// ── Files received from other devices ───────────────────────────────────────────────────────

/// Directories under the app data directory where synchronised files live (first component of
/// every `relative_path` the attachment commands write). A received file is only ever written
/// inside one of them, so a row from another device cannot point at the database or settings.
pub const MEDIA_STORAGE_DIRECTORIES: &[&str] = &["wo_attachments", "di_attachments", "asset_photos", "library_documents"];

/// `relative_path` as a path inside one of [`MEDIA_STORAGE_DIRECTORIES`] (plain components only,
/// at least a directory and a file name); `None` otherwise.
pub fn media_storage_path(relative_path: &str) -> Option<&std::path::Path> {
    use std::path::{Component, Path};
    let path = Path::new(relative_path);
    let components: Vec<Component<'_>> = path.components().collect();
    let plain = components.iter().all(|c| matches!(c, Component::Normal(_)));
    let first = components.first().and_then(|c| c.as_os_str().to_str());
    (plain && components.len() >= 2 && first.is_some_and(|d| MEDIA_STORAGE_DIRECTORIES.contains(&d))).then_some(path)
}

/// Queue the download of a received `media_attachments` row (inbound apply, same transaction).
/// A row already queued keeps its state.
pub async fn queue_media_download(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    object_key: &str,
    content_sha256: &str,
    byte_size: i64,
    relative_path: Option<&str>,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT OR IGNORE INTO sync_media_downloads
             (entity_sync_id, object_key, content_sha256, byte_size, relative_path)
         VALUES (?, ?, ?, ?, ?)",
        [
            entity_sync_id.into(),
            object_key.into(),
            content_sha256.into(),
            byte_size.into(),
            relative_path.map(str::to_string).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The media row was deleted on another device: nothing left to download. A file already stored
/// stays (attachment deletes never remove files, see `wo::attachments`).
pub async fn forget_media_download(conn: &impl ConnectionTrait, entity_sync_id: &str) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "DELETE FROM sync_media_downloads WHERE entity_sync_id = ? AND status <> 'downloaded'",
        [entity_sync_id.into()],
    ))
    .await?;
    Ok(())
}

/// Availability of a synchronised file on this device, for the attachment lists.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFileState {
    pub relative_path: String,
    /// `available` (on disk), `pending` (received from another device, download waiting or in
    /// progress), `failed` (download given up, `retry_media_download` queues it again) or
    /// `missing` (not on disk and no download known).
    pub state: &'static str,
    pub last_error: Option<String>,
    pub attempt_count: i64,
}

pub async fn media_file_states(
    db: &impl ConnectionTrait,
    media_root: &std::path::Path,
    relative_paths: &[String],
) -> AppResult<Vec<MediaFileState>> {
    let mut states = Vec::with_capacity(relative_paths.len());
    for relative_path in relative_paths {
        let on_disk = media_storage_path(relative_path)
            .map(|p| media_root.join(p))
            .is_some_and(|p| p.is_file());
        if on_disk {
            states.push(MediaFileState {
                relative_path: relative_path.clone(),
                state: "available",
                last_error: None,
                attempt_count: 0,
            });
            continue;
        }
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT status, last_error, attempt_count FROM sync_media_downloads
                 WHERE relative_path = ? AND status <> 'downloaded'
                 ORDER BY CASE status WHEN 'pending' THEN 0 ELSE 1 END LIMIT 1",
                [relative_path.clone().into()],
            ))
            .await?;
        let state = match &row {
            Some(r) => match r
                .try_get::<String>("", "status")
                .map_err(|e| AppError::SyncError(format!("sync_media_downloads.status: {e}")))?
                .as_str()
            {
                "pending" => "pending",
                _ => "failed",
            },
            None => "missing",
        };
        states.push(MediaFileState {
            relative_path: relative_path.clone(),
            state,
            last_error: row
                .as_ref()
                .map(|r| r.try_get::<Option<String>>("", "last_error"))
                .transpose()
                .map_err(|e| AppError::SyncError(format!("sync_media_downloads.last_error: {e}")))?
                .flatten(),
            attempt_count: row
                .as_ref()
                .map(|r| r.try_get::<i64>("", "attempt_count"))
                .transpose()
                .map_err(|e| AppError::SyncError(format!("sync_media_downloads.attempt_count: {e}")))?
                .unwrap_or(0),
        });
    }
    Ok(states)
}

/// Queue a failed (or not yet attempted) download of the file stored at `relative_path` again,
/// from the first attempt. Returns how many downloads were queued (0: nothing to download).
pub async fn retry_media_download(db: &impl ConnectionTrait, relative_path: &str) -> AppResult<u64> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_media_downloads
             SET status = 'pending', attempt_count = 0, next_attempt_at = NULL, last_error = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE relative_path = ? AND status <> 'downloaded'",
            [relative_path.into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vps::object_storage::DeploymentEnvironment;

    #[test]
    fn sha256_is_64_hex() {
        let h = content_sha256_hex(b"hello");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tenant_media_key_uses_media_prefix() {
        let key = build_tenant_media_object_key(
            DeploymentEnvironment::Dev,
            "tenant-1",
            "work_orders",
            "wo-sync-1",
            &content_sha256_hex(b"payload"),
        )
        .expect("key");
        assert!(key.full_key.contains("/media/"));
        assert_eq!(key.category, ObjectCategory::TenantMedia);
    }

    #[test]
    fn media_storage_path_stays_in_media_directories() {
        assert!(media_storage_path("wo_attachments/12/abc-file.pdf").is_some());
        assert!(media_storage_path("library_documents/sops/abc-doc.pdf").is_some());
        assert!(media_storage_path("maintafox.db").is_none());
        assert!(media_storage_path("wo_attachments").is_none());
        assert!(media_storage_path("wo_attachments/../maintafox.db").is_none());
        assert!(media_storage_path("/etc/passwd").is_none());
        assert!(media_storage_path("C:\\Windows\\x").is_none());
        assert!(media_storage_path("backups/x.db").is_none());
    }

    #[test]
    fn verify_hash_rejects_mismatch() {
        assert!(verify_content_hash(b"a", "00").is_err());
        let h = content_sha256_hex(b"a");
        assert!(verify_content_hash(b"a", &h).is_ok());
    }
}
