//! Site scope of work permits (step 6, `docs/SITE_SCOPED_ROLES.md` §2 and §4, Permits row).
//!
//! A permit is anchored at its org node `work_permits.entity_id` (derived at creation from the
//! node given, else the linked work order's node, else the equipment's installed node). A
//! plant-wide role acts on every permit; a role on a node acts on the permits of that node and
//! below. A permit without a usable node (`NULL`, `0` or negative: not written by the screens, but
//! possible on old or received rows) is for plant-wide roles only, refused to the others with a
//! coded reason — and counted, never silently left out of a list.
//!
//! What belongs to a permit (isolations, suspensions, hand-over logs, LOTO cards) is checked at its
//! permit's node; when the permit is gone it has no site (plant-wide users only).
//!
//! Each rule has a `check_*` form (the scope decision on the user's [`AccessMap`], what the tests
//! exercise) and a `require_*` form (the commands: the cached map, the check, then the entitlement
//! and licence checks of `require_permission!` for the permission that passed). The generic pieces
//! (held anywhere, SQL predicate, equipment and work-order nodes) are those of `wo::access`.
//!
//! Hand-over receivers (CMMS Advisor decision, option A): the people who may receive a permit are
//! the active accounts holding `ptw.close` at the permit's node (plant-wide holders included) —
//! [`eligible_receiver_sql`] is the one rule, used by the picker and by the save.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::sync::RwLock;

use crate::auth::scope::{self, AccessMap, AccessScope, Anchor};
use crate::errors::{AppError, AppResult};
use crate::permit::domain::{WorkPermitCreateInput, WorkPermitUpdateInput};
use crate::rbac::cache::PermissionCache;
use crate::rbac::permissions::{PTW_CLOSE, PTW_ISSUE};
use crate::wo::access::{check_anywhere, equipment_node, wo_anchor};

pub use crate::wo::access::ScopeSql;

/// The anchor column of a permit (alias `wp`).
pub const PERMIT_ANCHOR_SQL: &str = "wp.entity_id";

/// A permit without a usable org node (alias `wp`): NULL, `0` or negative.
pub const PERMIT_WITHOUT_SITE_SQL: &str = "(wp.entity_id IS NULL OR wp.entity_id <= 0)";

/// The reader's scope as an SQL predicate on [`PERMIT_ANCHOR_SQL`].
pub fn scope_sql(scope: &AccessScope) -> ScopeSql {
    ScopeSql::on(scope, PERMIT_ANCHOR_SQL)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Anchors
// ═══════════════════════════════════════════════════════════════════════════════

/// The org node of a permit; `None` when the permit does not exist.
pub async fn permit_anchor_opt(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Option<Anchor>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_id FROM work_permits WHERE id = ?",
            [permit_id.into()],
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

/// The org node of a permit; not found when the permit does not exist.
pub async fn permit_anchor(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Anchor> {
    permit_anchor_opt(db, permit_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: permit_id.to_string(),
        })
}

/// A record that belongs to one permit (its `permit_id`), addressed by its own id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermitChild {
    /// An isolation point (`permit_isolations`).
    Isolation,
    /// A suspension (`permit_suspensions`).
    Suspension,
    /// A hand-over log (`permit_handover_logs`).
    HandoverLog,
}

impl PermitChild {
    /// The table (fixed names, never user input).
    fn table(self) -> &'static str {
        match self {
            Self::Isolation => "permit_isolations",
            Self::Suspension => "permit_suspensions",
            Self::HandoverLog => "permit_handover_logs",
        }
    }

    /// The entity named by the not-found error (the one the record's own functions use).
    fn entity(self) -> &'static str {
        match self {
            Self::Isolation => "PermitIsolation",
            Self::Suspension => "PermitSuspension",
            Self::HandoverLog => "PermitHandoverLog",
        }
    }
}

