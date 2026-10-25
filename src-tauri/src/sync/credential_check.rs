//! A received password change, and a received re-enable or restore of an account, are re-checked
//! against the person who made them (S3 follow-up: the password reset author).
//!
//! The password of an account travels with its row (`password_hash`, `password_changed_at`,
//! `force_password_change`, SYNC_STRATEGY §7). An administrator whose right to manage users was
//! withdrawn on PC B can still, on a PC A that has not received the withdrawal, reset another
//! account's password (an administrator's included) or re-enable a disabled account; every PC would
//! apply it. The row now names who did it (migration 261): `password_changed_by_id` and
//! `status_changed_by_id`, sent as `password_changed_by_sync_id` / `status_changed_by_sync_id`, and
//! `password_set_by_id` / `password_set_at`: who last set the password other than the account itself
//! (an administrator's reset, the creator), and when, kept by the account's own later changes.
//!
//! Before a received account row is written ([`inbound_credential_check`], from
//! `sync::row_codec::apply_row` right after `apply_hooks::before_upsert_checked`, so a row the other
//! checks refuse writes nothing here):
//! - **a password change** (on an account held here: another `password_hash`, or another
//!   `password_changed_at`; on a new account: a password) is accepted when its author is the account
//!   itself (a change of one's own password, the forced first change, the first administrator, a
//!   vendor recovery account) **and** whoever set the password it replaces could (the received
//!   `password_set_by` is the account itself, or held the right at `password_set_at`: otherwise a
//!   demoted administrator resets an account, signs in with the temporary password and changes it
//!   "as the account"), or when the author held `adm.users` at that time (the rule of
//!   `sync::grant_check::author_check`; a plant administrator when the account is an administrator
//!   account, `commands::admin_users::is_admin_account`). An author holding `adm.users` now without
//!   being a plant administrator must also hold every permission the account can reach here
//!   (`commands::admin_users::ensure_may_manage_account`; it cannot be rebuilt for the past). A
//!   change of `force_password_change` alone is applied unchecked (it gives nothing); a change of
//!   who set the password, or when, is judged like a password change.
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
//! The waits are S3's (`sync::grant_check`): an author not applied yet makes the row wait for that
//! account (released after a complete pull). S3's "applied for review" outcome is not used: an
//! author still missing after a complete pull, or whose loss of the right is not known here, is
//! refused (holding a password or a re-enable back costs nothing).
//!
//! On the PC that made the change, nothing is undone when the author's withdrawal arrives: the
//! password they set is listed in S3's "grants to review" notice (`grant_check::grants_made_by`,
//! lead decision O2). The times compared are the sender's clock, as for every S3 check (O3).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::{json, Value};

use crate::errors::AppResult;
use crate::permit::sync_check::{field, Payload};
use crate::rbac::permissions::ADM_USERS;
use crate::sync::grant_check;
use crate::sync::materialize::ApplyOutcome;
use crate::sync::row_layout::EntityLayout;

/// Audit code of a received password change whose author could not make it here.
pub(crate) const PASSWORD_CHANGE_UNAUTHORISED: &str = "PASSWORD_CHANGE_UNAUTHORISED";

/// Audit code of a received re-enable or restore whose author could not make it here.
pub(crate) const ACCOUNT_REENABLE_UNAUTHORISED: &str = "ACCOUNT_REENABLE_UNAUTHORISED";

/// Payload keys of the password: accepted or kept together.
pub(crate) const CREDENTIAL_KEYS: [&str; 6] = [
    "password_hash",
    "password_changed_at",
    "password_changed_by_sync_id",
    "password_set_by_sync_id",
    "password_set_at",
    "force_password_change",
];

/// Payload keys of the account's state: accepted or kept together.
pub(crate) const STATUS_KEYS: [&str; 3] = ["is_active", "deleted_at", "status_changed_by_sync_id"];

const PASSWORD_AUTHOR_KEY: &str = "password_changed_by_sync_id";
const STATUS_AUTHOR_KEY: &str = "status_changed_by_sync_id";
/// Who last set the password other than the account itself, and when (migration 261).
const SET_BY_KEY: &str = "password_set_by_sync_id";
const SET_AT_KEY: &str = "password_set_at";

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
    // Who set the password, or when, changed: judged like a password change.
    hash_changed || date_changed || root_changed(Some(held), received)
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

