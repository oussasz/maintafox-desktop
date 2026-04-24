//! Read-optimized projections for the Organization Designer workspace.
//!
//! These queries return flattened, denormalized rows suitable for the
//! designer UI. They are read-only and never mutate data.
//!
//! Sub-phase 01 File 03 — Sprint S1.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgDesignerNodeRow {
    pub node_id: i64,
    pub parent_id: Option<i64>,
    pub ancestor_path: String,
    pub depth: i64,
    pub code: String,
    pub name: String,
    pub status: String,
    pub row_version: i64,
    pub node_type_id: i64,
    pub node_type_code: String,
    pub node_type_label: String,
    pub can_host_assets: bool,
    pub can_own_work: bool,
    pub can_carry_cost_center: bool,
    pub can_aggregate_kpis: bool,
    pub can_receive_permits: bool,
    pub child_count: i64,
    pub active_responsibility_count: i64,
    pub active_binding_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgDesignerSnapshot {
    pub active_model_id: Option<i64>,
    pub active_model_version: Option<i64>,
    /// Present when a draft model exists; enables UI to label the draft (e.g. "Draft v4").
    pub draft_model_id: Option<i64>,
    pub draft_model_version: Option<i64>,
    pub nodes: Vec<OrgDesignerNodeRow>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_designer row decode failed for column '{column}': {e}"
    ))
}

fn map_designer_node(row: &QueryResult) -> AppResult<OrgDesignerNodeRow> {
    Ok(OrgDesignerNodeRow {
        node_id: row
            .try_get::<i64>("", "node_id")
            .map_err(|e| decode_err("node_id", e))?,
        parent_id: row
            .try_get::<Option<i64>>("", "parent_id")
            .map_err(|e| decode_err("parent_id", e))?,
        ancestor_path: row
            .try_get::<String>("", "ancestor_path")
            .map_err(|e| decode_err("ancestor_path", e))?,
        depth: row
            .try_get::<i64>("", "depth")
            .map_err(|e| decode_err("depth", e))?,
        code: row
            .try_get::<String>("", "code")
            .map_err(|e| decode_err("code", e))?,
        name: row
            .try_get::<String>("", "name")
            .map_err(|e| decode_err("name", e))?,
        status: row
            .try_get::<String>("", "status")
            .map_err(|e| decode_err("status", e))?,
        row_version: row
            .try_get::<i64>("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        node_type_id: row
            .try_get::<i64>("", "node_type_id")
            .map_err(|e| decode_err("node_type_id", e))?,
        node_type_code: row
            .try_get::<String>("", "node_type_code")
            .map_err(|e| decode_err("node_type_code", e))?,
        node_type_label: row
            .try_get::<String>("", "node_type_label")
            .map_err(|e| decode_err("node_type_label", e))?,
        can_host_assets: i64_to_bool(
            row.try_get::<i64>("", "can_host_assets")
                .map_err(|e| decode_err("can_host_assets", e))?,
        ),
        can_own_work: i64_to_bool(
            row.try_get::<i64>("", "can_own_work")
                .map_err(|e| decode_err("can_own_work", e))?,
        ),
        can_carry_cost_center: i64_to_bool(
            row.try_get::<i64>("", "can_carry_cost_center")
                .map_err(|e| decode_err("can_carry_cost_center", e))?,
        ),
        can_aggregate_kpis: i64_to_bool(
            row.try_get::<i64>("", "can_aggregate_kpis")
                .map_err(|e| decode_err("can_aggregate_kpis", e))?,
        ),
        can_receive_permits: i64_to_bool(
            row.try_get::<i64>("", "can_receive_permits")
                .map_err(|e| decode_err("can_receive_permits", e))?,
        ),
        child_count: row
            .try_get::<i64>("", "child_count")
            .map_err(|e| decode_err("child_count", e))?,
        active_responsibility_count: row
            .try_get::<i64>("", "active_responsibility_count")
            .map_err(|e| decode_err("active_responsibility_count", e))?,
        active_binding_count: row
            .try_get::<i64>("", "active_binding_count")
            .map_err(|e| decode_err("active_binding_count", e))?,
    })
}

// ─── SQL fragment ─────────────────────────────────────────────────────────────

/// The designer projection query. Returns one row per non-deleted node, ordered
/// by `ancestor_path` for stable tree rendering. Counts only include active
/// (non-expired) responsibilities and bindings.
const DESIGNER_SNAPSHOT_SQL: &str = r"
    SELECT
        n.id        AS node_id,
        n.parent_id,
        n.ancestor_path,
        n.depth,
        n.code,
        n.name,
        n.status,
        n.row_version,
        n.node_type_id,
        t.code      AS node_type_code,
        t.label     AS node_type_label,
        t.can_host_assets,
        t.can_own_work,
        t.can_carry_cost_center,
        t.can_aggregate_kpis,
        t.can_receive_permits,
        (SELECT COUNT(*)
         FROM org_nodes c
         WHERE c.parent_id = n.id AND c.deleted_at IS NULL
        ) AS child_count,
        (SELECT COUNT(*)
         FROM org_node_responsibilities r
         WHERE r.node_id = n.id AND r.valid_to IS NULL
        ) AS active_responsibility_count,
        (SELECT COUNT(*)
         FROM org_entity_bindings b
         WHERE b.node_id = n.id AND b.valid_to IS NULL
        ) AS active_binding_count
    FROM org_nodes n
    JOIN org_node_types t ON t.id = n.node_type_id
    WHERE n.deleted_at IS NULL
    ORDER BY n.ancestor_path ASC
";

// ─── Service functions ────────────────────────────────────────────────────────

/// Return the complete designer snapshot: model metadata (active, draft) and the
/// full flattened org tree. When **no** active and **no** draft model exist, the
/// snapshot has empty nodes. When only a draft exists (not yet first-published),
/// node rows still project through `org_nodes` and draft-scoped `org_node_types`.
pub async fn get_org_designer_snapshot(
    db: &DatabaseConnection,
) -> AppResult<OrgDesignerSnapshot> {
    let model_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, version_number FROM org_structure_models WHERE status = 'active' LIMIT 1"
                .to_string(),
        ))
        .await?;

    let (active_model_id, active_model_version) = match model_row {
        Some(row) => {
            let mid: i64 = row
                .try_get("", "id")
                .map_err(|e| decode_err("id", e))?;
            let mver: i64 = row
                .try_get("", "version_number")
                .map_err(|e| decode_err("version_number", e))?;
            (Some(mid), Some(mver))
        }
        None => (None, None),
    };

    let draft_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, version_number FROM org_structure_models WHERE status = 'draft' LIMIT 1"
                .to_string(),
        ))
        .await?;

    let (draft_model_id, draft_model_version) = match draft_row {
        Some(row) => {
            let id: i64 = row
                .try_get("", "id")
                .map_err(|e| decode_err("id", e))?;
            let v: i64 = row
                .try_get("", "version_number")
                .map_err(|e| decode_err("version_number", e))?;
            (Some(id), Some(v))
        }
        None => (None, None),
    };

    if active_model_id.is_none() && draft_model_id.is_none() {
        return Ok(OrgDesignerSnapshot {
            active_model_id: None,
            active_model_version: None,
            draft_model_id: None,
            draft_model_version: None,
            nodes: Vec::new(),
        });
    }

    // Flattened tree: operational nodes, joined to their node type row (any model).
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            DESIGNER_SNAPSHOT_SQL.to_string(),
        ))
        .await?;

    let nodes: Vec<OrgDesignerNodeRow> =
        rows.iter().map(map_designer_node).collect::<AppResult<_>>()?;

    Ok(OrgDesignerSnapshot {
        active_model_id,
        active_model_version,
        draft_model_id,
        draft_model_version,
        nodes,
    })
}

