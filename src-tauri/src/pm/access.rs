//! Site scope of preventive-maintenance plans (step 6, `docs/SITE_SCOPED_ROLES.md`; CMMS Advisor
//! decision on PM plan rights, 2026-11).
//!
//! A PM plan is anchored where it applies:
//! - a plan on one equipment: the org node the equipment is installed at (not installed anywhere:
//!   no site, plant-wide roles only);
//! - a plan on an organisation location: that org node (a role on the node or above it covers it);
//! - a plan on an equipment family or a criticality group spans sites: a plant-wide object, for
//!   plant-wide roles only.
//!
//! The plan rights are checked at that anchor: `pm.create` (a new plan at its target, a new version
//! of a plan, a revision from a finding), `pm.edit` (the plan, its versions, publishing, the
//! lifecycle steps other than approval), `pm.approve` (approval) and `pm.delete` (a draft plan or
//! version). A plant-wide role acts on every plan, as before.
//!
//! Changing a plan's target so that its anchor changes (another site, or a plant-wide family /
//! criticality group) is a site change: only on a plan still in draft, with `pm.edit` at the old
//! anchor and at the new one (the rule of a work-order draft's equipment change, `wo::access`), and
//! recorded in the audit trail in the same transaction (`commands::pm::update_pm_plan`). A change of
//! target inside the same anchor is an ordinary edit.
//!
//! Occurrences and work orders already generated keep their own site (`pm::generation` is not
//! changed here); the occurrence, execution and generation commands keep their plant-wide checks.
//!
//! Reading (`pm.view`, CMMS Advisor decision 2026-11-06): the plan list, the occurrence lists and
//! the due figures are limited in SQL to the plans the reader may see ([`plan_view_filter`]); a
//! plan and what belongs to it (versions, occurrences, executions, findings, readiness, KPIs) is
//! read with `pm.view` at the plan's anchor ([`check_view_plan`]). A family or criticality-group
//! plan is visible, read-only, to every `pm.view` holder whatever the scope of their role; a plan
//! on equipment installed nowhere is for plant-wide readers only (as a work order without a site).
//!
//! Each rule has a `check_*` form (the decision on the user's [`AccessMap`], what the tests
//! exercise) and a `require_*` form (the commands: the cached map, the check, then the entitlement
//! and licence checks of `require_permission!` for the permission that passed).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::sync::RwLock;

use crate::auth::scope::{self, AccessMap, AccessScope, Anchor};
use crate::errors::{AppError, AppResult};
use crate::rbac::cache::PermissionCache;
use crate::rbac::permissions::{PM_CREATE, PM_EDIT, PM_VIEW};
use crate::wo::access::ScopeSql;

/// The org node a stored plan (alias `alias`, written by the caller, never user input) is checked
/// at, as SQL for the read models and the read filters (`NULL`: a plant-wide plan, or an equipment
/// installed nowhere). Same rule as [`target_anchor`].
pub fn plan_access_node_sql(alias: &str) -> String {
    format!(
        "(CASE LOWER(TRIM({alias}.asset_scope_type)) \
           WHEN 'equipment' THEN (SELECT e.installed_at_node_id FROM equipment e WHERE e.id = {alias}.asset_scope_id) \
           WHEN 'location' THEN {alias}.asset_scope_id END)"
    )
}

/// SQL (plan alias `alias`): a family or criticality-group plan, visible to every `pm.view` holder.
fn plant_wide_plan_sql(alias: &str) -> String {
    format!("LOWER(TRIM({alias}.asset_scope_type)) IN ('family', 'criticality_group')")
}

/// The plans (alias `alias`) a reader with `pm.view` held in `scope` sees: everything plant-wide;
/// otherwise the plans anchored in their subtrees, and every family / criticality-group plan.
pub fn plan_view_filter(scope: &AccessScope, alias: &str) -> ScopeSql {
    if scope.is_everywhere() {
        return ScopeSql::everywhere();
    }
    let inside = ScopeSql::on(scope, &plan_access_node_sql(alias));
    ScopeSql {
        predicate: format!("({} OR {})", plant_wide_plan_sql(alias), inside.predicate),
        binds: inside.binds,
    }
}

