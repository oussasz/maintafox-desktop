//! Publish validation and node-type remap service.
//!
//! Before a draft structure model can be activated, the validator confirms
//! that live nodes can be safely mapped into the new model's node-type
//! vocabulary.  The remap plan bridges old active type IDs to new draft
//! type IDs by matching on the stable `code` field.
//!
//! Sub-phase 01 — File 04 — Sprint S1.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgValidationIssue {
    pub code: String,
    /// `"error"` (blocking) or `"warning"` (informational).
    pub severity: String,
    pub message: String,
    pub related_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgPublishValidationResult {
    pub model_id: i64,
    pub can_publish: bool,
    pub issue_count: i64,
    pub blocking_count: i64,
    pub issues: Vec<OrgValidationIssue>,
    pub remap_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTypeRemap {
    pub old_type_id: i64,
    pub old_type_code: String,
    pub new_type_id: i64,
    pub new_type_code: String,
}

// ─── Internal query-result structs ────────────────────────────────────────────

struct DraftNodeType {
    id: i64,
    code: String,
    is_root_type: bool,
    can_host_assets: bool,
    can_own_work: bool,
    can_carry_cost_center: bool,
}

struct LiveNodeInfo {
    node_id: i64,
    node_name: String,
    type_code: String,
    cost_center_code: Option<String>,
    parent_type_code: Option<String>,
}

// ─── Graph analysis ───────────────────────────────────────────────────────────

/// Analyze the draft model's relationship-rule graph for reachability and cycles.
///
/// Returns (set of type codes reachable from the root, whether a cycle exists).
fn analyze_type_graph(
    root_code: &str,
    all_codes: &[String],
    edges: &HashMap<String, Vec<String>>,
) -> (HashSet<String>, bool) {
    // ── BFS for reachability from the root type ───────────────────────────
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();
    reachable.insert(root_code.to_string());
    queue.push_back(root_code.to_string());

    while let Some(current) = queue.pop_front() {
        if let Some(children) = edges.get(&current) {
            for child in children {
                if reachable.insert(child.clone()) {
                    queue.push_back(child.clone());
                }
            }
        }
    }

    // ── Cycle detection via Kahn's topological sort ───────────────────────
    let mut in_degree: HashMap<String, usize> =
        all_codes.iter().map(|c| (c.clone(), 0)).collect();

    for (parent, children) in edges {
        if in_degree.contains_key(parent) {
            for child in children {
                if let Some(deg) = in_degree.get_mut(child) {
                    *deg += 1;
                }
            }
        }
    }

    let mut topo_queue: VecDeque<String> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(code, _)| code.clone())
        .collect();

    let mut processed = 0usize;
    while let Some(node) = topo_queue.pop_front() {
        processed += 1;
        if let Some(children) = edges.get(&node) {
            for child in children {
                if let Some(deg) = in_degree.get_mut(child) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        topo_queue.push_back(child.clone());
                    }
                }
            }
        }
    }

    let has_cycle = processed < all_codes.len();
    (reachable, has_cycle)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_validation row decode failed for column '{column}': {e}"
    ))
}