/// What an author key of the received row says.
enum Named {
    /// Not in the row: sent by a PC with an older version.
    Absent,
    /// In the row, naming nobody.
    Nobody,
    Author(String),
}

fn named(received: &Payload, key: &str) -> Named {
    match received.get(key) {
        None => Named::Absent,
        Some(_) => field(received, key).map_or(Named::Nobody, |a| Named::Author(a.to_string())),
    }
}

fn refusal(
    kind: Kind,
    reason: Reason,
    author_sync: Option<String>,
    changed_at: Option<&str>,
    detail: String,
) -> Verdict {
    Verdict::Refuse(Refusal {
        kind,
        reason,
        author_sync,
        changed_at: changed_at.map(str::to_string),
        detail,
    })
}

/// `author_sync` held `adm.users` here when it made the change (`at`, sender's clock): S3's
/// `author_check` (a plant administrator for an administrator account), without its "applied for
/// review" outcome: an author never received after a complete pull, or whose loss of the right is
/// not known here, is refused (holding a password or a re-enable back costs nothing). An author
/// holding `adm.users` now without being a plant administrator must also hold every permission the
/// account can reach here.
async fn judge_author(
    conn: &impl ConnectionTrait,
    kind: Kind,
    entity_sync_id: &str,
    target: Option<i64>,
    received: &Payload,
    author_sync: &str,
    at: Option<&str>,
) -> AppResult<Verdict> {
    let what = match kind {
        Kind::Password => format!("password of account {entity_sync_id}"),
        Kind::Status => format!("re-enable of account {entity_sync_id}"),
    };
    let admin = admin_target(conn, target, received).await?;
    match grant_check::author_check(conn, &what, author_sync, ADM_USERS, admin, at, None).await? {
        None => {}
        Some(ApplyOutcome::Conflict { message, .. }) => {
            return Ok(refusal(
                kind,
                Reason::Unauthorised,
                Some(author_sync.to_string()),
                at,
                message,
            ));
        }
        Some(other) => return Ok(Verdict::Stop(Some(other))),
    }
    if let (Some(target), Some(author)) = (target, grant_check::local_id(conn, "user_accounts", author_sync).await?) {
        let missing = permissions_missing(conn, author, target).await?;
        if !missing.is_empty() {
            let shown: Vec<&str> = missing.iter().take(10).map(String::as_str).collect();
            return Ok(refusal(
                kind,
                Reason::Unauthorised,
                Some(author_sync.to_string()),
                at,
                format!(
                    "{what}: its author does not hold every permission of that account here; missing: {}",
                    shown.join(", ")
                ),
            ));
        }
    }
    Ok(Verdict::Accept)
}

/// The received row sets another root for the password than the one held here (who set it, when).
fn root_changed(held: Option<&Payload>, received: &Payload) -> bool {
    let Some(held) = held else {
        return true;
    };
    (received.contains_key(SET_BY_KEY) && field(received, SET_BY_KEY) != field(held, SET_BY_KEY))
        || (received.contains_key(SET_AT_KEY)
            && instant_text_differs(field(held, SET_AT_KEY), field(received, SET_AT_KEY)))
}

/// The received root is the one held here: the same `password_set_by` (both naming nobody counts
/// as the same) **and** the same `password_set_at`, which must be there on both sides. A root
/// naming nobody is only trusted with the very date this PC holds (migration 261 dates every
/// password set before 0.9.2): any other date may be a reset made by a version that did not record
/// who made it.
fn same_root(held: &Payload, received: &Payload) -> bool {
    let same_by = field(received, SET_BY_KEY) == field(held, SET_BY_KEY);
    let same_at = match (field(held, SET_AT_KEY), field(received, SET_AT_KEY)) {
        (Some(h), Some(r)) => !instant_text_differs(Some(h), Some(r)),
        _ => false,
    };
    same_by && same_at
}

