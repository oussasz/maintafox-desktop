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
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, QueryResult, Statement, TransactionTrait,
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
        relation_id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
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
pub(crate) async fn assert_asset_active(db: &impl ConnectionTrait, asset_id: i64) -> AppResult<()> {
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
            "GATE_ASSET_HIER_STATUS:status={status}: L'équipement {asset_id} a le statut '{status}' et ne peut pas \
             participer à une relation hiérarchique active."
        )]));
    }
    Ok(())
}

/// Validate that `relationship_type` exists in the
/// `equipment.hierarchy_relationship` lookup domain.
async fn validate_relationship_type(db: &impl ConnectionTrait, code: &str) -> AppResult<()> {
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
            "GATE_ASSET_HIER_RELATION_UNKNOWN: Type de relation '{code}' introuvable dans le domaine \
             'equipment.hierarchy_relationship'."
        )]));
    }
    Ok(())
}

/// For single-parent relation types (PARENT_CHILD, INSTALLED_IN), verify that
/// the child does not already have an active parent with the same type.
async fn assert_single_parent(db: &impl ConnectionTrait, child_id: i64, relation_type: &str) -> AppResult<()> {
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
            "GATE_ASSET_HIER_PARENT_EXISTS: L'équipement {child_id} a déjà un parent actif pour le type \
             de relation '{relation_type}'. Dissociez d'abord la relation existante."
        )]));
    }
    Ok(())
}

/// Detect hierarchy cycles via BFS. Starting from `parent_id`, walk up through
/// active relations. If `child_id` is encountered as an ancestor, creating the
/// proposed link would form a cycle.
pub(crate) async fn detect_cycle(db: &impl ConnectionTrait, parent_id: i64, child_id: i64) -> AppResult<()> {
    if parent_id == child_id {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_HIER_SELF_PARENT: Un équipement ne peut pas être son propre parent.".into(),
        ]));
    }

    // Walk up from parent_id. If we reach child_id, a cycle would be formed.
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![parent_id];

    while let Some(current) = queue.pop() {
        if current == child_id {
            return Err(AppError::ValidationFailed(vec![
                "GATE_ASSET_HIER_CYCLE: Cette relation créerait un cycle dans la hiérarchie \
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
pub async fn list_asset_children(db: &DatabaseConnection, parent_asset_id: i64) -> AppResult<Vec<AssetHierarchyRow>> {
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
pub async fn list_asset_parents(db: &DatabaseConnection, child_asset_id: i64) -> AppResult<Vec<AssetHierarchyRow>> {
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
    assert_single_parent(&txn, payload.child_asset_id, &payload.relation_type).await?;

    // ── Insert hierarchy relation ────────────────────────────────────────
    let effective_from = payload
        .effective_from
        .unwrap_or_else(|| crate::clock::now().to_rfc3339());
    let result = insert_relation_in(
        &txn,
        payload.parent_asset_id,
        payload.child_asset_id,
        &payload.relation_type,
        &effective_from,
    )
    .await?;

    txn.commit().await?;

    tracing::info!(relation_id = result.relation_id, "asset hierarchy link created");
    Ok(result)
}

/// Reads one hierarchy relation in the caller's transaction.
async fn fetch_relation_in(txn: &impl ConnectionTrait, relation_id: i64) -> AppResult<AssetHierarchyRow> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, parent_equipment_id, child_equipment_id, relationship_type, effective_from, effective_to FROM equipment_hierarchy WHERE id = ?",
            [relation_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("hierarchy relation {relation_id} not found")))?;
    map_hierarchy_row(&row)
}

/// The write of a link, in the caller's transaction (checks are the caller's): a new active
/// relation from `effective_from`.
async fn insert_relation_in(
    txn: &impl ConnectionTrait,
    parent_asset_id: i64,
    child_asset_id: i64,
    relation_type: &str,
    effective_from: &str,
) -> AppResult<AssetHierarchyRow> {
    let now = crate::clock::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_hierarchy \
         (parent_equipment_id, child_equipment_id, relationship_type, \
          effective_from, created_at) \
         VALUES (?, ?, ?, ?, ?)",
        [
            parent_asset_id.into(),
            child_asset_id.into(),
            relation_type.into(),
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
    let relation_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    fetch_relation_in(txn, relation_id).await
}

/// The write of an unlink, in the caller's transaction: the relation is ended at `effective_to`,
/// never deleted. A relation already ended is refused.
async fn end_relation_in(
    txn: &impl ConnectionTrait,
    relation_id: i64,
    effective_to: &str,
) -> AppResult<AssetHierarchyRow> {
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
            "GATE_ASSET_HIER_RELATION_ENDED: La relation {relation_id} est déjà terminée \
             (effective_to est déjà défini)."
        )]));
    }

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment_hierarchy SET effective_to = ? WHERE id = ?",
        [effective_to.into(), relation_id.into()],
    ))
    .await?;
    fetch_relation_in(txn, relation_id).await
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
    let end_date = effective_to.unwrap_or_else(|| crate::clock::now().to_rfc3339());
    let result = end_relation_in(&txn, relation_id, &end_date).await?;
    txn.commit().await?;

    tracing::info!(relation_id, "asset hierarchy link ended");
    Ok(result)
}

