//! Technician phones, as the desktop manages them (owner-approved 2026-10-29, decision register:
//! admin-issued QR enrolment). The licence key is never given to a phone.
//!
//! An administrator (`adm.users`, plus a fresh step-up for what grants or removes access) issues an
//! enrolment for ONE person at ONE site; the licence server answers a one-time QR token and an
//! 8-character code, valid 15 minutes, single use, cancellable. The QR carries only the server
//! address and that token. The phone that scans it is bound to that person and site. Only a person
//! whose account the administrator may manage (`admin_users::ensure_may_manage_account`) can be
//! given a phone.
//!
//! The "Téléphones" list shows the phones of the plant (person, site, enrolled by / at, last sync)
//! with "Révoquer": the slot is freed at once; at its next contact the phone uploads its pending
//! work, then erases its data (a phone reported lost uploads nothing). Disabling a person's account
//! revokes all their phones: the revocation is recorded in the transaction that disables the
//! account ([`record_person_revocation_in`], called by `admin_users`), so it cannot be lost, and is
//! sent at once or by the follow-up task as soon as the licence server can be reached. The
//! follow-up task also compares the server's phones with the accounts of this PC each round and
//! revokes any phone still active whose person is disabled or deleted here (a lost record, a
//! reinstalled PC, a disable received from another PC).
//!
//! What this PC issued is followed until the phone uses it, so the administrators get a notice of
//! each enrolment ([`ENROLMENT_NOTICE_CATEGORY`]). Every step, refused and failed attempts
//! included, is in the audit log here and in the licence server's ledger.
//!
//! The follow-up state (issued enrolments, pending revocations) is a device setting of this PC: it
//! names enrolments and people, never a secret.

use std::collections::BTreeSet;
use std::time::Duration;

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::{Mutex, MutexGuard};

use crate::auth::rbac::PermissionScope;
use crate::auth::session_manager::AuthenticatedUser;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::sync::transport::client::{ControlPlaneClient, TransportError};
use crate::{require_permission, require_session, require_step_up};

/// Notification category of "a phone was enrolled" (migration 266; audience: `adm.users` holders).
pub const ENROLMENT_NOTICE_CATEGORY: &str = "mobile_phone_enrolled";

const FOLLOW_UP_KEY: &str = "mobile.enrolment_follow_up";
const FOLLOW_UP_SCOPE: &str = "device";
/// How often the follow-up task looks at open enrolments, pending revocations and the phones of
/// disabled accounts.
const FOLLOW_UP_INTERVAL: Duration = Duration::from_secs(60);
/// Longest wait between two rounds while the licence server cannot be reached (the wait doubles
/// from [`FOLLOW_UP_INTERVAL`] at each unreachable round, up to this).
const FOLLOW_UP_MAX_WAIT: Duration = Duration::from_secs(15 * 60);
/// The QR payload's scheme and version; the phone refuses anything else.
pub const QR_PAYLOAD_PREFIX: &str = "maintafox-enrol:1?";

const ISSUE_PATH: &str = "/api/v1/mobile-enrolments";
const DEVICES_PATH: &str = "/api/v1/mobile-devices";
const REVOKE_PERSON_PATH: &str = "/api/v1/mobile-devices/revoke-person";

/// A phone the licence server still lets work (`MobilePhone::status`).
const PHONE_ACTIVE: &str = "active";

/// One read-modify-write of the follow-up state at a time on this PC. Taken before any database
/// transaction that writes the state, never while waiting on the licence server.
static FOLLOW_UP_LOCK: Mutex<()> = Mutex::const_new(());
/// One sender of the pending revocations at a time (the list screen and the follow-up task).
static SENDING_LOCK: Mutex<()> = Mutex::const_new(());

const OFFLINE: &str = "MOBILE_ENROL_OFFLINE";
const NOT_FOUND: &str = "MOBILE_ENROL_NOT_FOUND";

fn refused(code: &str, message: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {message}")])
}

/// The error is the refusal `code` of this module.
fn has_code(e: &AppError, code: &str) -> bool {
    matches!(e, AppError::ValidationFailed(messages)
        if messages.first().and_then(|m| m.split_once(':')).is_some_and(|(c, _)| c == code))
}

/// What a serde_json error says without the value it choked on: its category and position.
fn json_error_place(e: &serde_json::Error) -> String {
    format!("{:?} at line {} column {}", e.classify(), e.line(), e.column())
}

// ── Licence server calls ────────────────────────────────────────────────────────────────────────

/// Why the licence server refused a call, as the screens show it.
fn server_refusal(status: u16, body: &str) -> AppError {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_default();
    match code.as_str() {
        "mobile_not_licensed" => refused(
            "MOBILE_ENROL_NOT_LICENSED",
            "The licence does not include phones. Ask your vendor to add phones to the licence.",
        ),
        "slot_limit_reached" => refused(
            "MOBILE_ENROL_NO_FREE_SLOT",
            "Every phone of the licence is in use. Revoke a phone first, or ask your vendor for more.",
        ),
        "enrolment_not_found" | "device_not_found" => {
            refused(NOT_FOUND, "The licence server does not know this enrolment or phone.")
        }
        _ => {
            tracing::warn!(
                status,
                code = code.as_str(),
                "mobile enrolment: licence server refused the call"
            );
            refused(
                "MOBILE_ENROL_SERVER_REFUSED",
                &format!("The licence server refused the request (HTTP {status} {code})."),
            )
        }
    }
}

fn transport_failure(e: &TransportError) -> AppError {
    tracing::warn!(error = %e.message(), "mobile enrolment: licence server unreachable");
    refused(
        OFFLINE,
        "The licence server cannot be reached. Phones are managed online: check the connection and try again.",
    )
}

/// This PC's device token, or why phones cannot be managed from it.
async fn desktop_bearer(db: &DatabaseConnection) -> AppResult<String> {
    if cfg!(target_os = "android") {
        return Err(refused(
            "MOBILE_ENROL_DESKTOP_ONLY",
            "Phones are managed from a desktop.",
        ));
    }
    crate::commands::product_license::sync_activation_bearer_token(db)
        .await?
        .ok_or_else(|| {
            refused(
                "MOBILE_ENROL_PC_NOT_ACTIVATED",
                "This PC has no active licence: phones cannot be managed from it.",
            )
        })
}

/// The licence server as this PC calls it: its address and this PC's device token.
struct Server {
    base_url: String,
    token: String,
}

impl Server {
    /// For a command: the built-in licence server, or why phones cannot be managed from this PC.
    async fn of_this_pc(db: &DatabaseConnection) -> AppResult<Self> {
        Ok(Self {
            base_url: env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL").to_string(),
            token: desktop_bearer(db).await?,
        })
    }

    /// For the background work: none while this PC has no active licence (nothing to do then).
    async fn for_background(db: &DatabaseConnection) -> Option<Self> {
        match crate::commands::product_license::sync_activation_bearer_token(db).await {
            Ok(Some(token)) => Some(Self {
                base_url: env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL").to_string(),
                token,
            }),
            Ok(None) => None,
            Err(e) => {
                tracing::error!(error = %e, "mobile enrolment: this PC's licence token not readable");
                None
            }
        }
    }

    fn client(&self) -> AppResult<ControlPlaneClient> {
        ControlPlaneClient::new(
            &self.base_url,
            crate::sync::transport::client::ClientTimeouts::interactive(),
        )
        .map_err(AppError::SyncError)
    }

    async fn post<T: for<'de> Deserialize<'de>>(&self, path: &str, body: serde_json::Value) -> AppResult<T> {
        let answer = self
            .client()?
            .post_json_raw(path, Some(&self.token), body.to_string())
            .await
            .map_err(|e| transport_failure(&e))?;
        read_answer(path, answer)
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> AppResult<T> {
        let answer = self
            .client()?
            .get_json_raw(path, Some(&self.token))
            .await
            .map_err(|e| transport_failure(&e))?;
        read_answer(path, answer)
    }
}

fn read_answer<T: for<'de> Deserialize<'de>>(path: &str, (status, body): (u16, String)) -> AppResult<T> {
    if !(200..300).contains(&status) {
        return Err(server_refusal(status, &body));
    }
    serde_json::from_str(&body).map_err(|e| {
        // Never the value: an answer can hold a token or a code.
        tracing::warn!(path, error = %json_error_place(&e), "mobile enrolment: unreadable licence server answer");
        refused(
            "MOBILE_ENROL_SERVER_ANSWER_INVALID",
            "The licence server's answer could not be read.",
        )
    })
}

/// A path segment from an identifier the server gave (`enr_…`, `act_…`): anything else is refused,
/// so no identifier can change the path.
fn path_id(id: &str) -> AppResult<&str> {
    let ok = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(id)
    } else {
        Err(refused("MOBILE_ENROL_ID_INVALID", "This identifier is not valid."))
    }
}

