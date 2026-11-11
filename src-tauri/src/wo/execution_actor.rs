//! Who may carry out a work order (L5-08, CMMS Advisor decision of 2026-11-05).
//!
//! The execution actions of a work order — start, pause, resume, hold, part and tool use,
//! downtime entries, task results, the close-out details (failure coding, root cause, service
//! cost), "Terminé" (mechanical completion), close and cancel — are for its **execution actors**
//! only (CMMS Advisor answers of 2026-11-06 for start … downtime):
//! - its assignee (`primary_responsible_id`, the "responsable principal" the assignment names;
//!   the model has one person field, so the assignee and the responsible are the same person);
//! - a member of its assigned team (`assigned_team_id`): a crew assignment in force now
//!   (`personnel_team_assignments`, the person's login active), as the notifications count the
//!   members of a team;
//! - its planner (`planner_id`);
//! - a holder of `ot.approve` at the work order's site (the site rule of `wo::access`: plant-wide,
//!   or on the work order's org node or above; a work order without a node: plant-wide only).
//!
//! Anyone else holding the action's right at the site sees the work order read-only, except their
//! **own** labour ([`check_labor_for`]), as on mobile. A work order with no assignee and no team is
//! carried out by its planner or an `ot.approve` holder only (fail safe: nobody is its actor by
//! default). One exception: the creator of a work order not yet released (a draft,
//! `WoStatus::is_released`) may cancel it ([`check_cancel`]).
//!
//! Outside the rule: the technical verification (`save_verification`, by design not the
//! responsible's, R6), the store's issue desk (`issue_wo_part_to_issue`, `inv.manage`), and
//! planning (dates, assignment, priority).
//!
//! The rule only narrows: each command checks its own right first (`ot.close` for the close, the
//! edit right for the others), then this rule. Planning (dates, assignment, priority) keeps the
//! site rule alone.
//!
//! A refusal names the assignee or the team (`GATE_WO_EXECUTION_*`, with the names as parameters:
//! the screen says it in the user's language, and greys the buttons with the same text from
//! [`standing`]).

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use tokio::sync::RwLock;

use crate::auth::scope::{self, AccessMap, AccessScope, Anchor};
use crate::errors::{AppError, AppResult};
use crate::rbac::cache::PermissionCache;
use crate::rbac::permissions::{OT_APPROVE, PLAN_CONFIRM};
use crate::wo::access::{self, WoChild};
use crate::wo::workflow::WoStatus;

/// Refusal codes (texts in `errors:gateError`).
pub mod codes {
    /// Assigned to a person (no team).
    pub const ASSIGNED_PERSON: &str = "GATE_WO_EXECUTION_ASSIGNED_PERSON";
    /// Assigned to a team (no person).
    pub const ASSIGNED_TEAM: &str = "GATE_WO_EXECUTION_ASSIGNED_TEAM";
    /// Assigned to a person and a team.
    pub const ASSIGNED_PERSON_TEAM: &str = "GATE_WO_EXECUTION_ASSIGNED_PERSON_TEAM";
    /// Assigned to nobody: its planner or an approver only.
    pub const UNASSIGNED: &str = "GATE_WO_EXECUTION_UNASSIGNED";
    /// Choosing the planner, the assignee or the team without `ot.approve` / `plan.confirm`.
    pub const PEOPLE_RIGHT_REQUIRED: &str = "GATE_WO_PEOPLE_RIGHT_REQUIRED";
}

/// Why the user is an execution actor of the work order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorRole {
    Assignee,
    TeamMember,
    Planner,
    Approver,
}

/// Why the user is not: the refusal code and its parameters (`assignee`, `team`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotActor {
    pub code: String,
    pub params: HashMap<String, String>,
}

impl NotActor {
    /// The refusal the commands return (coded, with the names; French sentence as fallback).
    pub fn into_error(self) -> AppError {
        let message = match self.code.as_str() {
            codes::ASSIGNED_PERSON => format!(
                "OT assigné à {} — seule la saisie de vos propres heures est possible.",
                self.params.get("assignee").map_or("", String::as_str)
            ),
            codes::ASSIGNED_TEAM => format!(
                "OT assigné à l'équipe {} — seule la saisie de vos propres heures est possible.",
                self.params.get("team").map_or("", String::as_str)
            ),
            codes::ASSIGNED_PERSON_TEAM => format!(
                "OT assigné à {} (équipe {}) — seule la saisie de vos propres heures est possible.",
                self.params.get("assignee").map_or("", String::as_str),
                self.params.get("team").map_or("", String::as_str)
            ),
            _ => "OT assigné à personne : seuls son planificateur et les approbateurs du site l'exécutent ; \
                  seule la saisie de vos propres heures est possible."
                .to_string(),
        };
        crate::errors::org_validation_failed(&self.code, format!("{}: {message}", self.code), self.params)
    }
}

