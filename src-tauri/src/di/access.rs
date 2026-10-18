//! Site scope of intervention requests (step 6, `docs/SITE_SCOPED_ROLES.md` §2 and §4, DI row).
//!
//! A request is anchored at its org node (`intervention_requests.org_node_id`). A plant-wide role
//! acts on every request; a role on a node acts on the requests of that node and below. A request
//! without a node (the `0` some rows from before decision O7 carry) is for plant-wide roles only,
//! refused to the others with a coded reason — and counted, never silently left out of a list.
//!
//! Each rule has a `check_*` form (the scope decision on the user's [`AccessMap`], what the tests
//! exercise) and a `require_*` form (the commands: the cached map, the check, then the
//! entitlement and licence checks of `require_permission!` for the permission that passed).
//! Lists, counts and statistics use [`ScopeSql`] in their SQL, before paging.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::sync::RwLock;

use crate::auth::scope::{self, AccessMap, AccessScope, Anchor};
use crate::errors::{AppError, AppResult};
use crate::rbac::cache::PermissionCache;
use crate::rbac::permissions::{DI_CONVERT, DI_CREATE, DI_CREATE_OWN, DI_REVIEW, OT_CREATE};

/// The anchor column of a request (alias `ir`).
pub const DI_ANCHOR_SQL: &str = "ir.org_node_id";

/// A request without a usable org node (alias `ir`): `0`, negative or NULL.
pub const DI_WITHOUT_SITE_SQL: &str = "(ir.org_node_id IS NULL OR ir.org_node_id <= 0)";

// ═══════════════════════════════════════════════════════════════════════════════
// SQL scope (lists, counts, statistics, audit log)
// ═══════════════════════════════════════════════════════════════════════════════

/// The reader's scope as an SQL predicate on [`DI_ANCHOR_SQL`] and its binds. The binds go at the
/// predicate's place in the statement (after the binds of the placeholders before it).
#[derive(Debug, Clone)]
pub struct ScopeSql {
    pub predicate: String,
    pub binds: Vec<sea_orm::Value>,
}

impl ScopeSql {
    pub fn of(scope: &AccessScope) -> Self {
        Self::on(scope, DI_ANCHOR_SQL)
    }

    /// The same predicate on another anchor expression (written by the caller, never user input).
    pub fn on(scope: &AccessScope, anchor_expr: &str) -> Self {
        let (predicate, binds) = scope::scope_filter_sql(scope, anchor_expr);
        Self { predicate, binds }
    }

    /// No restriction (`1=1`): the plant-wide reader, and the internal callers (home, dashboard)
    /// that are scoped in a later stage.
    pub fn everywhere() -> Self {
        Self::of(&AccessScope::Everywhere)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Anchors
// ═══════════════════════════════════════════════════════════════════════════════

/// The org node of a request; `None` when the request does not exist.
pub async fn di_anchor_opt(db: &impl ConnectionTrait, di_id: i64) -> AppResult<Option<Anchor>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT org_node_id FROM intervention_requests WHERE id = ?",
            [di_id.into()],
        ))
        .await?;
    match row {
        Some(row) => {
            let node: Option<i64> = row.try_get("", "org_node_id")?;
            Ok(Some(Anchor::of(node)))
        }
        None => Ok(None),
    }
}

/// The org node of a request; not found when the request does not exist.
pub async fn di_anchor(db: &impl ConnectionTrait, di_id: i64) -> AppResult<Anchor> {
    di_anchor_opt(db, di_id).await?.ok_or_else(|| AppError::NotFound {
        entity: "InterventionRequest".into(),
        id: di_id.to_string(),
    })
}

/// The org node of the request an attachment belongs to; not found when the attachment does not
/// exist. An attachment whose request is gone has no site (plant-wide users only).
pub async fn attachment_anchor(db: &impl ConnectionTrait, attachment_id: i64) -> AppResult<Anchor> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ir.org_node_id AS org_node_id \
               FROM di_attachments a \
               LEFT JOIN intervention_requests ir ON ir.id = a.di_id \
              WHERE a.id = ?",
            [attachment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "DiAttachment".into(),
            id: attachment_id.to_string(),
        })?;
    let node: Option<i64> = row.try_get("", "org_node_id")?;
    Ok(Anchor::of(node))
}