/// The occurrences (alias `po`) whose plan the reader sees ([`plan_view_filter`]).
pub fn occurrence_view_filter(scope: &AccessScope) -> ScopeSql {
    if scope.is_everywhere() {
        return ScopeSql::everywhere();
    }
    let plans = plan_view_filter(scope, "vp");
    ScopeSql {
        predicate: format!(
            "EXISTS (SELECT 1 FROM pm_plans vp WHERE vp.id = po.pm_plan_id AND {})",
            plans.predicate
        ),
        binds: plans.binds,
    }
}

/// What a plan targets (`pm_plans.asset_scope_type` / `asset_scope_id`) and its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTarget {
    pub scope_type: String,
    pub scope_id: Option<i64>,
    pub lifecycle_status: String,
    pub row_version: i64,
}

/// A change of a plan's anchor, for the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSiteMove {
    pub old_scope_type: String,
    pub old_scope_id: Option<i64>,
    pub old_anchor: Anchor,
    pub new_scope_type: String,
    pub new_scope_id: Option<i64>,
    pub new_anchor: Anchor,
}

impl PlanSiteMove {
    /// The audit details (old and new target and org node; `null`: plant-wide / no site).
    pub fn details_json(&self) -> String {
        serde_json::json!({
            "old_scope_type": self.old_scope_type,
            "old_scope_id": self.old_scope_id,
            "old_org_node_id": anchor_node(self.old_anchor),
            "old_anchor": anchor_kind(self.old_anchor),
            "new_scope_type": self.new_scope_type,
            "new_scope_id": self.new_scope_id,
            "new_org_node_id": anchor_node(self.new_anchor),
            "new_anchor": anchor_kind(self.new_anchor),
        })
        .to_string()
    }
}

fn anchor_node(anchor: Anchor) -> Option<i64> {
    match anchor {
        Anchor::Node(id) => Some(id),
        Anchor::Unknown | Anchor::Plant => None,
    }
}

fn anchor_kind(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::Node(_) => "site",
        Anchor::Unknown => "no_site",
        Anchor::Plant => "plant_wide",
    }
}

/// Audit action of a plan moved to another site (`audit_events.action_code`).
pub const AUDIT_ACTION_SITE_CHANGED: &str = "pm.plan_site_changed";

// ═══════════════════════════════════════════════════════════════════════════════
// Anchors
// ═══════════════════════════════════════════════════════════════════════════════

/// The anchor of a plan targeting `scope_type` / `scope_id`: an equipment's installed node, a
/// location's node, plant-wide for a family or a criticality group (and for any other type: the
/// plan validation refuses it, never a site user's way around the check).
pub async fn target_anchor(db: &impl ConnectionTrait, scope_type: &str, scope_id: Option<i64>) -> AppResult<Anchor> {
    match scope_type.trim().to_lowercase().as_str() {
        "equipment" => match scope_id.filter(|&id| id > 0) {
            Some(eq) => Ok(Anchor::of(crate::wo::access::equipment_node(db, eq).await?)),
            None => Ok(Anchor::Unknown),
        },
        "location" => Ok(Anchor::of(scope_id)),
        _ => Ok(Anchor::Plant),
    }
}

/// The target, lifecycle and row version of a plan; not found when it does not exist.
pub async fn plan_target(db: &impl ConnectionTrait, plan_id: i64) -> AppResult<PlanTarget> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT asset_scope_type, asset_scope_id, lifecycle_status, row_version FROM pm_plans WHERE id = ?",
            [plan_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan".into(),
            id: plan_id.to_string(),
        })?;
    Ok(PlanTarget {
        scope_type: row.try_get("", "asset_scope_type")?,
        scope_id: row.try_get("", "asset_scope_id")?,
        lifecycle_status: row.try_get("", "lifecycle_status")?,
        row_version: row.try_get("", "row_version")?,
    })
}

/// The anchor of a stored plan; not found when it does not exist.
pub async fn plan_anchor(db: &impl ConnectionTrait, plan_id: i64) -> AppResult<Anchor> {
    let target = plan_target(db, plan_id).await?;
    target_anchor(db, &target.scope_type, target.scope_id).await
}

/// The plan of a version; not found when the version does not exist.
pub async fn version_plan(db: &impl ConnectionTrait, version_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pm_plan_id FROM pm_plan_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan_version".into(),
            id: version_id.to_string(),
        })?;
    Ok(row.try_get("", "pm_plan_id")?)
}

