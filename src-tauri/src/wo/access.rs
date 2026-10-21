//! Site scope of work orders (step 6, `docs/SITE_SCOPED_ROLES.md` §2 and §4, Work orders row).
//!
//! A work order is anchored at its org node `work_orders.entity_id` (copied from the equipment's
//! installed node when the order is created). A plant-wide role acts on every work order; a role on
//! a node acts on the work orders of that node and below. A work order without a node (`NULL`,
//! rows from before the node was required) is for plant-wide roles only, refused to the others
//! with a coded reason — and counted, never silently left out of the list.
//!
//! What belongs to a work order (labour, parts, tasks, tools, downtime, attachments) is checked at
//! its work order's node; when the work order is gone it has no site (plant-wide users only).
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
use crate::rbac::permissions::OT_CREATE;

/// The anchor column of a work order (alias `wo`).
pub const WO_ANCHOR_SQL: &str = "wo.entity_id";

/// A work order without a usable org node (alias `wo`): NULL, `0` or negative.
pub const WO_WITHOUT_SITE_SQL: &str = "(wo.entity_id IS NULL OR wo.entity_id <= 0)";

// ═══════════════════════════════════════════════════════════════════════════════
// SQL scope (lists, counts, statistics, audit log)
// ═══════════════════════════════════════════════════════════════════════════════

/// The reader's scope as an SQL predicate on [`WO_ANCHOR_SQL`] and its binds. The binds go at the
/// predicate's place in the statement (after the binds of the placeholders before it).
#[derive(Debug, Clone)]
pub struct ScopeSql {
    pub predicate: String,
    pub binds: Vec<sea_orm::Value>,
}

impl ScopeSql {
    pub fn of(scope: &AccessScope) -> Self {
        Self::on(scope, WO_ANCHOR_SQL)
    }

    /// The same predicate on another anchor expression (written by the caller, never user input).
    pub fn on(scope: &AccessScope, anchor_expr: &str) -> Self {
        let (predicate, binds) = scope::scope_filter_sql(scope, anchor_expr);
        Self { predicate, binds }
    }

    /// No restriction (`1=1`): the plant-wide reader, and the internal callers (home, reports,
    /// analytics) that are scoped in a later stage.
    pub fn everywhere() -> Self {
        Self::of(&AccessScope::Everywhere)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Anchors
// ═══════════════════════════════════════════════════════════════════════════════

/// The org node of a work order; `None` when the work order does not exist.
pub async fn wo_anchor_opt(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<Anchor>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?;
    match row {
        Some(row) => {
            let node: Option<i64> = row.try_get("", "entity_id")?;
            Ok(Some(Anchor::of(node)))
        }
        None => Ok(None),
    }
}

/// The org node of a work order; not found when the work order does not exist.
pub async fn wo_anchor(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Anchor> {
    wo_anchor_opt(db, wo_id).await?.ok_or_else(|| wo_not_found(wo_id))
}

fn wo_not_found(wo_id: i64) -> AppError {
    AppError::NotFound {
        entity: "WorkOrder".into(),
        id: wo_id.to_string(),
    }
}

/// A record that belongs to one work order (its `work_order_id`), addressed by its own id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WoChild {
    /// A labour entry (`work_order_interveners`).
    Labor,
    /// A part line (`work_order_parts`).
    Part,
    /// A task (`work_order_tasks`).
    Task,
    /// A tool line (`work_order_tools`).
    Tool,
    /// A downtime segment (`work_order_downtime_segments`).
    Downtime,
    /// An attachment (`work_order_attachments`).
    Attachment,
}

impl WoChild {
    /// The table (fixed names, never user input).
    fn table(self) -> &'static str {
        match self {
            Self::Labor => "work_order_interveners",
            Self::Part => "work_order_parts",
            Self::Task => "work_order_tasks",
            Self::Tool => "work_order_tools",
            Self::Downtime => "work_order_downtime_segments",
            Self::Attachment => "work_order_attachments",
        }
    }

    /// The entity named by the not-found error (the one the record's own functions use).
    fn entity(self) -> &'static str {
        match self {
            Self::Labor => "WoIntervener",
            Self::Part => "WoPart",
            Self::Task => "WoTask",
            Self::Tool => "WoTool",
            Self::Downtime => "WoDowntimeSegment",
            Self::Attachment => "WoAttachment",
        }
    }
}

/// The org node of the work order a child record belongs to; not found when the record does not
/// exist. A record whose work order is gone has no site (plant-wide users only).
pub async fn child_anchor(db: &impl ConnectionTrait, child: WoChild, id: i64) -> AppResult<Anchor> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.entity_id AS entity_id \
                   FROM {} c \
                   LEFT JOIN work_orders wo ON wo.id = c.work_order_id \
                  WHERE c.id = ?",
                child.table()
            ),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: child.entity().into(),
            id: id.to_string(),
        })?;
    let node: Option<i64> = row.try_get("", "entity_id")?;
    Ok(Anchor::of(node))
}

/// The org node an equipment is installed at (`None`: not installed anywhere, or no such
/// equipment — the create / update validation reports the latter).
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

