//! Asset hierarchy and org-node binding service.
//!
//! Phase 2 - Sub-phase 02 - File 01 - Sprint S2.
//!
//! This service provides governed parent-child hierarchy operations on top of
//! the existing `equipment_hierarchy` table (migration 005, extended by
//! migration 010 with effective dating).
//!
//! Column reconciliation:
//!   roadmap field        → DB column
//!   ─────────────────────────────────────────
//!   relation_id          → equipment_hierarchy.id
//!   parent_asset_id      → equipment_hierarchy.parent_equipment_id
//!   child_asset_id       → equipment_hierarchy.child_equipment_id
//!   relation_type        → equipment_hierarchy.relationship_type
//!   effective_from       → equipment_hierarchy.effective_from
//!   effective_to         → equipment_hierarchy.effective_to

use crate::assets::identity;
use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

/// Hierarchy relation row for reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetHierarchyRow {
    pub relation_id: i64,
    pub parent_asset_id: i64,
    pub child_asset_id: i64,
    pub relation_type: String,
    pub effective_from: Option<String>,
    pub effective_to: Option<String>,
}

/// Payload for linking two assets in a parent-child hierarchy relation.
#[derive(Debug, Deserialize)]
pub struct LinkAssetPayload {
    pub parent_asset_id: i64,
    pub child_asset_id: i64,
    pub relation_type: String,
    pub effective_from: Option<String>,
}

// ─── Constants ────────────────────────────────────────────────────────────────

/// Relation types that enforce at most one active parent per child.
const SINGLE_PARENT_TYPES: &[&str] = &["PARENT_CHILD", "INSTALLED_IN"];

// ─── Row mapping ──────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "equipment_hierarchy row decode failed for column '{column}': {e}"
    ))
}

fn map_hierarchy_row(row: &QueryResult) -> AppResult<AssetHierarchyRow> {
    Ok(AssetHierarchyRow {
        relation_id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        parent_asset_id: row
            .try_get::<i64>("", "parent_equipment_id")
            .map_err(|e| decode_err("parent_equipment_id", e))?,
        child_asset_id: row
            .try_get::<i64>("", "child_equipment_id")
            .map_err(|e| decode_err("child_equipment_id", e))?,
        relation_type: row
            .try_get::<String>("", "relationship_type")
            .map_err(|e| decode_err("relationship_type", e))?,
        effective_from: row
            .try_get::<Option<String>>("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?,
        effective_to: row
            .try_get::<Option<String>>("", "effective_to")
            .map_err(|e| decode_err("effective_to", e))?,
    })
}

// ─── Validation helpers ───────────────────────────────────────────────────────

/// Assert that an equipment row exists, is not deleted, and is not
/// DECOMMISSIONED or SCRAPPED.
async fn assert_asset_active(
    db: &impl ConnectionTrait,
    asset_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lifecycle_status FROM equipment \
             WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    let status: String = row
        .try_get("", "lifecycle_status")
        .map_err(|e| decode_err("lifecycle_status", e))?;
    if status == "DECOMMISSIONED" || status == "SCRAPPED" {
        return Err(AppError::ValidationFailed(vec![format!(
            "L'équipement {asset_id} a le statut '{status}' et ne peut pas \
             participer à une relation hiérarchique active."
        )]));
    }
    Ok(())
}

/// Validate that `relationship_type` exists in the
/// `equipment.hierarchy_relationship` lookup domain.
async fn validate_relationship_type(
    db: &impl ConnectionTrait,
    code: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = 'equipment.hierarchy_relationship' \
               AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [code.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Type de relation '{code}' introuvable dans le domaine \
             'equipment.hierarchy_relationship'."
        )]));
    }
    Ok(())
}

/// For single-parent relation types (PARENT_CHILD, INSTALLED_IN), verify that
/// the child does not already have an active parent with the same type.
async fn assert_single_parent(
    db: &impl ConnectionTrait,
    child_id: i64,
    relation_type: &str,
) -> AppResult<()> {
    if !SINGLE_PARENT_TYPES.contains(&relation_type) {
        return Ok(());
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM equipment_hierarchy \
             WHERE child_equipment_id = ? AND relationship_type = ? \
             AND effective_to IS NULL",
            [child_id.into(), relation_type.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "L'équipement {child_id} a déjà un parent actif pour le type \
             de relation '{relation_type}'. Dissociez d'abord la relation existante."
        )]));
    }
    Ok(())
}