// ─── Component move (L2-03) ───────────────────────────────────────────────────

/// Payload of "Déplacer le composant": the same asset record (and serial number) goes under
/// another parent asset, from a date.
#[derive(Debug, Clone, Deserialize)]
pub struct MoveAssetComponentPayload {
    pub asset_id: i64,
    /// The new parent; `None` removes the component to stock (no parent).
    pub new_parent_asset_id: Option<i64>,
    /// Effective date `YYYY-MM-DD`; `None` is today.
    pub effective_date: Option<String>,
    /// Free-text note of the move (no reference list of move reasons exists).
    pub notes: Option<String>,
    /// The component's row version the person saw.
    pub expected_row_version: i64,
}

/// What a component move changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveAssetComponentResult {
    /// The parent relation ended at the effective date (none for a first installation).
    pub ended_relation: Option<AssetHierarchyRow>,
    /// The relation created from the effective date (none for a removal to stock).
    pub new_relation: Option<AssetHierarchyRow>,
    /// The `PARENT_CHANGED` lifecycle event recorded on the component.
    pub event: crate::assets::lifecycle::AssetLifecycleEvent,
}

/// Relation type of a component link when the component has no parent yet: the one the
/// "Ajouter un composant" flow creates.
const COMPONENT_RELATION_TYPE: &str = "PARENT_CHILD";

/// Asset state read for a move: version, status and site (installation node).
struct MoveAssetState {
    row_version: i64,
    status: String,
    node_id: Option<i64>,
}

async fn move_asset_state(txn: &impl ConnectionTrait, asset_id: i64) -> AppResult<MoveAssetState> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version, lifecycle_status, installed_at_node_id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    Ok(MoveAssetState {
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        status: row
            .try_get("", "lifecycle_status")
            .map_err(|e| decode_err("lifecycle_status", e))?,
        node_id: row
            .try_get("", "installed_at_node_id")
            .map_err(|e| decode_err("installed_at_node_id", e))?,
    })
}

/// Refuses a retired asset (tag `retired` of the status reference data) in a component move.
async fn refuse_retired_in_move(txn: &impl ConnectionTrait, asset_id: i64, status: &str) -> AppResult<()> {
    if identity::is_retired_status_code(txn, status).await? {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_ASSET_MOVE_RETIRED:status={status}: L'équipement {asset_id} a le statut '{status}' et ne peut \
             pas être posé ou déposé."
        )]));
    }
    Ok(())
}