/// The plan a finding was recorded on (finding → execution → occurrence); not found otherwise.
pub async fn finding_plan(db: &impl ConnectionTrait, finding_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.pm_plan_id FROM pm_findings pf \
               JOIN pm_executions pe ON pe.id = pf.pm_execution_id \
               JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id \
              WHERE pf.id = ?",
            [finding_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_finding".into(),
            id: finding_id.to_string(),
        })?;
    Ok(row.try_get("", "pm_plan_id")?)
}

/// The plan of an occurrence; not found when the occurrence does not exist.
pub async fn occurrence_plan(db: &impl ConnectionTrait, occurrence_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pm_plan_id FROM pm_occurrences WHERE id = ?",
            [occurrence_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_occurrence".into(),
            id: occurrence_id.to_string(),
        })?;
    Ok(row.try_get("", "pm_plan_id")?)
}

/// The plan of an execution (execution → occurrence); not found when the execution does not exist.
pub async fn execution_plan(db: &impl ConnectionTrait, execution_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.pm_plan_id FROM pm_executions pe \
               JOIN pm_occurrences po ON po.id = pe.pm_occurrence_id \
              WHERE pe.id = ?",
            [execution_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_execution".into(),
            id: execution_id.to_string(),
        })?;
    Ok(row.try_get("", "pm_plan_id")?)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Scope decisions (on the user's access map)
// ═══════════════════════════════════════════════════════════════════════════════

/// Reading at an anchor: `pm.view` there; a plant-wide plan (family / criticality group) is read
/// by every `pm.view` holder.
async fn check_view_at(db: &impl ConnectionTrait, map: &AccessMap, anchor: Anchor) -> AppResult<()> {
    let scope = crate::wo::access::check_anywhere(map, PM_VIEW)?;
    if anchor == Anchor::Plant {
        return Ok(());
    }
    scope::check_anchor(db, &scope, PM_VIEW, anchor).await
}

/// Where the reader holds `pm.view` (held nowhere: refused as before); filters the lists.
pub fn check_view_anywhere(map: &AccessMap) -> AppResult<AccessScope> {
    crate::wo::access::check_anywhere(map, PM_VIEW)
}

/// Reading the plan `plan_id` and what belongs to it (not found when it does not exist).
pub async fn check_view_plan(db: &impl ConnectionTrait, map: &AccessMap, plan_id: i64) -> AppResult<()> {
    crate::wo::access::check_anywhere(map, PM_VIEW)?;
    let anchor = plan_anchor(db, plan_id).await?;
    check_view_at(db, map, anchor).await
}

/// Reading what a plan targeting `scope_type` / `scope_id` covers (the plan form's previews).
pub async fn check_view_target(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    scope_type: &str,
    scope_id: Option<i64>,
) -> AppResult<()> {
    crate::wo::access::check_anywhere(map, PM_VIEW)?;
    let anchor = target_anchor(db, scope_type, scope_id).await?;
    check_view_at(db, map, anchor).await
}

/// `permission` on the plan `plan_id`: refused as before when the user holds it nowhere, not found
/// when the plan does not exist, then checked at the plan's anchor.
pub async fn check_on_plan(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    plan_id: i64,
) -> AppResult<Anchor> {
    let scope = crate::wo::access::check_anywhere(map, permission)?;
    let anchor = plan_anchor(db, plan_id).await?;
    scope::check_anchor(db, &scope, permission, anchor).await?;
    Ok(anchor)
}

/// `permission` on the plan of the version `version_id` (not found when the version does not exist).
pub async fn check_on_version(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    permission: &str,
    version_id: i64,
) -> AppResult<Anchor> {
    crate::wo::access::check_anywhere(map, permission)?;
    let plan_id = version_plan(db, version_id).await?;
    check_on_plan(db, map, permission, plan_id).await
}

/// A new plan: `pm.create` at its target's anchor (a family or a criticality group: plant-wide
/// roles only).
pub async fn check_create_at(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    scope_type: &str,
    scope_id: Option<i64>,
) -> AppResult<Anchor> {
    let scope = crate::wo::access::check_anywhere(map, PM_CREATE)?;
    let anchor = target_anchor(db, scope_type, scope_id).await?;
    scope::check_anchor(db, &scope, PM_CREATE, anchor).await?;
    Ok(anchor)
}

fn site_change_not_draft(status: &str) -> AppError {
    AppError::ValidationFailed(vec![format!(
        "GATE_PM_SITE_CHANGE_DRAFT_ONLY:status={status}: A plan's location can be changed only while it is a draft; duplicate it to the new location, then retire the old plan."
    )])
}

/// An edit of the plan `plan_id` that may change its target (`new_scope_type` / `new_scope_id`,
/// merged with the stored ones as `queries::update_pm_plan` merges them): `pm.edit` at the plan's
/// anchor; when the anchor changes, the plan must still be a draft and `pm.edit` is needed at the
/// new anchor too. The plan must still be at `expected_row_version` (what was checked is what the
/// update changes: any change in between moves the version and the update is refused). Returns the
/// site change to record, if any.
pub async fn check_plan_update(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    plan_id: i64,
    expected_row_version: i64,
    new_scope_type: Option<&str>,
    new_scope_id: Option<i64>,
) -> AppResult<Option<PlanSiteMove>> {
    let scope = crate::wo::access::check_anywhere(map, PM_EDIT)?;
    let current = plan_target(db, plan_id).await?;
    let old_anchor = target_anchor(db, &current.scope_type, current.scope_id).await?;
    scope::check_anchor(db, &scope, PM_EDIT, old_anchor).await?;
    if current.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: PM plan was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    let new_type = new_scope_type.map_or_else(|| current.scope_type.clone(), |t| t.trim().to_lowercase());
    let new_id = new_scope_id.or(current.scope_id);
    let new_anchor = target_anchor(db, &new_type, new_id).await?;
    if new_anchor == old_anchor {
        return Ok(None);
    }
    if current.lifecycle_status != "draft" {
        return Err(site_change_not_draft(&current.lifecycle_status));
    }
    scope::check_anchor(db, &scope, PM_EDIT, new_anchor).await?;
    Ok(Some(PlanSiteMove {
        old_scope_type: current.scope_type,
        old_scope_id: current.scope_id,
        old_anchor,
        new_scope_type: new_type,
        new_scope_id: new_id,
        new_anchor,
    }))
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

/// [`check_on_plan`], then entitlement and licence.
pub async fn require_on_plan(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    plan_id: i64,
) -> AppResult<()> {
    check_on_plan(db, &access_map(db, cache, user_id).await?, permission, plan_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_on_version`], then entitlement and licence.
pub async fn require_on_version(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    version_id: i64,
) -> AppResult<()> {
    check_on_version(db, &access_map(db, cache, user_id).await?, permission, version_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_create_at`], then entitlement and licence.
pub async fn require_create_at(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    scope_type: &str,
    scope_id: Option<i64>,
) -> AppResult<()> {
    check_create_at(db, &access_map(db, cache, user_id).await?, scope_type, scope_id).await?;
    enforce(db, user_id, PM_CREATE).await
}

/// [`check_plan_update`], then entitlement and licence.
pub async fn require_plan_update(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    plan_id: i64,
    expected_row_version: i64,
    new_scope_type: Option<&str>,
    new_scope_id: Option<i64>,
) -> AppResult<Option<PlanSiteMove>> {
    let site_move = check_plan_update(
        db,
        &access_map(db, cache, user_id).await?,
        plan_id,
        expected_row_version,
        new_scope_type,
        new_scope_id,
    )
    .await?;
    enforce(db, user_id, PM_EDIT).await?;
    Ok(site_move)
}

/// [`check_view_anywhere`], then entitlement and licence: the reader's scope, to filter a list.
pub async fn require_view_anywhere(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
) -> AppResult<AccessScope> {
    let scope = check_view_anywhere(&access_map(db, cache, user_id).await?)?;
    enforce(db, user_id, PM_VIEW).await?;
    Ok(scope)
}

/// [`check_view_plan`], then entitlement and licence.
pub async fn require_view_plan(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    plan_id: i64,
) -> AppResult<()> {
    check_view_plan(db, &access_map(db, cache, user_id).await?, plan_id).await?;
    enforce(db, user_id, PM_VIEW).await
}

/// [`check_view_target`], then entitlement and licence.
pub async fn require_view_target(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    scope_type: &str,
    scope_id: Option<i64>,
) -> AppResult<()> {
    check_view_target(db, &access_map(db, cache, user_id).await?, scope_type, scope_id).await?;
    enforce(db, user_id, PM_VIEW).await
}
