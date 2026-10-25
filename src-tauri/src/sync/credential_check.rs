//! A received password change, and a received re-enable or restore of an account, are re-checked
//! against the person who made them (S3 follow-up: the password reset author).
//!
//! The password of an account travels with its row (`password_hash`, `password_changed_at`,
//! `force_password_change`, SYNC_STRATEGY §7). An administrator whose right to manage users was
//! withdrawn on PC B can still, on a PC A that has not received the withdrawal, reset another
//! account's password (an administrator's included) or re-enable a disabled account; every PC would
//! apply it. The row now names who did it (migration 261): `password_changed_by_id` and
//! `status_changed_by_id`, sent as `password_changed_by_sync_id` / `status_changed_by_sync_id`.
//!
//! Before a received account row is written ([`inbound_credential_check`], from
//! `sync::row_codec::apply_row` right after `apply_hooks::before_upsert_checked`, so a row the other
//! checks refuse writes nothing here):
//! - **a password change** (on an account held here: another `password_hash`, or another
//!   `password_changed_at`; on a new account: a password) is accepted when its author is the account
//!   itself (a change of one's own password, the forced first change, the first administrator, a
//!   vendor recovery account), or when the author held `adm.users` at that time (the rule of
//!   `sync::grant_check::author_check`; a plant administrator when the account is an administrator
//!   account, `commands::admin_users::is_admin_account`). An author holding `adm.users` now without
//!   being a plant administrator must also hold every permission the account can reach here
//!   (`commands::admin_users::ensure_may_manage_account`; it cannot be rebuilt for the past). A
//!   change of `force_password_change` alone is applied unchecked (it gives nothing).
//! - **a re-enable or restore** (`is_active` 0 → 1, `deleted_at` cleared) of an account held here:
//!   the same rule for `status_changed_by_sync_id`, dated by the row's `updated_at`. Disabling or
//!   deleting is never checked (it only takes access away).
//! - Author absent from the row (a PC with an older version) or named as nobody: refused on an
//!   account held here (fail closed, lead decision O1); accepted on a new account (the first pull of
//!   a new PC, a password that predates the upgrade).
//!
//! A refused change is **held back, not the row**: its keys are taken out of the payload, so this
//! PC keeps its password (nobody is signed out) or the account stays disabled, and the rest of the
//! row applies (a later disable, a rename, still reach every PC). A new account whose password is
//! refused is created without one (`password_hash` NULL, `force_password_change` 1): it cannot sign
//! in here until an authorised reset or change arrives, and sign-in says so
//! ([`password_withheld_on_arrival`]). Each refusal writes an audit event
//! ([`PASSWORD_CHANGE_UNAUTHORISED`], [`ACCOUNT_REENABLE_UNAUTHORISED`]) and a notice to the
//! administrators (`settings::received_notice::send_grant_review_notice`), in a savepoint of the
//! apply: when either cannot be written, the whole row waits and is tried again (a credential is
//! never dropped untold). No Sync-centre conflict is opened (lead decision O7).
//!
//! The waits and the "applied for review" outcome are S3's (`sync::grant_check`): an author not
//! applied yet makes the row wait for that account (released after a complete pull), an author whose
//! loss of the right is not known here is applied and listed in the run's review notice.
//!
//! On the PC that made the change, nothing is undone when the author's withdrawal arrives: the
//! password they set is listed in S3's "grants to review" notice (`grant_check::grants_made_by`,
//! lead decision O2). The times compared are the sender's clock, as for every S3 check (O3).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Value};

use crate::errors::AppResult;
use crate::permit::sync_check::{field, Payload};
use crate::rbac::permissions::ADM_USERS;
use crate::sync::grant_check::{self, ReviewItem};
use crate::sync::materialize::ApplyOutcome;
use crate::sync::row_layout::EntityLayout;

/// Audit code of a received password change whose author could not make it here.
pub(crate) const PASSWORD_CHANGE_UNAUTHORISED: &str = "PASSWORD_CHANGE_UNAUTHORISED";

