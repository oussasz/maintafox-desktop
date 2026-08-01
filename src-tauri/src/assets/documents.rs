//! Asset document link governance service.
//!
//! Phase 2 - Sub-phase 02 - File 02 - Sprint S3.
//!
//! Provides governed document references for assets. Links point to managed
//! documents by stable reference ids (not ad-hoc URLs) and include purpose
//! codes from the `equipment.document_link_purpose` lookup domain.
//!
//! Document links are superseded or expired, not hard-deleted.
//! `valid_to IS NULL` means the link is currently active.
//!
//! Column reconciliation (roadmap → DB):
//!   roadmap field        → DB column
//!   ─────────────────────────────────────────
//!   asset_id             → asset_document_links.asset_id
//!   document_ref         → asset_document_links.document_ref
//!   link_purpose         → asset_document_links.link_purpose
//!   is_primary           → asset_document_links.is_primary
//!   valid_from           → asset_document_links.valid_from
//!   valid_to             → asset_document_links.valid_to
//!   created_by_id        → asset_document_links.created_by_id
//!   created_at           → asset_document_links.created_at

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

/// Read-side document link record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetDocumentLink {
    pub id: i64,
    pub asset_id: i64,
    pub document_ref: String,
    pub link_purpose: String,
    pub is_primary: bool,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub created_by_id: Option<i64>,
    pub created_at: String,
}

/// Payload for creating or updating a document link.
#[derive(Debug, Deserialize)]
pub struct UpsertDocumentLinkPayload {
    pub asset_id: i64,
    pub document_ref: String,
    pub link_purpose: String,
    pub is_primary: Option<bool>,
    pub valid_from: Option<String>,
}

// ─── Row mapping ──────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "asset_document_links row decode failed for column '{column}': {e}"
    ))
}

fn map_doc_link(row: &QueryResult) -> AppResult<AssetDocumentLink> {
    Ok(AssetDocumentLink {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        asset_id: row
            .try_get::<i64>("", "asset_id")
            .map_err(|e| decode_err("asset_id", e))?,
        document_ref: row
            .try_get::<String>("", "document_ref")
            .map_err(|e| decode_err("document_ref", e))?,
        link_purpose: row
            .try_get::<String>("", "link_purpose")
            .map_err(|e| decode_err("link_purpose", e))?,
        is_primary: row
            .try_get::<bool>("", "is_primary")
            .map_err(|e| decode_err("is_primary", e))?,
        valid_from: row
            .try_get::<Option<String>>("", "valid_from")
            .map_err(|e| decode_err("valid_from", e))?,
        valid_to: row
            .try_get::<Option<String>>("", "valid_to")
            .map_err(|e| decode_err("valid_to", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

// ─── Validation helpers ───────────────────────────────────────────────────────

/// Validate that a code exists in a given lookup domain.
async fn validate_lookup(
    db: &impl ConnectionTrait,
    domain_key: &str,
    code: &str,
    field_label: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = ? \
               AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [domain_key.into(), code.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Valeur '{code}' introuvable dans le domaine '{domain_key}' ({field_label})."
        )]));
    }
    Ok(())
}

/// Assert that an equipment row exists and is not soft-deleted.
async fn assert_asset_exists(
    db: &impl ConnectionTrait,
    asset_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        });
    }
    Ok(())
}