/// The org node an equipment is installed at (`None`: not installed anywhere, or no such
/// equipment — the create validation reports the latter).
pub async fn equipment_node(db: &impl ConnectionTrait, equipment_id: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(row.try_get::<Option<i64>>("", "installed_at_node_id")?),
        None => Ok(None),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Scope decisions (on the user's access map)
// ═══════════════════════════════════════════════════════════════════════════════

fn denied(permission: &str) -> AppError {
    AppError::PermissionDenied(format!("Permission requise : {permission}"))
}

/// Where the user holds `permission`; refused (as `require_permission!` refuses) when nowhere.
pub fn check_anywhere(map: &AccessMap, permission: &str) -> AppResult<AccessScope> {
    let scope = map.scope_of(permission);
    if scope.is_anywhere() {
        Ok(scope)
    } else {
        Err(denied(permission))
    }
}

/// One of `permissions` at `anchor` (the "screen or review", "own or any" rules of the commands):
/// the first one held there is returned. When none is held there, the coded scope refusal of the
/// first one held elsewhere (outside your sites, no site), else the plain refusal
/// `denied_message`.
pub async fn check_any_at<'p>(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permissions: &[&'p str],
    anchor: Anchor,
    denied_message: &str,
) -> AppResult<&'p str> {
    let mut scoped_refusal: Option<AppError> = None;
    for &permission in permissions {
        match scope::check_anchor(db, &map.scope_of(permission), permission, anchor).await {
            Ok(()) => return Ok(permission),
            Err(AppError::PermissionDenied(_)) => {}
            Err(other) => {
                if scoped_refusal.is_none() {
                    scoped_refusal = Some(other);
                }
            }
        }
    }
    Err(scoped_refusal.unwrap_or_else(|| AppError::PermissionDenied(denied_message.to_string())))
}

/// `permission` on the request `di_id`: refused as before when the user holds it nowhere, not
/// found when the request does not exist, then checked at the request's org node.
pub async fn check_on_di(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    di_id: i64,
) -> AppResult<Anchor> {
    let scope = check_anywhere(map, permission)?;
    let anchor = di_anchor(db, di_id).await?;
    scope::check_anchor(db, &scope, permission, anchor).await?;
    Ok(anchor)
}

/// Like [`check_on_di`] for the reads that list what belongs to a request (events,
/// attachments): a request that does not exist (or no longer exists) has no site, so only a
/// plant-wide user reads what is left of it, as before (an empty list, or orphan rows).
pub async fn check_on_di_records(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    di_id: i64,
) -> AppResult<()> {
    let scope = check_anywhere(map, permission)?;
    let anchor = di_anchor_opt(db, di_id).await?.unwrap_or(Anchor::Unknown);
    scope::check_anchor(db, &scope, permission, anchor).await
}

/// `permission` on the request an attachment belongs to (refused as before when held nowhere,
/// not found when the attachment does not exist).
pub async fn check_on_attachment(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    attachment_id: i64,
) -> AppResult<()> {
    let scope = check_anywhere(map, permission)?;
    let anchor = attachment_anchor(db, attachment_id).await?;
    scope::check_anchor(db, &scope, permission, anchor).await
}

/// One of `permissions` on the request `di_id`: refused with `denied_message` (as before) when the
/// user holds none of them anywhere, not found when the request does not exist, then
/// [`check_any_at`] at the request's org node.
pub async fn check_any_on_di<'p>(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permissions: &[&'p str],
    di_id: i64,
    denied_message: &str,
) -> AppResult<&'p str> {
    if !permissions.iter().any(|p| map.scope_of(p).is_anywhere()) {
        return Err(AppError::PermissionDenied(denied_message.to_string()));
    }
    let anchor = di_anchor(db, di_id).await?;
    check_any_at(db, map, permissions, anchor, denied_message).await
}

/// The requester with `di.create.own`, or anyone with `di.review`, at the request's org node
/// (edit the draft, add an attachment) — the commands' rule, unchanged, at the anchor.
pub async fn check_own_or_review(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i32,
    di: &super::domain::InterventionRequest,
) -> AppResult<&'static str> {
    let anchor = Anchor::of(Some(di.org_node_id));
    if di.submitter_id == i64::from(user_id) {
        check_any_at(
            db,
            map,
            &[DI_CREATE_OWN, DI_REVIEW],
            anchor,
            &format!("Permission requise : {DI_REVIEW}"),
        )
        .await
    } else {
        scope::check_anchor(db, &map.scope_of(DI_REVIEW), DI_REVIEW, anchor).await?;
        Ok(DI_REVIEW)
    }
}

/// Decision O7: a new request must name its org node (refused for everyone otherwise).
pub fn require_org_node(org_node_id: i64) -> AppResult<i64> {
    if org_node_id > 0 {
        return Ok(org_node_id);
    }
    Err(crate::errors::org_validation_failed(
        "DI_ORG_NODE_REQUIRED",
        "A request must be attached to an org node (its site).",
        std::collections::HashMap::from([("org_node_id".to_string(), org_node_id.to_string())]),
    ))
}

