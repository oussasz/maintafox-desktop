//! Impact preview engine for the Organization Designer.
//!
//! Computes a read-only preview of the operational consequences of structural
//! mutations (move, deactivate, responsibility reassignment) *before* the admin
//! commits the action. The preview never writes to the database.
//!
//! Model scoping:
//! - Structural counts (descendants, child counts) are scoped to the subject
//!   node's `structure_model_id` so draft and active trees are never mixed.
//! - Operational counts (responsibilities, bindings) are resolved via
//!   `origin_node_id`: when the subject node is a draft clone (origin_node_id
//!   IS NOT NULL), ops records are counted against the origin (active) node
//!   because those records have not yet been remapped.
//!
//! Sub-phase 01 File 03 — Sprint S1.

use crate::errors::{issue_params, AppError, AppResult, AppValidationIssue};
use crate::org::fail::{fail, fail_params};
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
    pub blockers: Vec<AppValidationIssue>,
    pub warnings: Vec<AppValidationIssue>,
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

fn preview_blocker(
    code: &str,
    message: impl Into<String>,
    pairs: &[(&str, String)],
) -> AppValidationIssue {
    AppValidationIssue::error(code, message, issue_params(pairs))
}

fn preview_warning(
    code: &str,
    message: impl Into<String>,
    pairs: &[(&str, String)],
) -> AppValidationIssue {
    AppValidationIssue::warning(code, message, issue_params(pairs))
}

/// Standard future-module dependency placeholders.
/// These will be replaced with real counts as each module is implemented.
fn future_dependency_placeholders() -> Vec<OrgImpactDependencySummary> {
    vec![
        OrgImpactDependencySummary {
            domain: "assets".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Asset dependency impact is not available yet.".to_string()),
        },
        OrgImpactDependencySummary {
            domain: "open_work".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Open work order dependency impact is not available yet.".to_string()),
        },
        OrgImpactDependencySummary {
            domain: "permits".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Permit dependency impact is not available yet.".to_string()),
        },
        OrgImpactDependencySummary {
            domain: "inventory".to_string(),
            status: "unavailable".to_string(),
            count: None,
            note: Some("Inventory dependency impact is not available yet.".to_string()),
        },
    ]
}

/// Fetch basic node info needed for preview computations, including
/// model-scope and origin fields.
struct NodeBrief {
    id: i64,
    node_type_id: i64,
    ancestor_path: String,
    #[allow(dead_code)]
    status: String,
    structure_model_id: i64,
    /// Set on draft clones; ops records are attached to this active node id.
    origin_node_id: Option<i64>,
}

/// The node id to use when querying operational records (responsibilities,
/// bindings, equipment, DI). For draft clones, ops are still on the origin
/// (active) node until the model is published.
impl NodeBrief {
    fn ops_node_id(&self) -> i64 {
        self.origin_node_id.unwrap_or(self.id)
    }
}

async fn fetch_node_brief(
    db: &impl ConnectionTrait,
    node_id: i64,
) -> AppResult<NodeBrief> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, node_type_id, ancestor_path, status, \
                    structure_model_id, origin_node_id \
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
        structure_model_id: row
            .try_get::<i64>("", "structure_model_id")
            .map_err(|e| decode_err("structure_model_id", e))?,
        origin_node_id: row
            .try_get::<Option<i64>>("", "origin_node_id")
            .map_err(|e| decode_err("origin_node_id", e))?,
    })
}

