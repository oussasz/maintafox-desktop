//! "Rejoindre": join a work order in progress or on hold (CMMS Advisor decision 2026-11-11,
//! option b; L1-05 / L5-08).
//!
//! Anyone holding `ot.edit` at the work order's site (checked by the command) may join any work
//! order in progress or on hold — a dépannage or not — to help: joining starts their own labour
//! entry now (`wo::labor::add_labor_entry`, open until they close it), which is what a person who
//! does not carry out the work order may always book (`wo::execution_actor::check_labor_for`). The
//! joiner does not become an execution actor: start, pause, completion and close stay with the
//! assignee, the team, the planner and the site approvers; to carry it out, a person is assigned.
//!
//! - Refused while the joiner already has labour open on the work order ([`CODE_ALREADY_ON_IT`]);
//!   the status is checked by the labour booking (in progress or on hold only).
//! - The assignee is told "X a rejoint" ([`JOINED_MESSAGE_KEY`], category `wo_assigned`, routed to
//!   the work order's assignee), unless the joiner is the assignee or there is none.
//!
//! A per-work-order list of helpers (option a) is left to a later version.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit_event_with, NotificationEventInput, NotificationMessage};
use crate::wo::labor::{add_labor_entry, AddLaborInput, WoIntervener};
use crate::wo::queries::get_work_order;

/// Refusal: the joiner already has labour open on this work order.
pub const CODE_ALREADY_ON_IT: &str = "GATE_WO_JOIN_ALREADY_ON_IT";
/// Notification category of the notice (routed to the work order's assignee).
pub const JOINED_CATEGORY: &str = "wo_assigned";
/// Message of the notice "X a rejoint l'OT".
pub const JOINED_MESSAGE_KEY: &str = "wo_joined";

#[derive(Debug, Clone, Deserialize)]
pub struct WoJoinInput {
    pub wo_id: i64,
    /// The signed-in user (set by the command).
    #[serde(default)]
    pub actor_id: i64,
}

fn read_err(e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("wo join: {e}"))
}

/// Open labour entries of `user` on `wo_id` (started, not ended), those older than `before_id`
/// only when it is given.
async fn open_entries(db: &DatabaseConnection, wo_id: i64, user: i64, before_id: Option<i64>) -> AppResult<i64> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM work_order_interveners               WHERE work_order_id = ? AND intervener_id = ? AND ended_at IS NULL AND hours_worked IS NULL                 AND (? IS NULL OR id < ?)",
            [wo_id.into(), user.into(), before_id.into(), before_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "n").map_err(read_err))
        .transpose()?
        .unwrap_or(0))
}

fn already_on_it() -> AppError {
    AppError::ValidationFailed(vec![format!(
        "{CODE_ALREADY_ON_IT}: Vous avez déjà de la main-d'œuvre en cours sur cet OT."
    )])
}

/// Joins the work order: starts the user's own labour now and tells the assignee.
///
/// The booking runs in its own transaction (`wo::labor::book_labor`), so two joins sent at the
/// same moment could both pass the first check: after the booking, an entry that finds an older
/// open one of the same person on the work order is removed again (it holds no hours yet) and the
/// join is refused — only the first stays.
pub async fn join_wo(db: &DatabaseConnection, input: WoJoinInput) -> AppResult<WoIntervener> {
    if open_entries(db, input.wo_id, input.actor_id, None).await? > 0 {
        return Err(already_on_it());
    }
    let entry = add_labor_entry(
        db,
        AddLaborInput {
            wo_id: input.wo_id,
            intervener_id: input.actor_id,
            skill_id: None,
            started_at: Some(crate::wo::time::now_utc_z()),
            ended_at: None,
            hours_worked: None,
            hourly_rate: None,
            notes: None,
        },
    )
    .await?;
    if open_entries(db, input.wo_id, input.actor_id, Some(entry.id)).await? > 0 {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM work_order_interveners WHERE id = ? AND ended_at IS NULL AND hours_worked IS NULL",
            [entry.id.into()],
        ))
        .await?;
        return Err(already_on_it());
    }
    notify_joined(db, input.wo_id, input.actor_id).await;
    Ok(entry)
}

/// "X a rejoint l'OT" to the assignee (fire-and-log, as every notification).
async fn notify_joined(conn: &impl ConnectionTrait, wo_id: i64, joiner: i64) {
    let wo = match get_work_order(conn, wo_id).await {
        Ok(Some(wo)) => wo,
        _ => return,
    };
    if wo.primary_responsible_id.is_none() || wo.primary_responsible_id == Some(joiner) {
        return;
    }
    let who = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(NULLIF(TRIM(display_name), ''), username) AS name FROM user_accounts WHERE id = ?",
            [joiner.into()],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String>("", "name").ok())
        .unwrap_or_default();
    let input = NotificationEventInput {
        source_module: "work_orders".into(),
        source_record_id: Some(wo.id.to_string()),
        event_code: JOINED_MESSAGE_KEY.into(),
        category_code: JOINED_CATEGORY.into(),
        severity: "info".into(),
        dedupe_key: Some(format!("{JOINED_MESSAGE_KEY}-{}-{joiner}", wo.id)),
        payload_json: None,
        title: format!("{who} a rejoint l'OT {}", wo.code),
        body: Some(wo.title.clone()),
        action_url: Some(format!("/work-orders?openWo={}", wo.id)),
    };
    let message = NotificationMessage::new(
        JOINED_MESSAGE_KEY,
        serde_json::json!({ "code": wo.code, "title": wo.title, "who": who }),
    );
    if let Err(err) = emit_event_with(conn, input, Some(&message)).await {
        tracing::warn!(error = %err, wo_id = wo.id, "wo joined notification emit failed");
    }
}