fn parse_rfc3339_utc(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// "Déplacer le composant" (L2-03): ends the component's active parent relation at the effective
/// date, creates the relation under the new parent from that date, records a `PARENT_CHANGED`
/// lifecycle event (from/to parent, note, person) and an audit entry, all in one transaction.
/// Works as a first installation (no current parent) and as a removal to stock (no new parent).
/// The component, its current parent and its new parent must each be in a site where `scope`
/// (where the person holds `eq.manage`) allows acting.
pub async fn move_asset_component(
    db: &DatabaseConnection,
    payload: MoveAssetComponentPayload,
    scope: &crate::auth::scope::AccessScope,
    actor_id: i32,
) -> AppResult<MoveAssetComponentResult> {
    let txn = db.begin().await?;
    let result = move_asset_component_in(&txn, payload, scope, actor_id).await?;
    txn.commit().await?;
    tracing::info!(
        asset_id = result.event.asset_id,
        event_id = result.event.id,
        "asset component moved"
    );
    Ok(result)
}

async fn move_asset_component_in(
    txn: &DatabaseTransaction,
    payload: MoveAssetComponentPayload,
    scope: &crate::auth::scope::AccessScope,
    actor_id: i32,
) -> AppResult<MoveAssetComponentResult> {
    use crate::auth::scope::{check_anchor, Anchor};
    let permission = crate::rbac::permissions::EQ_MANAGE;
    let asset_id = payload.asset_id;

    // ── The component: version seen, not retired, in the person's sites ──
    let component = move_asset_state(txn, asset_id).await?;
    check_anchor(txn, scope, permission, Anchor::of(component.node_id)).await?;
    if component.row_version != payload.expected_row_version {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : version attendue {}, version actuelle {}.",
            payload.expected_row_version, component.row_version
        )]));
    }
    refuse_retired_in_move(txn, asset_id, &component.status).await?;

    // ── Effective date: a real date, not in the future ───────────────────
    let now = crate::clock::now();
    // The person's calendar day (the screen proposes the local date).
    let today = crate::clock::now_local().date_naive();
    let date = match payload.effective_date.as_deref().map(str::trim) {
        None | Some("") => today,
        Some(raw) => chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "GATE_ASSET_MOVE_DATE_INVALID: Date d'effet illisible '{raw}' (attendu AAAA-MM-JJ)."
            )])
        })?,
    };
    if date > today {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_ASSET_MOVE_DATE_FUTURE: La date d'effet {date} est dans le futur."
        )]));
    }

    // ── The current parent (single-parent relation types) ────────────────
    let types_sql = SINGLE_PARENT_TYPES
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let active = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT id, parent_equipment_id, child_equipment_id, relationship_type, effective_from, effective_to FROM equipment_hierarchy WHERE child_equipment_id = ? AND effective_to IS NULL AND relationship_type IN ({types_sql}) ORDER BY id"
            ),
            [asset_id.into()],
        ))
        .await?
        .iter()
        .map(map_hierarchy_row)
        .collect::<AppResult<Vec<_>>>()?;
    // One parent per type is allowed by a link, so a component can be under two parents at once
    // (one per single-parent type): which one it leaves is then not known, the move is refused.
    if active.len() > 1 {
        let count = active.len();
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_ASSET_MOVE_SEVERAL_PARENTS:count={count}: Le composant a {count} parents actifs ; \
             dissociez-en d'abord un dans la structure."
        )]));
    }
    let current = active.into_iter().next();
    let from_parent = current.as_ref().map(|r| r.parent_asset_id);

    if from_parent == payload.new_parent_asset_id {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_MOVE_SAME_PARENT: Le composant est déjà à cet emplacement (même parent).".into(),
        ]));
    }

    // ── Current parent's site; the date is not before the installation ───
    let mut effective_at = if date == today {
        now
    } else {
        date.and_hms_opt(0, 0, 0).expect("midnight exists").and_utc()
    };
    // Not before the component's last removal: its history of parents never overlaps.
    let last_removal: Option<String> = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT MAX(effective_to) AS last_to FROM equipment_hierarchy WHERE child_equipment_id = ? AND effective_to IS NOT NULL AND relationship_type IN ({types_sql})"
            ),
            [asset_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<Option<String>>("", "last_to"))
        .transpose()
        .map_err(|e| decode_err("last_to", e))?
        .flatten();
    if let Some(removed_at) = last_removal.as_deref() {
        let removed_on = removed_at.get(..10).unwrap_or(removed_at);
        if let Ok(removed_date) = chrono::NaiveDate::parse_from_str(removed_on, "%Y-%m-%d") {
            if date < removed_date {
                return Err(AppError::ValidationFailed(vec![format!(
                    "GATE_ASSET_MOVE_DATE_BEFORE_REMOVAL:removed_on={removed_date}: La date d'effet {date} \
                     précède la dernière dépose du composant, le {removed_date}."
                )]));
            }
        }
        // Same day as the last removal: the new installation is not stamped before it.
        if let Some(removed) = parse_rfc3339_utc(removed_at) {
            if effective_at < removed {
                effective_at = removed;
            }
        }
    }
    if let Some(cur) = &current {
        let parent = move_asset_state(txn, cur.parent_asset_id).await?;
        check_anchor(txn, scope, permission, Anchor::of(parent.node_id)).await?;
        if let Some(installed_at) = cur.effective_from.as_deref() {
            let installed_on = installed_at.get(..10).unwrap_or(installed_at);
            if let Ok(installed_date) = chrono::NaiveDate::parse_from_str(installed_on, "%Y-%m-%d") {
                if date < installed_date {
                    return Err(AppError::ValidationFailed(vec![format!(
                        "GATE_ASSET_MOVE_DATE_BEFORE_INSTALL:installed_on={installed_date}: La date d'effet {date} \
                         précède la pose actuelle du {installed_date}."
                    )]));
                }
            }
            // Same day as the installation: the removal is not stamped before it.
            if let Some(installed) = parse_rfc3339_utc(installed_at) {
                if effective_at < installed {
                    effective_at = installed;
                }
            }
        }
    }

    // ── New parent: no cycle, not retired, in the person's sites ─────────
    let relation_type = current
        .as_ref()
        .map(|r| r.relation_type.clone())
        .unwrap_or_else(|| COMPONENT_RELATION_TYPE.to_string());
    if let Some(new_parent) = payload.new_parent_asset_id {
        detect_cycle(txn, new_parent, asset_id).await?;
        let parent = move_asset_state(txn, new_parent).await?;
        check_anchor(txn, scope, permission, Anchor::of(parent.node_id)).await?;
        refuse_retired_in_move(txn, new_parent, &parent.status).await?;
        validate_relationship_type(txn, &relation_type).await?;
    }

    // ── Writes: end, link, version, event + audit ────────────────────────
    let effective_at = effective_at.to_rfc3339();
    let ended_relation = match &current {
        Some(cur) => Some(end_relation_in(txn, cur.relation_id, &effective_at).await?),
        None => None,
    };
    let new_relation = match payload.new_parent_asset_id {
        Some(new_parent) => Some(insert_relation_in(txn, new_parent, asset_id, &relation_type, &effective_at).await?),
        None => None,
    };
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment SET updated_at = ?, row_version = row_version + 1 WHERE id = ?",
        [now.to_rfc3339().into(), asset_id.into()],
    ))
    .await?;

    let notes = payload
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    let event = crate::assets::lifecycle::record_parent_change_in(
        txn,
        crate::assets::lifecycle::ParentChange {
            asset_id,
            occurred_at: effective_at,
            from_parent_asset_id: from_parent,
            to_parent_asset_id: payload.new_parent_asset_id,
            relation_type,
            notes,
        },
        actor_id,
    )
    .await?;

    Ok(MoveAssetComponentResult {
        ended_relation,
        new_relation,
        event,
    })
}