// ── People and sites ────────────────────────────────────────────────────────────────────────────

/// A person who may be given a phone: an active account that signs in with its own password.
#[derive(Debug, Clone, Serialize)]
pub struct EnrolmentPerson {
    pub user_id: i32,
    pub sync_id: String,
    pub label: String,
    pub username: String,
}

/// A site a phone can be bound to: an organisation node whose type can own work.
#[derive(Debug, Clone, Serialize)]
pub struct EnrolmentSite {
    pub node_id: i64,
    pub sync_id: String,
    pub code: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnrolmentCandidates {
    pub people: Vec<EnrolmentPerson>,
    pub sites: Vec<EnrolmentSite>,
}

const PERSON_SQL: &str =
    "SELECT ua.id, ua.entity_sync_id, COALESCE(NULLIF(TRIM(ua.display_name), ''), ua.username) AS label, \
     ua.username FROM user_accounts ua \
     WHERE ua.deleted_at IS NULL AND ua.is_active = 1 AND ua.entity_sync_id IS NOT NULL \
       AND ua.password_hash IS NOT NULL AND ua.identity_mode IN ('local', 'hybrid')";

fn decode_err(what: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("mobile enrolment {what}: {e}"))
}

fn person_of(r: &sea_orm::QueryResult) -> AppResult<EnrolmentPerson> {
    Ok(EnrolmentPerson {
        user_id: r.try_get("", "id").map_err(|e| decode_err("person", e))?,
        sync_id: r.try_get("", "entity_sync_id").map_err(|e| decode_err("person", e))?,
        label: r.try_get("", "label").map_err(|e| decode_err("person", e))?,
        username: r.try_get("", "username").map_err(|e| decode_err("person", e))?,
    })
}

fn site_sql() -> String {
    format!(
        "SELECT n.id, n.entity_sync_id, n.code, n.name FROM org_nodes n JOIN org_node_types t ON t.id = n.node_type_id \
          WHERE n.deleted_at IS NULL AND t.can_own_work = 1 AND n.entity_sync_id IS NOT NULL AND {}",
        crate::settings::site::NOT_A_DRAFT_NODE_SQL
    )
}

fn site_of(r: &sea_orm::QueryResult) -> AppResult<EnrolmentSite> {
    Ok(EnrolmentSite {
        node_id: r.try_get("", "id").map_err(|e| decode_err("site", e))?,
        sync_id: r.try_get("", "entity_sync_id").map_err(|e| decode_err("site", e))?,
        code: r.try_get("", "code").map_err(|e| decode_err("site", e))?,
        label: r.try_get("", "name").map_err(|e| decode_err("site", e))?,
    })
}

/// The caller may manage this person's account (the rule of every other account change); a
/// refusal is `Ok(false)`, any other error is returned.
async fn may_manage(db: &DatabaseConnection, caller: &AuthenticatedUser, user_id: i32) -> AppResult<bool> {
    match crate::commands::admin_users::ensure_may_manage_account(db, caller, i64::from(user_id)).await {
        Ok(()) => Ok(true),
        Err(AppError::PermissionDenied(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

async fn candidates_in(db: &DatabaseConnection, caller: &AuthenticatedUser) -> AppResult<EnrolmentCandidates> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{PERSON_SQL} ORDER BY label COLLATE NOCASE"),
        ))
        .await?;
    // Only the people whose account the caller may manage: a phone is access to that account.
    let mut people = Vec::new();
    for row in &rows {
        let person = person_of(row)?;
        if may_manage(db, caller, person.user_id).await? {
            people.push(person);
        }
    }
    let sites = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{} ORDER BY n.name COLLATE NOCASE", site_sql()),
        ))
        .await?
        .iter()
        .map(site_of)
        .collect::<AppResult<Vec<_>>>()?;
    Ok(EnrolmentCandidates { people, sites })
}

/// The signed-in administrator as the licence server records them (sync id and name).
async fn actor_of(db: &DatabaseConnection, user: &AuthenticatedUser) -> AppResult<(String, String)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
            [user.user_id.into()],
        ))
        .await?;
    let sync_id: Option<String> = row
        .map(|r| r.try_get("", "entity_sync_id"))
        .transpose()
        .map_err(|e| decode_err("actor", e))?
        .flatten();
    let sync_id = sync_id.ok_or_else(|| {
        refused(
            "MOBILE_ENROL_ACTOR_UNKNOWN",
            "Your account is not known to the other PCs yet: synchronise, then try again.",
        )
    })?;
    let label = user
        .display_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| user.username.clone());
    Ok((sync_id, label))
}

/// An account of this PC as the revocations judge it.
struct LocalAccount {
    user_id: i32,
    sync_id: Option<String>,
    label: String,
    /// Enabled and not deleted.
    in_use: bool,
    /// Who last enabled, disabled, restored or deleted it (none when a rule did).
    status_changed_by_id: Option<i32>,
}

const LOCAL_ACCOUNT_SQL: &str =
    "SELECT id, entity_sync_id, COALESCE(NULLIF(TRIM(display_name), ''), username) AS label, \
     CASE WHEN is_active = 1 AND deleted_at IS NULL THEN 1 ELSE 0 END AS in_use, status_changed_by_id \
     FROM user_accounts";

fn local_account_of(r: &sea_orm::QueryResult) -> AppResult<LocalAccount> {
    Ok(LocalAccount {
        user_id: r.try_get("", "id").map_err(|e| decode_err("account", e))?,
        sync_id: r.try_get("", "entity_sync_id").map_err(|e| decode_err("account", e))?,
        label: r.try_get("", "label").map_err(|e| decode_err("account", e))?,
        in_use: r.try_get::<i32>("", "in_use").map_err(|e| decode_err("account", e))? == 1,
        status_changed_by_id: r
            .try_get("", "status_changed_by_id")
            .map_err(|e| decode_err("account", e))?,
    })
}

async fn local_account<C: ConnectionTrait>(
    conn: &C,
    column: &str,
    value: sea_orm::Value,
) -> AppResult<Option<LocalAccount>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("{LOCAL_ACCOUNT_SQL} WHERE {column} = ?"),
        [value],
    ))
    .await?
    .map(|r| local_account_of(&r))
    .transpose()
}

/// Who the licence server records for a revocation this PC sends on someone's behalf: their sync
/// id (none while their account is not synchronised yet) and name; nobody when no one is named.
struct Author {
    user_id: Option<i32>,
    sync_id: Option<String>,
    label: Option<String>,
}

async fn author_of(db: &DatabaseConnection, user_id: Option<i32>) -> AppResult<Author> {
    let Some(user_id) = user_id else {
        return Ok(Author {
            user_id: None,
            sync_id: None,
            label: None,
        });
    };
    let account = local_account(db, "id", user_id.into()).await?;
    Ok(Author {
        user_id: Some(user_id),
        sync_id: account.as_ref().and_then(|a| a.sync_id.clone()),
        label: account.map(|a| a.label),
    })
}

// ── Follow-up state (device setting) ────────────────────────────────────────────────────────────

/// An enrolment this PC issued, followed until it is used, cancelled or expired.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IssuedEnrolment {
    enrolment_id: String,
    person_label: String,
    site_label: String,
    issued_by_user_id: i32,
    expires_at: String,
}

/// The phones of a disabled person, still to revoke on the licence server. Only local ids: the
/// sync ids are read when it is sent, so recording it never depends on them.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRevocation {
    user_id: i64,
    person_label: String,
    requested_by_user_id: i32,
    requested_at: String,
    #[serde(default)]
    last_error: Option<String>,
}

