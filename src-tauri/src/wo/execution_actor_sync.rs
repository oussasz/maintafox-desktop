//! Work-order execution changes received by synchronisation, re-checked against who made them
//! (L5-08c, `sync::apply_hooks`).
//!
//! On the PC where it is made, an execution action of a work order needs its right at the work
//! order's site (`ot.edit`, `ot.close` for the close) and its author must carry the work order out
//! (`wo::execution_actor`: its assignee, a member of its team, its planner, or an `ot.approve` holder
//! at its site). A person whose right or assignment was withdrawn on another PC can still act on a
//! PC that has not received the withdrawal. On arrival, the same rule is judged again, **with the
//! rights the author holds here now** (`auth::scope::access_map_now`, the map the local commands
//! use; an account disabled or deleted here holds none) and the work order as written here:
//! - **a status change** (start, hold or pause, resume, "Terminé", close, cancellation), after the
//!   work order and its group are written ([`judge_received_transition`], after
//!   `wo::closeout_sync::judge_closeout`): its author is the person of the status-history row of
//!   the change (`wo_state_transition_log.actor_id`, the transition into the work order's status
//!   now among the rows written by this apply, whatever the time it records). The rule is the
//!   command's: `wo::execution_actor::check_execution_on_wo` (`ot.close` for the close, `ot.edit`
//!   otherwise), `check_cancel_from` for a cancellation (the creator of a draft cancels it, judged
//!   on the status it was cancelled from), `wo::access::check_depannage_at` for the start a
//!   dépannage makes at its creation (`wo::depannage::AUTO_REASON_CODE`, the work order new here;
//!   received on a work order held here, that start keeps the actor rule). A hold the application
//!   makes for a permit (suspended, ended elsewhere, expired: the fixed delay reasons of
//!   `wo::execution`) stops the work and is not judged. Reopening and planning are outside the rule.
//! - **a task result** (a result new or changed, `work_order_tasks`), before it is written
//!   ([`inbound_task_result_check`]): its author is `completed_by`, judged with `ot.edit` and the
//!   actor rule on its work order.
//!
//! Refused: a `WO_EXECUTION_UNAUTHORISED` sync conflict (the work order's whole group, or the task,
//! is rolled back; "take theirs" judges it again), for a person to decide in the Sync centre. A
//! history row that names nobody for a change a person makes: `WO_EXECUTION_AUTHOR_UNKNOWN` (fail
//! closed). An author not applied here yet, or whose rights are still waiting in the inbox: the row
//! waits. A change whose author is not here at all (its history row not received with it: rows
//! recorded before the status history travelled, a page split; or a task result without its author)
//! is applied and recorded in the audit trail (`WO_EXECUTION_AUTHOR_NOT_RECEIVED`), never silently.
//! Snapshot history of a first join is not judged (`CloseoutEntry::judged`).
//!
//! Not judged on arrival (no author recorded on those rows): part, tool and downtime lines and the
//! close-out details (failure coding, root cause); see docs/KNOWN_LIMITS.md.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::json;

use crate::auth::scope::AccessMap;
use crate::errors::{AppError, AppResult};
use crate::permit::sync_check::{conflict, field, local_id, local_user, local_wire, overlay, wait, Payload};
use crate::rbac::permissions::{OT_APPROVE, OT_CLOSE, OT_EDIT};
use crate::settings::site::InboundVerdict;
use crate::sync::materialize::ApplyOutcome;
use crate::wo::closeout_sync::CloseoutEntry;

/// Conflict codes (shown in the sync centre, `sync.conflictKind`).
pub(crate) mod codes {
    /// The author does not hold the right at the work order's site here now, or does not carry it
    /// out (assignee, team, planner, approver).
    pub const UNAUTHORISED: &str = "WO_EXECUTION_UNAUTHORISED";
    /// The history row of a change a person makes names nobody.
    pub const AUTHOR_UNKNOWN: &str = "WO_EXECUTION_AUTHOR_UNKNOWN";
}