/// Detect hierarchy cycles via BFS. Starting from `parent_id`, walk up through
/// active relations. If `child_id` is encountered as an ancestor, creating the
/// proposed link would form a cycle.
async fn detect_cycle(
    db: &impl ConnectionTrait,
    parent_id: i64,
    child_id: i64,
) -> AppResult<()> {
    if parent_id == child_id {
        return Err(AppError::ValidationFailed(vec![
            "Un équipement ne peut pas être son propre parent.".into(),
        ]));
    }

    // Walk up from parent_id. If we reach child_id, a cycle would be formed.
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![parent_id];

    while let Some(current) = queue.pop() {
        if current == child_id {
            return Err(AppError::ValidationFailed(vec![
                "Cette relation créerait un cycle dans la hiérarchie \
                 des équipements."
                    .into(),
            ]));
        }
        if !visited.insert(current) {
            continue;
        }
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT parent_equipment_id FROM equipment_hierarchy \
                 WHERE child_equipment_id = ? AND effective_to IS NULL",
                [current.into()],
            ))
            .await?;
        for row in &rows {
            let pid: i64 = row
                .try_get("", "parent_equipment_id")
                .map_err(|e| decode_err("parent_equipment_id", e))?;
            queue.push(pid);
        }
    }

    Ok(())
}

// ─── Service functions ────────────────────────────────────────────────────────

/// List active child relations for a given parent asset.
pub async fn list_asset_children(
    db: &DatabaseConnection,
    parent_asset_id: i64,
) -> AppResult<Vec<AssetHierarchyRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_equipment_id, child_equipment_id, \
             relationship_type, effective_from, effective_to \
             FROM equipment_hierarchy \
             WHERE parent_equipment_id = ? AND effective_to IS NULL \
             ORDER BY child_equipment_id ASC",
            [parent_asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_hierarchy_row).collect()
}

/// List active parent relations for a given child asset.
pub async fn list_asset_parents(
    db: &DatabaseConnection,
    child_asset_id: i64,
) -> AppResult<Vec<AssetHierarchyRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_equipment_id, child_equipment_id, \
             relationship_type, effective_from, effective_to \
             FROM equipment_hierarchy \
             WHERE child_equipment_id = ? AND effective_to IS NULL \
             ORDER BY parent_equipment_id ASC",
            [child_asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_hierarchy_row).collect()
}

/// Link two assets in a parent-child hierarchy relation.
///
/// Validation:
///   - parent and child cannot be the same asset
///   - no hierarchy cycles allowed
///   - child must not already have an active parent for single-parent types
///   - both assets must be active (not decommissioned/scrapped)
///   - relation type must exist in `equipment.hierarchy_relationship` domain
pub async fn link_asset_hierarchy(
    db: &DatabaseConnection,
    payload: LinkAssetPayload,
    _actor_id: i32,
) -> AppResult<AssetHierarchyRow> {
    let txn = db.begin().await?;

    // ── Self-reference and cycle detection ────────────────────────────────
    detect_cycle(&txn, payload.parent_asset_id, payload.child_asset_id).await?;

    // ── Both assets must exist and be active ─────────────────────────────
    assert_asset_active(&txn, payload.parent_asset_id).await?;
    assert_asset_active(&txn, payload.child_asset_id).await?;

    // ── Validate relationship type against lookup domain ─────────────────
    validate_relationship_type(&txn, &payload.relation_type).await?;

    // ── Single-parent enforcement ────────────────────────────────────────
    assert_single_parent(
        &txn,
        payload.child_asset_id,
        &payload.relation_type,
    )
    .await?;

    // ── Insert hierarchy relation ────────────────────────────────────────
    let now = Utc::now().to_rfc3339();
    let effective_from = payload
        .effective_from
        .unwrap_or_else(|| now.clone());

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_hierarchy \
         (parent_equipment_id, child_equipment_id, relationship_type, \
          effective_from, created_at) \
         VALUES (?, ?, ?, ?, ?)",
        [
            payload.parent_asset_id.into(),
            payload.child_asset_id.into(),
            payload.relation_type.into(),
            effective_from.into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve inserted row via last_insert_rowid
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .expect("last_insert_rowid always returns");
    let relation_id: i64 = id_row
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_equipment_id, child_equipment_id, \
             relationship_type, effective_from, effective_to \
             FROM equipment_hierarchy WHERE id = ?",
            [relation_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "hierarchy link created but not found after insert"
            ))
        })?;
    let result = map_hierarchy_row(&row)?;

    txn.commit().await?;

    tracing::info!(
        relation_id = result.relation_id,
        "asset hierarchy link created"
    );
    Ok(result)
}

