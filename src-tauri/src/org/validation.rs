//! Publish validation and atomic promote service.
//!
//! Before a draft structure model can be activated the validator confirms that
//! the draft *node tree* is structurally sound against the draft *type schema*.
//! Node checks now evaluate **draft nodes** (`structure_model_id = draft_model_id`)
//! rather than the global live tree; the two trees are never mixed.
//!
//! `publish_model_with_remap` performs the full atomic promote:
//!   1. Validate draft (draft tree vs draft schema)
//!   2. Build active-node → draft-node map via origin_node_id
//!   3. Detect unmapped active nodes still referenced by ops FKs → fail early
//!   4. Remap all operational FKs (old active node ids → new draft node ids)
//!   5. Soft-delete all nodes in the old active model
//!   6. Supersede old active model; activate draft
//!   7. Clear origin_node_id on the newly active nodes
//!   8. Commit
//!
//! First publish (no prior active model): skip steps 3–5 & 7.
//!
//! Sub-phase 01 — File 04 — Sprint S1.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::errors::{issue_params, org_validation_failed_issues, AppError, AppResult, AppValidationIssue};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

// ─── Public types ─────────────────────────────────────────────────────────────

/// Publish / preview validation issue (alias of shared IPC shape).
pub type OrgValidationIssue = AppValidationIssue;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgPublishValidationResult {
    pub model_id: i64,
    pub can_publish: bool,
    pub issue_count: i64,
    pub blocking_count: i64,
    pub issues: Vec<OrgValidationIssue>,
    /// Number of active nodes that will be remapped to draft clones.
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

struct DraftNodeInfo {
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
    let mut in_degree: HashMap<String, usize> = all_codes.iter().map(|c| (c.clone(), 0)).collect();

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

fn blocking_issue(
    code: &str,
    message: String,
    related_id: Option<i64>,
    params: HashMap<String, String>,
) -> OrgValidationIssue {
    AppValidationIssue::error_with_related(code, message, related_id, params)
}

// ─── Service functions ────────────────────────────────────────────────────────

/// Validate a draft structure model for publish readiness.
///
/// Structural schema checks (1–5, 9–10) evaluate the draft type vocabulary.
/// Node tree checks (6–8) evaluate **draft nodes** (`structure_model_id = model_id`)
/// against the draft type rules. No assumption is made about a global live tree.
///
/// `can_publish` is `true` only when zero blocking issues exist.
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

    let model_status: String = model_row.try_get("", "status").map_err(|e| decode_err("status", e))?;

    if model_status != "draft" {
        return Err(crate::errors::org_validation_failed(
            "ORG_VALIDATE_NOT_DRAFT",
            format!("This model is '{model_status}', not a draft. Only drafts can be checked for publish."),
            crate::errors::issue_params(&[("status", model_status)]),
        ));
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
                id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
                code: row.try_get::<String>("", "code").map_err(|e| decode_err("code", e))?,
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

    let draft_type_by_code: HashMap<&str, &DraftNodeType> = draft_types.iter().map(|t| (t.code.as_str(), t)).collect();

    // ── 2. Exactly one root node type ─────────────────────────────────────
    let root_types: Vec<&DraftNodeType> = draft_types.iter().filter(|t| t.is_root_type).collect();

    if root_types.is_empty() {
        issues.push(blocking_issue(
            "NO_ROOT_TYPE",
            "This draft has no top-level organization type. Mark exactly one type as the root.".to_string(),
            None,
            HashMap::new(),
        ));
    } else if root_types.len() > 1 {
        let root_count = root_types.len().to_string();
        issues.push(blocking_issue(
            "MULTIPLE_ROOT_TYPES",
            format!("This draft has {root_count} top-level types. Keep only one type marked as the root."),
            None,
            issue_params(&[("rootCount", root_count)]),
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
                let type_code = (*code).to_string();
                let count_str = count.to_string();
                issues.push(blocking_issue(
                    "DUPLICATE_TYPE_CODE",
                    format!("The type code '{type_code}' is used {count_str} times. Each type needs a unique code."),
                    None,
                    issue_params(&[("typeCode", type_code), ("count", count_str)]),
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
        let child_code: String = row.try_get("", "child_code").map_err(|e| decode_err("child_code", e))?;
        edges.entry(parent_code.clone()).or_default().push(child_code.clone());
        allowed_pairs.insert((parent_code, child_code));
    }

    // ── 4 & 5. Reachability from root and cycle detection ─────────────────
    let all_codes: Vec<String> = draft_types.iter().map(|t| t.code.clone()).collect();

    if let Some(root) = root_types.first() {
        let (reachable, has_cycle) = analyze_type_graph(&root.code, &all_codes, &edges);

        // 4. Every type must be reachable from the root
        for t in &draft_types {
            if !reachable.contains(&t.code) {
                let type_code = t.code.clone();
                issues.push(blocking_issue(
                    "UNREACHABLE_TYPE",
                    format!(
                        "The level '{type_code}' is not connected to the top of your organization. \
                         Add a relationship rule so it can sit under the root or another allowed level."
                    ),
                    Some(t.id),
                    issue_params(&[("typeCode", type_code)]),
                ));
            }
        }

        // 5. No cycles in the rule graph
        if has_cycle {
            issues.push(blocking_issue(
                "RULE_GRAPH_CYCLE",
                "Some relationship rules form a loop (A under B and B under A). Remove the circular link.".to_string(),
                None,
                HashMap::new(),
            ));
        }
    }

    // ── 9. At least one type with can_own_work ────────────────────────────
    if !draft_types.iter().any(|t| t.can_own_work) {
        issues.push(blocking_issue(
            "NO_WORK_CAPABLE_TYPE",
            "No organization level is set up to own work. Turn on work ownership for at least one level.".to_string(),
            None,
            HashMap::new(),
        ));
    }

    // ── 10. At least one type with can_host_assets ────────────────────────
    if !draft_types.iter().any(|t| t.can_host_assets) {
        issues.push(blocking_issue(
            "NO_ASSET_CAPABLE_TYPE",
            "No organization level is set up to host assets. Turn on asset hosting for at least one level.".to_string(),
            None,
            HashMap::new(),
        ));
    }

    // ── Draft-tree node checks (6–8) ──────────────────────────────────────
    // Evaluate the draft tree (structure_model_id = model_id) against draft
    // type rules. This replaces the old live-tree checks and works for both
    // first publish (empty draft tree) and subsequent publishes.
    let draft_node_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.id AS node_id, n.name AS node_name, \
                    nt.code AS type_code, n.cost_center_code, \
                    pnt.code AS parent_type_code \
             FROM org_nodes n \
             JOIN org_node_types nt ON nt.id = n.node_type_id \
             LEFT JOIN org_nodes p ON p.id = n.parent_id \
                  AND p.deleted_at IS NULL \
                  AND p.structure_model_id = n.structure_model_id \
             LEFT JOIN org_node_types pnt ON pnt.id = p.node_type_id \
             WHERE n.deleted_at IS NULL \
               AND n.structure_model_id = ?",
            [model_id.into()],
        ))
        .await?;

    let draft_nodes: Vec<DraftNodeInfo> = draft_node_rows
        .into_iter()
        .map(|row| {
            Ok(DraftNodeInfo {
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

    for node in &draft_nodes {
        // 6. Every draft node's type code exists in this draft model
        let Some(draft_type) = draft_type_by_code.get(node.type_code.as_str()) else {
            let node_name = node.node_name.clone();
            let type_code = node.type_code.clone();
            issues.push(blocking_issue(
                "MISSING_TYPE_CODE",
                format!(
                    "'{node_name}' uses the level '{type_code}', which is not defined in this draft. \
                     Assign a valid level or add that type."
                ),
                Some(node.node_id),
                issue_params(&[("nodeName", node_name), ("typeCode", type_code)]),
            ));
            continue;
        };

        // 7. Parent-child pair remains allowed by the draft model's rules
        if let Some(ref parent_code) = node.parent_type_code {
            if !allowed_pairs.contains(&(parent_code.clone(), node.type_code.clone())) {
                let node_name = node.node_name.clone();
                let parent_type = parent_code.clone();
                let child_type = node.type_code.clone();
                issues.push(blocking_issue(
                    "PARENT_CHILD_NOT_ALLOWED",
                    format!(
                        "'{node_name}' ({child_type}) cannot sit under a {parent_type} with the \
                         current relationship rules. Move it or update the rules."
                    ),
                    Some(node.node_id),
                    issue_params(&[
                        ("nodeName", node_name),
                        ("parentType", parent_type),
                        ("childType", child_type),
                    ]),
                ));
            }
        }

        // 8. cost_center_code requires can_carry_cost_center on the draft type
        if node.cost_center_code.is_some() && !draft_type.can_carry_cost_center {
            let node_name = node.node_name.clone();
            let type_code = node.type_code.clone();
            issues.push(blocking_issue(
                "COST_CENTER_INCOMPATIBLE",
                format!(
                    "'{node_name}' has a cost center, but its level '{type_code}' does not allow \
                     cost centers. Remove the cost center or allow them on that level."
                ),
                Some(node.node_id),
                issue_params(&[("nodeName", node_name), ("typeCode", type_code)]),
            ));
        }
    }

    // Unmapped active nodes with ops refs block publish (same rule as publish_model_with_remap).
    if let Some(active_id) = crate::org::model_scope::try_get_active_model_id(db).await? {
        let unmapped = collect_unmapped_active_nodes_with_ops_refs(db, model_id, active_id).await?;
        for u in unmapped {
            let node_name = u.node_name.clone();
            let ops_ref_count = u.ops_ref_count.to_string();
            issues.push(blocking_issue(
                "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS",
                format!(
                    "'{node_name}' is still used by live data but is missing from this draft. \
                     Repair the draft copies, or detach those links before publishing."
                ),
                Some(u.node_id),
                issue_params(&[("nodeName", node_name), ("opsRefCount", ops_ref_count)]),
            ));
        }
    }

    // remap_count = draft nodes with an origin_node_id (clones of active nodes)
    let remap_count_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_nodes \
             WHERE structure_model_id = ? AND origin_node_id IS NOT NULL AND deleted_at IS NULL",
            [model_id.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let remap_count: i64 = remap_count_row.try_get("", "cnt").map_err(|e| decode_err("cnt", e))?;

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
pub async fn build_type_remap_plan(db: &impl ConnectionTrait, draft_model_id: i64) -> AppResult<Vec<NodeTypeRemap>> {
    let active_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1".to_string(),
        ))
        .await?;

    let Some(active_row) = active_row else {
        return Ok(Vec::new());
    };

    let active_model_id: i64 = active_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

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

// ─── FK remap helpers ─────────────────────────────────────────────────────────

/// Remap a single INTEGER FK column in a table.
async fn remap_integer_fk(
    txn: &impl ConnectionTrait,
    table: &str,
    column: &str,
    old_id: i64,
    new_id: i64,
    now: &str,
) -> AppResult<()> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &format!(
            "UPDATE {table} SET {column} = ?, updated_at = ?, row_version = row_version + 1 \
             WHERE {column} = ?"
        ),
        [new_id.into(), now.to_string().into(), old_id.into()],
    ))
    .await?;
    Ok(())
}

/// Remap an INTEGER FK on tables that have `row_version` but no `updated_at`.
async fn remap_integer_fk_row_version_only(
    txn: &impl ConnectionTrait,
    table: &str,
    column: &str,
    old_id: i64,
    new_id: i64,
) -> AppResult<()> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &format!(
            "UPDATE {table} SET {column} = ?, row_version = row_version + 1 \
             WHERE {column} = ?"
        ),
        [new_id.into(), old_id.into()],
    ))
    .await?;
    Ok(())
}

/// Remap a TEXT scope_reference column in user_scope_assignments for a specific scope_type.
async fn remap_scope_reference(
    txn: &impl ConnectionTrait,
    scope_type: &str,
    old_id: i64,
    new_id: i64,
    now: &str,
) -> AppResult<()> {
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_scope_assignments \
         SET scope_reference = ?, updated_at = ?, row_version = row_version + 1 \
         WHERE scope_type = ? AND scope_reference = ? AND deleted_at IS NULL",
        [
            new_id.to_string().into(),
            now.to_string().into(),
            scope_type.to_string().into(),
            old_id.to_string().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Count how many ops records reference a given active node id.
/// Returns a non-zero count if the node has any live ops references.
async fn count_ops_references(txn: &impl ConnectionTrait, active_node_id: i64) -> AppResult<i64> {
    // Only filter `deleted_at` on tables that actually have that column
    // (equipment, user_scope_assignments). personnel / work_orders /
    // intervention_requests use hard deletes or status flags instead.
    let check_sql = format!(
        "SELECT \
          (SELECT COUNT(*) FROM equipment WHERE installed_at_node_id = {id} AND deleted_at IS NULL) + \
          (SELECT COUNT(*) FROM equipment WHERE functional_position_node_id = {id} AND deleted_at IS NULL) + \
          (SELECT COUNT(*) FROM intervention_requests WHERE org_node_id = {id}) + \
          (SELECT COUNT(*) FROM work_orders WHERE location_id = {id}) + \
          (SELECT COUNT(*) FROM personnel WHERE primary_entity_id = {id}) + \
          (SELECT COUNT(*) FROM personnel WHERE primary_team_id = {id}) + \
          (SELECT COUNT(*) FROM user_scope_assignments WHERE scope_reference = '{id}' AND scope_type IN ('entity','org_node','site','team') AND deleted_at IS NULL) + \
          (SELECT COUNT(*) FROM capacity_rules WHERE entity_id = {id}) + \
          (SELECT COUNT(*) FROM capacity_rules WHERE team_id = {id}) + \
          (SELECT COUNT(*) FROM planning_windows WHERE entity_id = {id}) + \
          (SELECT COUNT(*) FROM schedule_commitments WHERE assigned_team_id = {id}) + \
          (SELECT COUNT(*) FROM cost_centers WHERE entity_id = {id}) + \
          (SELECT COUNT(*) FROM budget_lines WHERE team_id = {id}) + \
          (SELECT COUNT(*) FROM inspection_templates WHERE org_scope_id = {id}) + \
          (SELECT COUNT(*) FROM closeout_validation_policies WHERE entity_id = {id}) \
         AS total",
        id = active_node_id
    );
    let row = txn
        .query_one(Statement::from_string(DbBackend::Sqlite, check_sql))
        .await?
        .expect("scalar COUNT query always returns a row");
    let total: i64 = row.try_get("", "total").map_err(|e| decode_err("total", e))?;
    Ok(total)
}

/// Active live node that has ops FKs but no draft clone with `origin_node_id`.
#[derive(Debug, Clone)]
pub struct UnmappedActiveNodeWithOpsRefs {
    pub node_id: i64,
    pub node_name: String,
    pub ops_ref_count: i64,
}

/// Shared publish/validate check: active nodes with ops refs and no draft lineage clone.
pub async fn collect_unmapped_active_nodes_with_ops_refs(
    db: &impl ConnectionTrait,
    draft_model_id: i64,
    active_model_id: i64,
) -> AppResult<Vec<UnmappedActiveNodeWithOpsRefs>> {
    let clone_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT origin_node_id AS active_id \
             FROM org_nodes \
             WHERE structure_model_id = ? \
               AND origin_node_id IS NOT NULL \
               AND deleted_at IS NULL",
            [draft_model_id.into()],
        ))
        .await?;

    let mut remapped: HashSet<i64> = HashSet::new();
    for row in &clone_rows {
        let active_id: i64 = row.try_get("", "active_id").map_err(|e| decode_err("active_id", e))?;
        remapped.insert(active_id);
    }

    let active_node_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, name FROM org_nodes \
             WHERE structure_model_id = ? AND deleted_at IS NULL",
            [active_model_id.into()],
        ))
        .await?;

    let mut out = Vec::new();
    for row in &active_node_rows {
        let active_id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        if remapped.contains(&active_id) {
            continue;
        }
        let active_name: String = row.try_get("", "name").map_err(|e| decode_err("name", e))?;
        let ref_count = count_ops_references(db, active_id).await?;
        if ref_count > 0 {
            out.push(UnmappedActiveNodeWithOpsRefs {
                node_id: active_id,
                node_name: active_name,
                ops_ref_count: ref_count,
            });
        }
    }
    Ok(out)
}

/// Execute all ops FK remaps for a single (old → new) node pair.
async fn remap_node_ops_fks(txn: &impl ConnectionTrait, old_id: i64, new_id: i64, now: &str) -> AppResult<()> {
    // Tables whose updated_at column has a different name or might not track it
    // use a simpler form; all standard tables follow the same schema convention.
    remap_integer_fk(txn, "equipment", "installed_at_node_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "equipment", "functional_position_node_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "intervention_requests", "org_node_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "work_orders", "location_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "personnel", "primary_entity_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "personnel", "primary_team_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "capacity_rules", "entity_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "capacity_rules", "team_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "planning_windows", "entity_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "schedule_commitments", "assigned_team_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "cost_centers", "entity_id", old_id, new_id, now).await?;
    remap_integer_fk(txn, "budget_lines", "team_id", old_id, new_id, now).await?;
    remap_integer_fk_row_version_only(txn, "inspection_templates", "org_scope_id", old_id, new_id).await?;
    remap_integer_fk(txn, "closeout_validation_policies", "entity_id", old_id, new_id, now).await?;

    // user_scope_assignments.scope_reference is TEXT; remap both scope types that store node IDs.
    remap_scope_reference(txn, "entity", old_id, new_id, now).await?;
    remap_scope_reference(txn, "org_node", old_id, new_id, now).await?;
    remap_scope_reference(txn, "site", old_id, new_id, now).await?;
    remap_scope_reference(txn, "team", old_id, new_id, now).await?;

    Ok(())
}

// ─── Publish ──────────────────────────────────────────────────────────────────

/// Atomically promote a draft model to active.
///
/// Transaction sequence:
/// 1. Validate draft tree (draft nodes vs draft schema).
/// 2. If `can_publish = false` → abort with `AppError::OrgValidationFailed`.
/// 3. (When prior active model exists)
///    a. Build active-id → draft-id map from `origin_node_id` on draft nodes.
///    b. Detect active nodes with no draft clone that still have ops FK refs → fail.
///    c. Remap all ops FKs: old active node id → new draft node id.
///    d. Soft-delete all nodes in the old active model (`deleted_at = now`).
///    e. Supersede the old active model.
/// 4. Activate the draft model.
/// 5. Clear `origin_node_id` on the newly active nodes (they are now the live tree).
/// 6. Commit.
pub async fn publish_model_with_remap(
    db: &DatabaseConnection,
    draft_model_id: i64,
    actor_id: i32,
) -> AppResult<OrgPublishValidationResult> {
    let txn = db.begin().await?;

    // ── Step 1: validate inside the transaction ───────────────────────────
    let validation = validate_draft_model_for_publish(&txn, draft_model_id).await?;

    if !validation.can_publish {
        let blocking: Vec<AppValidationIssue> = validation
            .issues
            .iter()
            .filter(|i| i.severity == "error")
            .cloned()
            .collect();
        return Err(org_validation_failed_issues(blocking));
    }

    let now = Utc::now().to_rfc3339();

    // ── Step 2: resolve prior active model ───────────────────────────────
    let active_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM org_structure_models WHERE status = 'active' LIMIT 1".to_string(),
        ))
        .await?;

    let old_active_model_id: Option<i64> = match active_row {
        Some(row) => Some(row.try_get("", "id").map_err(|e| decode_err("id", e))?),
        None => None,
    };

    let mut effective_remap_count: i64 = 0;

    if let Some(old_model_id) = old_active_model_id {
        // ── Step 3a: build active→draft node map from origin_node_id ─────
        let clone_rows = txn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id AS draft_id, origin_node_id AS active_id \
                 FROM org_nodes \
                 WHERE structure_model_id = ? \
                   AND origin_node_id IS NOT NULL \
                   AND deleted_at IS NULL",
                [draft_model_id.into()],
            ))
            .await?;

        let mut remap: HashMap<i64, i64> = HashMap::new(); // active_id → draft_id
        for row in &clone_rows {
            let draft_id: i64 = row.try_get("", "draft_id").map_err(|e| decode_err("draft_id", e))?;
            let active_id: i64 = row.try_get("", "active_id").map_err(|e| decode_err("active_id", e))?;
            remap.insert(active_id, draft_id);
        }
        effective_remap_count = remap.len() as i64;

        // ── Step 3b: detect unmapped active nodes with live ops refs ──────
        let unmapped = collect_unmapped_active_nodes_with_ops_refs(&txn, draft_model_id, old_model_id).await?;
        if !unmapped.is_empty() {
            let issues: Vec<AppValidationIssue> = unmapped
                .into_iter()
                .map(|u| {
                    AppValidationIssue::error_with_related(
                        "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS",
                        format!(
                            "'{}' is still used by live data but is missing from this draft. \
                             Repair the draft copies, or detach those links before publishing.",
                            u.node_name
                        ),
                        Some(u.node_id),
                        issue_params(&[
                            ("nodeName", u.node_name.clone()),
                            ("opsRefCount", u.ops_ref_count.to_string()),
                        ]),
                    )
                })
                .collect();
            return Err(org_validation_failed_issues(issues));
        }

        // ── Step 3c: remap all ops FKs ───────────────────────────────────
        for (old_id, new_id) in &remap {
            remap_node_ops_fks(&txn, *old_id, *new_id, &now).await?;
        }

        // ── Step 3d: soft-delete all nodes in the old active model ────────
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_nodes \
             SET deleted_at = ?, updated_at = ?, row_version = row_version + 1 \
             WHERE structure_model_id = ? AND deleted_at IS NULL",
            [now.clone().into(), now.clone().into(), old_model_id.into()],
        ))
        .await?;

        // ── Step 3e: supersede the old active model ───────────────────────
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_structure_models \
             SET status = 'superseded', superseded_at = ?, updated_at = ? \
             WHERE id = ?",
            [now.clone().into(), now.clone().into(), old_model_id.into()],
        ))
        .await?;
    }

    // ── Step 4: activate the draft model ──────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_structure_models \
         SET status = 'active', activated_at = ?, activated_by_id = ?, updated_at = ? \
         WHERE id = ?",
        [
            now.clone().into(),
            actor_id.into(),
            now.clone().into(),
            draft_model_id.into(),
        ],
    ))
    .await?;

    // ── Step 5: clear origin_node_id on the newly active nodes ────────────
    // They are now the live tree; the origin reference is no longer meaningful.
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_nodes \
         SET origin_node_id = NULL, updated_at = ?, row_version = row_version + 1 \
         WHERE structure_model_id = ? AND origin_node_id IS NOT NULL AND deleted_at IS NULL",
        [now.clone().into(), draft_model_id.into()],
    ))
    .await?;

    txn.commit().await?;

    tracing::info!(
        model_id = draft_model_id,
        remap_count = effective_remap_count,
        actor = actor_id,
        first_publish = old_active_model_id.is_none(),
        "org structure model published with atomic promote"
    );

    let mut result = validation;
    result.remap_count = effective_remap_count;
    Ok(result)
}