/// Audit code of an execution change applied without its author here (see the module rule).
pub(crate) const AUTHOR_NOT_RECEIVED: &str = "WO_EXECUTION_AUTHOR_NOT_RECEIVED";

/// The fields of a task's result (as `wo::closeout_sync`).
const TASK_RESULT_KEYS: &[&str] = &["is_completed", "result_code", "completed_at", "completed_by_sync_id"];

fn sync_err(e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("execution re-check of a received row: {e}"))
}

/// The status history of work order `wo_sync` held here: the highest local id of its rows (0: none;
/// read before a received change is written, so the rows this apply writes have higher ids).
pub(crate) async fn held_transition_max_id(conn: &impl ConnectionTrait, wo_sync: &str) -> AppResult<i64> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(t.id), 0) AS max_id \
               FROM work_orders w JOIN wo_state_transition_log t ON t.wo_id = w.id \
              WHERE w.entity_sync_id = ?",
            [wo_sync.into()],
        ))
        .await?;
    match row {
        Some(row) => row.try_get("", "max_id").map_err(sync_err),
        None => Ok(0),
    }
}

/// What a status change asks of its author.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// `check_execution_on_wo` with this right.
    Execution(&'static str),
    /// `check_cancel_from` (`ot.edit`).
    Cancel,
    /// The start a dépannage makes at its creation (`check_depannage_at`).
    DepannageStart,
}

/// The rule of a status-history row (`None`: not an execution action, or a hold made for a permit).
/// `new_here`: the work order did not exist here before this apply (the dépannage start made at its
/// creation comes only with the work order itself).
fn rule_of(action: &str, reason_code: Option<&str>, is_depannage: bool, new_here: bool) -> Option<Rule> {
    match action {
        "start" if new_here && is_depannage && reason_code == Some(crate::wo::depannage::AUTO_REASON_CODE) => {
            Some(Rule::DepannageStart)
        }
        "hold"
            if reason_code.is_some_and(|r| {
                [
                    crate::wo::execution::PERMIT_SUSPENDED_DELAY_REASON,
                    crate::wo::execution::PERMIT_ENDED_DELAY_REASON,
                    crate::wo::execution::PERMIT_EXPIRED_DELAY_REASON,
                ]
                .contains(&r)
            }) =>
        {
            None
        }
        "start" | "hold" | "resume" | "complete" => Some(Rule::Execution(OT_EDIT)),
        "close" => Some(Rule::Execution(OT_CLOSE)),
        "cancel" => Some(Rule::Cancel),
        _ => None,
    }
}

fn permission_of(rule: Rule) -> &'static str {
    match rule {
        Rule::Execution(p) => p,
        Rule::Cancel | Rule::DepannageStart => OT_EDIT,
    }
}

/// A refusal of the rule (`Some`: why), the database errors passed on.
fn refused(result: AppResult<()>) -> AppResult<Option<String>> {
    match result {
        Ok(()) => Ok(None),
        Err(
            e @ (AppError::PermissionDenied(_)
            | AppError::Permission { .. }
            | AppError::ValidationFailed(_)
            | AppError::OrgValidationFailed(_)),
        ) => Ok(Some(e.to_string())),
        Err(other) => Err(other),
    }
}

/// How a received execution change is judged.
enum Judgement {
    Allowed,
    /// The author or their rights are still arriving.
    Wait(String),
    Refused(String),
}

