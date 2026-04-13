//! Impact preview engine for the Organization Designer.
//!
//! Computes a read-only preview of the operational consequences of structural
//! mutations (move, deactivate, responsibility reassignment) *before* the admin
//! commits the action. The preview never writes to the database.
//!
//! The dependency summary includes placeholders for modules that are not yet
//! implemented, so the designer UI can show "unavailable" counts consistently.
//!
//! Sub-phase 01 File 03 — Sprint S1.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrgPreviewAction {
    MoveNode,
    DeactivateNode,
    ReassignResponsibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgImpactDependencySummary {
    pub domain: String,
    pub status: String,
    pub count: Option<i64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgImpactPreview {
    pub action: OrgPreviewAction,
    pub subject_node_id: i64,
    pub affected_node_count: i64,
    pub descendant_count: i64,
    pub active_responsibility_count: i64,
    pub active_binding_count: i64,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub dependencies: Vec<OrgImpactDependencySummary>,
}

#[derive(Debug, Deserialize)]
pub struct PreviewOrgChangePayload {
    pub action: String,
    pub node_id: i64,
    pub new_parent_id: Option<i64>,
    pub responsibility_type: Option<String>,
    pub replacement_person_id: Option<i64>,
    pub replacement_team_id: Option<i64>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "impact_preview row decode failed for column '{column}': {e}"
    ))
}

/// Standard future-module dependency placeholders.
/// These will be replaced with real counts as each module is implemented.
fn future_dependency_placeholders() -> Vec<OrgImpactDependencySummary> {
    vec![
        OrgImpactDependencySummary {
            domain: "assets".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Module 6.3 not yet implemented".to_string()),
        },
        OrgImpactDependencySummary {
            domain: "open_work".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Modules 6.4/6.5 not yet implemented".to_string()),
        },
        OrgImpactDependencySummary {
            domain: "permits".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Module 6.23 not yet implemented".to_string()),
        },
        OrgImpactDependencySummary {
            domain: "inventory".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Module 6.8 not yet implemented".to_string()),
        },
    ]
}