/// The equipment a work order is on (`None`: no such work order, or none recorded).
async fn wo_equipment(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(row.try_get::<Option<i64>>("", "equipment_id")?),
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

/// `permission` on the work order `wo_id`: refused as before when the user holds it nowhere, not
/// found when the work order does not exist, then checked at the work order's org node.
pub async fn check_on_wo(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    wo_id: i64,
) -> AppResult<Anchor> {
    let scope = check_anywhere(map, permission)?;
    let anchor = wo_anchor(db, wo_id).await?;
    scope::check_anchor(db, &scope, permission, anchor).await?;
    Ok(anchor)
}

/// Like [`check_on_wo`] for the reads of what belongs to a work order (labour, parts, tasks,
/// history, costs…): a work order that does not exist (or no longer exists) has no site, so only
/// a plant-wide user reads what is left of it, as before (an empty list, or orphan rows).
pub async fn check_on_wo_records(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    wo_id: i64,
) -> AppResult<()> {
    let scope = check_anywhere(map, permission)?;
    let anchor = wo_anchor_opt(db, wo_id).await?.unwrap_or(Anchor::Unknown);
    scope::check_anchor(db, &scope, permission, anchor).await
}

/// `permission` on the work order a child record belongs to (refused as before when held
/// nowhere, not found when the record does not exist).
pub async fn check_on_child(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    child: WoChild,
    id: i64,
) -> AppResult<()> {
    let scope = check_anywhere(map, permission)?;
    let anchor = child_anchor(db, child, id).await?;
    scope::check_anchor(db, &scope, permission, anchor).await
}

/// The org node a new work order gets (`queries::create_work_order`): the node given, else the
/// installed node of its equipment.
pub async fn new_wo_node(
    db: &impl ConnectionTrait,
    entity_id: Option<i64>,
    equipment_id: Option<i64>,
) -> AppResult<Option<i64>> {
    if let Some(node) = entity_id.filter(|&e| e > 0) {
        return Ok(Some(node));
    }
    match equipment_id.filter(|&e| e > 0) {
        Some(eq) => equipment_node(db, eq).await,
        None => Ok(None),
    }
}

/// Rule §2.6: creating a work order needs `ot.create` at the new work order's org node (derived
/// as the create does), and at its equipment's node when that is another one (a work order on
/// equipment of another site is refused). Without a derivable node: plant-wide users only (the
/// create then refuses it for everyone, as before: a work order needs a node).
pub async fn check_create_at(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    entity_id: Option<i64>,
    equipment_id: Option<i64>,
) -> AppResult<()> {
    let scope = check_anywhere(map, OT_CREATE)?;
    let node = new_wo_node(db, entity_id, equipment_id).await?;
    scope::check_anchor(db, &scope, OT_CREATE, Anchor::of(node)).await?;
    if let Some(eq) = equipment_id.filter(|&e| e > 0) {
        if let Some(eq_node) = equipment_node(db, eq).await? {
            if Some(eq_node) != node {
                scope::check_anchor(db, &scope, OT_CREATE, Anchor::of(Some(eq_node))).await?;
            }
        }
    }
    Ok(())
}

/// Rule §2.7: `permission` on the work order, and — when the equipment changes — at the new
/// equipment's installed node too (equipment not installed anywhere: plant-wide users only).
pub async fn check_equipment_change(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    wo_id: i64,
    new_equipment_id: Option<i64>,
) -> AppResult<()> {
    let scope = map.scope_of(permission);
    check_on_wo(db, map, permission, wo_id).await?;
    let Some(new_eq) = new_equipment_id else {
        return Ok(());
    };
    if wo_equipment(db, wo_id).await? == Some(new_eq) {
        return Ok(());
    }
    let node = equipment_node(db, new_eq).await?;
    scope::check_anchor(db, &scope, permission, Anchor::of(node)).await
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

/// Lists and statistics: where the user holds `permission` (the returned scope filters the rows).
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

/// Whether the user holds `permission` somewhere (no refusal; a read gate with alternatives).
pub async fn holds_anywhere(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
) -> AppResult<bool> {
    Ok(access_map(db, cache, user_id).await?.scope_of(permission).is_anywhere())
}

/// [`check_on_wo`], then entitlement and licence.
pub async fn require_on_wo(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    wo_id: i64,
) -> AppResult<Anchor> {
    let anchor = check_on_wo(db, &access_map(db, cache, user_id).await?, permission, wo_id).await?;
    enforce(db, user_id, permission).await?;
    Ok(anchor)
}

/// [`check_on_wo_records`], then entitlement and licence.
pub async fn require_on_wo_records(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    wo_id: i64,
) -> AppResult<()> {
    check_on_wo_records(db, &access_map(db, cache, user_id).await?, permission, wo_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_on_child`], then entitlement and licence.
pub async fn require_on_child(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    child: WoChild,
    id: i64,
) -> AppResult<()> {
    check_on_child(db, &access_map(db, cache, user_id).await?, permission, child, id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_create_at`], then entitlement and licence.
pub async fn require_create_at(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    entity_id: Option<i64>,
    equipment_id: Option<i64>,
) -> AppResult<()> {
    check_create_at(db, &access_map(db, cache, user_id).await?, entity_id, equipment_id).await?;
    enforce(db, user_id, OT_CREATE).await
}

/// [`check_equipment_change`], then entitlement and licence.
pub async fn require_equipment_change(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    wo_id: i64,
    new_equipment_id: Option<i64>,
) -> AppResult<()> {
    check_equipment_change(
        db,
        &access_map(db, cache, user_id).await?,
        permission,
        wo_id,
        new_equipment_id,
    )
    .await?;
    enforce(db, user_id, permission).await
}