/// Rule §2.6: creating a request needs `di.create` (any request) or `di.create.own` at the new
/// request's org node, and at its equipment's node when that is another one (a request on
/// equipment of another site is refused). Decision O7 first: no org node, refused for everyone.
/// Returns the permissions that passed.
pub async fn check_create_at(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    org_node_id: i64,
    equipment_id: i64,
) -> AppResult<Vec<&'static str>> {
    let node = require_org_node(org_node_id)?;
    let permissions = [DI_CREATE, DI_CREATE_OWN];
    let denied_message = format!("Permission requise : {DI_CREATE_OWN}");
    let mut passed = vec![check_any_at(db, map, &permissions, Anchor::Node(node), &denied_message).await?];
    if let Some(eq_node) = equipment_node(db, equipment_id).await? {
        if eq_node != node {
            let at_equipment = check_any_at(db, map, &permissions, Anchor::Node(eq_node), &denied_message).await?;
            if !passed.contains(&at_equipment) {
                passed.push(at_equipment);
            }
        }
    }
    Ok(passed)
}

/// Converting a request needs `di.convert` on it, and the work-order create right where the new
/// work order is anchored: `conversion` copies the request's org node into
/// `work_orders.entity_id`, so the same node.
pub async fn check_convert(db: &impl ConnectionTrait, map: &AccessMap, di_id: i64) -> AppResult<Anchor> {
    let anchor = check_on_di(db, map, DI_CONVERT, di_id).await?;
    let wo_scope = check_anywhere(map, OT_CREATE)?;
    scope::check_anchor(db, &wo_scope, OT_CREATE, anchor).await?;
    Ok(anchor)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command checks: the decision, then entitlement and licence (as `require_permission!`)
// ═══════════════════════════════════════════════════════════════════════════════

async fn access_map(db: &DatabaseConnection, cache: &RwLock<PermissionCache>, user_id: i32) -> AppResult<AccessMap> {
    scope::access_map_cached(db, cache, user_id).await
}

async fn enforce(db: &DatabaseConnection, user_id: i32, permission: &str) -> AppResult<()> {
    crate::entitlements::queries::enforce_capability_for_permission(db, permission).await?;
    crate::license::queries::enforce_permission_matrix(db, user_id, permission).await
}

/// Lists: where the user holds `permission` (the returned scope filters the rows).
pub async fn require_anywhere(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
) -> AppResult<AccessScope> {
    let scope = check_anywhere(&access_map(db, cache, user_id).await?, permission)?;
    enforce(db, user_id, permission).await?;
    Ok(scope)
}

/// [`check_on_di`], then entitlement and licence.
pub async fn require_on_di(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    di_id: i64,
) -> AppResult<Anchor> {
    let anchor = check_on_di(db, &access_map(db, cache, user_id).await?, permission, di_id).await?;
    enforce(db, user_id, permission).await?;
    Ok(anchor)
}

/// [`check_on_di_records`], then entitlement and licence.
pub async fn require_on_di_records(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    di_id: i64,
) -> AppResult<()> {
    check_on_di_records(db, &access_map(db, cache, user_id).await?, permission, di_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_on_attachment`], then entitlement and licence.
pub async fn require_on_attachment(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    attachment_id: i64,
) -> AppResult<()> {
    check_on_attachment(db, &access_map(db, cache, user_id).await?, permission, attachment_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_any_on_di`], then entitlement and licence for the permission that passed.
pub async fn require_any_on_di(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permissions: &[&str],
    di_id: i64,
    denied_message: &str,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    let passed = check_any_on_di(db, &map, permissions, di_id, denied_message).await?;
    enforce(db, user_id, passed).await
}

/// [`check_own_or_review`], then entitlement and licence for the permission that passed.
pub async fn require_own_or_review(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    di: &super::domain::InterventionRequest,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    let passed = check_own_or_review(db, &map, user_id, di).await?;
    enforce(db, user_id, passed).await
}

/// [`check_create_at`], then entitlement and licence for the permissions that passed.
pub async fn require_create_at(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    org_node_id: i64,
    equipment_id: i64,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    for permission in check_create_at(db, &map, org_node_id, equipment_id).await? {
        enforce(db, user_id, permission).await?;
    }
    Ok(())
}

/// [`check_convert`], then entitlement and licence for both rights.
pub async fn require_convert(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    di_id: i64,
) -> AppResult<()> {
    check_convert(db, &access_map(db, cache, user_id).await?, di_id).await?;
    enforce(db, user_id, DI_CONVERT).await?;
    enforce(db, user_id, OT_CREATE).await
}
