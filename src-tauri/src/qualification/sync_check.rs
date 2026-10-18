//! A qualification override received by synchronisation is checked again before it is written
//! (`sync::apply_hooks::before_upsert_checked`), with the rules of `qualification::gate`:
//! - a reason of at least ten characters;
//! - one subject (a personnel record, or a login with none — work orders only; a login linked to a
//!   personnel record is overridden as that person) and a context that matches its keys (a work
//!   order, or a permit and a LOTO role);
//! - never approved by the person it lets work (separation of duties);
//! - approved by someone who holds the certification right (`trn.certify`) here, now. This is
//!   evaluated on arrival: an approver whose right was withdrawn since makes it a conflict (fails
//!   closed), for a person to decide.
//!
//! An override already held here passes (append-only). A person or approver not applied here yet
//! makes it wait.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::permit::sync_check::{conflict, field, wait, Payload};
use crate::settings::site::InboundVerdict;

/// Local id of a row named by its sync id; `None` when not here.
async fn local_id(conn: &impl ConnectionTrait, table: &str, sync_id: &str) -> AppResult<Option<i64>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT id FROM \"{table}\" WHERE entity_sync_id = ?"),
        [sync_id.into()],
    ))
    .await?
    .map(|r| {
        r.try_get::<i64>("", "id")
            .map_err(|e| AppError::SyncError(format!("{table}.id: {e}")))
    })
    .transpose()
}

pub(crate) async fn inbound_override_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    if local_id(conn, "qualification_overrides", entity_sync_id)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    let reason = field(received, "reason").unwrap_or_default();
    if reason.chars().count() < super::gate::MIN_OVERRIDE_REASON_CHARS {
        return conflict(
            "QUALIFICATION_OVERRIDE_REASON_SHORT",
            "qualification override without a reason of at least 10 characters".into(),
        );
    }
    let person = field(received, "personnel_sync_id");
    let account = field(received, "user_sync_id");
    let context_ok = match field(received, "context_kind") {
        Some("work_order") => field(received, "work_order_sync_id").is_some(),
        Some("permit_role") => {
            field(received, "permit_sync_id").is_some() && field(received, "role_code").is_some() && account.is_none()
        }
        _ => false,
    };
    let Some(approver_sync) = field(received, "approved_by_sync_id") else {
        return conflict(
            "QUALIFICATION_OVERRIDE_INVALID",
            "qualification override that names nobody who approved it".into(),
        );
    };
    if person.is_some() == account.is_some() || !context_ok {
        return conflict(
            "QUALIFICATION_OVERRIDE_INVALID",
            "qualification override whose person or context is incomplete".into(),
        );
    }
    let Some(approver) = local_id(conn, "user_accounts", approver_sync).await? else {
        return wait(format!("approver {approver_sync} is not applied yet"));
    };
    match (person, account) {
        (_, Some(account)) if account == approver_sync => {
            return conflict(
                "QUALIFICATION_OVERRIDE_SELF",
                "qualification override approved by the login it lets work".into(),
            );
        }
        (_, Some(account)) => {
            // A login linked to a personnel record is overridden as that person
            // (`QUALIFICATION_OVERRIDE_USE_PERSON` in `gate::record_account_work_order_override`).
            let Some(user) = local_id(conn, "user_accounts", account).await? else {
                return wait(format!("login {account} is not applied yet"));
            };
            if super::gate::personnel_for_user(conn, user).await?.is_some() {
                return conflict(
                    "QUALIFICATION_OVERRIDE_USE_PERSON",
                    "qualification override on a login that has a personnel record: the person must be overridden"
                        .into(),
                );
            }
        }
        (Some(person), _) => {
            let Some(personnel) = local_id(conn, "personnel", person).await? else {
                return wait(format!("person {person} is not applied yet"));
            };
            if super::gate::personnel_for_user(conn, approver).await? == Some(personnel) {
                return conflict(
                    "QUALIFICATION_OVERRIDE_SELF",
                    "qualification override approved by the person it lets work".into(),
                );
            }
        }
        _ => {}
    }
    if !crate::settings::site::holds_tenant_right(conn, approver, crate::rbac::permissions::TRN_CERTIFY).await? {
        // Rows giving the approver the right are still waiting here: wait (never applied without it).
        if crate::permit::sync_check::rights_may_still_arrive(
            conn,
            approver_sync,
            crate::rbac::permissions::TRN_CERTIFY,
        )
        .await?
        {
            return wait(format!("the rights of approver {approver_sync} are still arriving"));
        }
        return conflict(
            "QUALIFICATION_OVERRIDE_UNAUTHORISED",
            "qualification override approved by someone who does not hold the certification right here".into(),
        );
    }
    Ok(None)
}