fn blocking_issue(code: &str, message: String, related_id: Option<i64>) -> OrgValidationIssue {
    OrgValidationIssue {
        code: code.to_string(),
        severity: "error".to_string(),
        message,
        related_id,
    }
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Validate a draft structure model for publish readiness.
///
/// Returns a result struct with all detected issues.  `can_publish` is `true`
/// only when zero blocking issues exist.
///
/// Accepts any `ConnectionTrait` implementor so it can run both standalone
/// (with `&DatabaseConnection`) and inside a transaction.
pub async fn validate_draft_model_for_publish(
    db: &impl ConnectionTrait,
    model_id: i64,
) -> AppResult<OrgPublishValidationResult> {
    let mut issues: Vec<OrgValidationIssue> = Vec::new();

    // ── 1. Model exists and is in draft status ────────────────────────────
    let model_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, status FROM org_structure_models WHERE id = ?",
            [model_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_structure_model".to_string(),
            id: model_id.to_string(),
        })?;

    let model_status: String = model_row
        .try_get("", "status")
        .map_err(|e| decode_err("status", e))?;

    if model_status != "draft" {
        return Err(AppError::ValidationFailed(vec![format!(
            "model {model_id} is '{model_status}', not 'draft' — only draft models can be validated for publish"
        )]));
    }

    // ── Fetch all active draft node types ─────────────────────────────────
    let type_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, is_root_type, can_host_assets, can_own_work, can_carry_cost_center \
             FROM org_node_types WHERE structure_model_id = ? AND is_active = 1",
            [model_id.into()],
        ))
        .await?;

    let draft_types: Vec<DraftNodeType> = type_rows
        .into_iter()
        .map(|row| {
            Ok(DraftNodeType {
                id: row
                    .try_get::<i64>("", "id")
                    .map_err(|e| decode_err("id", e))?,
                code: row
                    .try_get::<String>("", "code")
                    .map_err(|e| decode_err("code", e))?,
                is_root_type: row
                    .try_get::<i32>("", "is_root_type")
                    .map_err(|e| decode_err("is_root_type", e))?
                    != 0,
                can_host_assets: row
                    .try_get::<i32>("", "can_host_assets")
                    .map_err(|e| decode_err("can_host_assets", e))?
                    != 0,
                can_own_work: row
                    .try_get::<i32>("", "can_own_work")
                    .map_err(|e| decode_err("can_own_work", e))?
                    != 0,
                can_carry_cost_center: row
                    .try_get::<i32>("", "can_carry_cost_center")
                    .map_err(|e| decode_err("can_carry_cost_center", e))?
                    != 0,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    let draft_type_by_code: HashMap<&str, &DraftNodeType> =
        draft_types.iter().map(|t| (t.code.as_str(), t)).collect();

    // ── 2. Exactly one root node type ─────────────────────────────────────
    let root_types: Vec<&DraftNodeType> =
        draft_types.iter().filter(|t| t.is_root_type).collect();

    if root_types.is_empty() {
        issues.push(blocking_issue(
            "NO_ROOT_TYPE",
            "draft model has no root node type".to_string(),
            None,
        ));
    } else if root_types.len() > 1 {
        issues.push(blocking_issue(
            "MULTIPLE_ROOT_TYPES",
            format!(
                "draft model has {} root types (expected exactly 1)",
                root_types.len()
            ),
            None,
        ));
    }

    // ── 3. No duplicate type codes ────────────────────────────────────────
    {
        let mut code_counts: HashMap<&str, usize> = HashMap::new();
        for t in &draft_types {
            *code_counts.entry(t.code.as_str()).or_insert(0) += 1;
        }
        for (code, count) in &code_counts {
            if *count > 1 {
                issues.push(blocking_issue(
                    "DUPLICATE_TYPE_CODE",
                    format!(
                        "node type code '{}' appears {} times in the draft model",
                        code, count
                    ),
                    None,
                ));
            }
        }
    }

    // ── Fetch draft relationship rules as (parent_code, child_code) pairs ─
    let rule_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pt.code AS parent_code, ct.code AS child_code \
             FROM org_type_relationship_rules r \
             JOIN org_node_types pt ON pt.id = r.parent_type_id \
             JOIN org_node_types ct ON ct.id = r.child_type_id \
             WHERE r.structure_model_id = ?",
            [model_id.into()],
        ))
        .await?;

    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    let mut allowed_pairs: HashSet<(String, String)> = HashSet::new();

    for row in &rule_rows {
        let parent_code: String = row
            .try_get("", "parent_code")
            .map_err(|e| decode_err("parent_code", e))?;
        let child_code: String = row
            .try_get("", "child_code")
            .map_err(|e| decode_err("child_code", e))?;
        edges
            .entry(parent_code.clone())
            .or_default()
            .push(child_code.clone());
        allowed_pairs.insert((parent_code, child_code));
    }

    // ── 4 & 5. Reachability from root and cycle detection ─────────────────
    let all_codes: Vec<String> = draft_types.iter().map(|t| t.code.clone()).collect();

    if let Some(root) = root_types.first() {
        let (reachable, has_cycle) = analyze_type_graph(&root.code, &all_codes, &edges);

        // 4. Every type must be reachable from the root
        for t in &draft_types {
            if !reachable.contains(&t.code) {
                issues.push(blocking_issue(
                    "UNREACHABLE_TYPE",
                    format!(
                        "node type '{}' is not reachable from the root through relationship rules",
                        t.code
                    ),
                    Some(t.id),
                ));
            }
        }

        // 5. No cycles in the rule graph
        if has_cycle {
            issues.push(blocking_issue(
                "RULE_GRAPH_CYCLE",
                "the relationship-rule graph contains a cycle".to_string(),
                None,
            ));
        }
    }

    // ── 9. At least one type with can_own_work ────────────────────────────
    if !draft_types.iter().any(|t| t.can_own_work) {
        issues.push(blocking_issue(
            "NO_WORK_CAPABLE_TYPE",
            "no active node type in the draft model has can_own_work enabled".to_string(),
            None,
        ));
    }

    // ── 10. At least one type with can_host_assets ────────────────────────
    if !draft_types.iter().any(|t| t.can_host_assets) {
        issues.push(blocking_issue(
            "NO_ASSET_CAPABLE_TYPE",
            "no active node type in the draft model has can_host_assets enabled".to_string(),
            None,
        ));
    }

    // ── Live-node checks (6 – 8) ─────────────────────────────────────────
    // Skipped when no active model exists (first publish — no live nodes).
    let active_model_exists = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1".to_string(),
        ))
        .await?
        .is_some();

    let mut remap_count: i64 = 0;

    if active_model_exists {
        let live_rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT n.id AS node_id, n.name AS node_name, \
                        nt.code AS type_code, n.cost_center_code, \
                        pnt.code AS parent_type_code \
                 FROM org_nodes n \
                 JOIN org_node_types nt ON nt.id = n.node_type_id \
                 LEFT JOIN org_nodes p ON p.id = n.parent_id AND p.deleted_at IS NULL \
                 LEFT JOIN org_node_types pnt ON pnt.id = p.node_type_id \
                 WHERE n.deleted_at IS NULL AND n.status = 'active'"
                    .to_string(),
            ))
            .await?;

        let live_nodes: Vec<LiveNodeInfo> = live_rows
            .into_iter()
            .map(|row| {
                Ok(LiveNodeInfo {
                    node_id: row
                        .try_get::<i64>("", "node_id")
                        .map_err(|e| decode_err("node_id", e))?,
                    node_name: row
                        .try_get::<String>("", "node_name")
                        .map_err(|e| decode_err("node_name", e))?,
                    type_code: row
                        .try_get::<String>("", "type_code")
                        .map_err(|e| decode_err("type_code", e))?,
                    cost_center_code: row
                        .try_get::<Option<String>>("", "cost_center_code")
                        .map_err(|e| decode_err("cost_center_code", e))?,
                    parent_type_code: row
                        .try_get::<Option<String>>("", "parent_type_code")
                        .map_err(|e| decode_err("parent_type_code", e))?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;

        // Count distinct type codes used by live nodes that map into the draft model
        let used_codes: HashSet<&str> =
            live_nodes.iter().map(|n| n.type_code.as_str()).collect();
        remap_count = used_codes
            .iter()
            .filter(|c| draft_type_by_code.contains_key(**c))
            .count() as i64;

        for node in &live_nodes {
            // 6. Every active live node's type code exists in the draft model
            let Some(draft_type) = draft_type_by_code.get(node.type_code.as_str()) else {
                issues.push(blocking_issue(
                    "MISSING_TYPE_CODE",
                    format!(
                        "live node '{}' (id={}) uses type code '{}' which does not exist in the draft model",
                        node.node_name, node.node_id, node.type_code
                    ),
                    Some(node.node_id),
                ));
                continue;
            };

            // 7. Parent-child pair remains allowed by the draft model's rules
            if let Some(ref parent_code) = node.parent_type_code {
                if !allowed_pairs
                    .contains(&(parent_code.clone(), node.type_code.clone()))
                {
                    issues.push(blocking_issue(
                        "PARENT_CHILD_NOT_ALLOWED",
                        format!(
                            "live node '{}' (id={}) has parent type '{}' / child type '{}' \
                             which is not allowed in the draft model",
                            node.node_name, node.node_id, parent_code, node.type_code
                        ),
                        Some(node.node_id),
                    ));
                }
            }

            // 8. cost_center_code requires can_carry_cost_center on the draft type
            if node.cost_center_code.is_some() && !draft_type.can_carry_cost_center {
                issues.push(blocking_issue(
                    "COST_CENTER_INCOMPATIBLE",
                    format!(
                        "live node '{}' (id={}) has a cost_center_code but type '{}' in the \
                         draft model does not allow cost centers",
                        node.node_name, node.node_id, node.type_code
                    ),
                    Some(node.node_id),
                ));
            }
        }
    }

    // ── Build result ──────────────────────────────────────────────────────
    let blocking_count = issues.iter().filter(|i| i.severity == "error").count() as i64;
    let issue_count = issues.len() as i64;

    Ok(OrgPublishValidationResult {
        model_id,
        can_publish: blocking_count == 0,
        issue_count,
        blocking_count,
        issues,
        remap_count,
    })
}

/// Build a remap plan mapping active-model type IDs to draft-model type IDs
/// by matching on the stable node-type `code`.
///
/// Returns an empty vec when no active model exists (first publish).
pub async fn build_type_remap_plan(
    db: &impl ConnectionTrait,
    draft_model_id: i64,
) -> AppResult<Vec<NodeTypeRemap>> {
    let active_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1".to_string(),
        ))
        .await?;

    let Some(active_row) = active_row else {
        return Ok(Vec::new());
    };

    let active_model_id: i64 = active_row
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT old_t.id AS old_type_id, old_t.code AS old_type_code, \
                    new_t.id AS new_type_id, new_t.code AS new_type_code \
             FROM org_node_types old_t \
             JOIN org_node_types new_t ON new_t.code = old_t.code \
                  AND new_t.structure_model_id = ? \
             WHERE old_t.structure_model_id = ? \
                   AND old_t.is_active = 1 \
                   AND new_t.is_active = 1",
            [draft_model_id.into(), active_model_id.into()],
        ))
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(NodeTypeRemap {
                old_type_id: row
                    .try_get::<i64>("", "old_type_id")
                    .map_err(|e| decode_err("old_type_id", e))?,
                old_type_code: row
                    .try_get::<String>("", "old_type_code")
                    .map_err(|e| decode_err("old_type_code", e))?,
                new_type_id: row
                    .try_get::<i64>("", "new_type_id")
                    .map_err(|e| decode_err("new_type_id", e))?,
                new_type_code: row
                    .try_get::<String>("", "new_type_code")
                    .map_err(|e| decode_err("new_type_code", e))?,
            })
        })
        .collect()
}