/// Audit code of a received re-enable or restore whose author could not make it here.
pub(crate) const ACCOUNT_REENABLE_UNAUTHORISED: &str = "ACCOUNT_REENABLE_UNAUTHORISED";

/// Payload keys of the password: accepted or kept together.
pub(crate) const CREDENTIAL_KEYS: [&str; 4] = [
    "password_hash",
    "password_changed_at",
    "password_changed_by_sync_id",
    "force_password_change",
];

/// Payload keys of the account's state: accepted or kept together.
pub(crate) const STATUS_KEYS: [&str; 3] = ["is_active", "deleted_at", "status_changed_by_sync_id"];

const PASSWORD_AUTHOR_KEY: &str = "password_changed_by_sync_id";
const STATUS_AUTHOR_KEY: &str = "status_changed_by_sync_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Password,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// The author did not hold the right here at that time (or never did).
    Unauthorised,
    /// The row does not carry the author key: sent by an older version.
    OlderVersion,
    /// The row names no author.
    NoAuthor,
}

impl Reason {
    fn code(self) -> &'static str {
        match self {
            Self::Unauthorised => "unauthorised",
            Self::OlderVersion => "older_version",
            Self::NoAuthor => "no_author",
        }
    }
}

/// A received change held back.
struct Refusal {
    kind: Kind,
    reason: Reason,
    author_sync: Option<String>,
    changed_at: Option<String>,
    detail: String,
}

enum Verdict {
    Accept,
    /// The whole row stops (it waits).
    Stop(Option<ApplyOutcome>),
    Refuse(Refusal),
}

fn instant_text_differs(held: Option<&str>, received: Option<&str>) -> bool {
    match (held, received) {
        (None, None) => false,
        (Some(h), Some(r)) => match (crate::settings::site::instant(h), crate::settings::site::instant(r)) {
            (Some(h), Some(r)) => h != r,
            // Unreadable: differs unless the text is the same.
            _ => h != r,
        },
        _ => true,
    }
}

/// The received row changes the password held here (`held`), or brings one (a new account).
fn password_changed(held: Option<&Payload>, received: &Payload) -> bool {
    let Some(held) = held else {
        return field(received, "password_hash").is_some();
    };
    // A key left out keeps the value held here; a password removed is a change too.
    let hash_changed =
        received.contains_key("password_hash") && field(received, "password_hash") != field(held, "password_hash");
    let date_changed = received.contains_key("password_changed_at")
        && instant_text_differs(
            field(held, "password_changed_at"),
            field(received, "password_changed_at"),
        );
    hash_changed || date_changed
}

/// The received row re-enables or restores the account held here.
fn status_widened(held: &Payload, received: &Payload) -> bool {
    let reactivated = received.contains_key("is_active")
        && grant_check::truthy(received, "is_active")
        && !grant_check::truthy(held, "is_active");
    let restored = received.contains_key("deleted_at")
        && field(received, "deleted_at").is_none()
        && field(held, "deleted_at").is_some();
    reactivated || restored
}

/// The account is an administrator account here or by the received row (a plant administrator
/// must have made the change).
async fn admin_target(conn: &impl ConnectionTrait, target: Option<i64>, received: &Payload) -> AppResult<bool> {
    if grant_check::truthy(received, "is_admin") {
        return Ok(true);
    }
    match target {
        Some(id) => crate::commands::admin_users::is_admin_account(conn, id).await,
        None => Ok(false),
    }
}

/// The local rule of `ensure_may_manage_account` for an author holding `adm.users` now without
/// being a plant administrator: every permission the account can reach here. The missing ones.
async fn permissions_missing(conn: &impl ConnectionTrait, author: i64, target: i64) -> AppResult<Vec<String>> {
    if author == target || grant_check::is_plant_admin(conn, author).await? {
        return Ok(Vec::new());
    }
    if !grant_check::holds(conn, author, ADM_USERS).await? {
        // Judged by its time (`author_check`); the past set of rights cannot be rebuilt.
        return Ok(Vec::new());
    }
    let reachable = crate::rbac::resolver::reachable_permissions(conn, target).await?;
    let held = crate::rbac::resolver::permanent_tenant_permissions(conn, author).await?;
    let mut missing: Vec<String> = reachable.into_iter().filter(|p| !held.contains(p)).collect();
    missing.sort();
    Ok(missing)
}