/// `author` (local id), judged with the rights held here now on work order `wo_id` as written here.
async fn judge_author(
    conn: &impl ConnectionTrait,
    author: i64,
    wo_id: i64,
    rule: Rule,
    from_status: Option<&str>,
) -> AppResult<Judgement> {
    let account = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id, COALESCE(is_active, 0) AS active, deleted_at, \
                    COALESCE(NULLIF(TRIM(display_name), ''), username) AS name \
               FROM user_accounts WHERE id = ?",
            [author.into()],
        ))
        .await?;
    let Some(account) = account else {
        return Ok(Judgement::Refused(format!(
            "its author (n° {author}) is not an account here"
        )));
    };
    let author_sync: Option<String> = account.try_get("", "entity_sync_id").map_err(sync_err)?;
    let name: Option<String> = account.try_get("", "name").map_err(sync_err)?;
    let name = name.unwrap_or_else(|| format!("n° {author}"));
    let active: i64 = account.try_get("", "active").map_err(sync_err)?;
    let deleted: Option<String> = account.try_get("", "deleted_at").map_err(sync_err)?;
    // An account disabled or deleted here holds no right now.
    if active == 0 || deleted.is_some() {
        return Ok(Judgement::Refused(format!(
            "its author {name} has no active account here now"
        )));
    }
    let Ok(user) = i32::try_from(author) else {
        return Ok(Judgement::Refused(format!(
            "its author {name} is not a valid account here"
        )));
    };
    let map: AccessMap = crate::auth::scope::access_map_now(conn, user).await?;
    let permission = permission_of(rule);
    let outcome = match rule {
        Rule::Execution(p) => crate::wo::execution_actor::check_execution_on_wo(conn, &map, author, p, wo_id).await,
        Rule::Cancel => {
            crate::wo::execution_actor::check_cancel_from(
                conn,
                &map,
                author,
                OT_EDIT,
                wo_id,
                from_status.unwrap_or_default(),
            )
            .await
        }
        Rule::DepannageStart => depannage_start(conn, &map, author, wo_id).await,
    };
    let Some(why) = refused(outcome)? else {
        return Ok(Judgement::Allowed);
    };
    // Rights still waiting in the inbox (a role given before the change, not applied yet).
    if let Some(sync) = author_sync.as_deref() {
        for p in [permission, OT_APPROVE] {
            if crate::permit::sync_check::rights_may_still_arrive(conn, sync, p).await? {
                return Ok(Judgement::Wait(format!(
                    "the rights of its author {name} are still arriving"
                )));
            }
        }
    }
    Ok(Judgement::Refused(format!("its author {name}: {why}")))
}

/// The dépannage rule (`wo::access::check_depannage_at`) on the work order as written: created and
/// started by `author`, assigned to someone else only with `ot.emergency_assign`.
async fn depannage_start(conn: &impl ConnectionTrait, map: &AccessMap, author: i64, wo_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_id, equipment_id, primary_responsible_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| sync_err("work order vanished after its apply"))?;
    let entity: Option<i64> = row.try_get("", "entity_id").map_err(sync_err)?;
    let equipment: Option<i64> = row.try_get("", "equipment_id").map_err(sync_err)?;
    let assignee: Option<i64> = row.try_get("", "primary_responsible_id").map_err(sync_err)?;
    let to_other = assignee.is_some_and(|a| a > 0 && a != author);
    crate::wo::access::check_depannage_at(conn, map, entity, equipment, to_other).await
}

/// Records an execution change applied without its author here (see the module rule).
async fn record_author_not_received(
    conn: &impl ConnectionTrait,
    target_type: &str,
    target_id: i64,
    details: serde_json::Value,
) -> AppResult<()> {
    tracing::warn!(target_type, target_id, %details, "received execution change applied without its author here");
    crate::audit::writer::write_audit_event_in(
        conn,
        crate::audit::writer::AuditEventInput {
            action_code: AUTHOR_NOT_RECEIVED.to_string(),
            target_type: Some(target_type.to_string()),
            target_id: Some(target_id.to_string()),
            actor_id: None,
            auth_context: "system".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "security".to_string(),
            details_json: Some(details),
        },
    )
    .await?;
    Ok(())
}