impl PendingRevocation {
    /// The same request (a later disable of the same account after a re-enable is another one).
    fn same_as(&self, other: &Self) -> bool {
        self.user_id == other.user_id && self.requested_at == other.requested_at
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FollowUp {
    #[serde(default)]
    issued: Vec<IssuedEnrolment>,
    #[serde(default)]
    pending_revocations: Vec<PendingRevocation>,
}

/// Proof that the caller holds the follow-up lock (see [`lock_follow_up`]).
pub(crate) struct FollowUpGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

/// Takes the follow-up lock. A caller that records a revocation in its own transaction takes it
/// BEFORE opening the transaction and keeps it until the commit: nothing else then reads the state
/// before the transaction ends and overwrites what it recorded.
pub(crate) async fn lock_follow_up() -> FollowUpGuard {
    FollowUpGuard(FOLLOW_UP_LOCK.lock().await)
}

async fn load_follow_up<C: ConnectionTrait>(conn: &C) -> AppResult<FollowUp> {
    let Some(row) = crate::settings::get_setting(conn, FOLLOW_UP_KEY, FOLLOW_UP_SCOPE).await? else {
        return Ok(FollowUp::default());
    };
    serde_json::from_str(&row.setting_value_json).map_err(|e| {
        // Never rebuilt silently: a pending revocation must not be lost.
        AppError::Internal(anyhow::anyhow!(
            "mobile enrolment follow-up state unreadable: {}",
            json_error_place(&e)
        ))
    })
}

/// Writes the follow-up state with its row in the settings history (as `settings::set_setting`
/// does), through `conn`: inside the caller's transaction when it is one.
async fn save_follow_up<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    state: &FollowUp,
    actor: Option<i32>,
    why: &str,
) -> AppResult<()> {
    let value = serde_json::to_string(state)?;
    let (category, risk) =
        crate::settings::catalog::definition(FOLLOW_UP_KEY).map_or(("general", "low"), |d| (d.category, d.risk));
    let now = Utc::now().to_rfc3339();
    let tx = conn.begin().await?;
    let old = crate::settings::get_setting(&tx, FOLLOW_UP_KEY, FOLLOW_UP_SCOPE)
        .await?
        .map(|s| s.setting_value_json);
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO app_settings \
           (setting_key, setting_scope, setting_value_json, category, setting_risk, validation_status, \
            last_modified_by_id, last_modified_at) \
         VALUES (?, ?, ?, ?, ?, 'valid', ?, ?) \
         ON CONFLICT(setting_key, setting_scope) DO UPDATE SET \
           setting_value_json = excluded.setting_value_json, \
           last_modified_by_id = excluded.last_modified_by_id, \
           last_modified_at = excluded.last_modified_at",
        [
            FOLLOW_UP_KEY.into(),
            FOLLOW_UP_SCOPE.into(),
            value.clone().into(),
            category.into(),
            risk.into(),
            actor.map(i64::from).into(),
            now.clone().into(),
        ],
    ))
    .await?;
    crate::settings::insert_change_event(
        &tx,
        FOLLOW_UP_KEY,
        why,
        old.as_deref(),
        Some(&value),
        old.as_deref().map(crate::settings::sha256_hex),
        Some(crate::settings::sha256_hex(&value)),
        actor.map(i64::from),
        &now,
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

// ── Enrolments ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct IssueMobileEnrolmentInput {
    pub user_id: i32,
    pub site_node_id: i64,
}

#[derive(Deserialize)]
struct IssueAnswer {
    enrolment_id: String,
    token: String,
    code: String,
    issued_at: String,
    expires_at: String,
}

impl std::fmt::Debug for IssueAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssueAnswer")
            .field("enrolment_id", &self.enrolment_id)
            .field("token", &"<redacted>")
            .field("code", &"<redacted>")
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// What the administrator shows the technician: the QR (server address + one-time token), the
/// typed code, and until when they work.
#[derive(Clone, Serialize)]
pub struct IssuedMobileEnrolment {
    pub enrolment_id: String,
    pub qr_payload: String,
    pub code: String,
    pub issued_at: String,
    pub expires_at: String,
    pub person_label: String,
    pub site_label: String,
}

impl std::fmt::Debug for IssuedMobileEnrolment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedMobileEnrolment")
            .field("enrolment_id", &self.enrolment_id)
            .field("qr_payload", &"<redacted>")
            .field("code", &"<redacted>")
            .field("issued_at", &self.issued_at)
            .field("expires_at", &self.expires_at)
            .field("person_label", &self.person_label)
            .field("site_label", &self.site_label)
            .finish()
    }
}

/// Percent-encodes everything but the URL unreserved characters.
fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The QR's text: the server address and the one-time token, nothing else.
pub fn qr_payload(api_base: &str, token: &str) -> String {
    format!(
        "{QR_PAYLOAD_PREFIX}s={}&t={}",
        percent_encode(api_base.trim_end_matches('/')),
        percent_encode(token)
    )
}

async fn issue_in(
    db: &DatabaseConnection,
    server: &Server,
    caller: &AuthenticatedUser,
    input: &IssueMobileEnrolmentInput,
) -> AppResult<IssuedMobileEnrolment> {
    let person = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{PERSON_SQL} AND ua.id = ?"),
            [input.user_id.into()],
        ))
        .await?
        .map(|r| person_of(&r))
        .transpose()?
        .ok_or_else(|| {
            refused(
                "MOBILE_ENROL_PERSON_INVALID",
                "This person cannot be given a phone: the account must be active and sign in with its own password.",
            )
        })?;
    // A phone is access to that account: only for an account the caller may manage.
    crate::commands::admin_users::ensure_may_manage_account(db, caller, i64::from(person.user_id)).await?;
    let site = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{} AND n.id = ?", site_sql()),
            [input.site_node_id.into()],
        ))
        .await?
        .map(|r| site_of(&r))
        .transpose()?
        .ok_or_else(|| refused("MOBILE_ENROL_SITE_INVALID", "This site cannot be chosen for a phone."))?;
    let (actor_sync_id, actor_label) = actor_of(db, caller).await?;
    let answer: IssueAnswer = server
        .post(
            ISSUE_PATH,
            serde_json::json!({
                "person_sync_id": person.sync_id,
                "person_label": person.label,
                "site_sync_id": site.sync_id,
                "site_label": site.label,
                "issued_by_sync_id": actor_sync_id,
                "issued_by_label": actor_label,
            }),
        )
        .await?;
    path_id(&answer.enrolment_id)?;

    audit(
        db,
        Some(caller.user_id),
        AUDIT_ENROLMENT_ISSUED,
        &answer.enrolment_id,
        "Phone enrolment issued",
        serde_json::json!({
            "person_user_id": person.user_id,
            "person_sync_id": person.sync_id,
            "site_node_id": site.node_id,
            "site_sync_id": site.sync_id,
            "expires_at": answer.expires_at,
        }),
    )
    .await;
    let followed = async {
        let _guard = lock_follow_up().await;
        let mut state = load_follow_up(db).await?;
        state.issued.push(IssuedEnrolment {
            enrolment_id: answer.enrolment_id.clone(),
            person_label: person.label.clone(),
            site_label: site.label.clone(),
            issued_by_user_id: caller.user_id,
            expires_at: answer.expires_at.clone(),
        });
        save_follow_up(db, &state, Some(caller.user_id), "phone enrolment issued").await
    }
    .await;
    if let Err(e) = followed {
        // Not followed here, it would never be announced: withdrawn on the server instead.
        tracing::error!(error = %e, "mobile enrolment: issued enrolment not recorded; cancelling it");
        let cancelled: AppResult<serde_json::Value> = server
            .post(
                &format!("{ISSUE_PATH}/{}/cancel", answer.enrolment_id),
                serde_json::json!({ "actor_sync_id": actor_sync_id, "actor_label": actor_label }),
            )
            .await;
        match cancelled {
            Ok(_) => {
                audit(
                    db,
                    Some(caller.user_id),
                    AUDIT_ENROLMENT_CANCELLED,
                    &answer.enrolment_id,
                    "Phone enrolment cancelled: it could not be recorded on this PC",
                    serde_json::json!({}),
                )
                .await;
            }
            Err(ce) => tracing::error!(
                error = %ce,
                enrolment_id = answer.enrolment_id.as_str(),
                "mobile enrolment: unrecorded enrolment not cancelled (it expires on its own)"
            ),
        }
        return Err(e);
    }
    Ok(IssuedMobileEnrolment {
        qr_payload: qr_payload(&server.base_url, &answer.token),
        enrolment_id: answer.enrolment_id,
        code: answer.code,
        issued_at: answer.issued_at,
        expires_at: answer.expires_at,
        person_label: person.label,
        site_label: site.label,
    })
}

/// Where an enrolment stands on the licence server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobileEnrolmentStatus {
    pub enrolment_id: String,
    /// `pending`, `redeemed`, `cancelled` or `expired`.
    pub status: String,
    pub person_label: String,
    pub site_label: String,
    pub expires_at: Option<String>,
    pub redeemed_at: Option<String>,
}

async fn status_in(db: &DatabaseConnection, server: &Server, enrolment_id: &str) -> AppResult<MobileEnrolmentStatus> {
    let status: MobileEnrolmentStatus = server.get(&format!("{ISSUE_PATH}/{}", path_id(enrolment_id)?)).await?;
    settle_issued(db, &status).await?;
    Ok(status)
}