/// Count non-deleted descendants of a node within the same structure model.
async fn count_descendants(
    db: &impl ConnectionTrait,
    node_id: i64,
    ancestor_path: &str,
    structure_model_id: i64,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes \
             WHERE ancestor_path LIKE ? AND id != ? \
               AND deleted_at IS NULL \
               AND structure_model_id = ?",
            [
                format!("{ancestor_path}%").into(),
                node_id.into(),
                structure_model_id.into(),
            ],
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Count active descendants (status = 'active') within the same structure model.
async fn count_active_descendants(
    db: &impl ConnectionTrait,
    node_id: i64,
    ancestor_path: &str,
    structure_model_id: i64,
) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes \
             WHERE ancestor_path LIKE ? AND id != ? \
               AND deleted_at IS NULL AND status = 'active' \
               AND structure_model_id = ?",
            [
                format!("{ancestor_path}%").into(),
                node_id.into(),
                structure_model_id.into(),
            ],
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Count active responsibilities on a node and its structural descendants.
///
/// The ops node id (origin when draft clone) is used for the subject node.
/// Descendants are resolved within the same structure model, then their ops
/// ids are similarly resolved.
async fn count_subtree_active_responsibilities(
    db: &impl ConnectionTrait,
    node: &NodeBrief,
) -> AppResult<i64> {
    let ops_id = node.ops_node_id();

    // Collect all descendant ids within the same model, resolve to ops ids.
    let desc_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, origin_node_id FROM org_nodes \
             WHERE ancestor_path LIKE ? AND id != ? \
               AND deleted_at IS NULL \
               AND structure_model_id = ?",
            [
                format!("{}%", node.ancestor_path).into(),
                node.id.into(),
                node.structure_model_id.into(),
            ],
        ))
        .await?;

    // Build set of effective (ops) ids to query responsibilities for.
    let mut ops_ids: Vec<i64> = vec![ops_id];
    for row in &desc_rows {
        let desc_id: i64 = row
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
        let desc_origin: Option<i64> = row
            .try_get("", "origin_node_id")
            .map_err(|e| decode_err("origin_node_id", e))?;
        ops_ids.push(desc_origin.unwrap_or(desc_id));
    }

    // Deduplicate.
    ops_ids.sort_unstable();
    ops_ids.dedup();

    if ops_ids.is_empty() {
        return Ok(0);
    }

    let placeholders = ops_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let values: Vec<sea_orm::Value> = ops_ids.iter().map(|&id| id.into()).collect();

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS cnt FROM org_node_responsibilities \
                 WHERE node_id IN ({placeholders}) AND valid_to IS NULL"
            ),
            values,
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Count active bindings on a node and its structural descendants (ops ids).
async fn count_subtree_active_bindings(
    db: &impl ConnectionTrait,
    node: &NodeBrief,
) -> AppResult<i64> {
    let ops_id = node.ops_node_id();

    let desc_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, origin_node_id FROM org_nodes \
             WHERE ancestor_path LIKE ? AND id != ? \
               AND deleted_at IS NULL \
               AND structure_model_id = ?",
            [
                format!("{}%", node.ancestor_path).into(),
                node.id.into(),
                node.structure_model_id.into(),
            ],
        ))
        .await?;

    let mut ops_ids: Vec<i64> = vec![ops_id];
    for row in &desc_rows {
        let desc_id: i64 = row
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
        let desc_origin: Option<i64> = row
            .try_get("", "origin_node_id")
            .map_err(|e| decode_err("origin_node_id", e))?;
        ops_ids.push(desc_origin.unwrap_or(desc_id));
    }

    ops_ids.sort_unstable();
    ops_ids.dedup();

    if ops_ids.is_empty() {
        return Ok(0);
    }

    let placeholders = ops_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let values: Vec<sea_orm::Value> = ops_ids.iter().map(|&id| id.into()).collect();

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS cnt FROM org_entity_bindings \
                 WHERE node_id IN ({placeholders}) AND valid_to IS NULL"
            ),
            values,
        ))
        .await?
        .expect("COUNT always returns a row");
    row.try_get::<i64>("", "cnt")
        .map_err(|e| decode_err("cnt", e))
}

/// Check whether a parent-child type rule exists in the model that owns both nodes.
async fn is_parent_child_allowed(
    db: &impl ConnectionTrait,
    parent_node: &NodeBrief,
    child_type_id: i64,
) -> AppResult<bool> {
    // Use the child's model (both parent and child must share a model in normal operation).
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_type_relationship_rules r \
             JOIN org_structure_models m ON m.id = r.structure_model_id \
             WHERE r.parent_type_id = ? AND r.child_type_id = ? \
               AND r.structure_model_id = ?",
            [
                parent_node.node_type_id.into(),
                child_type_id.into(),
                parent_node.structure_model_id.into(),
            ],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    Ok(cnt > 0)
}

// ─── Preview functions ────────────────────────────────────────────────────────

/// Preview the consequences of moving `node_id` under `new_parent_id`.
pub async fn preview_move_node(
    db: &DatabaseConnection,
    node_id: i64,
    new_parent_id: i64,
) -> AppResult<OrgImpactPreview> {
    let node = fetch_node_brief(db, node_id).await?;
    let parent = fetch_node_brief(db, new_parent_id).await?;

    let descendant_count =
        count_descendants(db, node.id, &node.ancestor_path, node.structure_model_id).await?;
    let active_resp = count_subtree_active_responsibilities(db, &node).await?;
    let active_bind = count_subtree_active_bindings(db, &node).await?;

    let mut blockers: Vec<AppValidationIssue> = Vec::new();
    let mut warnings: Vec<AppValidationIssue> = Vec::new();

    // Cycle detection — new parent is inside this node's subtree.
    let self_segment = format!("/{}/", node.id);
    if parent.ancestor_path.contains(&self_segment) || parent.id == node.id {
        blockers.push(preview_blocker(
            "ORG_PREVIEW_MOVE_CYCLE",
            "Cannot move this node under one of its own descendants.",
            &[],
        ));
    }

    // Parent-child type rule validation (scoped to the node's model).
    if !is_parent_child_allowed(db, &parent, node.node_type_id).await? {
        blockers.push(preview_blocker(
            "ORG_PREVIEW_PARENT_CHILD_NOT_ALLOWED",
            "The structure model does not allow this parent-child type combination.",
            &[],
        ));
    }

    if active_resp > 0 {
        warnings.push(preview_warning(
            "ORG_PREVIEW_SUBTREE_RESPONSIBILITIES",
            format!(
                "{active_resp} active responsibility assignment(s) in the affected subtree."
            ),
            &[("count", active_resp.to_string())],
        ));
    }

    if active_bind > 0 {
        warnings.push(preview_warning(
            "ORG_PREVIEW_SUBTREE_BINDINGS",
            format!("{active_bind} active external binding(s) in the affected subtree."),
            &[("count", active_bind.to_string())],
        ));
    }

    Ok(OrgImpactPreview {
        action: OrgPreviewAction::MoveNode,
        subject_node_id: node_id,
        affected_node_count: 1 + descendant_count,
        descendant_count,
        active_responsibility_count: active_resp,
        active_binding_count: active_bind,
        blockers,
        warnings,
        dependencies: future_dependency_placeholders(),
    })
}

