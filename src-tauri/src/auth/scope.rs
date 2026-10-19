//! Site-scoped access (step 6, `docs/SITE_SCOPED_ROLES.md`).
//!
//! A role held plant-wide (`tenant` assignment) grants everywhere. A role held on an org node
//! (`org_node`, and the legacy `site` / `entity` types) grants on that node and every node below
//! it. A record is checked at its *anchor* org node; a record without one is refused to a user
//! who is not plant-wide, with a coded reason — never silently.
//!
//! - [`access_scope`]: where a user holds a permission (cached per user, like the plant-wide set).
//! - [`require_at`] / `require_permission_at!`: an action on a record at its anchor.
//! - [`scope_filter_sql`]: the same rule as an SQL predicate for lists, counts and analytics.
//! - [`my_access`] / [`permissions_at_node`]: what the screens show.
//!
//! Plant-wide objects (settings, reference data, users, roles) keep `require_permission!` with
//! `PermissionScope::Global`, which counts plant-wide assignments only.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;
use tokio::sync::RwLock;

use crate::errors::{AppError, AppResult};
use crate::rbac::cache::PermissionCache;
use crate::rbac::resolver::{assignment_clock, in_force_binds, next_assignment_change, ASSIGNMENT_IN_FORCE_SQL};

/// Assignment scope types that grant on an org node and its subtree.
pub const NODE_SCOPE_TYPES_SQL: &str = "('org_node', 'site', 'entity')";

/// A node-scope assignment (alias `usa`) whose org node still exists: a role on a node deleted
/// since grants nothing (and does not let its holder sign in).
pub const LIVE_NODE_ASSIGNMENT_SQL: &str = "usa.scope_type IN ('org_node', 'site', 'entity') AND EXISTS ( \
     SELECT 1 FROM org_nodes n WHERE n.id = CAST(TRIM(usa.scope_reference) AS INTEGER) AND n.deleted_at IS NULL)";

/// The org node id of a node-scope assignment (alias `usa`) in SQL, read as strictly as
/// [`load_access_map`] reads it in Rust (`trim()` then `parse::<i64>()`): the reference trimmed of
/// ASCII whitespace must be an optional sign followed by digits only, else the expression is NULL
/// and matches no node (`CAST` alone would read `"12x"` as node 12).
pub const NODE_REFERENCE_ID_SQL: &str = "(CASE WHEN TRIM(usa.scope_reference, ' ' || char(9, 10, 11, 12, 13)) \
         GLOB '[0-9+-]*' \
     AND SUBSTR(TRIM(usa.scope_reference, ' ' || char(9, 10, 11, 12, 13)), 2) NOT GLOB '*[^0-9]*' \
     AND TRIM(usa.scope_reference, ' ' || char(9, 10, 11, 12, 13)) GLOB '*[0-9]*' \
     THEN CAST(TRIM(usa.scope_reference, ' ' || char(9, 10, 11, 12, 13)) AS INTEGER) END)";

/// Where a user holds one permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessScope {
    /// Plant-wide (a tenant assignment, or an elevated administrator).
    Everywhere,
    /// On these org nodes and every node below them; empty: nowhere.
    Subtrees(BTreeSet<i64>),
}

impl AccessScope {
    pub fn is_everywhere(&self) -> bool {
        matches!(self, Self::Everywhere)
    }

    /// Held somewhere (plant-wide or on at least one node).
    pub fn is_anywhere(&self) -> bool {
        match self {
            Self::Everywhere => true,
            Self::Subtrees(roots) => !roots.is_empty(),
        }
    }
}

/// The org node a record is checked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// The record belongs to this org node.
    Node(i64),
    /// The record has no derivable org node: plant-wide users only, refused visibly otherwise.
    Unknown,
    /// A plant-wide object (no site by design): plant-wide users only.
    Plant,
}

impl Anchor {
    /// `Node` for a positive id, `Unknown` for none (or the `0` some old rows carry).
    pub fn of(node: Option<i64>) -> Self {
        match node {
            Some(id) if id > 0 => Self::Node(id),
            _ => Self::Unknown,
        }
    }
}

/// Every permission of `user_id` with where it is held: one query, cached per user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessMap {
    pub elevated: bool,
    pub everywhere: BTreeSet<String>,
    /// Permission → the org nodes it is held on (subtree roots).
    pub on_nodes: BTreeMap<String, BTreeSet<i64>>,
}

impl AccessMap {
    pub fn scope_of(&self, permission: &str) -> AccessScope {
        if self.elevated || self.everywhere.contains(permission) {
            return AccessScope::Everywhere;
        }
        AccessScope::Subtrees(self.on_nodes.get(permission).cloned().unwrap_or_default())
    }
}