/// A received status change of a work order, after it and its group were written (same
/// transaction): its author judged with the rights held here now (see the module rule). `Some` (a
/// conflict, or a wait) rolls everything back.
pub(crate) async fn judge_received_transition(
    conn: &impl ConnectionTrait,
    entry: &CloseoutEntry,
) -> AppResult<Option<ApplyOutcome>> {
    if !entry.judged {
        return Ok(None);
    }
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT w.id, w.code, COALESCE(w.is_depannage, 0) AS is_depannage, s.code AS status \
               FROM work_orders w JOIN work_order_statuses s ON s.id = w.status_id \
              WHERE w.entity_sync_id = ?",
            [entry.wo_sync_id.as_str().into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let wo_id: i64 = row.try_get("", "id").map_err(sync_err)?;
    let code: Option<String> = row.try_get("", "code").map_err(sync_err)?;
    let code = code.unwrap_or_else(|| entry.wo_sync_id.clone());
    let is_depannage: i64 = row.try_get("", "is_depannage").map_err(sync_err)?;
    let status: String = row.try_get("", "status").map_err(sync_err)?;
    if entry.previous_status.as_deref() == Some(status.as_str()) && !matches!(status.as_str(), "completed" | "closed") {
        return Ok(None);
    }
    // The transition into the status now among the rows written by this apply (a row held here
    // before belongs to an earlier change), the latest by its time: a backdated one is judged too.
    let transition = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, action, from_status, actor_id, reason_code, acted_at \
               FROM wo_state_transition_log WHERE wo_id = ? AND to_status = ? AND id > ? \
              ORDER BY julianday(acted_at) DESC, id DESC LIMIT 1",
            [
                wo_id.into(),
                status.as_str().into(),
                entry.held_transition_max_id.into(),
            ],
        ))
        .await?;
    let transition = match transition {
        Some(t) => t,
        None => {
            // Its history row is not here (see the module rule): applied, recorded.
            record_author_not_received(
                conn,
                "work_order",
                wo_id,
                json!({
                    "work_order_sync_id": entry.wo_sync_id,
                    "status": status,
                    "previous_status": entry.previous_status,
                    "reason": "transition_not_received",
                }),
            )
            .await?;
            return Ok(None);
        }
    };
    let action: String = transition.try_get("", "action").map_err(sync_err)?;
    let reason: Option<String> = transition.try_get("", "reason_code").map_err(sync_err)?;
    let Some(rule) = rule_of(
        &action,
        reason.as_deref(),
        is_depannage != 0,
        entry.previous_status.is_none(),
    ) else {
        return Ok(None);
    };
    let from_status: Option<String> = transition.try_get("", "from_status").map_err(sync_err)?;
    let acted_at: Option<String> = transition.try_get("", "acted_at").map_err(sync_err)?;
    let what = format!(
        "work order {code}: '{action}' to '{status}' made on {}",
        acted_at.as_deref().unwrap_or("?")
    );
    let Some(author) = transition.try_get::<Option<i64>>("", "actor_id").map_err(sync_err)? else {
        return Ok(Some(ApplyOutcome::Conflict {
            code: codes::AUTHOR_UNKNOWN.into(),
            message: format!("{what}: its status history does not say who made it"),
        }));
    };
    Ok(
        match judge_author(conn, author, wo_id, rule, from_status.as_deref()).await? {
            Judgement::Allowed => None,
            Judgement::Wait(why) => crate::sync::grant_check::wait(format!("{what}: {why}")),
            Judgement::Refused(why) => Some(ApplyOutcome::Conflict {
                code: codes::UNAUTHORISED.into(),
                message: format!("{what}, refused here: {why}"),
            }),
        },
    )
}

fn result_changed(local: Option<&Payload>, row: &Payload) -> bool {
    TASK_RESULT_KEYS.iter().any(|k| {
        local.and_then(|l| l.get(*k)).unwrap_or(&serde_json::Value::Null)
            != row.get(*k).unwrap_or(&serde_json::Value::Null)
    })
}

fn completed(p: &Payload) -> bool {
    crate::sync::grant_check::truthy(p, "is_completed")
}