/// The org node of the permit a child record belongs to; not found when the record does not
/// exist. A record whose permit is gone has no site (plant-wide users only).
pub async fn child_anchor(db: &impl ConnectionTrait, child: PermitChild, id: i64) -> AppResult<Anchor> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wp.entity_id AS entity_id \
                   FROM {} c \
                   LEFT JOIN work_permits wp ON wp.id = c.permit_id \
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

/// The equipment, node and linked work order a permit holds now (`None`: no such permit).
async fn permit_fields(
    db: &impl ConnectionTrait,
    permit_id: i64,
) -> AppResult<Option<(i64, Option<i64>, Option<i64>)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT asset_id, entity_id, linked_work_order_id FROM work_permits WHERE id = ?",
            [permit_id.into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(Some((
            row.try_get::<i64>("", "asset_id")?,
            row.try_get::<Option<i64>>("", "entity_id")?,
            row.try_get::<Option<i64>>("", "linked_work_order_id")?,
        ))),
        None => Ok(None),
    }
}

/// The equipment and node of a work order (`None`: no such work order).
async fn wo_fields(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<(Option<i64>, Option<i64>)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id, entity_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(Some((
            row.try_get::<Option<i64>>("", "equipment_id")?,
            row.try_get::<Option<i64>>("", "entity_id")?,
        ))),
        None => Ok(None),
    }
}