async fn load_access_map(db: &impl ConnectionTrait, user_id: i32) -> AppResult<AccessMap> {
    if crate::auth::rbac::user_has_elevated_admin_access(db, user_id).await? {
        return Ok(AccessMap {
            elevated: true,
            ..AccessMap::default()
        });
    }
    let now = assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(in_force_binds(&now));
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT DISTINCT p.name, usa.scope_type, usa.scope_reference \
                   FROM user_scope_assignments usa \
                   JOIN role_permissions rp ON rp.role_id = usa.role_id \
                   JOIN permissions p ON p.id = rp.permission_id \
                  WHERE usa.user_id = ? AND {ASSIGNMENT_IN_FORCE_SQL} \
                    AND (usa.scope_type = 'tenant' OR ({LIVE_NODE_ASSIGNMENT_SQL}))"
            ),
            values,
        ))
        .await?;
    let mut map = AccessMap::default();
    for row in rows {
        let name: String = row.try_get("", "name")?;
        let scope_type: String = row.try_get("", "scope_type")?;
        if scope_type == "tenant" {
            map.everywhere.insert(name);
            continue;
        }
        let reference: Option<String> = row.try_get("", "scope_reference")?;
        // A reference that is not a node id grants nothing (`validate_assignment_scope` refuses
        // them at assignment; an old malformed row is not widened to plant-wide).
        if let Some(node) = reference.as_deref().and_then(|r| r.trim().parse::<i64>().ok()) {
            map.on_nodes.entry(name).or_default().insert(node);
        }
    }
    for name in &map.everywhere {
        map.on_nodes.remove(name);
    }
    Ok(map)
}

/// The user's access map, from the cache when fresh (the plant-wide set's rules: 120 s, until the
/// next assignment change, cleared on every role change).
pub async fn access_map_cached(
    db: &impl ConnectionTrait,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
) -> AppResult<AccessMap> {
    let uid = i64::from(user_id);
    if let Some(map) = cache.read().await.get_access(uid) {
        return Ok(map.clone());
    }
    let map = load_access_map(db, user_id).await?;
    // An unreadable bound: the map is not kept at all.
    let valid_until = next_assignment_change(db, uid).await?.map(|at| {
        chrono::DateTime::parse_from_rfc3339(&at)
            .map(|t| t.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now())
    });
    cache.write().await.put_access(uid, map.clone(), valid_until);
    Ok(map)
}

/// Where `user_id` holds `permission` (uncached; tests and one-off reads).
pub async fn access_scope(db: &impl ConnectionTrait, user_id: i32, permission: &str) -> AppResult<AccessScope> {
    Ok(load_access_map(db, user_id).await?.scope_of(permission))
}

/// The org node and all its ancestors (structural: parents are followed whatever their status).
pub async fn ancestor_chain(db: &impl ConnectionTrait, node_id: i64) -> AppResult<Vec<i64>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE up(id, parent_id) AS ( \
               SELECT id, parent_id FROM org_nodes WHERE id = ? \
               UNION SELECT n.id, n.parent_id FROM org_nodes n JOIN up ON n.id = up.parent_id) \
             SELECT id FROM up",
            [node_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| r.try_get::<i64>("", "id").map_err(AppError::from))
        .collect()
}

/// Whether `scope` covers the org node `node_id`.
pub async fn covers(db: &impl ConnectionTrait, scope: &AccessScope, node_id: i64) -> AppResult<bool> {
    match scope {
        AccessScope::Everywhere => Ok(true),
        AccessScope::Subtrees(roots) if roots.is_empty() => Ok(false),
        AccessScope::Subtrees(roots) => Ok(ancestor_chain(db, node_id).await?.iter().any(|n| roots.contains(n))),
    }
}

fn refusal(code: &str, message: &str, permission: &str, node: Option<i64>) -> AppError {
    let mut params = HashMap::from([("permission".to_string(), permission.to_string())]);
    if let Some(node) = node {
        params.insert("org_node_id".to_string(), node.to_string());
    }
    crate::errors::org_validation_failed(code, message, params)
}

