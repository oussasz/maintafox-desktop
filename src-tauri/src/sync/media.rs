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
    fn verify_hash_rejects_mismatch() {
        assert!(verify_content_hash(b"a", "00").is_err());
        let h = content_sha256_hex(b"a");
        assert!(verify_content_hash(b"a", &h).is_ok());
    }
}