/// Unlink an asset hierarchy relation by setting `effective_to`.
///
/// The row remains in the table for historical evidence — no hard delete.
pub async fn unlink_asset_hierarchy(
    db: &DatabaseConnection,
    relation_id: i64,
    effective_to: Option<String>,
    _actor_id: i32,
) -> AppResult<AssetHierarchyRow> {
    let txn = db.begin().await?;

    // ── Verify relation exists and is currently active ────────────────────
    let existing = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, effective_to FROM equipment_hierarchy WHERE id = ?",
            [relation_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment_hierarchy".into(),
            id: relation_id.to_string(),
        })?;

    let current_eff_to: Option<String> = existing
        .try_get("", "effective_to")
        .map_err(|e| decode_err("effective_to", e))?;

    if current_eff_to.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "La relation {relation_id} est déjà terminée \
             (effective_to est déjà défini)."
        )]));
    }

    // ── Set effective_to ─────────────────────────────────────────────────
    let end_date = effective_to.unwrap_or_else(|| Utc::now().to_rfc3339());

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment_hierarchy SET effective_to = ? WHERE id = ?",
        [end_date.into(), relation_id.into()],
    ))
    .await?;

    // ── Fetch updated row ────────────────────────────────────────────────
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_equipment_id, child_equipment_id, \
             relationship_type, effective_from, effective_to \
             FROM equipment_hierarchy WHERE id = ?",
            [relation_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "hierarchy relation {relation_id} not found after update"
            ))
        })?;
    let result = map_hierarchy_row(&row)?;

    txn.commit().await?;

    tracing::info!(relation_id, "asset hierarchy link ended");
    Ok(result)
}

/// Move an asset to a different org node.
///
/// Preserves the asset row and increments `row_version` for optimistic
/// concurrency control. Decommissioned/scrapped assets cannot be moved
/// (admin override deferred to File 04).
pub async fn move_asset_org_node(
    db: &DatabaseConnection,
    asset_id: i64,
    new_org_node_id: i64,
    expected_row_version: i64,
    _actor_id: i32,
) -> AppResult<identity::Asset> {
    let txn = db.begin().await?;

    // ── Fetch current row and verify row_version ─────────────────────────
    let current = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version, lifecycle_status FROM equipment \
             WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;

    let current_version: i64 = current
        .try_get("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    if current_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![format!(
            "Conflit de version : version attendue {expected_row_version}, \
             version actuelle {current_version}."
        )]));
    }

    let status: String = current
        .try_get("", "lifecycle_status")
        .map_err(|e| decode_err("lifecycle_status", e))?;
    if status == "DECOMMISSIONED" || status == "SCRAPPED" {
        return Err(AppError::ValidationFailed(vec![format!(
            "L'équipement {asset_id} a le statut '{status}' et ne peut pas \
             être déplacé. Contactez un administrateur pour forcer le \
             déplacement."
        )]));
    }

    // ── Validate new org node ────────────────────────────────────────────
    identity::assert_org_node_active(&txn, new_org_node_id).await?;

    // ── Update org node and increment row_version ────────────────────────
    let now = Utc::now().to_rfc3339();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment SET installed_at_node_id = ?, updated_at = ?, \
         row_version = row_version + 1 WHERE id = ?",
        [new_org_node_id.into(), now.into(), asset_id.into()],
    ))
    .await?;

    // ── Re-fetch full asset with resolved codes ──────────────────────────
    let sql = format!(
        "SELECT {} {} WHERE e.id = ?",
        identity::ASSET_SELECT,
        identity::ASSET_FROM,
    );
    let asset_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "equipment {asset_id} not found after org-node move"
            ))
        })?;
    let asset = identity::map_asset(&asset_row)?;

    txn.commit().await?;

    tracing::info!(asset_id, new_org_node_id, "asset org node moved");
    Ok(asset)
}
