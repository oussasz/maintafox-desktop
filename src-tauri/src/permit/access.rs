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
use crate::permit::queries::PermitEditTarget;
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

/// The equipment of a work order (`None` inside: none recorded); `None`: no such work order.
async fn wo_equipment(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<Option<i64>>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(Some(row.try_get::<Option<i64>>("", "equipment_id")?)),
        None => Ok(None),
    }
}

/// The org node and equipment a new permit gets, as `queries::resolve_permit_scope` takes them:
/// the node given, else the node derived from the equipment and work order
/// (`queries::derived_permit_node`, `None` when not usable); the equipment given, else the linked
/// work order's. Equipment or a linked work order that does not exist is not found (as the create
/// reports it). Whether a given node is allowed (on the derived node's chain) is the create's rule,
/// checked last by [`check_create_at`].
pub async fn new_permit_node(
    db: &impl ConnectionTrait,
    input: &WorkPermitCreateInput,
) -> AppResult<(Option<i64>, Option<i64>)> {
    let mut asset_id = input.asset_id;
    if let Some(wo_id) = input.linked_work_order_id {
        let wo_equipment = wo_equipment(db, wo_id).await?.ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
        asset_id = asset_id.or(wo_equipment);
    }
    let node = match (input.entity_id, asset_id) {
        (Some(node), _) => Some(node),
        (None, Some(asset)) => {
            crate::permit::queries::derived_permit_node(db, asset, input.linked_work_order_id).await?
        }
        (None, None) => None,
    };
    Ok((node, asset_id))
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

/// The installed node of equipment `equipment_id` as an anchor (installed nowhere: no site); not
/// found when the equipment does not exist (as the create reports it).
async fn existing_equipment_anchor(db: &impl ConnectionTrait, equipment_id: i64) -> AppResult<Anchor> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Equipment".into(),
            id: equipment_id.to_string(),
        })?;
    Ok(Anchor::of(row.try_get::<Option<i64>>("", "installed_at_node_id")?))
}

/// Rule §2.6: creating a permit needs `ptw.issue` at the new permit's org node (derived as the
/// create does), at its equipment's installed node when that is another one (equipment installed
/// nowhere: plant-wide users only, as an edit), and — when it is linked to a work order — at that
/// work order's node (a site user never attaches a permit to another site's work order, nor to one
/// without a site). Without a derivable node: plant-wide users only.
///
/// The refusals keep the order of the other checks: the right held nowhere, then what the create
/// itself reports first (a work order or equipment that does not exist: not found; no equipment
/// given nor on the work order: the create's validation error), then the sites, then the create's
/// site rule (`queries::resolve_permit_scope`, the same function as the save: the node given must be
/// the one derived from the equipment or work order, or a level above it; without a usable derived
/// node the permit is refused), so the check and the create agree.
pub async fn check_create_at(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    input: &WorkPermitCreateInput,
) -> AppResult<()> {
    let scope = check_anywhere(map, PTW_ISSUE)?;
    let (node, asset_id) = new_permit_node(db, input).await?;
    let asset = asset_id.ok_or_else(crate::permit::queries::asset_required_error)?;
    let asset_anchor = existing_equipment_anchor(db, asset).await?;
    let anchor = Anchor::of(node);
    scope::check_anchor(db, &scope, PTW_ISSUE, anchor).await?;
    if asset_anchor != anchor {
        scope::check_anchor(db, &scope, PTW_ISSUE, asset_anchor).await?;
    }
    if let Some(wo_id) = input.linked_work_order_id {
        let wo = wo_anchor(db, wo_id).await?;
        if wo != anchor {
            scope::check_anchor(db, &scope, PTW_ISSUE, wo).await?;
        }
    }
    crate::permit::queries::resolve_permit_scope(db, input).await?;
    Ok(())
}

/// Rule §2.7: `ptw.issue` on the permit at its current node, and at every new place an edit
/// gives it: the node the save will store (`queries::permit_edit_target`, one rule for both: the
/// node given, else, when the equipment or the linked work order changes, the node derived from
/// them; a node that cannot be derived is for plant-wide users only), the installed node of new
/// equipment when that is another one (equipment not installed anywhere: plant-wide users only),
/// the node of a newly linked work order. New equipment or a work order that does not exist is not
/// found, before any site. The rules of the change itself (status, locks, planned isolations, the
/// node on the derived node's chain) are the save's. Returns the target checked: the save refuses
/// to store another one (`queries::update_work_permit_checked`).
pub async fn check_update(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    input: &WorkPermitUpdateInput,
) -> AppResult<PermitEditTarget> {
    let scope = map.scope_of(PTW_ISSUE);
    let current_anchor = check_on_permit(db, map, PTW_ISSUE, input.id).await?;
    let current = crate::permit::queries::get_work_permit(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: input.id.to_string(),
        })?;
    // Not found before any site: unknown new equipment (even with a node given) or work order
    // (`permit_edit_target` derives the node from them).
    if let Some(asset) = input.asset_id.filter(|a| *a != current.asset_id) {
        existing_equipment_anchor(db, asset).await?;
    }
    let target = crate::permit::queries::permit_edit_target(db, &current, input).await?;
    let stored_anchor = Anchor::of(target.entity_id);
    if stored_anchor != current_anchor {
        scope::check_anchor(db, &scope, PTW_ISSUE, stored_anchor).await?;
    }
    if target.asset_id != current.asset_id {
        let node = Anchor::of(equipment_node(db, target.asset_id).await?);
        if node != stored_anchor {
            scope::check_anchor(db, &scope, PTW_ISSUE, node).await?;
        }
    }
    if let Some(new_wo) = target
        .linked_work_order_id
        .filter(|w| Some(*w) != current.linked_work_order_id)
    {
        let wo = wo_anchor(db, new_wo).await?;
        if wo != stored_anchor {
            scope::check_anchor(db, &scope, PTW_ISSUE, wo).await?;
        }
    }
    Ok(target)
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
/// its parents (the subtree rule of [`scope::access_scope`]: the same in-force assignments, live
/// nodes and strict reading of the node reference). A permit without a site: plant-wide holders
/// only. Activity and "not the signer" are the callers' filters.
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
                    "({live} AND {node_id} IN ({marks}))",
                    live = scope::LIVE_NODE_ASSIGNMENT_SQL,
                    node_id = scope::NODE_REFERENCE_ID_SQL,
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

/// [`check_update`], then entitlement and licence; returns the target checked.
pub async fn require_update(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    input: &WorkPermitUpdateInput,
) -> AppResult<PermitEditTarget> {
    let target = check_update(db, &access_map(db, cache, user_id).await?, input).await?;
    enforce(db, user_id, PTW_ISSUE).await?;
    Ok(target)
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
