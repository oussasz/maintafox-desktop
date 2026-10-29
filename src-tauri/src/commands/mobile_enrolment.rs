//! Technician phones, as the desktop manages them (owner-approved 2026-10-29, decision register:
//! admin-issued QR enrolment). The licence key is never given to a phone.
//!
//! An administrator (`adm.users`, plus a fresh step-up for what grants or removes access) issues an
//! enrolment for ONE person at ONE site; the licence server answers a one-time QR token and an
//! 8-character code, valid 15 minutes, single use, cancellable. The QR carries only the server
//! address and that token. The phone that scans it is bound to that person and site.
//!
//! The "Téléphones" list shows the phones of the plant (person, site, enrolled by / at, last sync)
//! with "Révoquer": the slot is freed at once; at its next contact the phone uploads its pending
//! work, then erases its data (a phone reported lost uploads nothing). Disabling a person's account
//! revokes all their phones ([`queue_person_revocation`], called by `admin_users`): when this PC is
//! offline the revocation waits here and is sent by the follow-up task as soon as it can be.
//!
//! What this PC issued is followed until the phone uses it, so the administrators get a notice of
//! each enrolment ([`ENROLMENT_NOTICE_CATEGORY`]). Every step is in the audit log here and in the
//! licence server's ledger.
//!
//! The follow-up state (issued enrolments, pending revocations) is a device setting of this PC: it
//! names enrolments and people, never a secret.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex;

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
/// How often the follow-up task looks at open enrolments and pending revocations.
const FOLLOW_UP_INTERVAL: Duration = Duration::from_secs(60);
/// The QR payload's scheme and version; the phone refuses anything else.
pub const QR_PAYLOAD_PREFIX: &str = "maintafox-enrol:1?";

const ISSUE_PATH: &str = "/api/v1/mobile-enrolments";
const DEVICES_PATH: &str = "/api/v1/mobile-devices";
const REVOKE_PERSON_PATH: &str = "/api/v1/mobile-devices/revoke-person";

/// One read-modify-write of the follow-up state at a time on this PC.
static FOLLOW_UP_LOCK: Mutex<()> = Mutex::const_new(());