/// An issued enrolment this PC follows reached its end: a used one is announced to the
/// administrators and audited, once; it then leaves the follow-up list.
async fn settle_issued(db: &DatabaseConnection, status: &MobileEnrolmentStatus) -> AppResult<()> {
    if status.status == "pending" {
        return Ok(());
    }
    let _guard = lock_follow_up().await;
    let mut state = load_follow_up(db).await?;
    let Some(pos) = state.issued.iter().position(|e| e.enrolment_id == status.enrolment_id) else {
        return Ok(());
    };
    let entry = state.issued.remove(pos);
    if status.status == "redeemed" {
        audit(
            db,
            Some(entry.issued_by_user_id),
            AUDIT_PHONE_ENROLLED,
            &entry.enrolment_id,
            "Phone enrolled",
            serde_json::json!({
                "person_label": status.person_label,
                "site_label": status.site_label,
                "redeemed_at": status.redeemed_at,
            }),
        )
        .await;
        notify_enrolled(db, &entry, status).await;
    }
    save_follow_up(db, &state, None, "phone enrolment followed to its end").await
}

async fn notify_enrolled(db: &DatabaseConnection, entry: &IssuedEnrolment, status: &MobileEnrolmentStatus) {
    let params = serde_json::json!({ "person": status.person_label, "site": status.site_label });
    let input = crate::notifications::emitter::NotificationEventInput {
        source_module: "system".to_string(),
        source_record_id: Some(entry.enrolment_id.clone()),
        event_code: ENROLMENT_NOTICE_CATEGORY.to_string(),
        category_code: ENROLMENT_NOTICE_CATEGORY.to_string(),
        severity: "info".to_string(),
        dedupe_key: Some(format!("{ENROLMENT_NOTICE_CATEGORY}:{}", entry.enrolment_id)),
        payload_json: Some(params.to_string()),
        title: format!("Phone enrolled: {}", status.person_label),
        body: Some(format!(
            "A phone was enrolled for {} at {}.",
            status.person_label, status.site_label
        )),
        action_url: Some("/admin?tab=phones".to_string()),
    };
    let message = crate::notifications::emitter::NotificationMessage::new(ENROLMENT_NOTICE_CATEGORY, params);
    if let Err(e) =
        crate::notifications::emitter::emit_device_event_with(db, input, Some(&entry.enrolment_id), Some(&message))
            .await
    {
        tracing::error!(error = %e, "mobile enrolment: enrolment notice not created");
    }
}

async fn cancel_in(
    db: &DatabaseConnection,
    server: &Server,
    caller: &AuthenticatedUser,
    enrolment_id: &str,
) -> AppResult<MobileEnrolmentStatus> {
    let (actor_sync_id, actor_label) = actor_of(db, caller).await?;
    let id = path_id(enrolment_id)?;
    let _: serde_json::Value = server
        .post(
            &format!("{ISSUE_PATH}/{id}/cancel"),
            serde_json::json!({ "actor_sync_id": actor_sync_id, "actor_label": actor_label }),
        )
        .await?;
    audit(
        db,
        Some(caller.user_id),
        AUDIT_ENROLMENT_CANCELLED,
        id,
        "Phone enrolment cancelled",
        serde_json::json!({}),
    )
    .await;
    status_in(db, server, id).await
}

// ── Phones ──────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobilePhone {
    pub device_id: String,
    pub person_sync_id: Option<String>,
    pub person_label: Option<String>,
    pub site_label: Option<String>,
    pub enrolled_by_label: Option<String>,
    pub enrolled_at: Option<String>,
    pub last_sync_at: Option<String>,
    pub app_version: Option<String>,
    /// `active`, or `wipe_pending` (revoked; uploads its pending work, then erases its data).
    pub status: String,
    pub revoked_at: Option<String>,
    pub revoked_by_label: Option<String>,
}

/// The "Téléphones" list: the plant's phones, the licence's phone slots, and the people whose
/// phones this PC still has to revoke.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobilePhonesView {
    pub max_mobile_devices: i64,
    pub active_mobile_devices: i64,
    pub open_enrolments: i64,
    pub devices: Vec<MobilePhone>,
    #[serde(default)]
    pub pending_revocations: Vec<String>,
}

async fn phones_in(db: &DatabaseConnection, server: &Server) -> AppResult<MobilePhonesView> {
    // Revocations waiting on this PC go first, so the list shows their outcome.
    send_pending_revocations(db, server).await;
    let mut view: MobilePhonesView = server.get(DEVICES_PATH).await?;
    view.pending_revocations = load_follow_up(db)
        .await?
        .pending_revocations
        .into_iter()
        .map(|p| p.person_label)
        .collect();
    Ok(view)
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevokeMobilePhoneInput {
    pub device_id: String,
    /// The phone is lost or stolen: it uploads nothing before it is cut off.
    #[serde(default)]
    pub lost: bool,
}

async fn revoke_in(
    db: &DatabaseConnection,
    server: &Server,
    caller: &AuthenticatedUser,
    input: &RevokeMobilePhoneInput,
) -> AppResult<()> {
    let (actor_sync_id, actor_label) = actor_of(db, caller).await?;
    let id = path_id(&input.device_id)?;
    let _: serde_json::Value = server
        .post(
            &format!("{DEVICES_PATH}/{id}/revoke"),
            serde_json::json!({ "actor_sync_id": actor_sync_id, "actor_label": actor_label, "lost": input.lost }),
        )
        .await?;
    audit(
        db,
        Some(caller.user_id),
        AUDIT_PHONE_REVOKED,
        id,
        if input.lost {
            "Phone revoked (lost)"
        } else {
            "Phone revoked"
        },
        serde_json::json!({ "lost": input.lost }),
    )
    .await;
    Ok(())
}

/// Records, in the caller's transaction (`admin_users::set_user_active_impl`, disabling account
/// `user_id`), that all the phones of that person are to be revoked: committed or rolled back with
/// the disable. `_guard` is the follow-up lock, taken before the transaction opened. Sent after the
/// commit by [`send_pending_revocations_soon`], or by the follow-up task.
pub(crate) async fn record_person_revocation_in<C: ConnectionTrait + TransactionTrait>(
    conn: &C,
    _guard: &FollowUpGuard,
    user_id: i64,
    requested_by_user_id: i32,
) -> AppResult<()> {
    if cfg!(target_os = "android") {
        return Ok(());
    }
    let Some(account) = local_account(conn, "id", user_id.into()).await? else {
        return Ok(());
    };
    let mut state = load_follow_up(conn).await?;
    if state.pending_revocations.iter().any(|p| p.user_id == user_id) {
        return Ok(());
    }
    state.pending_revocations.push(PendingRevocation {
        user_id,
        person_label: account.label,
        requested_by_user_id,
        requested_at: Utc::now().to_rfc3339(),
        last_error: None,
    });
    save_follow_up(
        conn,
        &state,
        Some(requested_by_user_id),
        "phones of a disabled account to revoke",
    )
    .await
}

/// After a disable committed: the recorded revocations are sent at once in the background (when
/// the server cannot be reached, the follow-up task sends them later).
pub fn send_pending_revocations_soon(db: &DatabaseConnection) {
    if cfg!(target_os = "android") {
        return;
    }
    let db = db.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(server) = Server::for_background(&db).await {
            send_pending_revocations(&db, &server).await;
        }
    });
}

#[derive(Debug, Deserialize)]
struct RevokePersonAnswer {
    revoked_device_ids: Vec<String>,
    cancelled_enrolment_ids: Vec<String>,
}

/// Revokes every phone and open enrolment of a person on the licence server.
async fn revoke_person_on_server(
    server: &Server,
    person_sync_id: &str,
    author: &Author,
) -> AppResult<RevokePersonAnswer> {
    server
        .post(
            REVOKE_PERSON_PATH,
            serde_json::json!({
                "person_sync_id": person_sync_id,
                "actor_sync_id": author.sync_id,
                "actor_label": author.label,
            }),
        )
        .await
}