/// Expire the existing primary link for the same `(asset_id, link_purpose)`.
/// Returns the number of rows affected.
async fn expire_existing_primary(
    db: &impl ConnectionTrait,
    asset_id: i64,
    link_purpose: &str,
    now: &str,
) -> AppResult<u64> {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE asset_document_links SET valid_to = ? \
             WHERE asset_id = ? AND link_purpose = ? \
               AND is_primary = 1 AND valid_to IS NULL",
            [now.into(), asset_id.into(), link_purpose.into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

// ─── Select fragment ──────────────────────────────────────────────────────────

const DOC_LINK_SELECT: &str = r"
    id, asset_id, document_ref, link_purpose, is_primary,
    valid_from, valid_to, created_by_id, created_at
";

// ─── Service functions ────────────────────────────────────────────────────────

/// List document links for an asset.
///
/// When `include_expired` is false (default), only active links (`valid_to IS NULL`)
/// are returned.
pub async fn list_asset_document_links(
    db: &DatabaseConnection,
    asset_id: i64,
    include_expired: bool,
) -> AppResult<Vec<AssetDocumentLink>> {
    let sql = if include_expired {
        format!(
            "SELECT {DOC_LINK_SELECT} FROM asset_document_links \
             WHERE asset_id = ? \
             ORDER BY link_purpose ASC, is_primary DESC, created_at DESC"
        )
    } else {
        format!(
            "SELECT {DOC_LINK_SELECT} FROM asset_document_links \
             WHERE asset_id = ? AND valid_to IS NULL \
             ORDER BY link_purpose ASC, is_primary DESC, created_at DESC"
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_doc_link).collect()
}

/// Upsert a governed document link.
///
/// Validation:
///   - `asset_id` must exist and not be soft-deleted
///   - `link_purpose` must exist in `equipment.document_link_purpose` domain
///   - `document_ref` must not be empty
///   - if `is_primary = true`, the existing primary for the same
///     `(asset_id, link_purpose)` is automatically expired (supersession)
pub async fn upsert_asset_document_link(
    db: &DatabaseConnection,
    payload: UpsertDocumentLinkPayload,
    actor_id: i32,
) -> AppResult<AssetDocumentLink> {
    let txn = db.begin().await?;

    // ── 1. Validate asset exists ─────────────────────────────────────────
    assert_asset_exists(&txn, payload.asset_id).await?;

    // ── 2. Validate link_purpose against lookup domain ───────────────────
    validate_lookup(
        &txn,
        "equipment.document_link_purpose",
        &payload.link_purpose,
        "link_purpose",
    )
    .await?;

    // ── 3. Validate document_ref ─────────────────────────────────────────
    let document_ref = payload.document_ref.trim().to_string();
    if document_ref.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "La reference du document ne peut pas etre vide.".into(),
        ]));
    }

    // ── 4. Enforce primary uniqueness (supersession) ─────────────────────
    let is_primary = payload.is_primary.unwrap_or(false);
    let now = Utc::now().to_rfc3339();

    if is_primary {
        expire_existing_primary(&txn, payload.asset_id, &payload.link_purpose, &now)
            .await?;
    }

    // ── 5. Insert the new link ───────────────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO asset_document_links \
         (asset_id, document_ref, link_purpose, is_primary, \
          valid_from, valid_to, created_by_id, created_at) \
         VALUES (?, ?, ?, ?, ?, NULL, ?, ?)",
        [
            payload.asset_id.into(),
            document_ref.into(),
            payload.link_purpose.into(),
            i32::from(is_primary).into(),
            payload.valid_from.into(),
            (actor_id as i64).into(),
            now.clone().into(),
        ],
    ))
    .await?;

    // ── 6. Retrieve the inserted link ────────────────────────────────────
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {DOC_LINK_SELECT} FROM asset_document_links \
                 WHERE asset_id = ? ORDER BY id DESC LIMIT 1"
            ),
            [payload.asset_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "document link created but not found after insert"
            ))
        })?;
    let link = map_doc_link(&row)?;

    txn.commit().await?;

    tracing::info!(
        link_id = link.id,
        asset_id = link.asset_id,
        purpose = %link.link_purpose,
        primary = link.is_primary,
        "document link upserted (actor={})", actor_id
    );
    Ok(link)
}

/// Expire a document link by setting its `valid_to` timestamp.
///
/// Returns the updated link record.
pub async fn expire_asset_document_link(
    db: &DatabaseConnection,
    link_id: i64,
    valid_to: Option<String>,
    actor_id: i32,
) -> AppResult<AssetDocumentLink> {
    let expire_at = valid_to.unwrap_or_else(|| Utc::now().to_rfc3339());

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE asset_document_links SET valid_to = ? \
             WHERE id = ? AND valid_to IS NULL",
            [expire_at.into(), link_id.into()],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "asset_document_links".into(),
            id: link_id.to_string(),
        });
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &format!(
                "SELECT {DOC_LINK_SELECT} FROM asset_document_links WHERE id = ?"
            ),
            [link_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "asset_document_links".into(),
            id: link_id.to_string(),
        })?;
    let link = map_doc_link(&row)?;

    tracing::info!(
        link_id = link.id,
        asset_id = link.asset_id,
        purpose = %link.link_purpose,
        "document link expired (actor={})", actor_id
    );
    Ok(link)
}