/// Search nodes by text query, optional status filter, and optional type filter.
/// Returns designer-projection rows ordered by `ancestor_path`.
pub async fn search_nodes(
    db: &DatabaseConnection,
    query: &str,
    status_filter: Option<&str>,
    type_filter: Option<&str>,
) -> AppResult<Vec<OrgDesignerNodeRow>> {
    // Build dynamic WHERE clause.
    let mut conditions = vec!["n.deleted_at IS NULL".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();

    // Text search — match against code, name, or node type label.
    if !query.is_empty() {
        let pattern = format!("%{query}%");
        conditions.push(
            "(n.code LIKE ? COLLATE NOCASE OR n.name LIKE ? COLLATE NOCASE OR t.label LIKE ? COLLATE NOCASE)"
                .to_string(),
        );
        values.push(pattern.clone().into());
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }

    // Status filter.
    if let Some(status) = status_filter {
        conditions.push("n.status = ?".to_string());
        values.push(status.to_string().into());
    }

    // Node-type filter (by type code).
    if let Some(type_code) = type_filter {
        conditions.push("t.code = ?".to_string());
        values.push(type_code.to_string().into());
    }

    let where_clause = conditions.join(" AND ");

    let sql = format!(
        r"SELECT
            n.id        AS node_id,
            n.parent_id,
            n.ancestor_path,
            n.depth,
            n.code,
            n.name,
            n.status,
            n.row_version,
            n.node_type_id,
            t.code      AS node_type_code,
            t.label     AS node_type_label,
            t.can_host_assets,
            t.can_own_work,
            t.can_carry_cost_center,
            t.can_aggregate_kpis,
            t.can_receive_permits,
            (SELECT COUNT(*)
             FROM org_nodes c
             WHERE c.parent_id = n.id AND c.deleted_at IS NULL
            ) AS child_count,
            (SELECT COUNT(*)
             FROM org_node_responsibilities r
             WHERE r.node_id = n.id AND r.valid_to IS NULL
            ) AS active_responsibility_count,
            (SELECT COUNT(*)
             FROM org_entity_bindings b
             WHERE b.node_id = n.id AND b.valid_to IS NULL
            ) AS active_binding_count
        FROM org_nodes n
        JOIN org_node_types t ON t.id = n.node_type_id
        WHERE {where_clause}
        ORDER BY n.ancestor_path ASC"
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            values,
        ))
        .await?;

    rows.iter().map(map_designer_node).collect()
}