/// Judges one received change ([`Kind`]) against its author.
async fn judge(
    conn: &impl ConnectionTrait,
    kind: Kind,
    entity_sync_id: &str,
    target: Option<i64>,
    account: &str,
    received: &Payload,
    changed_at: Option<String>,
) -> AppResult<Verdict> {
    let author_key = match kind {
        Kind::Password => PASSWORD_AUTHOR_KEY,
        Kind::Status => STATUS_AUTHOR_KEY,
    };
    let refuse = |reason: Reason, author_sync: Option<String>, detail: String| {
        Verdict::Refuse(Refusal {
            kind,
            reason,
            author_sync,
            changed_at: changed_at.clone(),
            detail,
        })
    };
    let held = target.is_some();
    let author_sync = match received.get(author_key) {
        None if held => {
            return Ok(refuse(
                Reason::OlderVersion,
                None,
                format!("{account}: sent by a PC with an older version, which does not say who made it"),
            ))
        }
        None => return Ok(Verdict::Accept),
        Some(_) => match field(received, author_key) {
            Some(author) => author.to_string(),
            None if held => {
                return Ok(refuse(
                    Reason::NoAuthor,
                    None,
                    format!("{account}: the row does not say who made it"),
                ))
            }
            None => return Ok(Verdict::Accept),
        },
    };
    // One's own password (self change, forced first change, first administrator, recovery account).
    if kind == Kind::Password && author_sync == entity_sync_id {
        return Ok(Verdict::Accept);
    }
    let what = match kind {
        Kind::Password => format!("password of account {entity_sync_id}"),
        Kind::Status => format!("re-enable of account {entity_sync_id}"),
    };
    let author_label = grant_check::person_label(conn, &author_sync).await?;
    let review = ReviewItem {
        entity_type: "user_accounts",
        source: entity_sync_id.to_string(),
        line: match kind {
            Kind::Password => format!(
                "Mot de passe de / password of {account}, par / by {author_label}, {}",
                grant_check::day(changed_at.as_deref())
            ),
            Kind::Status => format!(
                "Compte réactivé / account re-enabled {account}, par / by {author_label}, {}",
                grant_check::day(changed_at.as_deref())
            ),
        },
    };
    let admin = admin_target(conn, target, received).await?;
    match grant_check::author_check(
        conn,
        &what,
        &author_sync,
        ADM_USERS,
        admin,
        changed_at.as_deref(),
        review,
    )
    .await?
    {
        None => {}
        Some(ApplyOutcome::Conflict { message, .. }) => {
            return Ok(refuse(Reason::Unauthorised, Some(author_sync), message));
        }
        Some(other) => return Ok(Verdict::Stop(Some(other))),
    }
    if let (Some(target), Some(author)) = (
        target,
        grant_check::local_id(conn, "user_accounts", &author_sync).await?,
    ) {
        let missing = permissions_missing(conn, author, target).await?;
        if !missing.is_empty() {
            let shown: Vec<&str> = missing.iter().take(10).map(String::as_str).collect();
            return Ok(refuse(
                Reason::Unauthorised,
                Some(author_sync),
                format!(
                    "{what}: its author does not hold every permission of that account here; missing: {}",
                    shown.join(", ")
                ),
            ));
        }
    }
    Ok(Verdict::Accept)
}