/// The user's standing on a work order's execution (`get_wo_execution_standing`, the screens).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionStanding {
    /// `Some` when the user may carry out the work order (the first reason found).
    pub actor: Option<ActorRole>,
    /// `Some` when they may not: the reason the screen shows on the greyed actions.
    pub refusal: Option<NotActor>,
    /// The user created this work order and it is not released yet: they may cancel it.
    pub own_draft: bool,
}

impl ExecutionStanding {
    pub fn allowed(&self) -> bool {
        self.actor.is_some()
    }
}

struct WoPeople {
    anchor: Anchor,
    /// Its creator (`requester_id`, the signed-in user who created it).
    creator: Option<i64>,
    /// Not yet released (a draft), from the status model.
    unreleased: bool,
    responsible: Option<i64>,
    responsible_name: Option<String>,
    planner: Option<i64>,
    team: Option<i64>,
    team_name: Option<String>,
}

async fn wo_people(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoPeople> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.entity_id, wo.primary_responsible_id, wo.planner_id, wo.assigned_team_id, \
                    wo.requester_id, ws.code AS status_code, \
                    COALESCE(NULLIF(TRIM(ur.display_name), ''), ur.username) AS responsible_name, \
                    tm.name AS team_name \
               FROM work_orders wo \
               LEFT JOIN work_order_statuses ws ON ws.id = wo.status_id \
               LEFT JOIN user_accounts ur ON ur.id = wo.primary_responsible_id \
               LEFT JOIN teams tm ON tm.id = wo.assigned_team_id \
              WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let positive = |v: Option<i64>| v.filter(|&id| id > 0);
    let status: Option<String> = row.try_get("", "status_code")?;
    // An unknown status counts as released (fail safe: no creator exception).
    let unreleased = status
        .as_deref()
        .and_then(|code| WoStatus::try_from_str(code).ok())
        .is_some_and(|st| !st.is_released());
    Ok(WoPeople {
        anchor: Anchor::of(row.try_get("", "entity_id")?),
        creator: positive(row.try_get("", "requester_id")?),
        unreleased,
        responsible: positive(row.try_get("", "primary_responsible_id")?),
        responsible_name: row.try_get("", "responsible_name")?,
        planner: positive(row.try_get("", "planner_id")?),
        team: positive(row.try_get("", "assigned_team_id")?),
        team_name: row.try_get("", "team_name")?,
    })
}

/// Whether the login `user_id` (active) is a member of `team_id` now: a crew assignment in force
/// (the assignment table holds both plain dates and full timestamps, `personnel::queries`).
async fn is_team_member(db: &impl ConnectionTrait, user_id: i64, team_id: i64) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS x FROM personnel_team_assignments x \
                   JOIN user_accounts ua ON ua.personnel_id = x.personnel_id \
                  WHERE ua.id = ? AND x.team_id = ? \
                    AND ua.is_active = 1 AND ua.deleted_at IS NULL \
                    AND {IN_FORCE_NOW_SQL} \
                  LIMIT 1"
            ),
            [user_id.into(), team_id.into()],
        ))
        .await?
        .is_some())
}

/// A crew assignment (alias `x`) in force now. A plain date (`YYYY-MM-DD`) counts for that whole
/// day: from its start for `valid_from`, to its end for `valid_to`, as `personnel::queries` reads
/// them (`date(...)`). A timestamp (the end written when someone leaves a crew) counts to the
/// instant. Read with `julianday`, so `T`/space and a trailing `Z` compare alike.
const IN_FORCE_NOW_SQL: &str = "(x.valid_from IS NULL OR TRIM(x.valid_from) = '' \
       OR (LENGTH(TRIM(x.valid_from)) <= 10 AND date(x.valid_from) <= date('now')) \
       OR (LENGTH(TRIM(x.valid_from)) > 10 AND julianday(x.valid_from) <= julianday('now'))) \
   AND (x.valid_to IS NULL OR TRIM(x.valid_to) = '' \
       OR (LENGTH(TRIM(x.valid_to)) <= 10 AND date(x.valid_to) >= date('now')) \
       OR (LENGTH(TRIM(x.valid_to)) > 10 AND julianday(x.valid_to) >= julianday('now')))";