/// Sends the revocations waiting on this PC; each one sent (or found no longer needed) is audited
/// and leaves the list. The list is read under the lock, sent without it, and only the entries
/// sent are then removed: one recorded meanwhile stays. False when the server cannot be reached.
async fn send_pending_revocations(db: &DatabaseConnection, server: &Server) -> bool {
    let _sending = SENDING_LOCK.lock().await;
    let pending = {
        let _guard = lock_follow_up().await;
        match load_follow_up(db).await {
            Ok(state) => state.pending_revocations,
            Err(e) => {
                tracing::error!(error = %e, "mobile enrolment: follow-up state not readable");
                return true;
            }
        }
    };
    if pending.is_empty() {
        return true;
    }
    let mut reached = true;
    let mut done: Vec<PendingRevocation> = Vec::new();
    let mut failed: Vec<(PendingRevocation, String)> = Vec::new();
    for entry in pending {
        match send_one_revocation(db, server, &entry).await {
            Ok(()) => done.push(entry),
            Err(e) => {
                reached = !has_code(&e, OFFLINE);
                failed.push((entry, e.to_string()));
                if !reached {
                    break;
                }
            }
        }
    }

    let _guard = lock_follow_up().await;
    let mut state = match load_follow_up(db).await {
        Ok(state) => state,
        Err(e) => {
            tracing::error!(error = %e, "mobile enrolment: follow-up state not readable");
            return reached;
        }
    };
    let mut changed = false;
    state.pending_revocations.retain(|p| {
        let keep = !done.iter().any(|d| d.same_as(p));
        changed |= !keep;
        keep
    });
    for (entry, message) in failed {
        if let Some(p) = state.pending_revocations.iter_mut().find(|p| p.same_as(&entry)) {
            if p.last_error.as_deref() != Some(message.as_str()) {
                p.last_error = Some(message);
                changed = true;
            }
        }
    }
    if changed {
        if let Err(e) = save_follow_up(db, &state, None, "phones of disabled accounts revoked").await {
            tracing::error!(error = %e, "mobile enrolment: follow-up state not saved");
        }
    }
    reached
}

/// One pending revocation: sent, or found no longer needed (the account was enabled again, or was
/// never known to the licence server). Ok means it leaves the list.
async fn send_one_revocation(db: &DatabaseConnection, server: &Server, entry: &PendingRevocation) -> AppResult<()> {
    let account = local_account(db, "id", entry.user_id.into()).await?;
    let Some(person_sync_id) = account.as_ref().and_then(|a| a.sync_id.clone()) else {
        // No sync id: never known to the licence server, so it has no phone.
        return Ok(());
    };
    if account.as_ref().is_some_and(|a| a.in_use) {
        audit(
            db,
            Some(entry.requested_by_user_id),
            AUDIT_REVOCATION_WITHDRAWN,
            &person_sync_id,
            "Revocation of phones withdrawn: the account was enabled again",
            serde_json::json!({ "person_label": entry.person_label, "requested_at": entry.requested_at }),
        )
        .await;
        return Ok(());
    }
    let author = author_of(db, Some(entry.requested_by_user_id)).await?;
    let answer = revoke_person_on_server(server, &person_sync_id, &author).await?;
    audit(
        db,
        Some(entry.requested_by_user_id),
        AUDIT_PHONE_REVOKED,
        &person_sync_id,
        "Phones of a disabled account revoked",
        serde_json::json!({
            "person_label": entry.person_label,
            "revoked_device_ids": answer.revoked_device_ids,
            "cancelled_enrolment_ids": answer.cancelled_enrolment_ids,
            "requested_at": entry.requested_at,
        }),
    )
    .await;
    Ok(())
}

/// The phones the licence server still lets work, compared with the accounts of this PC: the
/// phones of a person disabled or deleted here are revoked (audited, in the name of whoever last
/// changed the account's status). A person this PC does not know (yet) is left alone.
async fn reconcile_phones(db: &DatabaseConnection, server: &Server) -> AppResult<()> {
    let view: MobilePhonesView = server.get(DEVICES_PATH).await?;
    let people: BTreeSet<String> = view
        .devices
        .iter()
        .filter(|d| d.status == PHONE_ACTIVE)
        .filter_map(|d| d.person_sync_id.clone())
        .collect();
    for person_sync_id in people {
        let Some(account) = local_account(db, "entity_sync_id", person_sync_id.clone().into()).await? else {
            continue;
        };
        if account.in_use {
            continue;
        }
        let author = author_of(db, account.status_changed_by_id).await?;
        match revoke_person_on_server(server, &person_sync_id, &author).await {
            Ok(answer) => {
                audit(
                    db,
                    author.user_id,
                    AUDIT_PHONE_REVOKED,
                    &person_sync_id,
                    "Phones of a disabled account revoked (found still active)",
                    serde_json::json!({
                        "person_user_id": account.user_id,
                        "person_label": account.label,
                        "revoked_device_ids": answer.revoked_device_ids,
                        "cancelled_enrolment_ids": answer.cancelled_enrolment_ids,
                    }),
                )
                .await;
            }
            Err(e) if has_code(&e, OFFLINE) => return Err(e),
            Err(e) => {
                tracing::error!(error = %e, "mobile enrolment: phone of a disabled account not revoked");
                audit(
                    db,
                    author.user_id,
                    AUDIT_PHONE_REVOKE_FAILURE,
                    &person_sync_id,
                    "Phones of a disabled account not revoked (retried next round)",
                    serde_json::json!({ "person_user_id": account.user_id, "error": bounded(&e.to_string()) }),
                )
                .await;
            }
        }
    }
    Ok(())
}

/// Follows the enrolments this PC issued: a used one is announced; one the licence server no
/// longer knows is dropped. False when the server cannot be reached.
async fn follow_issued(db: &DatabaseConnection, server: &Server) -> AppResult<bool> {
    let issued = load_follow_up(db).await?.issued;
    for entry in issued {
        let status = match path_id(&entry.enrolment_id) {
            Ok(id) => server.get::<MobileEnrolmentStatus>(&format!("{ISSUE_PATH}/{id}")).await,
            Err(e) => {
                tracing::error!("mobile enrolment: a followed enrolment has an invalid id; dropped");
                Err(e)
            }
        };
        let status = match status {
            Ok(status) => status,
            Err(e) if has_code(&e, OFFLINE) => return Ok(false),
            // Only an enrolment the server does not know (or an id that cannot be asked) ends here
            // without its outcome; any other failure is asked again next round.
            Err(e) if has_code(&e, NOT_FOUND) || has_code(&e, "MOBILE_ENROL_ID_INVALID") => MobileEnrolmentStatus {
                enrolment_id: entry.enrolment_id.clone(),
                status: "expired".into(),
                person_label: entry.person_label.clone(),
                site_label: entry.site_label.clone(),
                expires_at: Some(entry.expires_at.clone()),
                redeemed_at: None,
            },
            Err(e) => {
                tracing::debug!(error = %e, "mobile enrolment: status not read this round");
                continue;
            }
        };
        if let Err(e) = settle_issued(db, &status).await {
            tracing::error!(error = %e, "mobile enrolment: follow-up not recorded");
        }
    }
    Ok(true)
}

/// One round of the follow-up; false when the licence server could not be reached.
async fn follow_up_round(db: &DatabaseConnection, server: &Server) -> bool {
    if !send_pending_revocations(db, server).await {
        return false;
    }
    match reconcile_phones(db, server).await {
        Ok(()) => {}
        Err(e) if has_code(&e, OFFLINE) => return false,
        Err(e) => tracing::error!(error = %e, "mobile enrolment: phones not compared with the accounts"),
    }
    match follow_issued(db, server).await {
        Ok(reached) => reached,
        Err(e) => {
            tracing::error!(error = %e, "mobile enrolment: follow-up state not readable");
            true
        }
    }
}

/// The wait before the next round: [`FOLLOW_UP_INTERVAL`], doubled at each round the server could
/// not be reached, at most [`FOLLOW_UP_MAX_WAIT`].
fn follow_up_wait(unreachable_rounds: u32) -> Duration {
    FOLLOW_UP_INTERVAL
        .saturating_mul(2_u32.saturating_pow(unreachable_rounds.min(16)))
        .min(FOLLOW_UP_MAX_WAIT)
}

/// Background follow-up (desktop only, supervised task): sends pending revocations, revokes the
/// phones of accounts disabled or deleted here, and follows the enrolments this PC issued until
/// they end (a used one is announced). Each round runs as its own task: a panic is logged and the
/// next round comes as usual.
pub async fn run_follow_up(db: DatabaseConnection, cancel: tokio_util::sync::CancellationToken) {
    if cfg!(target_os = "android") {
        return;
    }
    let mut unreachable_rounds: u32 = 0;
    loop {
        let round_db = db.clone();
        let round = tokio::spawn(async move {
            match Server::for_background(&round_db).await {
                Some(server) => follow_up_round(&round_db, &server).await,
                // No active licence: nothing to send, not a connection problem.
                None => true,
            }
        })
        .await;
        unreachable_rounds = match round {
            Ok(true) => 0,
            Ok(false) => unreachable_rounds.saturating_add(1),
            Err(e) => {
                tracing::error!(error = %e, "mobile enrolment: follow-up round failed; next round as usual");
                0
            }
        };
        tokio::select! {
            () = cancel.cancelled() => break,
            () = tokio::time::sleep(follow_up_wait(unreachable_rounds)) => {}
        }
    }
}