fn refused(code: &str, message: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {message}")])
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
        "enrolment_not_found" | "device_not_found" => refused(
            "MOBILE_ENROL_NOT_FOUND",
            "The licence server does not know this enrolment or phone.",
        ),
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
        "MOBILE_ENROL_OFFLINE",
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

fn client() -> AppResult<ControlPlaneClient> {
    ControlPlaneClient::new(
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        crate::sync::transport::client::ClientTimeouts::interactive(),
    )
    .map_err(AppError::SyncError)
}

async fn server_post<T: for<'de> Deserialize<'de>>(token: &str, path: &str, body: serde_json::Value) -> AppResult<T> {
    let answer = client()?
        .post_json_raw(path, Some(token), body.to_string())
        .await
        .map_err(|e| transport_failure(&e))?;
    read_answer(path, answer)
}

async fn server_get<T: for<'de> Deserialize<'de>>(token: &str, path: &str) -> AppResult<T> {
    let answer = client()?
        .get_json_raw(path, Some(token))
        .await
        .map_err(|e| transport_failure(&e))?;
    read_answer(path, answer)
}

fn read_answer<T: for<'de> Deserialize<'de>>(path: &str, (status, body): (u16, String)) -> AppResult<T> {
    if !(200..300).contains(&status) {
        return Err(server_refusal(status, &body));
    }
    serde_json::from_str(&body).map_err(|e| {
        tracing::warn!(path, error = %e, "mobile enrolment: unreadable licence server answer");
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

async fn candidates_in(db: &DatabaseConnection) -> AppResult<EnrolmentCandidates> {
    let people = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{PERSON_SQL} ORDER BY label COLLATE NOCASE"),
        ))
        .await?
        .iter()
        .map(person_of)
        .collect::<AppResult<Vec<_>>>()?;
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

/// The phones of a disabled person, still to revoke on the licence server.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRevocation {
    person_sync_id: String,
    person_label: String,
    requested_by_user_id: i32,
    requested_by_sync_id: String,
    requested_by_label: String,
    requested_at: String,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FollowUp {
    #[serde(default)]
    issued: Vec<IssuedEnrolment>,
    #[serde(default)]
    pending_revocations: Vec<PendingRevocation>,
}

async fn load_follow_up(db: &DatabaseConnection) -> AppResult<FollowUp> {
    let Some(row) = crate::settings::get_setting(db, FOLLOW_UP_KEY, FOLLOW_UP_SCOPE).await? else {
        return Ok(FollowUp::default());
    };
    serde_json::from_str(&row.setting_value_json).map_err(|e| {
        // Never rebuilt silently: a pending revocation must not be lost.
        AppError::Internal(anyhow::anyhow!("mobile enrolment follow-up state unreadable: {e}"))
    })
}

async fn save_follow_up(db: &DatabaseConnection, state: &FollowUp, actor: Option<i32>, why: &str) -> AppResult<()> {
    crate::settings::set_setting(
        db,
        FOLLOW_UP_KEY,
        FOLLOW_UP_SCOPE,
        &serde_json::to_string(state)?,
        actor,
        why,
    )
    .await
}

// ── Enrolments ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct IssueMobileEnrolmentInput {
    pub user_id: i32,
    pub site_node_id: i64,
}

#[derive(Debug, Deserialize)]
struct IssueAnswer {
    enrolment_id: String,
    token: String,
    code: String,
    issued_at: String,
    expires_at: String,
}

/// What the administrator shows the technician: the QR (server address + one-time token), the
/// typed code, and until when they work.
#[derive(Debug, Clone, Serialize)]
pub struct IssuedMobileEnrolment {
    pub enrolment_id: String,
    pub qr_payload: String,
    pub code: String,
    pub issued_at: String,
    pub expires_at: String,
    pub person_label: String,
    pub site_label: String,
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
    let token = desktop_bearer(db).await?;
    let answer: IssueAnswer = server_post(
        &token,
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

    {
        let _guard = FOLLOW_UP_LOCK.lock().await;
        let mut state = load_follow_up(db).await?;
        state.issued.push(IssuedEnrolment {
            enrolment_id: answer.enrolment_id.clone(),
            person_label: person.label.clone(),
            site_label: site.label.clone(),
            issued_by_user_id: caller.user_id,
            expires_at: answer.expires_at.clone(),
        });
        save_follow_up(db, &state, Some(caller.user_id), "phone enrolment issued").await?;
    }
    audit(
        db,
        caller.user_id,
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
    Ok(IssuedMobileEnrolment {
        qr_payload: qr_payload(env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"), &answer.token),
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

async fn status_in(db: &DatabaseConnection, enrolment_id: &str) -> AppResult<MobileEnrolmentStatus> {
    let token = desktop_bearer(db).await?;
    let status: MobileEnrolmentStatus = server_get(&token, &format!("{ISSUE_PATH}/{}", path_id(enrolment_id)?)).await?;
    settle_issued(db, &status).await?;
    Ok(status)
}

/// An issued enrolment this PC follows reached its end: a used one is announced to the
/// administrators and audited, once; it then leaves the follow-up list.
async fn settle_issued(db: &DatabaseConnection, status: &MobileEnrolmentStatus) -> AppResult<()> {
    if status.status == "pending" {
        return Ok(());
    }
    let _guard = FOLLOW_UP_LOCK.lock().await;
    let mut state = load_follow_up(db).await?;
    let Some(pos) = state.issued.iter().position(|e| e.enrolment_id == status.enrolment_id) else {
        return Ok(());
    };
    let entry = state.issued.remove(pos);
    if status.status == "redeemed" {
        audit(
            db,
            entry.issued_by_user_id,
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
    caller: &AuthenticatedUser,
    enrolment_id: &str,
) -> AppResult<MobileEnrolmentStatus> {
    let (actor_sync_id, actor_label) = actor_of(db, caller).await?;
    let token = desktop_bearer(db).await?;
    let id = path_id(enrolment_id)?;
    let _: serde_json::Value = server_post(
        &token,
        &format!("{ISSUE_PATH}/{id}/cancel"),
        serde_json::json!({ "actor_sync_id": actor_sync_id, "actor_label": actor_label }),
    )
    .await?;
    audit(
        db,
        caller.user_id,
        AUDIT_ENROLMENT_CANCELLED,
        id,
        "Phone enrolment cancelled",
        serde_json::json!({}),
    )
    .await;
    status_in(db, id).await
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

async fn phones_in(db: &DatabaseConnection) -> AppResult<MobilePhonesView> {
    let token = desktop_bearer(db).await?;
    // Revocations waiting on this PC go first, so the list shows their outcome.
    send_pending_revocations(db, &token).await;
    let mut view: MobilePhonesView = server_get(&token, DEVICES_PATH).await?;
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
    caller: &AuthenticatedUser,
    input: &RevokeMobilePhoneInput,
) -> AppResult<()> {
    let (actor_sync_id, actor_label) = actor_of(db, caller).await?;
    let token = desktop_bearer(db).await?;
    let id = path_id(&input.device_id)?;
    let _: serde_json::Value = server_post(
        &token,
        &format!("{DEVICES_PATH}/{id}/revoke"),
        serde_json::json!({ "actor_sync_id": actor_sync_id, "actor_label": actor_label, "lost": input.lost }),
    )
    .await?;
    audit(
        db,
        caller.user_id,
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

/// Called after an account is disabled (`admin_users::deactivate_user`): every phone and open
/// enrolment of that person is revoked on the licence server. Recorded here first, then sent at
/// once in the background; while the server cannot be reached it stays recorded and the follow-up
/// task sends it as soon as it can. Never fails the disabling of the account.
pub async fn queue_person_revocation(db: &DatabaseConnection, caller: &AuthenticatedUser, user_id: i64) {
    if cfg!(target_os = "android") {
        return;
    }
    match record_person_revocation(db, caller, user_id).await {
        Ok(true) => {
            let db = db.clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(Some(token)) = crate::commands::product_license::sync_activation_bearer_token(&db).await {
                    send_pending_revocations(&db, &token).await;
                }
            });
        }
        Ok(false) => {}
        Err(e) => tracing::error!(
            error = %e,
            user_id,
            "mobile enrolment: revocation of a disabled account's phones not recorded"
        ),
    }
}

/// Records the revocation of `user_id`'s phones; false when the account has no sync id (never
/// known to the licence server, so it has no phone).
async fn record_person_revocation(
    db: &DatabaseConnection,
    caller: &AuthenticatedUser,
    user_id: i64,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id, COALESCE(NULLIF(TRIM(display_name), ''), username) AS label \
             FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(false) };
    let sync_id: Option<String> = row.try_get("", "entity_sync_id").map_err(|e| decode_err("person", e))?;
    let Some(person_sync_id) = sync_id else {
        return Ok(false);
    };
    let label: String = row.try_get("", "label").map_err(|e| decode_err("person", e))?;
    let (actor_sync_id, actor_label) = actor_of(db, caller).await?;
    let _guard = FOLLOW_UP_LOCK.lock().await;
    let mut state = load_follow_up(db).await?;
    if !state
        .pending_revocations
        .iter()
        .any(|p| p.person_sync_id == person_sync_id)
    {
        state.pending_revocations.push(PendingRevocation {
            person_sync_id,
            person_label: label,
            requested_by_user_id: caller.user_id,
            requested_by_sync_id: actor_sync_id,
            requested_by_label: actor_label,
            requested_at: Utc::now().to_rfc3339(),
            last_error: None,
        });
        save_follow_up(
            db,
            &state,
            Some(caller.user_id),
            "phones of a disabled account to revoke",
        )
        .await?;
    }
    Ok(true)
}

#[derive(Debug, Deserialize)]
struct RevokePersonAnswer {
    revoked_device_ids: Vec<String>,
    cancelled_enrolment_ids: Vec<String>,
}

/// Sends the revocations waiting on this PC; each one sent is audited and leaves the list.
async fn send_pending_revocations(db: &DatabaseConnection, token: &str) {
    let _guard = FOLLOW_UP_LOCK.lock().await;
    let mut state = match load_follow_up(db).await {
        Ok(state) => state,
        Err(e) => {
            tracing::error!(error = %e, "mobile enrolment: follow-up state not readable");
            return;
        }
    };
    if state.pending_revocations.is_empty() {
        return;
    }
    let mut changed = false;
    let mut still_pending = Vec::new();
    for mut pending in std::mem::take(&mut state.pending_revocations) {
        let answer: AppResult<RevokePersonAnswer> = server_post(
            token,
            REVOKE_PERSON_PATH,
            serde_json::json!({
                "person_sync_id": pending.person_sync_id,
                "actor_sync_id": pending.requested_by_sync_id,
                "actor_label": pending.requested_by_label,
            }),
        )
        .await;
        match answer {
            Ok(answer) => {
                changed = true;
                audit(
                    db,
                    pending.requested_by_user_id,
                    AUDIT_PHONE_REVOKED,
                    &pending.person_sync_id,
                    "Phones of a disabled account revoked",
                    serde_json::json!({
                        "person_label": pending.person_label,
                        "revoked_device_ids": answer.revoked_device_ids,
                        "cancelled_enrolment_ids": answer.cancelled_enrolment_ids,
                        "requested_at": pending.requested_at,
                    }),
                )
                .await;
            }
            Err(e) => {
                let message = e.to_string();
                if pending.last_error.as_deref() != Some(message.as_str()) {
                    pending.last_error = Some(message);
                    changed = true;
                }
                still_pending.push(pending);
            }
        }
    }
    state.pending_revocations = still_pending;
    if changed {
        if let Err(e) = save_follow_up(db, &state, None, "phones of disabled accounts revoked").await {
            tracing::error!(error = %e, "mobile enrolment: follow-up state not saved");
        }
    }
}

/// Background follow-up (desktop only): sends pending revocations, and follows the enrolments this
/// PC issued until they end (a used one is announced). Started at start-up.
pub async fn start_follow_up(db: DatabaseConnection) {
    if cfg!(target_os = "android") {
        return;
    }
    let mut ticker = tokio::time::interval(FOLLOW_UP_INTERVAL);
    loop {
        ticker.tick().await;
        let Ok(Some(token)) = crate::commands::product_license::sync_activation_bearer_token(&db).await else {
            continue;
        };
        send_pending_revocations(&db, &token).await;
        let issued = match load_follow_up(&db).await {
            Ok(state) => state.issued,
            Err(e) => {
                tracing::error!(error = %e, "mobile enrolment: follow-up state not readable");
                continue;
            }
        };
        for entry in issued {
            let status = match server_get::<MobileEnrolmentStatus>(
                &token,
                &format!("{ISSUE_PATH}/{}", entry.enrolment_id),
            )
            .await
            {
                Ok(status) => status,
                Err(e) => {
                    tracing::debug!(error = %e, "mobile enrolment: status not read this round");
                    // One the server no longer answers for is dropped a day after it expired.
                    let long_past = DateTime::parse_from_rfc3339(&entry.expires_at)
                        .map(|t| Utc::now() - t.with_timezone(&Utc) > chrono::Duration::days(1))
                        .unwrap_or(true);
                    if !long_past {
                        continue;
                    }
                    MobileEnrolmentStatus {
                        enrolment_id: entry.enrolment_id.clone(),
                        status: "expired".into(),
                        person_label: entry.person_label.clone(),
                        site_label: entry.site_label.clone(),
                        expires_at: Some(entry.expires_at.clone()),
                        redeemed_at: None,
                    }
                }
            };
            if let Err(e) = settle_issued(&db, &status).await {
                tracing::error!(error = %e, "mobile enrolment: follow-up not recorded");
            }
        }
    }
}

// ── Audit ───────────────────────────────────────────────────────────────────────────────────────

const AUDIT_ENROLMENT_ISSUED: &str = "mobile.enrolment_issued";
const AUDIT_ENROLMENT_CANCELLED: &str = "mobile.enrolment_cancelled";
const AUDIT_PHONE_ENROLLED: &str = "mobile.phone_enrolled";
const AUDIT_PHONE_REVOKED: &str = "mobile.phone_revoked";

async fn audit(
    db: &DatabaseConnection,
    actor_id: i32,
    event_type: &str,
    entity_id: &str,
    summary: &str,
    detail: serde_json::Value,
) {
    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type,
            actor_id: Some(actor_id),
            entity_type: Some("mobile_phone"),
            entity_id: Some(entity_id),
            summary,
            detail_json: Some(detail.to_string()),
            ..Default::default()
        },
    )
    .await;
}

// ── Commands ────────────────────────────────────────────────────────────────────────────────────

/// People who may be given a phone, and the sites a phone can be bound to.
#[tauri::command]
pub async fn list_mobile_enrolment_candidates(state: State<'_, AppState>) -> AppResult<EnrolmentCandidates> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    candidates_in(&state.db).await
}

/// Issues a phone enrolment for one person at one site (step-up required).
#[tauri::command]
pub async fn issue_mobile_enrolment(
    input: IssueMobileEnrolmentInput,
    state: State<'_, AppState>,
) -> AppResult<IssuedMobileEnrolment> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    require_step_up!(state);
    issue_in(&state.db, &user, &input).await
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
    status_in(&state.db, &enrolment_id).await
}

/// Cancels an enrolment not used yet (it only removes access, so no step-up).
#[tauri::command]
pub async fn cancel_mobile_enrolment(
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
    cancel_in(&state.db, &user, &enrolment_id).await
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
    phones_in(&state.db).await
}

/// Revokes a phone (step-up required).
#[tauri::command]
pub async fn revoke_mobile_phone(input: RevokeMobilePhoneInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_USERS,
        PermissionScope::Global
    );
    require_step_up!(state);
    revoke_in(&state.db, &user, &input).await
}

#[cfg(test)]
mod tests {
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
    }
}