/// Preview the consequences of deactivating `node_id`.
pub async fn preview_deactivate_node(
    db: &DatabaseConnection,
    node_id: i64,
) -> AppResult<OrgImpactPreview> {
    let node = fetch_node_brief(db, node_id).await?;

    let descendant_count =
        count_descendants(db, node.id, &node.ancestor_path, node.structure_model_id).await?;
    let active_desc =
        count_active_descendants(db, node.id, &node.ancestor_path, node.structure_model_id).await?;

    let ops_id = node.ops_node_id();

    let resp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_responsibilities \
             WHERE node_id = ? AND valid_to IS NULL",
            [ops_id.into()],
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
            [ops_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let active_bind: i64 = bind_row
        .try_get("", "cnt")
        .map_err(|e| decode_err("cnt", e))?;

    let mut blockers: Vec<AppValidationIssue> = Vec::new();
    let mut warnings: Vec<AppValidationIssue> = Vec::new();

    if active_desc > 0 {
        blockers.push(preview_blocker(
            "ORG_PREVIEW_ACTIVE_DESCENDANTS",
            format!("{active_desc} active descendant node(s) must be deactivated first."),
            &[("count", active_desc.to_string())],
        ));
    }

    if active_resp > 0 {
        blockers.push(preview_blocker(
            "ORG_PREVIEW_ACTIVE_RESPONSIBILITIES",
            format!(
                "{active_resp} active responsibility assignment(s) must be ended first."
            ),
            &[("count", active_resp.to_string())],
        ));
    }

    if active_bind > 0 {
        warnings.push(preview_warning(
            "ORG_PREVIEW_ORPHANED_BINDINGS",
            format!("{active_bind} active external binding(s) will be orphaned."),
            &[("count", active_bind.to_string())],
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
pub async fn preview_responsibility_reassignment(
    db: &DatabaseConnection,
    node_id: i64,
    responsibility_type: &str,
    replacement_person_id: Option<i64>,
    replacement_team_id: Option<i64>,
) -> AppResult<OrgImpactPreview> {
    let node = fetch_node_brief(db, node_id).await?;
    let ops_id = node.ops_node_id();

    let mut blockers: Vec<AppValidationIssue> = Vec::new();
    let mut warnings: Vec<AppValidationIssue> = Vec::new();

    if replacement_person_id.is_none() && replacement_team_id.is_none() {
        blockers.push(preview_blocker(
            "ORG_PREVIEW_REPLACEMENT_REQUIRED",
            "A replacement person or team must be specified for the reassignment.",
            &[],
        ));
    }

    let current_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, valid_to FROM org_node_responsibilities \
             WHERE node_id = ? AND responsibility_type = ? \
             ORDER BY created_at DESC LIMIT 1",
            [ops_id.into(), responsibility_type.to_string().into()],
        ))
        .await?;

    match current_row {
        Some(row) => {
            let valid_to: Option<String> = row
                .try_get("", "valid_to")
                .map_err(|e| decode_err("valid_to", e))?;
            if valid_to.is_some() {
                warnings.push(preview_warning(
                    "ORG_PREVIEW_ASSIGNMENT_ENDED",
                    format!(
                        "The current '{responsibility_type}' assignment is already ended — this will create a new assignment rather than replacing an active one."
                    ),
                    &[("responsibilityType", responsibility_type.to_string())],
                ));
            }
        }
        None => {
            warnings.push(preview_warning(
                "ORG_PREVIEW_NO_ASSIGNMENT",
                format!(
                    "No existing '{responsibility_type}' assignment found on this node — this will create the first assignment."
                ),
                &[("responsibilityType", responsibility_type.to_string())],
            ));
        }
    }

    let resp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_responsibilities \
             WHERE node_id = ? AND valid_to IS NULL",
            [ops_id.into()],
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
            [ops_id.into()],
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
                fail(
                    "ORG_PREVIEW_NEW_PARENT_REQUIRED",
                    "A new parent is required for a move preview.",
                )
            })?;
            preview_move_node(db, payload.node_id, new_parent_id).await
        }
        "deactivate" => preview_deactivate_node(db, payload.node_id).await,
        "reassign_responsibility" => {
            let responsibility_type = payload.responsibility_type.as_deref().ok_or_else(|| {
                fail(
                    "ORG_PREVIEW_RESPONSIBILITY_TYPE_REQUIRED",
                    "A responsibility type is required for a reassignment preview.",
                )
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
        other => Err(fail_params(
            "ORG_PREVIEW_UNKNOWN_ACTION",
            format!(
                "Unknown preview action '{other}'. Expected: move, deactivate, reassign_responsibility."
            ),
            &[("action", other.to_string())],
        )),
    }
}