// ── Audit ───────────────────────────────────────────────────────────────────────────────────────

const AUDIT_ENROLMENT_ISSUED: &str = "mobile.enrolment_issued";
const AUDIT_ENROLMENT_CANCELLED: &str = "mobile.enrolment_cancelled";
const AUDIT_PHONE_ENROLLED: &str = "mobile.phone_enrolled";
const AUDIT_PHONE_REVOKED: &str = "mobile.phone_revoked";
const AUDIT_REVOCATION_WITHDRAWN: &str = "mobile.phone_revocation_withdrawn";
// "failure" in the name: recorded with the result `fail` (`audit::infer_result`).
const AUDIT_ENROLMENT_ISSUE_FAILURE: &str = "mobile.enrolment_issue_failure";
const AUDIT_ENROLMENT_CANCEL_FAILURE: &str = "mobile.enrolment_cancel_failure";
const AUDIT_PHONE_REVOKE_FAILURE: &str = "mobile.phone_revoke_failure";

async fn audit(
    db: &DatabaseConnection,
    actor_id: Option<i32>,
    event_type: &str,
    entity_id: &str,
    summary: &str,
    detail: serde_json::Value,
) {
    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type,
            actor_id,
            entity_type: Some("mobile_phone"),
            entity_id: Some(entity_id),
            summary,
            detail_json: Some(detail.to_string()),
            ..Default::default()
        },
    )
    .await;
}

/// At most 500 characters of a message, for the audit log.
fn bounded(message: &str) -> String {
    message.chars().take(500).collect()
}

/// A refused or failed attempt (rights, step-up, rules, licence server) is audited with its reason.
async fn audit_failed_attempt<T>(
    db: &DatabaseConnection,
    actor_id: i32,
    event_type: &str,
    entity_id: &str,
    summary: &str,
    mut detail: serde_json::Value,
    result: &AppResult<T>,
) {
    let Err(e) = result else { return };
    if let Some(map) = detail.as_object_mut() {
        map.insert("error".into(), bounded(&e.to_string()).into());
    }
    audit(db, Some(actor_id), event_type, entity_id, summary, detail).await;
}

/// The identifier a refused attempt names, as typed (bounded: it may be anything).
fn attempted_id(id: &str) -> String {
    id.chars().take(64).collect()
}

// ── Commands ────────────────────────────────────────────────────────────────────────────────────

/// People who may be given a phone (those whose account the caller may manage), and the sites a
/// phone can be bound to.
#[tauri::command]
pub async fn list_mobile_enrolment_candidates(state: State<'_, AppState>) -> AppResult<EnrolmentCandidates> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    candidates_in(&state.db, &user).await
}

/// Issues a phone enrolment for one person at one site (step-up required).
#[tauri::command]
pub async fn issue_mobile_enrolment(
    input: IssueMobileEnrolmentInput,
    state: State<'_, AppState>,
) -> AppResult<IssuedMobileEnrolment> {
    let user = require_session!(state);
    let result = async {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        let server = Server::of_this_pc(&state.db).await?;
        issue_in(&state.db, &server, &user, &input).await
    }
    .await;
    audit_failed_attempt(
        &state.db,
        user.user_id,
        AUDIT_ENROLMENT_ISSUE_FAILURE,
        &input.user_id.to_string(),
        "Phone enrolment not issued",
        serde_json::json!({ "person_user_id": input.user_id, "site_node_id": input.site_node_id }),
        &result,
    )
    .await;
    result
}

/// Where an enrolment stands (the QR dialog follows it).
#[tauri::command]
pub async fn get_mobile_enrolment_status(
    enrolment_id: String,
    state: State<'_, AppState>,
) -> AppResult<MobileEnrolmentStatus> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    let server = Server::of_this_pc(&state.db).await?;
    status_in(&state.db, &server, &enrolment_id).await
}

/// Cancels an enrolment not used yet (it only removes access, so no step-up).
#[tauri::command]
pub async fn cancel_mobile_enrolment(
    enrolment_id: String,
    state: State<'_, AppState>,
) -> AppResult<MobileEnrolmentStatus> {
    let user = require_session!(state);
    let result = async {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        let server = Server::of_this_pc(&state.db).await?;
        cancel_in(&state.db, &server, &user, &enrolment_id).await
    }
    .await;
    audit_failed_attempt(
        &state.db,
        user.user_id,
        AUDIT_ENROLMENT_CANCEL_FAILURE,
        &attempted_id(&enrolment_id),
        "Phone enrolment not cancelled",
        serde_json::json!({}),
        &result,
    )
    .await;
    result
}

/// The plant's phones.
#[tauri::command]
pub async fn list_mobile_phones(state: State<'_, AppState>) -> AppResult<MobilePhonesView> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    let server = Server::of_this_pc(&state.db).await?;
    phones_in(&state.db, &server).await
}

