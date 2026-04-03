use sea_orm::{DatabaseConnection, DbBackend, FromQueryResult, Statement};
use serde::Serialize;
use crate::errors::{AppError, AppResult};
use super::SearchFilter;

// ── DTOs ──────────────────────────────────────────────────────────────────

/// Tree node row — used for the org hierarchy tree in the UI.
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct OrgNodeTreeRow {
    pub id: i32,
    pub sync_id: String,
    pub code: String,
    pub name: String,
    pub parent_id: Option<i32>,
    pub ancestor_path: String,
    pub depth: i32,
    pub status: String,
    pub node_type_id: i32,
    pub node_type_code: Option<String>,
    pub node_type_label: Option<String>,
    pub can_host_assets: i32,
    pub can_own_work: i32,
}

/// Lightweight option row for dropdowns (e.g. "which org node owns this equipment?")
#[derive(Debug, Clone, Serialize, FromQueryResult)]
pub struct OrgNodeOption {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub depth: i32,
    pub ancestor_path: String,
}

// ── Query filters ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct OrgNodeFilter {
    pub parent_id: Option<i32>,
    pub node_type_id: Option<i32>,
    pub status: Option<String>,
    pub can_host_assets: Option<bool>,
    pub can_own_work: Option<bool>,
    pub ancestor_path_prefix: Option<String>,
    #[serde(flatten)]
    pub search: SearchFilter,
}

// ── Repository functions ──────────────────────────────────────────────────

/// Returns the full org tree (all active nodes) ordered by depth and code.
/// Used for the hierarchy tree panel in the Org module.
pub async fn get_org_tree(db: &DatabaseConnection) -> AppResult<Vec<OrgNodeTreeRow>> {
    let sql = r#"
        SELECT
            n.id, n.sync_id, n.code, n.name, n.parent_id, n.ancestor_path,
            n.depth, n.status, n.node_type_id,
            t.code AS node_type_code, t.label AS node_type_label,
            t.can_host_assets, t.can_own_work
        FROM org_nodes n
        LEFT JOIN org_node_types t ON t.id = n.node_type_id
        WHERE n.deleted_at IS NULL AND n.status != 'decommissioned'
        ORDER BY n.depth ASC, n.code ASC
    "#;
    let stmt = Statement::from_string(DbBackend::Sqlite, sql.to_string());
    Ok(OrgNodeTreeRow::find_by_statement(stmt)
        .all(db)
        .await?)
}

/// Returns all descendant nodes of a given ancestor path prefix.
/// Uses the ancestor_path column for O(log n) subtree queries.
pub async fn get_descendants(
    db: &DatabaseConnection,
    ancestor_path_prefix: &str,
) -> AppResult<Vec<OrgNodeTreeRow>> {
    let sql = r#"
        SELECT
            n.id, n.sync_id, n.code, n.name, n.parent_id, n.ancestor_path,
            n.depth, n.status, n.node_type_id,
            t.code AS node_type_code, t.label AS node_type_label,
            t.can_host_assets, t.can_own_work
        FROM org_nodes n
        LEFT JOIN org_node_types t ON t.id = n.node_type_id
        WHERE n.ancestor_path LIKE ? AND n.deleted_at IS NULL
        ORDER BY n.depth ASC, n.code ASC
    "#;
    let pattern = format!("{ancestor_path_prefix}%");
    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, [pattern.into()]);
    Ok(OrgNodeTreeRow::find_by_statement(stmt)
        .all(db)
        .await?)
}

/// Returns a single org node by its id.
pub async fn get_node_by_id(
    db: &DatabaseConnection,
    node_id: i32,
) -> AppResult<OrgNodeTreeRow> {
    let sql = r#"
        SELECT
            n.id, n.sync_id, n.code, n.name, n.parent_id, n.ancestor_path,
            n.depth, n.status, n.node_type_id,
            t.code AS node_type_code, t.label AS node_type_label,
            t.can_host_assets, t.can_own_work
        FROM org_nodes n
        LEFT JOIN org_node_types t ON t.id = n.node_type_id
        WHERE n.id = ? AND n.deleted_at IS NULL
    "#;
    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, sql, [node_id.into()]);
    OrgNodeTreeRow::find_by_statement(stmt)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".into(),
            id: node_id.to_string(),
        })
}

/// Returns nodes that can host assets — used by the equipment form dropdown.
pub async fn get_asset_host_nodes(db: &DatabaseConnection) -> AppResult<Vec<OrgNodeOption>> {
    let sql = r#"
        SELECT n.id, n.code, n.name, n.depth, n.ancestor_path
        FROM org_nodes n
        INNER JOIN org_node_types t ON t.id = n.node_type_id
        WHERE t.can_host_assets = 1 AND n.deleted_at IS NULL AND n.status = 'active'
        ORDER BY n.depth ASC, n.name ASC
    "#;
    let stmt = Statement::from_string(DbBackend::Sqlite, sql.to_string());
    Ok(OrgNodeOption::find_by_statement(stmt)
        .all(db)
        .await?)
}

/// Returns nodes that can own work — used by work order scope dropdown.
pub async fn get_work_owner_nodes(db: &DatabaseConnection) -> AppResult<Vec<OrgNodeOption>> {
    let sql = r#"
        SELECT n.id, n.code, n.name, n.depth, n.ancestor_path
        FROM org_nodes n
        INNER JOIN org_node_types t ON t.id = n.node_type_id
        WHERE t.can_own_work = 1 AND n.deleted_at IS NULL AND n.status = 'active'
        ORDER BY n.depth ASC, n.name ASC
    "#;
    let stmt = Statement::from_string(DbBackend::Sqlite, sql.to_string());
    Ok(OrgNodeOption::find_by_statement(stmt)
        .all(db)
        .await?)
}