/// Whether `scope` covers the anchor (the site rule of `scope::check_anchor`, as a yes / no).
async fn scope_covers(db: &impl ConnectionTrait, scope: &AccessScope, anchor: Anchor) -> AppResult<bool> {
    if scope.is_everywhere() {
        return Ok(true);
    }
    match anchor {
        Anchor::Node(node) => scope::covers(db, scope, node).await,
        Anchor::Unknown | Anchor::Plant => Ok(false),
    }
}

/// The user's standing on the work order `wo_id` (not found when it does not exist). `map` is the
/// user's rights now.
pub async fn standing(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    wo_id: i64,
) -> AppResult<ExecutionStanding> {
    standing_at(db, map, user_id, wo_id, None).await
}

/// [`standing`], the creator's draft exception judged on `released_from` when given: the status the
/// work order had before a change already written (a cancellation received by synchronisation,
/// `wo::execution_actor_sync`); an unknown status counts as released (fail safe).
async fn standing_at(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    wo_id: i64,
    released_from: Option<&str>,
) -> AppResult<ExecutionStanding> {
    let mut wo = wo_people(db, wo_id).await?;
    if let Some(code) = released_from {
        wo.unreleased = WoStatus::try_from_str(code).is_ok_and(|st| !st.is_released());
    }
    let own_draft = wo.unreleased && wo.creator == Some(user_id);
    let allowed = |role| ExecutionStanding {
        actor: Some(role),
        refusal: None,
        own_draft,
    };
    if wo.responsible == Some(user_id) {
        return Ok(allowed(ActorRole::Assignee));
    }
    if wo.planner == Some(user_id) {
        return Ok(allowed(ActorRole::Planner));
    }
    if let Some(team) = wo.team {
        if is_team_member(db, user_id, team).await? {
            return Ok(allowed(ActorRole::TeamMember));
        }
    }
    if scope_covers(db, &map.scope_of(OT_APPROVE), wo.anchor).await? {
        return Ok(allowed(ActorRole::Approver));
    }
    // A name the join cannot give (an account or team deleted since): its number.
    let assignee = wo
        .responsible
        .map(|id| wo.responsible_name.clone().unwrap_or_else(|| format!("n° {id}")));
    let team = wo
        .team
        .map(|id| wo.team_name.clone().unwrap_or_else(|| format!("n° {id}")));
    let (code, params) = match (assignee, team) {
        (Some(a), Some(t)) => (
            codes::ASSIGNED_PERSON_TEAM,
            HashMap::from([("assignee".to_string(), a), ("team".to_string(), t)]),
        ),
        (Some(a), None) => (codes::ASSIGNED_PERSON, HashMap::from([("assignee".to_string(), a)])),
        (None, Some(t)) => (codes::ASSIGNED_TEAM, HashMap::from([("team".to_string(), t)])),
        (None, None) => (codes::UNASSIGNED, HashMap::new()),
    };
    Ok(ExecutionStanding {
        actor: None,
        refusal: Some(NotActor {
            code: code.to_string(),
            params,
        }),
        own_draft,
    })
}

/// Refuses unless the user is an execution actor of the work order.
pub async fn check_actor(db: &impl ConnectionTrait, map: &AccessMap, user_id: i64, wo_id: i64) -> AppResult<()> {
    match standing(db, map, user_id, wo_id).await?.refusal {
        Some(refusal) => Err(refusal.into_error()),
        None => Ok(()),
    }
}

/// An execution action on the work order: `permission` there (`wo::access::check_on_wo`, refused
/// as before without it), then the actor rule. The rule never stands in for the right.
pub async fn check_execution_on_wo(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    permission: &str,
    wo_id: i64,
) -> AppResult<()> {
    access::check_on_wo(db, map, permission, wo_id).await?;
    check_actor(db, map, user_id, wo_id).await
}

