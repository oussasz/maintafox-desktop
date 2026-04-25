//! Scope chain resolution — Phase 2 SP06-F02.
//!
//! Given an `org_node_id`, walks up the `org_nodes` hierarchy using a
//! recursive CTE and returns the full chain of ancestor scopes including a
//! synthetic tenant root. This allows a user assignment at any parent scope
//! to automatically grant access for operations at child scopes.
//!
//! The scope chain is ordered root → leaf. The synthetic tenant node
//! (id = 0) is always prepended so that tenant-wide assignments are
//! naturally included when matching against the chain.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;

// ── Types ────────────────────────────────────────────────────────────────────

/// A single node in the resolved scope chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeNode {
    /// `org_nodes.id` (0 for the synthetic tenant root).
    pub id: i64,
    /// Parent node id. `None` for root nodes and the synthetic tenant node.
    pub parent_id: Option<i64>,
    /// The node type code from `org_node_types` (e.g. `"site"`, `"entity"`).
    /// The synthetic tenant root uses `"tenant"`.
    pub scope_type: String,
}

/// An ordered chain of scope nodes from the tenant root down to the
/// requested leaf node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeChain {
    /// Nodes ordered root → leaf. `nodes[0]` is always the synthetic tenant
    /// node (id = 0, scope_type = `"tenant"`).
    pub nodes: Vec<ScopeNode>,
}

impl ScopeChain {
    /// Return all `org_node` ids in the chain (excludes the synthetic tenant 0).
    pub fn node_ids(&self) -> Vec<i64> {
        self.nodes
            .iter()
            .filter(|n| n.id != 0)
            .map(|n| n.id)
            .collect()
    }

    /// Return all scope reference strings suitable for matching against
    /// `user_scope_assignments.scope_reference`.
    /// Includes the stringified id of every real node in the chain.
    pub fn scope_references(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|n| n.id != 0)
            .map(|n| n.id.to_string())
            .collect()
    }

    /// Whether this chain contains the synthetic tenant root (always true
    /// for a well-formed chain, but useful as a guard).
    pub fn includes_tenant(&self) -> bool {
        self.nodes.first().map_or(false, |n| n.scope_type == "tenant")
    }
}

// ── Resolution ───────────────────────────────────────────────────────────────

/// Resolve the full upward scope chain for a given `org_node_id`.
///
/// Algorithm:
/// 1. Recursive CTE walks from the given node up through `parent_id` to root.
/// 2. JOINs `org_node_types` to get the type code for each level.
/// 3. Results are ordered by `depth ASC` (root first → leaf last).
/// 4. A synthetic tenant node (id=0, scope_type = `"tenant"`) is prepended.
///
/// Returns `Err(NotFound)` if `org_node_id` does not exist in `org_nodes`.
pub async fn resolve_scope_chain(
    db: &DatabaseConnection,
    org_node_id: i64,
) -> AppResult<ScopeChain> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"WITH RECURSIVE scope_cte(id, parent_id, node_type_id, depth) AS (
                SELECT id, parent_id, node_type_id, depth
                FROM org_nodes
                WHERE id = ? AND deleted_at IS NULL
                UNION ALL
                SELECT o.id, o.parent_id, o.node_type_id, o.depth
                FROM org_nodes o
                INNER JOIN scope_cte s ON o.id = s.parent_id
                WHERE o.deleted_at IS NULL
            )
            SELECT sc.id, sc.parent_id, sc.depth,
                   COALESCE(nt.code, 'unknown') AS scope_type
            FROM scope_cte sc
            LEFT JOIN org_node_types nt ON nt.id = sc.node_type_id
            ORDER BY sc.depth ASC",
            [org_node_id.into()],
        ))
        .await?;

    if rows.is_empty() {
        return Err(crate::errors::AppError::NotFound {
            entity: "OrgNode".into(),
            id: org_node_id.to_string(),
        });
    }

    // Build the chain: synthetic tenant root first, then real nodes (root → leaf).
    let mut nodes = Vec::with_capacity(rows.len() + 1);

    // Synthetic tenant node
    nodes.push(ScopeNode {
        id: 0,
        parent_id: None,
        scope_type: "tenant".to_owned(),
    });

    // Real hierarchy nodes (already ordered by depth ASC from the query)
    for row in &rows {
        nodes.push(ScopeNode {
            id: row.try_get("", "id")?,
            parent_id: row.try_get::<Option<i64>>("", "parent_id")?,
            scope_type: row.try_get("", "scope_type")?,
        });
    }

    Ok(ScopeChain { nodes })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_chain_accessors() {
        let chain = ScopeChain {
            nodes: vec![
                ScopeNode { id: 0, parent_id: None, scope_type: "tenant".into() },
                ScopeNode { id: 1, parent_id: None, scope_type: "site".into() },
                ScopeNode { id: 5, parent_id: Some(1), scope_type: "entity".into() },
            ],
        };

        assert!(chain.includes_tenant());
        assert_eq!(chain.node_ids(), vec![1, 5]);
        assert_eq!(chain.scope_references(), vec!["1", "5"]);
    }
}