/// Revokes a phone (step-up required).
#[tauri::command]
pub async fn revoke_mobile_phone(input: RevokeMobilePhoneInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    let result = async {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_USERS,
            PermissionScope::Global
        );
        require_step_up!(state);
        let server = Server::of_this_pc(&state.db).await?;
        revoke_in(&state.db, &server, &user, &input).await
    }
    .await;
    audit_failed_attempt(
        &state.db,
        user.user_id,
        AUDIT_PHONE_REVOKE_FAILURE,
        &attempted_id(&input.device_id),
        "Phone not revoked",
        serde_json::json!({ "lost": input.lost }),
        &result,
    )
    .await;
    result
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn qr_payload_holds_only_the_server_and_the_token() {
        let payload = qr_payload("https://api.maintafox.systems/", "Ab-_9xyz");
        assert_eq!(
            payload,
            "maintafox-enrol:1?s=https%3A%2F%2Fapi.maintafox.systems&t=Ab-_9xyz"
        );
    }

    #[test]
    fn path_ids_cannot_change_the_path() {
        assert!(path_id("enr_0123abcd").is_ok());
        for bad in ["", "../x", "a/b", "a?b", "a b", &"x".repeat(65)] {
            assert!(path_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn server_refusals_have_their_codes() {
        let code = |status, body: &str| server_refusal(status, body).to_string();
        assert!(code(403, r#"{"error":"mobile_not_licensed"}"#).contains("MOBILE_ENROL_NOT_LICENSED"));
        assert!(code(409, r#"{"error":"slot_limit_reached"}"#).contains("MOBILE_ENROL_NO_FREE_SLOT"));
        assert!(code(404, r#"{"error":"device_not_found"}"#).contains("MOBILE_ENROL_NOT_FOUND"));
        assert!(code(500, "not json").contains("MOBILE_ENROL_SERVER_REFUSED"));
        assert!(has_code(
            &server_refusal(404, r#"{"error":"enrolment_not_found"}"#),
            NOT_FOUND
        ));
        assert!(!has_code(&server_refusal(500, "{}"), NOT_FOUND));
    }

    #[test]
    fn the_token_and_the_code_never_reach_a_log() {
        let answer = IssueAnswer {
            enrolment_id: "enr_1".into(),
            token: "tok-secret-91".into(),
            code: "CODE7731".into(),
            issued_at: "2026-10-29T10:00:00Z".into(),
            expires_at: "2026-10-29T10:15:00Z".into(),
        };
        let issued = IssuedMobileEnrolment {
            enrolment_id: "enr_1".into(),
            qr_payload: qr_payload("https://api.example.test", "tok-secret-91"),
            code: "CODE7731".into(),
            issued_at: answer.issued_at.clone(),
            expires_at: answer.expires_at.clone(),
            person_label: "Tech".into(),
            site_label: "Site".into(),
        };
        for shown in [format!("{answer:?}"), format!("{issued:?}")] {
            assert!(shown.contains("enr_1"), "{shown}");
            assert!(
                !shown.contains("tok-secret-91") && !shown.contains("CODE7731"),
                "{shown}"
            );
        }
        // An unreadable answer is described by its place, never by the value it holds.
        let e = serde_json::from_str::<IssueAnswer>(r#"{"enrolment_id":"enr_1","token":91731}"#).unwrap_err();
        assert!(e.to_string().contains("91731"), "serde names the value");
        assert!(!json_error_place(&e).contains("91731"), "{}", json_error_place(&e));
        let refused = read_answer::<IssueAnswer>("/x", (200, r#"{"token":"tok-secret-91"#.into())).unwrap_err();
        assert!(!refused.to_string().contains("tok-secret-91"));
    }

    #[test]
    fn the_follow_up_backs_off_while_the_server_cannot_be_reached() {
        assert_eq!(follow_up_wait(0), FOLLOW_UP_INTERVAL);
        assert_eq!(follow_up_wait(1), FOLLOW_UP_INTERVAL * 2);
        assert_eq!(follow_up_wait(3), FOLLOW_UP_INTERVAL * 8);
        assert_eq!(follow_up_wait(50), FOLLOW_UP_MAX_WAIT);
    }

    // ── Fixtures ────────────────────────────────────────────────────────────────────────────────

    /// One request the stub licence server received.
    #[derive(Debug, Clone)]
    struct Received {
        method: String,
        path: String,
        body: String,
    }

    /// A licence server on 127.0.0.1 answering each request with `route(method, path)` (one
    /// request per connection), recording each request.
    struct Stub {
        server: Server,
        received: Arc<StdMutex<Vec<Received>>>,
    }

    impl Stub {
        fn received(&self) -> Vec<Received> {
            self.received.lock().expect("received").clone()
        }
    }

    async fn start_stub(route: impl Fn(&str, &str) -> (u16, String) + Send + 'static) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        let received = Arc::new(StdMutex::new(Vec::new()));
        let recorded = received.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = Vec::new();
                let mut chunk = [0_u8; 8 * 1024];
                let header_end = loop {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break None,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break Some(p + 4);
                    }
                };
                let Some(header_end) = header_end else { continue };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let mut first = head.lines().next().unwrap_or_default().split(' ');
                let method = first.next().unwrap_or_default().to_string();
                let path = first.next().unwrap_or_default().to_string();
                let length: usize = head
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, v)| v.trim().parse().ok())
                    .unwrap_or(0);
                while buf.len() < header_end + length {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let end = buf.len().min(header_end + length);
                let (status, body) = route(&method, &path);
                recorded.lock().expect("received").push(Received {
                    method,
                    path,
                    body: String::from_utf8_lossy(&buf[header_end..end]).to_string(),
                });
                let response = format!(
                    "HTTP/1.1 {status} STUB\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Stub {
            server: Server {
                base_url,
                token: "desktop-token-test".into(),
            },
            received,
        }
    }

    /// A licence server address nothing listens on.
    fn unreachable_server() -> Server {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        let addr = probe.local_addr().expect("addr");
        drop(probe);
        Server {
            base_url: format!("http://{addr}"),
            token: "desktop-token-test".into(),
        }
    }

    async fn scalar(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
        db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await
            .expect("query")
            .expect("row")
            .try_get::<i64>("", "v")
            .expect("decode")
    }

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    /// A local account signing in with its own password, sync id `<username>-sync`.
    async fn insert_account(db: &DatabaseConnection, username: &str, is_admin: bool) -> i64 {
        let now = Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts \
             (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin, \
              force_password_change, failed_login_attempts, created_at, updated_at, row_version) \
             VALUES (?, ?, 'local', 'x', 1, ?, 0, 0, ?, ?, 1)",
            [
                format!("{username}-sync").into(),
                username.into(),
                i32::from(is_admin).into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert account");
        scalar(
            db,
            "SELECT id AS v FROM user_accounts WHERE username = ?",
            vec![username.into()],
        )
        .await
    }

    /// An account holding only the user-administration right, plant-wide (not a plant administrator).
    async fn user_manager(db: &DatabaseConnection, username: &str) -> AuthenticatedUser {
        let now = Utc::now().to_rfc3339();
        let role = format!("{username}-role");
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version) \
             VALUES (?, ?, 'custom', 'active', 0, ?, ?, 1)",
            [
                format!("{role}-sync").into(),
                role.clone().into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await
        .expect("role");
        let role_id = scalar(db, "SELECT id AS v FROM roles WHERE name = ?", vec![role.into()]).await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) SELECT ?, id, ? FROM permissions WHERE name = ?",
            [
                role_id.into(),
                now.clone().into(),
                crate::rbac::permissions::ADM_USERS.into(),
            ],
        ))
        .await
        .expect("permission");
        let user = insert_account(db, username, false).await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, created_at, updated_at, row_version) \
             VALUES (?, ?, ?, 'tenant', ?, ?, 1)",
            [
                format!("{username}-usa").into(),
                user.into(),
                role_id.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("assignment");
        caller_for(user, username)
    }

    fn caller_for(user_id: i64, username: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: i32::try_from(user_id).expect("fits"),
            username: username.into(),
            display_name: None,
            is_admin: false,
            force_password_change: false,
            tenant_id: "tenant-test".into(),
            token_tenant_id: "tenant-test".into(),
        }
    }

    async fn plant_admin(db: &DatabaseConnection) -> AuthenticatedUser {
        let id = scalar(db, "SELECT id AS v FROM user_accounts WHERE username = 'admin'", vec![]).await;
        AuthenticatedUser {
            is_admin: true,
            ..caller_for(id, "admin")
        }
    }

    /// A site (node type that can own work, active structure model); returns its node id.
    async fn seed_site(db: &DatabaseConnection) -> i64 {
        exec(
            db,
            "INSERT INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
             VALUES (951, 'mob-model', 1, 'active', datetime('now'), datetime('now'))",
        )
        .await;
        exec(
            db,
            "INSERT INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, can_own_work, is_active, \
              created_at, updated_at) \
             VALUES (951, 'mob-nt', 951, 'SITE', 'Site', 1, 1, datetime('now'), datetime('now'))",
        )
        .await;
        exec(
            db,
            "INSERT INTO org_nodes (id, entity_sync_id, structure_model_id, code, name, node_type_id, status, \
              created_at, updated_at) \
             VALUES (951, 'mob-site', 951, 'SITE-M', 'Site M', 951, 'active', datetime('now'), datetime('now'))",
        )
        .await;
        951
    }

    async fn audited(db: &DatabaseConnection, action: &str, target: &str) -> i64 {
        scalar(
            db,
            "SELECT COUNT(*) AS v FROM audit_events WHERE action_code = ? AND target_id = ?",
            vec![action.into(), target.into()],
        )
        .await
    }

    async fn set_active(state: &AppState, caller: &AuthenticatedUser, user_id: i64, active: bool) -> AppResult<()> {
        crate::commands::admin_users::set_user_active_impl(
            state,
            caller,
            crate::commands::admin_users::SetUserActiveInput {
                user_id,
                reason: "test".into(),
            },
            active,
        )
        .await
    }

    fn issue_answer() -> String {
        serde_json::json!({
            "enrolment_id": "enr_issued1",
            "token": "tok-one-time-51",
            "code": "K7Q2M9XA",
            "issued_at": "2026-10-29T10:00:00Z",
            "expires_at": "2026-10-29T10:15:00Z",
        })
        .to_string()
    }

    // ── Who may be given a phone ────────────────────────────────────────────────────────────────

    /// A user manager who is not a plant administrator cannot give a phone (access) to an
    /// administrator account, nor see it offered; an ordinary technician can be given one.
    #[tokio::test]
    async fn a_phone_is_issued_only_for_an_account_the_caller_may_manage() {
        let db = crate::db::test_db::seeded().await;
        let manager = user_manager(&db, "phone_manager").await;
        let administrator = insert_account(&db, "phone_admin_target", true).await;
        let technician = insert_account(&db, "phone_technician", false).await;
        let site = seed_site(&db).await;
        let stub = start_stub(|method, path| match (method, path) {
            ("POST", ISSUE_PATH) => (200, issue_answer()),
            _ => (404, r#"{"error":"not_found"}"#.into()),
        })
        .await;

        let candidates = candidates_in(&db, &manager).await.expect("candidates");
        let offered: Vec<i32> = candidates.people.iter().map(|p| p.user_id).collect();
        assert!(offered.contains(&i32::try_from(technician).unwrap()), "{offered:?}");
        assert!(!offered.contains(&i32::try_from(administrator).unwrap()), "{offered:?}");

        let refused = issue_in(
            &db,
            &stub.server,
            &manager,
            &IssueMobileEnrolmentInput {
                user_id: i32::try_from(administrator).unwrap(),
                site_node_id: site,
            },
        )
        .await;
        assert!(matches!(refused, Err(AppError::PermissionDenied(_))), "{refused:?}");
        assert!(stub.received().is_empty(), "nothing asked of the licence server");

        let issued = issue_in(
            &db,
            &stub.server,
            &manager,
            &IssueMobileEnrolmentInput {
                user_id: i32::try_from(technician).unwrap(),
                site_node_id: site,
            },
        )
        .await
        .expect("issued for a technician");
        assert_eq!(issued.enrolment_id, "enr_issued1");
        assert!(
            issued.qr_payload.ends_with("&t=tok-one-time-51"),
            "{}",
            issued.qr_payload
        );
        let sent = stub.received();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].body.contains("phone_technician-sync"), "{}", sent[0].body);
        assert_eq!(audited(&db, AUDIT_ENROLMENT_ISSUED, "enr_issued1").await, 1);
        let followed = load_follow_up(&db).await.expect("state").issued;
        assert_eq!(followed.len(), 1);
        assert_eq!(followed[0].enrolment_id, "enr_issued1");
    }

    // ── Revocation of a disabled account's phones ───────────────────────────────────────────────

    /// The revocation is written by the disable's own transaction: a disable that fails late
    /// leaves no revocation, a disable that commits leaves exactly one.
    #[tokio::test]
    async fn the_revocation_is_recorded_and_rolled_back_with_the_disable() {
        let db = crate::db::test_db::seeded().await;
        let state = AppState::new(db.clone());
        let admin = plant_admin(&db).await;
        let technician = insert_account(&db, "leaving_technician", false).await;

        // The ledger row comes after the revocation in the transaction: refusing it fails the
        // disable once the revocation was recorded.
        exec(
            &db,
            "CREATE TRIGGER block_admin_ledger BEFORE INSERT ON admin_change_events \
             BEGIN SELECT RAISE(ABORT, 'disk full'); END",
        )
        .await;
        assert!(set_active(&state, &admin, technician, false).await.is_err());
        let still_active = scalar(
            &db,
            "SELECT is_active AS v FROM user_accounts WHERE id = ?",
            vec![technician.into()],
        )
        .await;
        assert_eq!(still_active, 1, "the disable was rolled back");
        assert!(
            load_follow_up(&db).await.expect("state").pending_revocations.is_empty(),
            "the revocation was rolled back with it"
        );

        exec(&db, "DROP TRIGGER block_admin_ledger").await;
        set_active(&state, &admin, technician, false).await.expect("disable");
        let pending = load_follow_up(&db).await.expect("state").pending_revocations;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].user_id, technician);
        assert_eq!(pending[0].person_label, "leaving_technician");
        assert_eq!(pending[0].requested_by_user_id, admin.user_id);
    }

    /// A recorded revocation is sent and leaves the list; one whose account was enabled again
    /// before it could be sent is withdrawn without asking the server.
    #[tokio::test]
    async fn pending_revocations_are_sent_or_withdrawn_when_the_account_is_back() {
        let db = crate::db::test_db::seeded().await;
        let state = AppState::new(db.clone());
        let admin = plant_admin(&db).await;
        let gone = insert_account(&db, "gone_technician", false).await;
        let back = insert_account(&db, "back_technician", false).await;
        set_active(&state, &admin, gone, false).await.expect("disable");
        set_active(&state, &admin, back, false).await.expect("disable");
        set_active(&state, &admin, back, true).await.expect("enable again");

        // Offline: nothing leaves the list, the reason is kept.
        assert!(!send_pending_revocations(&db, &unreachable_server()).await);
        let pending = load_follow_up(&db).await.expect("state").pending_revocations;
        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .any(|p| p.last_error.as_deref().is_some_and(|e| e.contains(OFFLINE))));

        let stub = start_stub(|method, path| match (method, path) {
            ("POST", REVOKE_PERSON_PATH) => (
                200,
                r#"{"revoked_device_ids":["act_gone1"],"cancelled_enrolment_ids":[]}"#.into(),
            ),
            _ => (404, r#"{"error":"not_found"}"#.into()),
        })
        .await;
        assert!(send_pending_revocations(&db, &stub.server).await);
        let sent = stub.received();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(sent[0].body.contains("gone_technician-sync"), "{}", sent[0].body);
        assert!(load_follow_up(&db).await.expect("state").pending_revocations.is_empty());
        assert_eq!(audited(&db, AUDIT_PHONE_REVOKED, "gone_technician-sync").await, 1);
        assert_eq!(
            audited(&db, AUDIT_REVOCATION_WITHDRAWN, "back_technician-sync").await,
            1
        );
    }

    /// Each round compares the server's phones with the accounts here: an active phone of a person
    /// disabled here (by sync, a lost record or another PC) is revoked and audited; the phones of
    /// active people and of people this PC does not know are left alone.
    #[tokio::test]
    async fn a_round_revokes_the_phones_of_people_disabled_here() {
        let db = crate::db::test_db::seeded().await;
        let admin = plant_admin(&db).await;
        let disabled = insert_account(&db, "disabled_elsewhere", false).await;
        insert_account(&db, "still_working", false).await;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts SET is_active = 0, status_changed_by_id = ? WHERE id = ?",
            [admin.user_id.into(), disabled.into()],
        ))
        .await
        .expect("disabled without this PC's command");
        let phone = |id: &str, person: &str, status: &str| {
            serde_json::json!({
                "device_id": id, "person_sync_id": person, "person_label": person, "site_label": "Site M",
                "enrolled_by_label": "admin", "enrolled_at": null, "last_sync_at": null, "app_version": null,
                "status": status, "revoked_at": null, "revoked_by_label": null,
            })
        };
        let devices = serde_json::json!({
            "max_mobile_devices": 5,
            "active_mobile_devices": 4,
            "open_enrolments": 0,
            "devices": [
                phone("act_1", "disabled_elsewhere-sync", "active"),
                phone("act_2", "still_working-sync", "active"),
                phone("act_3", "unknown-person-sync", "active"),
                phone("act_4", "disabled_elsewhere-sync", "wipe_pending"),
            ],
        })
        .to_string();
        let stub = start_stub(move |method, path| match (method, path) {
            ("GET", DEVICES_PATH) => (200, devices.clone()),
            ("POST", REVOKE_PERSON_PATH) => (
                200,
                r#"{"revoked_device_ids":["act_1"],"cancelled_enrolment_ids":[]}"#.into(),
            ),
            _ => (404, r#"{"error":"not_found"}"#.into()),
        })
        .await;

        assert!(follow_up_round(&db, &stub.server).await);
        let revokes: Vec<Received> = stub
            .received()
            .into_iter()
            .filter(|r| r.method == "POST" && r.path == REVOKE_PERSON_PATH)
            .collect();
        assert_eq!(revokes.len(), 1, "{revokes:?}");
        let body: serde_json::Value = serde_json::from_str(&revokes[0].body).expect("body");
        assert_eq!(body["person_sync_id"], "disabled_elsewhere-sync");
        assert_eq!(audited(&db, AUDIT_PHONE_REVOKED, "disabled_elsewhere-sync").await, 1);
    }

    /// An issued enrolment ends without its outcome only when the server no longer knows it; a
    /// server error or no connection keeps it followed (a used one must still be announced).
    #[tokio::test]
    async fn an_issued_enrolment_is_dropped_only_when_the_server_does_not_know_it() {
        let db = crate::db::test_db::seeded().await;
        let entry = |id: &str| IssuedEnrolment {
            enrolment_id: id.into(),
            person_label: "Tech".into(),
            site_label: "Site M".into(),
            issued_by_user_id: 1,
            expires_at: "2020-01-01T00:00:00Z".into(),
        };
        let state = FollowUp {
            issued: vec![entry("enr_gone"), entry("enr_flaky")],
            pending_revocations: Vec::new(),
        };
        save_follow_up(&db, &state, None, "test").await.expect("save");

        assert!(!follow_issued(&db, &unreachable_server()).await.expect("round"));
        assert_eq!(
            load_follow_up(&db).await.expect("state").issued.len(),
            2,
            "offline: kept"
        );

        let stub = start_stub(|_, path| match path {
            "/api/v1/mobile-enrolments/enr_gone" => (404, r#"{"error":"enrolment_not_found"}"#.into()),
            _ => (500, r#"{"error":"internal"}"#.into()),
        })
        .await;
        assert!(follow_issued(&db, &stub.server).await.expect("round"));
        let left: Vec<String> = load_follow_up(&db)
            .await
            .expect("state")
            .issued
            .into_iter()
            .map(|e| e.enrolment_id)
            .collect();
        assert_eq!(left, vec!["enr_flaky".to_string()]);
    }
}
