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
use crate::rbac::permissions::OT_APPROVE;
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
                "OT assigné à {} — seule la saisie de vos heures et d'une note est possible.",
                self.params.get("assignee").map_or("", String::as_str)
            ),
            codes::ASSIGNED_TEAM => format!(
                "OT assigné à l'équipe {} — seule la saisie de vos heures et d'une note est possible.",
                self.params.get("team").map_or("", String::as_str)
            ),
            codes::ASSIGNED_PERSON_TEAM => format!(
                "OT assigné à {} (équipe {}) — seule la saisie de vos heures et d'une note est possible.",
                self.params.get("assignee").map_or("", String::as_str),
                self.params.get("team").map_or("", String::as_str)
            ),
            _ => "OT assigné à personne : seuls son planificateur et les approbateurs du site l'exécutent ; \
                  seule la saisie de vos heures et d'une note est possible."
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
/// (`notifications::router::team_member_user_ids`, same clock and validity rule).
async fn is_team_member(db: &impl ConnectionTrait, user_id: i64, team_id: i64) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM personnel_team_assignments x \
               JOIN user_accounts ua ON ua.personnel_id = x.personnel_id \
              WHERE ua.id = ? AND x.team_id = ? \
                AND ua.is_active = 1 AND ua.deleted_at IS NULL \
                AND (x.valid_from IS NULL OR x.valid_from <= strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
                AND (x.valid_to IS NULL OR x.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
              LIMIT 1",
            [user_id.into(), team_id.into()],
        ))
        .await?
        .is_some())
}

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
    let wo = wo_people(db, wo_id).await?;
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
    access::check_on_wo(db, map, permission, wo_id).await?;
    let standing = standing(db, map, user_id, wo_id).await?;
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