/// Received account row re-checked against the authors of its password and of its re-enable; `Some`
/// stops the apply (it waits). A refused change is taken out of `payload` (see the module rule).
pub(crate) async fn inbound_credential_check(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &mut Payload,
) -> AppResult<Option<ApplyOutcome>> {
    if layout.def.entity_type != "user_accounts" {
        return Ok(None);
    }
    let held = crate::permit::sync_check::local_wire(conn, "user_accounts", entity_sync_id).await?;
    let target = match held {
        Some(_) => grant_check::local_id(conn, "user_accounts", entity_sync_id).await?,
        None => None,
    };
    let account = match &held {
        Some(_) => grant_check::person_label(conn, entity_sync_id).await?,
        None => field(payload, "display_name")
            .or_else(|| field(payload, "username"))
            .unwrap_or(entity_sync_id)
            .to_string(),
    };
    let mut refusals: Vec<Refusal> = Vec::new();

    if password_changed(held.as_ref(), payload) {
        let changed_at = field(payload, "password_changed_at")
            .or_else(|| {
                if held.is_none() {
                    field(payload, "created_at")
                } else {
                    None
                }
            })
            .map(str::to_string);
        match judge(
            conn,
            Kind::Password,
            entity_sync_id,
            target,
            &account,
            payload,
            changed_at,
        )
        .await?
        {
            Verdict::Accept => {}
            Verdict::Stop(outcome) => return Ok(outcome),
            Verdict::Refuse(r) => refusals.push(r),
        }
    }
    if let Some(held) = &held {
        if status_widened(held, payload) {
            let changed_at = field(payload, "updated_at").map(str::to_string);
            match judge(
                conn,
                Kind::Status,
                entity_sync_id,
                target,
                &account,
                payload,
                changed_at,
            )
            .await?
            {
                Verdict::Accept => {}
                Verdict::Stop(outcome) => return Ok(outcome),
                Verdict::Refuse(r) => refusals.push(r),
            }
        }
    }
    if refusals.is_empty() {
        return Ok(None);
    }
    if let Some(outcome) = record_refusals(conn, entity_sync_id, target, &account, held.is_none(), &refusals).await? {
        return Ok(Some(outcome));
    }
    for refusal in &refusals {
        match refusal.kind {
            Kind::Password if held.is_none() => {
                // A new account: created without a password (fail closed).
                payload.insert("password_hash".into(), Value::Null);
                payload.insert("password_changed_at".into(), Value::Null);
                payload.insert(PASSWORD_AUTHOR_KEY.into(), Value::Null);
                payload.insert("force_password_change".into(), Value::from(1));
            }
            Kind::Password => {
                for key in CREDENTIAL_KEYS {
                    payload.remove(key);
                }
            }
            Kind::Status => {
                for key in STATUS_KEYS {
                    payload.remove(key);
                }
            }
        }
    }
    Ok(None)
}

fn notice_texts(refusal: &Refusal, account: &str, author: &str, inserted: bool) -> (String, String, &'static str) {
    let at = refusal.changed_at.as_deref().unwrap_or("?");
    match (refusal.kind, refusal.reason) {
        (Kind::Password, Reason::OlderVersion) => (
            "Changement de mot de passe reçu d'une ancienne version : non appliqué / Password change received from an \
             older version: not applied"
                .to_string(),
            format!(
                "{account} : mot de passe changé sur un poste qui a une ancienne version de Maintafox (elle ne dit pas qui \
                 l'a fait) : non appliqué. Mettez ce poste à jour, puis changez de nouveau le mot de passe. / {account}: \
                 password change sent by a PC with an older Maintafox version (it does not say who made it): not \
                 applied. Update that PC, then change the password again."
            ),
            "password_change_from_older_version",
        ),
        (Kind::Password, _) => {
            let (note_fr, note_en) = if inserted {
                (
                    " ; le compte n'a pas de mot de passe sur ce poste d'ici là",
                    "; the account has no password on this PC until then",
                )
            } else {
                ("", "")
            };
            (
                "Changement de mot de passe reçu sans droit : non appliqué / Password change received without the right: \
                 not applied"
                    .to_string(),
                format!(
                    "{account} : mot de passe changé le {at} par {author}, qui n'avait pas (ou plus) le droit de gérer les \
                     utilisateurs ici. Ce poste garde le mot de passe actuel{note_fr}. Si ce changement était voulu, un \
                     administrateur le refait ; cela corrige aussi les autres postes. / {account}: password changed on {at} \
                     by {author}, who did not (or no longer) hold user management here. This PC kept the current \
                     password{note_en}. If the change was intended, an administrator makes it again; that also corrects \
                     the other PCs."
                ),
                "password_change_refused",
            )
        }
        (Kind::Status, _) => (
            "Réactivation de compte reçue sans droit : non appliquée / Account re-enable received without the right: not \
             applied"
                .to_string(),
            format!(
                "{account} : compte réactivé le {at} par {author}, qui n'avait pas (ou plus) le droit de gérer les \
                 utilisateurs ici. Le compte reste désactivé sur ce poste. Si c'était voulu, un administrateur le \
                 réactive de nouveau ; cela corrige aussi les autres postes. / {account}: account re-enabled on {at} by \
                 {author}, who did not (or no longer) hold user management here. The account stays disabled on this \
                 PC. If it was intended, an administrator enables it again; that also corrects the other PCs."
            ),
            "account_reenable_refused",
        ),
    }
}