/// Cancelling the work order: `permission` there (`wo::access::check_on_wo`), then the actor
/// rule, except for its creator while it is not released (CMMS Advisor 2026-11-06, Q2).
pub async fn check_cancel(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    permission: &str,
    wo_id: i64,
) -> AppResult<()> {
    cancel_rule(db, map, user_id, permission, wo_id, None).await
}

/// [`check_cancel`] of a cancellation already written (received by synchronisation): the creator's
/// draft exception is judged on `from_status`, the status the work order was cancelled from.
pub(crate) async fn check_cancel_from(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    permission: &str,
    wo_id: i64,
    from_status: &str,
) -> AppResult<()> {
    cancel_rule(db, map, user_id, permission, wo_id, Some(from_status)).await
}

async fn cancel_rule(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    permission: &str,
    wo_id: i64,
    released_from: Option<&str>,
) -> AppResult<()> {
    access::check_on_wo(db, map, permission, wo_id).await?;
    let standing = standing_at(db, map, user_id, wo_id, released_from).await?;
    match standing.refusal {
        Some(refusal) if !standing.own_draft => Err(refusal.into_error()),
        _ => Ok(()),
    }
}

/// An execution action on a record of the work order (a task result, a part or tool use, a
/// downtime segment): `permission` on its work order (`wo::access::check_on_child`), then the
/// actor rule on that work order.
pub async fn check_execution_on_child(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    permission: &str,
    child: WoChild,
    id: i64,
) -> AppResult<()> {
    access::check_on_child(db, map, permission, child, id).await?;
    let wo_id = access::child_wo_id(db, child, id).await?;
    check_actor(db, map, user_id, wo_id).await
}

/// The labour of someone who is not an execution actor: their own only (as on mobile). Called
/// after the command's own right check; an execution actor keeps the command's rules for booking
/// other people. `booked_user` / `booked_personnel`: the login and the register card the entry is
/// for (either may be unknown).
pub async fn check_labor_for(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    wo_id: i64,
    booked_user: Option<i64>,
    booked_personnel: Option<i64>,
) -> AppResult<()> {
    let standing = standing(db, map, user_id, wo_id).await?;
    let Some(refusal) = standing.refusal else {
        return Ok(());
    };
    if booked_user == Some(user_id) {
        return Ok(());
    }
    if let Some(personnel) = booked_personnel {
        if crate::qualification::gate::personnel_for_user(db, user_id).await? == Some(personnel) {
            return Ok(());
        }
    }
    Err(refusal.into_error())
}

/// [`check_labor_for`] on an existing labour entry (closing it): the person it is for.
pub async fn check_labor_entry(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    user_id: i64,
    entry_id: i64,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT work_order_id, intervener_id, personnel_id FROM work_order_interveners WHERE id = ?",
            [entry_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoIntervener".into(),
            id: entry_id.to_string(),
        })?;
    let wo_id: i64 = row.try_get("", "work_order_id")?;
    let booked_user: Option<i64> = row.try_get("", "intervener_id")?;
    let booked_personnel: Option<i64> = row.try_get("", "personnel_id")?;
    check_labor_for(db, map, user_id, wo_id, booked_user, booked_personnel).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Who is named on the work order (CMMS Advisor 2026-11-06)
// ═══════════════════════════════════════════════════════════════════════════════