/// The org node and equipment a new permit gets, derived exactly as
/// `queries::resolve_permit_scope` does: the node given, else the linked work order's node, else
/// the equipment's installed node; the equipment given, else the linked work order's. A linked work
/// order that does not exist is not found (as the create reports it).
pub async fn new_permit_node(
    db: &impl ConnectionTrait,
    input: &WorkPermitCreateInput,
) -> AppResult<(Option<i64>, Option<i64>)> {
    let mut asset_id = input.asset_id;
    let mut entity_id = input.entity_id;
    if let Some(wo_id) = input.linked_work_order_id {
        let (wo_equipment, wo_entity) = wo_fields(db, wo_id).await?.ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
        asset_id = asset_id.or(wo_equipment);
        entity_id = entity_id.or(wo_entity);
    }
    if entity_id.is_none() {
        if let Some(asset) = asset_id {
            entity_id = equipment_node(db, asset).await?;
        }
    }
    Ok((entity_id, asset_id))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Scope decisions (on the user's access map)
// ═══════════════════════════════════════════════════════════════════════════════

/// `permission` on the permit `permit_id`: refused as before when the user holds it nowhere, not
/// found when the permit does not exist, then checked at the permit's org node.
pub async fn check_on_permit(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    permit_id: i64,
) -> AppResult<Anchor> {
    let scope = check_anywhere(map, permission)?;
    let anchor = permit_anchor(db, permit_id).await?;
    scope::check_anchor(db, &scope, permission, anchor).await?;
    Ok(anchor)
}

/// Like [`check_on_permit`] for the reads of what belongs to a permit (isolations, suspensions,
/// hand-over logs): a permit that does not exist has no site, so only a plant-wide user reads what
/// is left of it, as before (an empty list, or orphan rows).
pub async fn check_on_permit_records(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    permit_id: i64,
) -> AppResult<()> {
    let scope = check_anywhere(map, permission)?;
    let anchor = permit_anchor_opt(db, permit_id).await?.unwrap_or(Anchor::Unknown);
    scope::check_anchor(db, &scope, permission, anchor).await
}

/// `permission` on the permit a child record belongs to (refused as before when held nowhere, not
/// found when the record does not exist).
pub async fn check_on_child(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    child: PermitChild,
    id: i64,
) -> AppResult<()> {
    let scope = check_anywhere(map, permission)?;
    let anchor = child_anchor(db, child, id).await?;
    scope::check_anchor(db, &scope, permission, anchor).await
}

/// Rule §2.6: creating a permit needs `ptw.issue` at the new permit's org node (derived as the
/// create does), at its equipment's installed node when that is another one, and — when it is
/// linked to a work order — at that work order's node (a site user never attaches a permit to
/// another site's work order, nor to one without a site). Without a derivable node: plant-wide
/// users only (the create then refuses it for everyone, as before: a permit needs a node).
pub async fn check_create_at(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    input: &WorkPermitCreateInput,
) -> AppResult<()> {
    let scope = check_anywhere(map, PTW_ISSUE)?;
    let (node, asset_id) = new_permit_node(db, input).await?;
    let anchor = Anchor::of(node);
    scope::check_anchor(db, &scope, PTW_ISSUE, anchor).await?;
    if let Some(asset) = asset_id {
        if let Some(asset_node) = equipment_node(db, asset).await? {
            if Some(asset_node) != node {
                scope::check_anchor(db, &scope, PTW_ISSUE, Anchor::of(Some(asset_node))).await?;
            }
        }
    }
    if let Some(wo_id) = input.linked_work_order_id {
        let wo = wo_anchor(db, wo_id).await?;
        if wo != anchor {
            scope::check_anchor(db, &scope, PTW_ISSUE, wo).await?;
        }
    }
    Ok(())
}

/// Rule §2.7: `ptw.issue` on the permit at its current node, and at every new place an edit
/// gives it (`queries::update_work_permit`: the node, the equipment and the linked work order may
/// change; the node is not recomputed from the equipment): a new node, the installed node of new
/// equipment (equipment not installed anywhere: plant-wide users only), the node of a newly linked
/// work order (not found when it does not exist).
pub async fn check_update(db: &impl ConnectionTrait, map: &AccessMap, input: &WorkPermitUpdateInput) -> AppResult<()> {
    let scope = map.scope_of(PTW_ISSUE);
    let current_anchor = check_on_permit(db, map, PTW_ISSUE, input.id).await?;
    let (asset_id, _entity_id, linked) = permit_fields(db, input.id).await?.ok_or_else(|| AppError::NotFound {
        entity: "WorkPermit".into(),
        id: input.id.to_string(),
    })?;
    if let Some(new_node) = input.entity_id {
        let new_anchor = Anchor::of(Some(new_node));
        if new_anchor != current_anchor {
            scope::check_anchor(db, &scope, PTW_ISSUE, new_anchor).await?;
        }
    }
    if let Some(new_asset) = input.asset_id.filter(|a| *a != asset_id) {
        let node = equipment_node(db, new_asset).await?;
        scope::check_anchor(db, &scope, PTW_ISSUE, Anchor::of(node)).await?;
    }
    if let Some(new_wo) = input.linked_work_order_id.filter(|w| Some(*w) != linked) {
        let wo = wo_anchor(db, new_wo).await?;
        scope::check_anchor(db, &scope, PTW_ISSUE, wo).await?;
    }
    Ok(())
}

/// A new isolation is checked at the permit it is added to; an existing one at its own permit
/// (by its own id) and at the permit the edit names (the save refuses moving it to another permit).
pub async fn check_isolation_upsert(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    isolation_id: Option<i64>,
    permit_id: i64,
) -> AppResult<()> {
    if let Some(id) = isolation_id {
        check_on_child(db, map, PTW_ISSUE, PermitChild::Isolation, id).await?;
    }
    check_on_permit(db, map, PTW_ISSUE, permit_id).await.map(|_| ())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hand-over receivers (CMMS Advisor decision, option A)
// ═══════════════════════════════════════════════════════════════════════════════

/// The one rule of who may receive a permit at `anchor`: an SQL predicate on the account `ua.id`
/// and its binds. True for the elevated administrators (who hold every right), and for the
/// accounts with a role in force carrying `ptw.close` plant-wide or on the anchor's node or one of
/// its parents (the subtree rule of [`scope::access_scope`]). A permit without a site: plant-wide
/// holders only. Activity and "not the signer" are the callers' filters.
pub async fn eligible_receiver_sql(
    db: &impl ConnectionTrait,
    anchor: Anchor,
) -> AppResult<(String, Vec<sea_orm::Value>)> {
    use crate::rbac::resolver::{assignment_clock, in_force_binds, permanent_binds, ASSIGNMENT_IN_FORCE_SQL};
    let now = assignment_clock();
    let mut binds: Vec<sea_orm::Value> = permanent_binds(&now).to_vec();
    binds.push(PTW_CLOSE.into());
    binds.extend(in_force_binds(&now));
    let node_clause = match anchor {
        Anchor::Node(node) => {
            let chain = scope::ancestor_chain(db, node).await?;
            if chain.is_empty() {
                "0=1".to_string()
            } else {
                let marks = vec!["?"; chain.len()].join(",");
                binds.extend(chain.into_iter().map(sea_orm::Value::from));
                format!(
                    "({live} AND CAST(TRIM(usa.scope_reference) AS INTEGER) IN ({marks}))",
                    live = scope::LIVE_NODE_ASSIGNMENT_SQL
                )
            }
        }
        Anchor::Unknown | Anchor::Plant => "0=1".to_string(),
    };
    let sql = format!(
        "(({elevated}) OR EXISTS ( \
            SELECT 1 FROM user_scope_assignments usa \
              JOIN role_permissions rp ON rp.role_id = usa.role_id \
              JOIN permissions p ON p.id = rp.permission_id \
             WHERE usa.user_id = ua.id AND p.name = ? AND {ASSIGNMENT_IN_FORCE_SQL} \
               AND (usa.scope_type = 'tenant' OR {node_clause})))",
        elevated = crate::auth::rbac::elevated_admin_sql(),
    );
    Ok((sql, binds))
}

/// Whether the account `user_id` may receive the permit `permit_id` ([`eligible_receiver_sql`];
/// a permit that does not exist has no site).
pub async fn may_receive_handover(db: &impl ConnectionTrait, permit_id: i64, user_id: i64) -> AppResult<bool> {
    let anchor = permit_anchor_opt(db, permit_id).await?.unwrap_or(Anchor::Unknown);
    let (eligible, binds) = eligible_receiver_sql(db, anchor).await?;
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(binds);
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT 1 AS ok FROM user_accounts ua WHERE ua.id = ? AND {eligible}"),
            values,
        ))
        .await?
        .is_some())
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

/// Lists and figures: where the user holds `permission` (the returned scope filters the rows).
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

/// [`check_on_permit`], then entitlement and licence.
pub async fn require_on_permit(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    permit_id: i64,
) -> AppResult<Anchor> {
    let anchor = check_on_permit(db, &access_map(db, cache, user_id).await?, permission, permit_id).await?;
    enforce(db, user_id, permission).await?;
    Ok(anchor)
}

/// [`check_on_permit_records`], then entitlement and licence.
pub async fn require_on_permit_records(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    permit_id: i64,
) -> AppResult<()> {
    check_on_permit_records(db, &access_map(db, cache, user_id).await?, permission, permit_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_on_child`], then entitlement and licence.
pub async fn require_on_child(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    child: PermitChild,
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
    input: &WorkPermitCreateInput,
) -> AppResult<()> {
    check_create_at(db, &access_map(db, cache, user_id).await?, input).await?;
    enforce(db, user_id, PTW_ISSUE).await
}

/// [`check_update`], then entitlement and licence.
pub async fn require_update(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    input: &WorkPermitUpdateInput,
) -> AppResult<()> {
    check_update(db, &access_map(db, cache, user_id).await?, input).await?;
    enforce(db, user_id, PTW_ISSUE).await
}

/// [`check_isolation_upsert`], then entitlement and licence.
pub async fn require_isolation_upsert(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    isolation_id: Option<i64>,
    permit_id: i64,
) -> AppResult<()> {
    check_isolation_upsert(db, &access_map(db, cache, user_id).await?, isolation_id, permit_id).await?;
    enforce(db, user_id, PTW_ISSUE).await
}