/// Audit event and administrators' notice of each refusal, in a savepoint of the apply. `Some`: they
/// could not be written; the whole row waits (fail closed).
async fn record_refusals(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    target: Option<i64>,
    account: &str,
    inserted: bool,
    refusals: &[Refusal],
) -> AppResult<Option<ApplyOutcome>> {
    conn.execute_unprepared("SAVEPOINT credential_refused").await?;
    let written = async {
        for refusal in refusals {
            let author = match &refusal.author_sync {
                Some(a) => grant_check::person_label(conn, a).await?,
                None => "?".to_string(),
            };
            let actor = match &refusal.author_sync {
                Some(a) => grant_check::local_id(conn, "user_accounts", a).await?,
                None => None,
            };
            let code = match refusal.kind {
                Kind::Password => PASSWORD_CHANGE_UNAUTHORISED,
                Kind::Status => ACCOUNT_REENABLE_UNAUTHORISED,
            };
            crate::audit::writer::write_audit_event_in(
                conn,
                crate::audit::writer::AuditEventInput {
                    action_code: code.to_string(),
                    target_type: Some("user_account".to_string()),
                    target_id: target.map(|t| t.to_string()),
                    actor_id: actor,
                    auth_context: "system".to_string(),
                    result: "blocked".to_string(),
                    before_hash: None,
                    after_hash: None,
                    retention_class: "security".to_string(),
                    details_json: Some(json!({
                        "account_sync_id": entity_sync_id,
                        "author_sync_id": refusal.author_sync,
                        "changed_at": refusal.changed_at,
                        "reason": refusal.reason.code(),
                        "new_account": inserted,
                        "detail": refusal.detail,
                    })),
                },
            )
            .await?;
            let (title, body, key) = notice_texts(refusal, account, &author, inserted);
            crate::settings::received_notice::send_grant_review_notice(
                conn,
                "user_accounts",
                entity_sync_id,
                title,
                body,
                crate::notifications::emitter::NotificationMessage::new(
                    key,
                    json!({
                        "account": account,
                        "author": author,
                        "changed_at": refusal.changed_at,
                        "new_account": inserted,
                    }),
                ),
                true,
            )
            .await?;
        }
        AppResult::Ok(())
    }
    .await;
    match written {
        Ok(()) => {
            conn.execute_unprepared("RELEASE credential_refused").await?;
            Ok(None)
        }
        Err(e) => {
            conn.execute_unprepared("ROLLBACK TO credential_refused").await?;
            conn.execute_unprepared("RELEASE credential_refused").await?;
            tracing::error!(error = %e, account = %entity_sync_id, "refused credential: audit or notice not written, the row waits");
            Ok(grant_check::wait(format!(
                "account {entity_sync_id}: a refused password or re-enable could not be recorded; tried again later"
            )))
        }
    }
}

/// The account arrived by sync without its password (a refused change on a new account, see the
/// module rule): sign-in says so instead of the generic failure (lead decision O4).
pub(crate) async fn password_withheld_on_arrival(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS w FROM user_accounts ua \
              WHERE ua.id = ? AND ua.password_hash IS NULL AND EXISTS ( \
                SELECT 1 FROM audit_events ae \
                 WHERE ae.action_code = ? \
                   AND json_extract(ae.details_json, '$.account_sync_id') = ua.entity_sync_id)",
            [user_id.into(), PASSWORD_CHANGE_UNAUTHORISED.into()],
        ))
        .await?
        .is_some())
}