/// Whether the user may choose the planner, the assignee or the team of a work order anchored at
/// `anchor`: `ot.approve` or `plan.confirm` there (the site rule of the actor check). Without it,
/// someone could make themselves an execution actor by naming themselves.
pub async fn may_choose_people(db: &impl ConnectionTrait, map: &AccessMap, anchor: Anchor) -> AppResult<bool> {
    for permission in [OT_APPROVE, PLAN_CONFIRM] {
        if scope_covers(db, &map.scope_of(permission), anchor).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn people_refusal() -> AppError {
    // The code is written out in the literal (it equals `codes::PEOPLE_RIGHT_REQUIRED`) so the
    // gate-code checks can read it.
    AppError::ValidationFailed(vec![
        "GATE_WO_PEOPLE_RIGHT_REQUIRED: Seul un approbateur ou un planificateur peut choisir le responsable, l'équipe ou le planificateur de l'OT."
            .to_string(),
    ])
}

/// A planner given at creation: refused without the right at the new work order's site (none
/// given: nothing to check; the system flows create without one). Exceptions to come with L1-05
/// (not on this branch): the dépannage self-assignment, assigning others through
/// `ot.emergency_assign`, and the dépannage "join".
pub async fn check_people_on_create(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    entity_id: Option<i64>,
    equipment_id: Option<i64>,
    planner_id: Option<i64>,
) -> AppResult<()> {
    if planner_id.is_none() {
        return Ok(());
    }
    let anchor = Anchor::of(access::new_wo_node(db, entity_id, equipment_id).await?);
    if may_choose_people(db, map, anchor).await? {
        Ok(())
    } else {
        Err(people_refusal())
    }
}

/// A planner set on a work order (`plan_wo`): none sent, or an unchanged value, passes (the dates,
/// duration and priority are saved without naming one, CMMS Advisor 2026-11-06 option b); a change
/// needs the right at the work order's site. Exceptions to come with L1-05 (not on this branch): the dépannage
/// self-assignment, assigning others through `ot.emergency_assign`, and the dépannage "join".
pub async fn check_planner_change(
    db: &impl ConnectionTrait,
    map: &AccessMap,
    wo_id: i64,
    planner_id: Option<i64>,
) -> AppResult<()> {
    // None (or `0`): the recorded planner is kept, an empty one stays empty (option b).
    let Some(planner_id) = planner_id.filter(|&id| id > 0) else {
        return Ok(());
    };
    let wo = wo_people(db, wo_id).await?;
    if Some(planner_id) == wo.planner {
        return Ok(());
    }
    if may_choose_people(db, map, wo.anchor).await? {
        Ok(())
    } else {
        Err(people_refusal())
    }
}

/// [`check_people_on_create`] with the user's cached rights.
pub async fn require_people_on_create(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    entity_id: Option<i64>,
    equipment_id: Option<i64>,
    planner_id: Option<i64>,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    check_people_on_create(db, &map, entity_id, equipment_id, planner_id).await
}

/// [`check_planner_change`] with the user's cached rights.
pub async fn require_planner_change(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    wo_id: i64,
    planner_id: Option<i64>,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    check_planner_change(db, &map, wo_id, planner_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Command forms: the user's cached rights, the check, then entitlement and licence (as
// `wo::access::require_on_wo`)
// ═══════════════════════════════════════════════════════════════════════════════

async fn access_map(db: &DatabaseConnection, cache: &RwLock<PermissionCache>, user_id: i32) -> AppResult<AccessMap> {
    scope::access_map_cached(db, cache, user_id).await
}

async fn enforce(db: &DatabaseConnection, user_id: i32, permission: &str) -> AppResult<()> {
    crate::entitlements::queries::enforce_capability_for_permission(db, permission).await?;
    crate::license::queries::enforce_permission_matrix(db, user_id, permission).await
}

/// [`check_execution_on_wo`], then entitlement and licence of `permission`.
pub async fn require_execution_on_wo(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    wo_id: i64,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    check_execution_on_wo(db, &map, i64::from(user_id), permission, wo_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_execution_on_child`], then entitlement and licence of `permission`.
pub async fn require_execution_on_child(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    child: WoChild,
    id: i64,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    check_execution_on_child(db, &map, i64::from(user_id), permission, child, id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_cancel`], then entitlement and licence of `permission`.
pub async fn require_cancel(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    permission: &str,
    wo_id: i64,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    check_cancel(db, &map, i64::from(user_id), permission, wo_id).await?;
    enforce(db, user_id, permission).await
}

/// [`check_labor_for`] with the user's cached rights (after the command's own right check).
pub async fn require_labor_for(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    wo_id: i64,
    booked_user: Option<i64>,
    booked_personnel: Option<i64>,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    check_labor_for(db, &map, i64::from(user_id), wo_id, booked_user, booked_personnel).await
}

/// [`check_labor_entry`] with the user's cached rights (after the command's own right check).
pub async fn require_labor_entry(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    entry_id: i64,
) -> AppResult<()> {
    let map = access_map(db, cache, user_id).await?;
    check_labor_entry(db, &map, i64::from(user_id), entry_id).await
}

/// [`standing`] with the user's cached rights (the screens).
pub async fn standing_of(
    db: &DatabaseConnection,
    cache: &RwLock<PermissionCache>,
    user_id: i32,
    wo_id: i64,
) -> AppResult<ExecutionStanding> {
    let map = access_map(db, cache, user_id).await?;
    standing(db, &map, i64::from(user_id), wo_id).await
}