/// A received task whose result is new or changed: its author (`completed_by`) judged with
/// `ot.edit` and the actor rule on its work order, with the rights held here now (see the module
/// rule). Before the task is written.
pub(crate) async fn inbound_task_result_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    let local = local_wire(conn, "work_order_tasks", entity_sync_id).await?;
    let row = overlay(local.as_ref(), received);
    if !completed(&row) || !result_changed(local.as_ref(), &row) {
        return Ok(None);
    }
    let Some(wo_sync) = field(&row, "work_order_sync_id").map(str::to_string) else {
        return Ok(None);
    };
    // First-join snapshot history (its work order's decision): applied as the plant holds it.
    if crate::sync::materialize::applying_snapshot_row()
        && crate::permit::sync_check::group_parent_judged(&wo_sync) == Some(false)
    {
        return Ok(None);
    }
    let Some(wo_id) = local_id(conn, "work_orders", &wo_sync).await? else {
        // Its work order is not here: the apply waits for it as for any row.
        return Ok(None);
    };
    let what = format!("task {entity_sync_id} of work order {wo_sync}: result");
    let Some(author_sync) = field(&row, "completed_by_sync_id").map(str::to_string) else {
        let task_id = local_id(conn, "work_order_tasks", entity_sync_id).await?.unwrap_or(0);
        record_author_not_received(
            conn,
            "work_order_task",
            task_id,
            json!({
                "task_sync_id": entity_sync_id,
                "work_order_sync_id": wo_sync,
                "reason": "result_author_not_received",
            }),
        )
        .await?;
        return Ok(None);
    };
    let Some(author) = local_user(conn, &author_sync).await? else {
        return wait(format!("{what}: its author {author_sync} is not applied here yet"));
    };
    match judge_author(conn, author, wo_id, Rule::Execution(OT_EDIT), None).await? {
        Judgement::Allowed => Ok(None),
        Judgement::Wait(why) => wait(format!("{what}: {why}")),
        Judgement::Refused(why) => conflict(codes::UNAUTHORISED, format!("{what}, refused here: {why}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{rule_of, Rule};
    use crate::rbac::permissions::{OT_CLOSE, OT_EDIT};

    #[test]
    fn the_rule_of_each_status_change() {
        assert_eq!(rule_of("start", None, false, false), Some(Rule::Execution(OT_EDIT)));
        assert_eq!(rule_of("resume", None, false, false), Some(Rule::Execution(OT_EDIT)));
        assert_eq!(
            rule_of("hold", Some("waiting_parts"), false, false),
            Some(Rule::Execution(OT_EDIT))
        );
        assert_eq!(rule_of("complete", None, false, false), Some(Rule::Execution(OT_EDIT)));
        assert_eq!(rule_of("close", None, false, false), Some(Rule::Execution(OT_CLOSE)));
        assert_eq!(rule_of("cancel", None, false, false), Some(Rule::Cancel));
        // The start a dépannage makes at its creation; a later normal start keeps the actor rule.
        assert_eq!(
            rule_of("start", Some(crate::wo::depannage::AUTO_REASON_CODE), true, true),
            Some(Rule::DepannageStart)
        );
        // Received on a work order held here: that start keeps the actor rule.
        assert_eq!(
            rule_of("start", Some(crate::wo::depannage::AUTO_REASON_CODE), true, false),
            Some(Rule::Execution(OT_EDIT))
        );
        assert_eq!(rule_of("start", None, true, false), Some(Rule::Execution(OT_EDIT)));
        // A hold the permit made stops the work: not judged.
        for reason in [
            crate::wo::execution::PERMIT_SUSPENDED_DELAY_REASON,
            crate::wo::execution::PERMIT_ENDED_DELAY_REASON,
            crate::wo::execution::PERMIT_EXPIRED_DELAY_REASON,
        ] {
            assert_eq!(rule_of("hold", Some(reason), false, false), None);
        }
        // Planning and reopening are outside the rule.
        for action in ["create", "submit", "mark_ready", "reopen", "approve"] {
            assert_eq!(rule_of(action, None, false, false), None);
        }
    }
}