/// Count non-deleted descendants of a node using its `ancestor_path`.
async fn count_descendants(
    db: &impl ConnectionTrait,
    node_id: i64,
    ancestor_path: &str,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes \
             WHERE ancestor_path LIKE ? AND id != ? AND deleted_at IS NULL",
            [format!("{ancestor_path}%").into(), node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Count active descendants (status = 'active') of a node.
async fn count_active_descendants(
    db: &impl ConnectionTrait,
    node_id: i64,
    ancestor_path: &str,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes \
             WHERE ancestor_path LIKE ? AND id != ? AND deleted_at IS NULL AND status = 'active'",
            [format!("{ancestor_path}%").into(), node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Count active responsibilities on a node and its descendants.
async fn count_subtree_active_responsibilities(
    db: &impl ConnectionTrait,
    node_id: i64,
    ancestor_path: &str,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_responsibilities r \
             JOIN org_nodes n ON n.id = r.node_id \
             WHERE (n.id = ? OR (n.ancestor_path LIKE ? AND n.id != ?)) \
               AND n.deleted_at IS NULL \
               AND r.valid_to IS NULL",
            [
                node_id.into(),
                format!("{ancestor_path}%").into(),
                node_id.into(),
            ],
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Count active bindings on a node and its descendants.
async fn count_subtree_active_bindings(
    db: &impl ConnectionTrait,
    node_id: i64,
    ancestor_path: &str,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_entity_bindings b \
             JOIN org_nodes n ON n.id = b.node_id \
             WHERE (n.id = ? OR (n.ancestor_path LIKE ? AND n.id != ?)) \
               AND n.deleted_at IS NULL \
               AND b.valid_to IS NULL",
            [
                node_id.into(),
                format!("{ancestor_path}%").into(),
                node_id.into(),
            ],
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Fetch basic node info needed for preview computations.
#[allow(dead_code)]
struct NodeBrief {
    id: i64,
    node_type_id: i64,
    ancestor_path: String,
    status: String,
}

async fn fetch_node_brief(
    db: &impl ConnectionTrait,
    node_id: i64,
) -> AppResult<NodeBrief> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, node_type_id, ancestor_path, status \
             FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [node_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".to_string(),
            id: node_id.to_string(),
        })?;
    Ok(NodeBrief {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        node_type_id: row
            .try_get::<i64>("", "node_type_id")
            .map_err(|e| decode_err("node_type_id", e))?,
        ancestor_path: row
            .try_get::<String>("", "ancestor_path")
            .map_err(|e| decode_err("ancestor_path", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
    })
}

/// Check whether a parent-child type rule exists in the active model.
async fn is_parent_child_allowed(
    db: &impl ConnectionTrait,
    parent_type_id: i64,
    child_type_id: i64,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_type_relationship_rules r \
             JOIN org_structure_models m ON m.id = r.structure_model_id AND m.status = 'active' \
             WHERE r.parent_type_id = ? AND r.child_type_id = ?",
            [parent_type_id.into(), child_type_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    Ok(cnt > 0)
}

// ─── Preview functions ────────────────────────────────────────────────────────

/// Preview the consequences of moving `node_id` under `new_parent_id`.
///
/// Blockers:
/// - `new_parent_id` is inside the subtree (creating a cycle)
/// - parent-child type rule is invalid in the active model
///
/// Warnings:
/// - descendants have active responsibilities
/// - descendants have active external bindings
pub async fn preview_move_node(
    db: &DatabaseConnection,
    node_id: i64,
    new_parent_id: i64,
) -> AppResult<OrgImpactPreview> {
    let node = fetch_node_brief(db, node_id).await?;
    let parent = fetch_node_brief(db, new_parent_id).await?;

    let descendant_count = count_descendants(db, node.id, &node.ancestor_path).await?;
    let active_resp = count_subtree_active_responsibilities(db, node.id, &node.ancestor_path).await?;
    let active_bind = count_subtree_active_bindings(db, node.id, &node.ancestor_path).await?;

    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Blocker: cycle detection — new parent is inside this node's subtree.
    let self_segment = format!("/{}/", node.id);
    if parent.ancestor_path.contains(&self_segment) || parent.id == node.id {
        blockers.push(
            "Cannot move this node under one of its own descendants (cycle detected)".to_string(),
        );
    }

    // Blocker: parent-child type rule validation.
    if !is_parent_child_allowed(db, parent.node_type_id, node.node_type_id).await? {
        blockers.push(
            "The active structure model does not allow this parent-child type combination"
                .to_string(),
        );
    }

    // Warning: active responsibilities in subtree.
    if active_resp > 0 {
        warnings.push(format!(
            "{active_resp} active responsibility assignment(s) in the affected subtree"
        ));
    }

    // Warning: active external bindings in subtree.
    if active_bind > 0 {
        warnings.push(format!(
            "{active_bind} active external binding(s) in the affected subtree"
        ));
    }

    // affected_node_count = self + descendants
    let affected_node_count = 1 + descendant_count;

    Ok(OrgImpactPreview {
        action: OrgPreviewAction::MoveNode,
        subject_node_id: node_id,
        affected_node_count,
        descendant_count,
        active_responsibility_count: active_resp,
        active_binding_count: active_bind,
        blockers,
        warnings,
        dependencies: future_dependency_placeholders(),
    })
}

/// Preview the consequences of deactivating `node_id`.
///
/// Blockers:
/// - active descendants exist (must deactivate children first)
/// - active responsibility assignments exist (must end them first)
///
/// Warnings:
/// - active external bindings exist on the node itself
pub async fn preview_deactivate_node(
    db: &DatabaseConnection,
    node_id: i64,
) -> AppResult<OrgImpactPreview> {
    let node = fetch_node_brief(db, node_id).await?;

    let descendant_count = count_descendants(db, node.id, &node.ancestor_path).await?;
    let active_desc = count_active_descendants(db, node.id, &node.ancestor_path).await?;

    // Responsibilities and bindings scoped to this node only (not subtree).
    let resp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_responsibilities \
             WHERE node_id = ? AND valid_to IS NULL",
            [node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let active_resp: i64 = resp_row
        .try_get("", "cnt")
        .map_err(|e| decode_err("cnt", e))?;

    let bind_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_entity_bindings \
             WHERE node_id = ? AND valid_to IS NULL",
            [node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let active_bind: i64 = bind_row
        .try_get("", "cnt")
        .map_err(|e| decode_err("cnt", e))?;

    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Blocker: active descendants.
    if active_desc > 0 {
        blockers.push(format!(
            "{active_desc} active descendant node(s) must be deactivated first"
        ));
    }

    // Blocker: active responsibilities.
    if active_resp > 0 {
        blockers.push(format!(
            "{active_resp} active responsibility assignment(s) must be ended first"
        ));
    }

    // Warning: active bindings.
    if active_bind > 0 {
        warnings.push(format!(
            "{active_bind} active external binding(s) will be orphaned"
        ));
    }

    Ok(OrgImpactPreview {
        action: OrgPreviewAction::DeactivateNode,
        subject_node_id: node_id,
        affected_node_count: 1,
        descendant_count,
        active_responsibility_count: active_resp,
        active_binding_count: active_bind,
        blockers,
        warnings,
        dependencies: future_dependency_placeholders(),
    })
}

/// Preview the consequences of reassigning a responsibility on `node_id`.
///
/// Blockers:
/// - replacement target is missing (neither person nor team provided)
///
/// Warnings:
/// - the current assignment of that type is already ended (historical)
pub async fn preview_responsibility_reassignment(
    db: &DatabaseConnection,
    node_id: i64,
    responsibility_type: &str,
    replacement_person_id: Option<i64>,
    replacement_team_id: Option<i64>,
) -> AppResult<OrgImpactPreview> {
    // Verify the node exists.
    let _node = fetch_node_brief(db, node_id).await?;

    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Blocker: no replacement target.
    if replacement_person_id.is_none() && replacement_team_id.is_none() {
        blockers.push(
            "A replacement person or team must be specified for the reassignment".to_string(),
        );
    }

    // Check the current assignment state.
    let current_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, valid_to FROM org_node_responsibilities \
             WHERE node_id = ? AND responsibility_type = ? \
             ORDER BY created_at DESC LIMIT 1",
            [node_id.into(), responsibility_type.to_string().into()],
        ))
        .await?;

    match current_row {
        Some(row) => {
            let valid_to: Option<String> = row
                .try_get("", "valid_to")
                .map_err(|e| decode_err("valid_to", e))?;
            if valid_to.is_some() {
                warnings.push(format!(
                    "The current '{responsibility_type}' assignment is already ended — \
                     this will create a new assignment rather than replacing an active one"
                ));
            }
        }
        None => {
            warnings.push(format!(
                "No existing '{responsibility_type}' assignment found on this node — \
                 this will create the first assignment"
            ));
        }
    }

    // Count current active responsibilities on the node (all types).
    let resp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_responsibilities \
             WHERE node_id = ? AND valid_to IS NULL",
            [node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let active_resp: i64 = resp_row
        .try_get("", "cnt")
        .map_err(|e| decode_err("cnt", e))?;

    let bind_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_entity_bindings \
             WHERE node_id = ? AND valid_to IS NULL",
            [node_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let active_bind: i64 = bind_row
        .try_get("", "cnt")
        .map_err(|e| decode_err("cnt", e))?;

    Ok(OrgImpactPreview {
        action: OrgPreviewAction::ReassignResponsibility,
        subject_node_id: node_id,
        affected_node_count: 1,
        descendant_count: 0,
        active_responsibility_count: active_resp,
        active_binding_count: active_bind,
        blockers,
        warnings,
        dependencies: future_dependency_placeholders(),
    })
}

/// Route a `PreviewOrgChangePayload` to the correct preview function.
pub async fn dispatch_preview(
    db: &DatabaseConnection,
    payload: PreviewOrgChangePayload,
) -> AppResult<OrgImpactPreview> {
    match payload.action.as_str() {
        "move" => {
            let new_parent_id = payload.new_parent_id.ok_or_else(|| {
                AppError::ValidationFailed(vec![
                    "new_parent_id is required for a move preview".to_string(),
                ])
            })?;
            preview_move_node(db, payload.node_id, new_parent_id).await
        }
        "deactivate" => preview_deactivate_node(db, payload.node_id).await,
        "reassign_responsibility" => {
            let responsibility_type = payload.responsibility_type.as_deref().ok_or_else(|| {
                AppError::ValidationFailed(vec![
                    "responsibility_type is required for a reassignment preview".to_string(),
                ])
            })?;
            preview_responsibility_reassignment(
                db,
                payload.node_id,
                responsibility_type,
                payload.replacement_person_id,
                payload.replacement_team_id,
            )
            .await
        }
        other => Err(AppError::ValidationFailed(vec![format!(
            "unknown preview action: '{other}'. Expected: move, deactivate, reassign_responsibility"
        )])),
    }
}