/// Move an asset to a different org node.
///
/// Preserves the asset row and increments `row_version` for optimistic
/// concurrency control. Decommissioned/scrapped assets cannot be moved
/// (admin override deferred to File 04). The move is recorded as a `MOVED` lifecycle event with
/// an audit entry in the same transaction (D19).
pub async fn move_asset_org_node(
    db: &DatabaseConnection,
    asset_id: i64,
    new_org_node_id: i64,
    expected_row_version: i64,
    actor_id: i32,
) -> AppResult<identity::Asset> {
    let txn = db.begin().await?;
    crate::assets::lifecycle::move_asset_recorded_in(&txn, asset_id, new_org_node_id, expected_row_version, actor_id)
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("equipment {asset_id} not found after org-node move")))?;
    let asset = identity::map_asset(&asset_row)?;

    txn.commit().await?;

    tracing::info!(asset_id, new_org_node_id, "asset org node moved");
    Ok(asset)
}

/// The checks and the write of a move, in the caller's transaction: the asset is at the version
/// the person saw, it is not retired, and the target node is active. Shared by the move action
/// and by a `MOVED` lifecycle event (`assets::lifecycle`), so neither can move an asset the other
/// would refuse (AU-77).
pub(crate) async fn move_asset_org_node_in(
    txn: &impl ConnectionTrait,
    asset_id: i64,
    new_org_node_id: i64,
    expected_row_version: i64,
) -> AppResult<()> {
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
            "GATE_RECORD_CHANGED_ELSEWHERE: Conflit de version : version attendue {expected_row_version}, \
             version actuelle {current_version}."
        )]));
    }

    let status: String = current
        .try_get("", "lifecycle_status")
        .map_err(|e| decode_err("lifecycle_status", e))?;
    // Retired as the reference data says (tag `retired` of EQUIPMENT.STATUS), as for new work.
    if identity::is_retired_status_code(txn, &status).await? {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_ASSET_HIER_MOVE_STATUS:status={status}: L'équipement {asset_id} a le statut '{status}' et ne peut pas \
             être déplacé. Contactez un administrateur pour forcer le \
             déplacement."
        )]));
    }

    // ── Validate new org node ────────────────────────────────────────────
    identity::assert_org_node_active(txn, new_org_node_id).await?;

    // ── Update org node and increment row_version ────────────────────────
    let now = crate::clock::now().to_rfc3339();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment SET installed_at_node_id = ?, updated_at = ?, \
         row_version = row_version + 1 WHERE id = ?",
        [new_org_node_id.into(), now.into(), asset_id.into()],
    ))
    .await?;
    Ok(())
}