/// Publish a draft model with full validation and transactional live-node remap.
///
/// Transaction sequence:
/// 1. Validate the draft model (all 10 checks)
/// 2. If `can_publish = false` → abort with `AppError::ValidationFailed`
/// 3. Build and execute the node-type remap (update `org_nodes.node_type_id`)
/// 4. Supersede the current active model
/// 5. Activate the draft model
pub async fn publish_model_with_remap(
    db: &DatabaseConnection,
    draft_model_id: i64,
    actor_id: i32,
) -> AppResult<OrgPublishValidationResult> {
    let txn = db.begin().await?;

    // ── Step 1: validate inside the transaction ───────────────────────────
    let validation = validate_draft_model_for_publish(&txn, draft_model_id).await?;

    if !validation.can_publish {
        let messages: Vec<String> = validation
            .issues
            .iter()
            .filter(|i| i.severity == "error")
            .map(|i| i.message.clone())
            .collect();
        return Err(AppError::ValidationFailed(messages));
    }

    let now = Utc::now().to_rfc3339();

    // ── Step 2: build and execute the remap plan ──────────────────────────
    let remap_plan = build_type_remap_plan(&txn, draft_model_id).await?;

    for remap in &remap_plan {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes \
             SET node_type_id = ?, row_version = row_version + 1, updated_at = ? \
             WHERE node_type_id = ? AND deleted_at IS NULL",
            [
                remap.new_type_id.into(),
                now.clone().into(),
                remap.old_type_id.into(),
            ],
        ))
        .await?;
    }

    // ── Step 3: supersede the current active model ────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_structure_models \
         SET status = 'superseded', superseded_at = ?, updated_at = ? \
         WHERE status = 'active'",
        [now.clone().into(), now.clone().into()],
    ))
    .await?;

    // ── Step 4: activate the draft model ──────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_structure_models \
         SET status = 'active', activated_at = ?, activated_by_id = ?, updated_at = ? \
         WHERE id = ?",
        [
            now.clone().into(),
            actor_id.into(),
            now.into(),
            draft_model_id.into(),
        ],
    ))
    .await?;

    txn.commit().await?;

    tracing::info!(
        model_id = draft_model_id,
        remap_count = remap_plan.len(),
        actor = actor_id,
        "org structure model published with remap"
    );

    // Return the validation result with the actual remap count from the plan.
    let mut result = validation;
    result.remap_count = remap_plan.len() as i64;
    Ok(result)
}