/// Refuses unless `scope` allows acting on a record anchored at `anchor`.
pub async fn check_anchor(
    db: &impl ConnectionTrait,
    scope: &AccessScope,
    permission: &str,
    anchor: Anchor,
) -> AppResult<()> {
    if scope.is_everywhere() {
        return Ok(());
    }
    if !scope.is_anywhere() {
        return Err(AppError::PermissionDenied(format!("Permission requise : {permission}")));
    }
    match anchor {
        Anchor::Node(node) => {
            if covers(db, scope, node).await? {
                Ok(())
            } else {
                Err(refusal(
                    "SCOPE_OUTSIDE_SITE",
                    "This record belongs to a site outside your roles.",
                    permission,
                    Some(node),
                ))
            }
        }
        Anchor::Unknown => Err(refusal(
            "SCOPE_RECORD_WITHOUT_SITE",
            "This record has no site: only a plant-wide role can act on it.",
            permission,
            None,
        )),
        Anchor::Plant => Err(refusal(
            "SCOPE_PLANT_WIDE_ONLY",
            "This is a plant-wide object: only a plant-wide role can act on it.",
            permission,
            None,
        )),
    }
}

/// The action check of `require_permission_at!`: the permission at the record's anchor, then the
/// entitlement and licence checks of `require_permission!`.
pub async fn require_at(
    db: &sea_orm::DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    anchor: Anchor,
) -> AppResult<()> {
    let scope = access_map_cached(db, cache, user_id).await?.scope_of(permission);
    check_anchor(db, &scope, permission, anchor).await?;
    crate::entitlements::queries::enforce_capability_for_permission(db, permission).await?;
    crate::license::queries::enforce_permission_matrix(db, user_id, permission).await?;
    Ok(())
}

/// SQL predicate: the org node `anchor_expr` is inside `scope` (`1=1` plant-wide, `0=1` nowhere).
/// `anchor_expr` is SQL written by the caller (a column), never user input. The returned values
/// bind the predicate's `?` and must be placed at the predicate's position in the caller's
/// statement (after the values of the placeholders that come before it).
/// A NULL anchor never matches a node scope, so rows without a site stay out for site users.
pub fn scope_filter_sql(scope: &AccessScope, anchor_expr: &str) -> (String, Vec<sea_orm::Value>) {
    match scope {
        AccessScope::Everywhere => ("1=1".to_string(), Vec::new()),
        AccessScope::Subtrees(roots) if roots.is_empty() => ("0=1".to_string(), Vec::new()),
        AccessScope::Subtrees(roots) => {
            let marks = vec!["?"; roots.len()].join(",");
            (
                format!(
                    "{anchor_expr} IN (WITH RECURSIVE scope_sub(id) AS ( \
                       SELECT id FROM org_nodes WHERE id IN ({marks}) \
                       UNION SELECT n.id FROM org_nodes n JOIN scope_sub s ON n.parent_id = s.id) \
                     SELECT id FROM scope_sub)"
                ),
                roots.iter().map(|r| sea_orm::Value::from(*r)).collect(),
            )
        }
    }
}

/// What the screens read: the permissions held plant-wide, and those held only on some nodes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MyAccess {
    /// Every permission when true (elevated administrator).
    pub all: bool,
    pub everywhere: Vec<String>,
    /// Org node id → permissions held on that node and below (not plant-wide).
    pub by_node: BTreeMap<i64, Vec<String>>,
}

pub fn my_access(map: &AccessMap) -> MyAccess {
    let mut by_node: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for (permission, nodes) in &map.on_nodes {
        for node in nodes {
            by_node.entry(*node).or_default().push(permission.clone());
        }
    }
    MyAccess {
        all: map.elevated,
        everywhere: map.everywhere.iter().cloned().collect(),
        by_node,
    }
}

/// The permissions that apply at `node_id`: plant-wide ones plus those held on the node or an
/// ancestor. `None` for an elevated administrator (every permission).
pub async fn permissions_at_node(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    node_id: i64,
) -> AppResult<Option<Vec<String>>> {
    if map.elevated {
        return Ok(None);
    }
    let chain: BTreeSet<i64> = ancestor_chain(db, node_id).await?.into_iter().collect();
    let mut out: BTreeSet<String> = map.everywhere.clone();
    for (permission, nodes) in &map.on_nodes {
        if nodes.iter().any(|n| chain.contains(n)) {
            out.insert(permission.clone());
        }
    }
    Ok(Some(out.into_iter().collect()))
}

/// Whether the user holds at least one in-force role on an org node: with no plant-wide role, enough
/// to sign in (a site-only user signs in and is limited to their sites).
pub async fn has_node_assignment(db: &impl ConnectionTrait, user_id: i32) -> AppResult<bool> {
    let now = assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(in_force_binds(&now));
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS x FROM user_scope_assignments usa WHERE usa.user_id = ? AND {ASSIGNMENT_IN_FORCE_SQL} \
                   AND {LIVE_NODE_ASSIGNMENT_SQL} LIMIT 1"
            ),
            values,
        ))
        .await?
        .is_some())
}