/// A received password change (see the module rule): its author, then who set the password it
/// carries (the root a change of one's own password keeps).
async fn judge_password(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    held: Option<&Payload>,
    target: Option<i64>,
    account: &str,
    received: &Payload,
) -> AppResult<Verdict> {
    let changed_at = field(received, "password_changed_at")
        .or_else(|| {
            if held.is_none() {
                field(received, "created_at")
            } else {
                None
            }
        })
        .map(str::to_string);
    let at = changed_at.as_deref();
    let is_held = held.is_some();
    let older = |what: &str| {
        refusal(
            Kind::Password,
            Reason::OlderVersion,
            None,
            at,
            format!("{account}: sent by a PC with an older version, which does not say {what}"),
        )
    };
    let author = match named(received, PASSWORD_AUTHOR_KEY) {
        Named::Absent if is_held => return Ok(older("who made it")),
        Named::Nobody if is_held => {
            return Ok(refusal(
                Kind::Password,
                Reason::NoAuthor,
                None,
                at,
                format!("{account}: the row does not say who made it"),
            ))
        }
        // A new account: its password predates the upgrade, or comes from an older version.
        Named::Absent | Named::Nobody => return Ok(Verdict::Accept),
        Named::Author(a) => a,
    };
    if author != entity_sync_id {
        match judge_author(conn, Kind::Password, entity_sync_id, target, received, &author, at).await? {
            Verdict::Accept => {}
            other => return Ok(other),
        }
    }
    // Who set the password: a change of one's own password keeps it, so a temporary password set by
    // someone without the right stays refused once its holder changes it.
    let root = match named(received, SET_BY_KEY) {
        Named::Absent if is_held => return Ok(older("who set the password")),
        Named::Absent => return Ok(Verdict::Accept),
        Named::Nobody => None,
        Named::Author(r) => Some(r),
    };
    if held.is_some_and(|h| same_root(h, received)) {
        // The root this PC holds: judged when it arrived, set here, or set before 0.9.2 (dated by
        // migration 261) and untouched since.
        return Ok(Verdict::Accept);
    }
    let set_at = field(received, SET_AT_KEY);
    match root {
        // Names nobody, with another date than the one held here: set by a version that does not
        // record who set it (a reset on a PC still before 0.9.2, carried over by its upgrade).
        None if is_held => Ok(refusal(
            Kind::Password,
            Reason::OlderVersion,
            None,
            set_at.or(at),
            format!("{account}: the password was set where it is not recorded who set it"),
        )),
        // A new account: a password set before 0.9.2 names nobody; legacy accounts must still
        // arrive (the new-account rule of the module).
        None => Ok(Verdict::Accept),
        // Its own root (the first administrator, a vendor recovery account).
        Some(r) if r == entity_sync_id => Ok(Verdict::Accept),
        // The author of this change set it: judged above.
        Some(r) if r == author => Ok(Verdict::Accept),
        Some(r) => judge_author(conn, Kind::Password, entity_sync_id, target, received, &r, set_at).await,
    }
}

/// A received re-enable or restore of an account held here: its author (see the module rule).
async fn judge_status(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    target: Option<i64>,
    account: &str,
    received: &Payload,
) -> AppResult<Verdict> {
    let at = field(received, "updated_at");
    match named(received, STATUS_AUTHOR_KEY) {
        Named::Absent => Ok(refusal(
            Kind::Status,
            Reason::OlderVersion,
            None,
            at,
            format!("{account}: sent by a PC with an older version, which does not say who made it"),
        )),
        Named::Nobody => Ok(refusal(
            Kind::Status,
            Reason::NoAuthor,
            None,
            at,
            format!("{account}: the row does not say who made it"),
        )),
        Named::Author(author) => judge_author(conn, Kind::Status, entity_sync_id, target, received, &author, at).await,
    }
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
        match judge_password(conn, entity_sync_id, held.as_ref(), target, &account, payload).await? {
            Verdict::Accept => {}
            Verdict::Stop(outcome) => return Ok(outcome),
            Verdict::Refuse(r) => refusals.push(r),
        }
    }
    if let Some(held) = &held {
        if status_widened(held, payload) {
            match judge_status(conn, entity_sync_id, target, &account, payload).await? {
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
                payload.insert(SET_BY_KEY.into(), Value::Null);
                payload.insert(SET_AT_KEY.into(), Value::Null);
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
