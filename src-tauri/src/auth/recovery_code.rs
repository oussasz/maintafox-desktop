//! Vendor administrator recovery code (CMMS Advisor decision 2026-10-19, owner rule "the plant is
//! never locked out").
//!
//! The built-in "admin" account is gone (review E-1), the first-start screen closes for good, and a
//! guard keeps the last lasting administrator (`auth::last_admin`). A plant can still lose every
//! administrator (a forgotten password, every administrator role ending). The vendor then checks
//! the customer's identity and issues a one-time code, signed with the same pinned Ed25519 keys as
//! the entitlement envelopes. The real gate is that identity check; this module makes sure the code
//! is genuine, meant for this PC, current and never used before.
//!
//! **The code**: `MFXREC1.<payload, base64url without padding>.<signature, lowercase hex>`. The
//! payload is canonical JSON with the keys of [`RecoveryPayload`] in exactly that order; the
//! signature is Ed25519 over [`SIGNING_DOMAIN`] followed by the payload bytes
//! (`maintafox-vendor-console/api/src/admin-recovery-signer.ts`; the shared vector is
//! `testdata/admin_recovery_vector.json`). Each refusal has its own [`Refusal`] code.
//!
//! **Redeeming it** ([`redeem_admin_recovery_code`], signed out): whatever the administrators of
//! this PC, the code creates exactly one new named account with the plant password policy, holding
//! the normal Administrator role plant-wide (tenant scope of this PC's organisation) with no end
//! date. An existing account is never reset or taken over: the username must be one this PC has
//! never had. In one write transaction: the account and its role, the code recorded as spent, the
//! audit event, and a notice for every other active administrator.
//!
//! **Nothing for strangers**: a signed-out caller learns only "this code is refused, and why"
//! until a genuine, current, unused code for this PC is presented; only then are the username and
//! password examined. No existing account is read back or changed.
//!
//! **The notice** ([`get_admin_recovery_notices`], [`acknowledge_admin_recovery_notice`]): every
//! administrator told sees it after each sign-in until that administrator acknowledges it; the
//! acknowledgement is audited.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::entitlements::trusted_keys::{verify_entitlement_signature, SignatureCheck, ENTITLEMENT_ISSUER};
use crate::errors::{AppError, AppResult};
use crate::state::AppState;

/// Text before the payload of every code.
pub const CODE_PREFIX: &str = "MFXREC1";
/// Bytes signed before the payload: a recovery signature can never pass as an entitlement one.
pub const SIGNING_DOMAIN: &str = "maintafox-admin-recovery:v1\n";
pub const RECOVERY_VERSION: u64 = 1;
pub const RECOVERY_PURPOSE: &str = "admin_recovery";
pub const RECOVERY_SIGNATURE_ALG: &str = "ed25519:admin-recovery:v1";
/// `licensing_trust_keys.purpose` of the pinned keys (the same keys as the entitlement envelopes).
const TRUST_KEY_PURPOSE: &str = "entitlement_signature";
/// How far in the future `issued_at` may be (clock drift between this PC and the vendor).
pub const MAX_CLOCK_SKEW_MINUTES: i64 = 5;
/// Longest code accepted for decoding (a real code is about 600 characters).
const MAX_CODE_LEN: usize = 4096;
/// Longest display name of the created administrator.
const MAX_DISPLAY_NAME_CHARS: usize = 120;

/// Audit action codes.
pub const AUDIT_USED: &str = "admin_recovery.used";
pub const AUDIT_REFUSED: &str = "admin_recovery.refused";
pub const AUDIT_NOTICE_ACKNOWLEDGED: &str = "admin_recovery.notice_acknowledged";

/// Coded form refusals (translated under `settings:errors`).
pub const USERNAME_INVALID: &str = "ADMIN_RECOVERY_USERNAME_INVALID";
pub const USERNAME_TAKEN: &str = "ADMIN_RECOVERY_USERNAME_TAKEN";
pub const DISPLAY_NAME_REQUIRED: &str = "ADMIN_RECOVERY_DISPLAY_NAME_REQUIRED";
pub const PASSWORD_MISMATCH: &str = "ADMIN_RECOVERY_PASSWORD_MISMATCH";
pub const RETRY: &str = "ADMIN_RECOVERY_RETRY";
pub const NOTICE_NOT_FOUND: &str = "ADMIN_RECOVERY_NOTICE_NOT_FOUND";

// ── The code ─────────────────────────────────────────────────────────────────────────────────────

/// Why a code is refused. [`Refusal::as_str`] is the reason name shared with the vendor console's
/// vector (`refusal_codes`); [`Refusal::error_code`] the coded message shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Not `MFXREC1.<base64url>.<hex>`, not UTF-8 JSON, or not the payload's exact shape.
    Malformed,
    /// Decodes, but re-serialising it in the canonical key order does not give the same bytes.
    NonCanonical,
    WrongIssuer,
    /// No pinned key with this id, or its operational record is missing or inactive.
    UntrustedKey,
    /// The key is marked compromised in `licensing_trust_keys`.
    CompromisedKey,
    InvalidSignature,
    WrongVersion,
    WrongPurpose,
    WrongAlgorithm,
    /// This PC holds no complete activation claim to compare with.
    NotActivated,
    WrongTenant,
    WrongMachine,
    WrongLicense,
    /// `now >= expires_at`.
    Expired,
    /// `issued_at` more than [`MAX_CLOCK_SKEW_MINUTES`] after now.
    IssuedInFuture,
    /// This `code_id` has already been redeemed on this PC.
    AlreadyUsed,
}

impl Refusal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::NonCanonical => "non_canonical",
            Self::WrongIssuer => "wrong_issuer",
            Self::UntrustedKey => "untrusted_key",
            Self::CompromisedKey => "compromised_key",
            Self::InvalidSignature => "invalid_signature",
            Self::WrongVersion => "wrong_version",
            Self::WrongPurpose => "wrong_purpose",
            Self::WrongAlgorithm => "wrong_algorithm",
            Self::NotActivated => "not_activated",
            Self::WrongTenant => "wrong_tenant",
            Self::WrongMachine => "wrong_machine",
            Self::WrongLicense => "wrong_license",
            Self::Expired => "expired",
            Self::IssuedInFuture => "issued_in_future",
            Self::AlreadyUsed => "already_used",
        }
    }

    pub const fn error_code(self) -> &'static str {
        match self {
            Self::Malformed => "ADMIN_RECOVERY_CODE_MALFORMED",
            Self::NonCanonical => "ADMIN_RECOVERY_CODE_NON_CANONICAL",
            Self::WrongIssuer => "ADMIN_RECOVERY_CODE_WRONG_ISSUER",
            Self::UntrustedKey => "ADMIN_RECOVERY_CODE_UNTRUSTED_KEY",
            Self::CompromisedKey => "ADMIN_RECOVERY_CODE_COMPROMISED_KEY",
            Self::InvalidSignature => "ADMIN_RECOVERY_CODE_INVALID_SIGNATURE",
            Self::WrongVersion => "ADMIN_RECOVERY_CODE_WRONG_VERSION",
            Self::WrongPurpose => "ADMIN_RECOVERY_CODE_WRONG_PURPOSE",
            Self::WrongAlgorithm => "ADMIN_RECOVERY_CODE_WRONG_ALGORITHM",
            Self::NotActivated => "ADMIN_RECOVERY_NOT_ACTIVATED",
            Self::WrongTenant => "ADMIN_RECOVERY_CODE_WRONG_TENANT",
            Self::WrongMachine => "ADMIN_RECOVERY_CODE_WRONG_MACHINE",
            Self::WrongLicense => "ADMIN_RECOVERY_CODE_WRONG_LICENSE",
            Self::Expired => "ADMIN_RECOVERY_CODE_EXPIRED",
            Self::IssuedInFuture => "ADMIN_RECOVERY_CODE_ISSUED_IN_FUTURE",
            Self::AlreadyUsed => "ADMIN_RECOVERY_CODE_ALREADY_USED",
        }
    }

    /// Whether the refusal is about a code the vendor really signed (worth an audit entry).
    const fn signed_by_vendor(self) -> bool {
        matches!(
            self,
            Self::WrongVersion
                | Self::WrongPurpose
                | Self::WrongAlgorithm
                | Self::NotActivated
                | Self::WrongTenant
                | Self::WrongMachine
                | Self::WrongLicense
                | Self::Expired
                | Self::IssuedInFuture
                | Self::AlreadyUsed
        )
    }

    fn into_error(self) -> AppError {
        crate::errors::org_validation_failed(
            self.error_code(),
            format!("Vendor recovery code refused ({}).", self.as_str()),
            std::collections::HashMap::new(),
        )
    }
}

/// The signed payload, field order = canonical key order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPayload {
    pub v: u64,
    pub purpose: String,
    pub code_id: String,
    pub tenant_id: String,
    pub machine_fingerprint: String,
    pub license_id: String,
    pub issuer: String,
    pub key_id: String,
    pub signature_alg: String,
    pub issued_at: String,
    pub expires_at: String,
}

fn json_str(value: &str) -> String {
    // Same escaping as the entitlement canonical payload and the server's JSON.stringify.
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// The canonical bytes of `payload` (fixed key order, no whitespace, JSON string escaping).
pub fn canonical_payload(payload: &RecoveryPayload) -> String {
    format!(
        "{{\"v\":{},\"purpose\":{},\"code_id\":{},\"tenant_id\":{},\"machine_fingerprint\":{},\"license_id\":{},\"issuer\":{},\"key_id\":{},\"signature_alg\":{},\"issued_at\":{},\"expires_at\":{}}}",
        payload.v,
        json_str(&payload.purpose),
        json_str(&payload.code_id),
        json_str(&payload.tenant_id),
        json_str(&payload.machine_fingerprint),
        json_str(&payload.license_id),
        json_str(&payload.issuer),
        json_str(&payload.key_id),
        json_str(&payload.signature_alg),
        json_str(&payload.issued_at),
        json_str(&payload.expires_at),
    )
}

/// A code split and decoded, its payload proven canonical; nothing checked against keys yet.
#[derive(Debug, Clone)]
pub struct DecodedCode {
    pub payload: RecoveryPayload,
    pub payload_json: String,
    pub signature_hex: String,
}

/// Splits and decodes `code`: prefix, base64url without padding, lowercase hex signature, UTF-8
/// JSON of exactly the payload's shape, canonical bytes.
pub fn decode_code(code: &str) -> Result<DecodedCode, Refusal> {
    use base64::Engine as _;

    let code = code.trim();
    if code.is_empty() || code.len() > MAX_CODE_LEN {
        return Err(Refusal::Malformed);
    }
    let mut parts = code.split('.');
    let (Some(prefix), Some(payload_b64), Some(signature_hex), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refusal::Malformed);
    };
    if prefix != CODE_PREFIX || payload_b64.is_empty() {
        return Err(Refusal::Malformed);
    }
    // Ed25519 signature: 64 bytes, written as 128 lowercase hex digits.
    if signature_hex.len() != 128 || !signature_hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(Refusal::Malformed);
    }
    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| Refusal::Malformed)?;
    let payload_json = String::from_utf8(payload_bytes).map_err(|_| Refusal::Malformed)?;
    let payload: RecoveryPayload = serde_json::from_str(&payload_json).map_err(|_| Refusal::Malformed)?;
    if canonical_payload(&payload) != payload_json {
        return Err(Refusal::NonCanonical);
    }
    Ok(DecodedCode {
        payload,
        payload_json,
        signature_hex: signature_hex.to_string(),
    })
}

/// The bytes the vendor signs.
pub fn signed_message(payload_json: &str) -> Vec<u8> {
    let mut message = Vec::with_capacity(SIGNING_DOMAIN.len() + payload_json.len());
    message.extend_from_slice(SIGNING_DOMAIN.as_bytes());
    message.extend_from_slice(payload_json.as_bytes());
    message
}

/// Issuer, cryptographic signature (pinned keys), then the fixed fields. `key_trust` is the
/// operational state of the key (`licensing_trust_keys`), checked before the signature.
pub fn check_signature(decoded: &DecodedCode, key_trust: Result<(), Refusal>) -> Result<(), Refusal> {
    let payload = &decoded.payload;
    if payload.issuer != ENTITLEMENT_ISSUER {
        return Err(Refusal::WrongIssuer);
    }
    key_trust?;
    match verify_entitlement_signature(
        &payload.issuer,
        &payload.key_id,
        &signed_message(&decoded.payload_json),
        &decoded.signature_hex,
    ) {
        SignatureCheck::Verified => {}
        SignatureCheck::InvalidSignature => return Err(Refusal::InvalidSignature),
        SignatureCheck::UntrustedKey => return Err(Refusal::UntrustedKey),
    }
    if payload.v != RECOVERY_VERSION {
        return Err(Refusal::WrongVersion);
    }
    if payload.purpose != RECOVERY_PURPOSE {
        return Err(Refusal::WrongPurpose);
    }
    if payload.signature_alg != RECOVERY_SIGNATURE_ALG {
        return Err(Refusal::WrongAlgorithm);
    }
    Ok(())
}

/// The activation of this PC a code must name (from the stored claim, never from the browser).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationBinding {
    pub tenant_id: String,
    pub machine_fingerprint: String,
    pub license_id: String,
}

/// Same organisation, machine and licence as this PC's stored activation claim (exact bytes).
pub fn check_binding(payload: &RecoveryPayload, binding: Option<&ActivationBinding>) -> Result<(), Refusal> {
    let Some(binding) = binding else {
        return Err(Refusal::NotActivated);
    };
    if payload.tenant_id != binding.tenant_id {
        return Err(Refusal::WrongTenant);
    }
    if payload.machine_fingerprint != binding.machine_fingerprint {
        return Err(Refusal::WrongMachine);
    }
    if payload.license_id != binding.license_id {
        return Err(Refusal::WrongLicense);
    }
    Ok(())
}

fn parse_instant(value: &str) -> Result<DateTime<Utc>, Refusal> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| Refusal::Malformed)
}

/// Current: `now < expires_at` and `issued_at <= now + MAX_CLOCK_SKEW_MINUTES`.
pub fn check_validity_window(payload: &RecoveryPayload, now: DateTime<Utc>) -> Result<(), Refusal> {
    let issued_at = parse_instant(&payload.issued_at)?;
    let expires_at = parse_instant(&payload.expires_at)?;
    if now >= expires_at {
        return Err(Refusal::Expired);
    }
    if issued_at > now + Duration::minutes(MAX_CLOCK_SKEW_MINUTES) {
        return Err(Refusal::IssuedInFuture);
    }
    Ok(())
}

/// Operational trust of the signing key: present, active, for the envelope purpose, not
/// compromised (`license::security::verify_trust_key`, as envelopes are checked).
async fn key_trust(conn: &impl ConnectionTrait, issuer: &str, key_id: &str) -> AppResult<Result<(), Refusal>> {
    match crate::license::security::verify_trust_key(conn, issuer, key_id, TRUST_KEY_PURPOSE).await {
        Ok(()) => Ok(Ok(())),
        Err(AppError::LicenseDenied { reason_code, .. }) if reason_code == "compromised_key" => {
            Ok(Err(Refusal::CompromisedKey))
        }
        Err(AppError::ValidationFailed(_) | AppError::LicenseDenied { .. }) => Ok(Err(Refusal::UntrustedKey)),
        Err(other) => Err(other),
    }
}

/// This PC's activation claim, from its database (`product_license` stored claim).
async fn stored_binding(db: &DatabaseConnection) -> AppResult<Option<ActivationBinding>> {
    Ok(crate::commands::product_license::get_activation_claim_record(db)
        .await?
        .map(|claim| ActivationBinding {
            tenant_id: claim.tenant_id,
            machine_fingerprint: claim.machine_fingerprint,
            license_id: claim.license_id,
        }))
}

async fn code_already_used(conn: &impl ConnectionTrait, code_id: &str) -> AppResult<bool> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT EXISTS (SELECT 1 FROM admin_recovery_codes_used WHERE code_id = ?) AS used",
            [code_id.into()],
        ))
        .await?;
    Ok(row.map(|r| r.try_get::<i64>("", "used")).transpose()?.unwrap_or(0) == 1)
}

/// Every check of a code against this PC, in order: shape and canonical form, issuer and key,
/// signature, fixed fields, this PC's activation, validity window, not used before. The outer
/// result is a database failure; the inner one the coded refusal.
pub async fn verify_code_in(
    db: &DatabaseConnection,
    code: &str,
    now: DateTime<Utc>,
) -> AppResult<Result<RecoveryPayload, Refusal>> {
    let decoded = match decode_code(code) {
        Ok(decoded) => decoded,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let trust = if decoded.payload.issuer == ENTITLEMENT_ISSUER {
        key_trust(db, &decoded.payload.issuer, &decoded.payload.key_id).await?
    } else {
        Ok(())
    };
    if let Err(refusal) = check_signature(&decoded, trust) {
        return Ok(Err(refusal));
    }
    let binding = stored_binding(db).await?;
    if let Err(refusal) = check_binding(&decoded.payload, binding.as_ref()) {
        return Ok(Err(refusal));
    }
    if let Err(refusal) = check_validity_window(&decoded.payload, now) {
        return Ok(Err(refusal));
    }
    if code_already_used(db, &decoded.payload.code_id).await? {
        return Ok(Err(Refusal::AlreadyUsed));
    }
    Ok(Ok(decoded.payload))
}

// ── Redeeming a code ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RedeemAdminRecoveryCodeInput {
    pub code: String,
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub password_confirmation: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedeemAdminRecoveryCodeResult {
    /// The username created (as stored), to sign in with.
    pub username: String,
}

fn coded(code: &str, message: &str) -> AppError {
    crate::errors::org_validation_failed(code, message, std::collections::HashMap::new())
}

/// A transaction holding the database write lock from its start (as
/// `commands::product_license::begin_write_transaction`): a second submission of the same code
/// waits, then finds the code spent.
async fn begin_write_transaction(db: &DatabaseConnection) -> AppResult<sea_orm::DatabaseTransaction> {
    let txn = db.begin().await?;
    txn.execute_unprepared("UPDATE user_accounts SET id = id WHERE 0")
        .await?;
    Ok(txn)
}

async fn audit_refusal(db: &DatabaseConnection, refusal: Refusal, code: &str) {
    if !refusal.signed_by_vendor() {
        tracing::warn!(
            event = "admin_recovery_code_refused",
            reason = refusal.as_str(),
            "Vendor recovery code refused"
        );
        return;
    }
    // A genuine vendor code refused on this PC: who issued it, for which code, and why.
    let code_id = decode_code(code).ok().map(|d| d.payload.code_id);
    let input = crate::audit::writer::AuditEventInput {
        action_code: AUDIT_REFUSED.to_string(),
        target_type: Some("admin_recovery_code".to_string()),
        target_id: code_id.clone(),
        actor_id: None,
        auth_context: "signed_out".to_string(),
        result: "failure".to_string(),
        before_hash: None,
        after_hash: None,
        retention_class: "security".to_string(),
        details_json: Some(serde_json::json!({
            "summary": "Vendor recovery code refused",
            "reason": refusal.as_str(),
            "code_id": code_id,
        })),
    };
    if let Err(err) = crate::audit::writer::write_audit_event(db, input).await {
        tracing::error!(error = %err, "admin recovery refusal could not be audited");
    }
}

/// The new account's username: the usual username rule, and never a name this PC has had (an
/// account, even disabled or deleted, is never taken over).
fn checked_username(raw: &str) -> AppResult<String> {
    crate::commands::admin_users::normalize_username(raw).map_err(|err| {
        let reason = match &err {
            AppError::ValidationFailed(messages) => messages.join(" "),
            other => other.to_string(),
        };
        coded(USERNAME_INVALID, &reason)
    })
}

fn checked_display_name(raw: &str) -> AppResult<String> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_DISPLAY_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(coded(
            DISPLAY_NAME_REQUIRED,
            "Give the new administrator's name (1 to 120 characters).",
        ));
    }
    Ok(name.to_string())
}

/// What the transaction wrote.
struct Created {
    user_id: i64,
    notified: usize,
}

async fn write_recovery(
    txn: &sea_orm::DatabaseTransaction,
    payload: &RecoveryPayload,
    username: &str,
    display_name: &str,
    password_hash: &str,
    now: DateTime<Utc>,
) -> AppResult<Result<Created, AppError>> {
    // Re-read under the write lock: another submission of the same code may have committed.
    if code_already_used(txn, &payload.code_id).await? {
        return Ok(Err(Refusal::AlreadyUsed.into_error()));
    }
    let taken = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT EXISTS (SELECT 1 FROM user_accounts WHERE LOWER(TRIM(username)) = LOWER(?)) AS taken",
            [username.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "taken"))
        .transpose()?
        .unwrap_or(0)
        == 1;
    if taken {
        return Ok(Err(coded(
            USERNAME_TAKEN,
            "This username already exists on this PC: choose a new one (a recovery code never takes over an existing account).",
        )));
    }
    let role_id: i64 = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM roles WHERE name = 'Administrator' AND deleted_at IS NULL LIMIT 1".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Administrator role not found in seed data")))?
        .try_get("", "id")?;

    // Told before the account exists: every other active administrator of this PC.
    let recipients = crate::auth::last_admin::active_administrator_ids(txn).await?;

    let stamp = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    // The password was chosen by the person creating the account: no forced change.
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts
            (entity_sync_id, username, display_name, identity_mode, password_hash,
             is_active, is_admin, force_password_change, failed_login_attempts,
             created_at, updated_at, row_version)
         VALUES (?, ?, ?, 'local', ?, 1, 0, 0, 0, ?, ?, 1)",
        [
            entity_sync_id.clone().into(),
            username.into(),
            display_name.into(),
            password_hash.into(),
            stamp.clone().into(),
            stamp.clone().into(),
        ],
    ))
    .await?;
    let user_id: i64 = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to read new user id")))?
        .try_get("", "id")?;

    // Plant-wide (tenant scope of this PC's organisation), normal Administrator role, no end date:
    // a lasting administrator (`auth::last_admin::lasting_administrator_ids`).
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments
            (entity_sync_id, user_id, role_id, scope_type, scope_reference,
             valid_from, valid_to, assigned_by_id, notes, created_at, updated_at, row_version)
         VALUES (?, ?, ?, 'tenant', ?, NULL, NULL, NULL, ?, ?, ?, 1)",
        [
            crate::sync::identity::new_entity_sync_id().into(),
            user_id.into(),
            role_id.into(),
            payload.tenant_id.clone().into(),
            "Administrator created with a vendor recovery code".into(),
            stamp.clone().into(),
            stamp.clone().into(),
        ],
    ))
    .await?;

    let consumed_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO admin_recovery_codes_used
            (code_id, tenant_id, license_id, key_id, issued_at, expires_at,
             created_user_id, created_username, consumed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            payload.code_id.clone().into(),
            payload.tenant_id.clone().into(),
            payload.license_id.clone().into(),
            payload.key_id.clone().into(),
            payload.issued_at.clone().into(),
            payload.expires_at.clone().into(),
            user_id.into(),
            username.into(),
            consumed_at.clone().into(),
        ],
    ))
    .await?;

    crate::audit::writer::write_audit_event_in(
        txn,
        crate::audit::writer::AuditEventInput {
            action_code: AUDIT_USED.to_string(),
            target_type: Some("user_account".to_string()),
            target_id: Some(user_id.to_string()),
            actor_id: None,
            auth_context: "signed_out".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "security".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Administrator created with a vendor recovery code",
                "code_id": payload.code_id,
                "tenant_id": payload.tenant_id,
                "license_id": payload.license_id,
                "key_id": payload.key_id,
                "issued_at": payload.issued_at,
                "expires_at": payload.expires_at,
                "username": username,
                "display_name": display_name,
                "role": "Administrator",
                "scope": "tenant",
                "notified_administrators": recipients,
            })),
        },
    )
    .await?;

    for recipient in &recipients {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO admin_recovery_notices
                (code_id, recipient_user_id, created_user_id, created_username, created_display_name,
                 created_at, acknowledged_at)
             VALUES (?, ?, ?, ?, ?, ?, NULL)",
            [
                payload.code_id.clone().into(),
                (*recipient).into(),
                user_id.into(),
                username.into(),
                display_name.into(),
                consumed_at.clone().into(),
            ],
        ))
        .await?;
    }

    Ok(Ok(Created {
        user_id,
        notified: recipients.len(),
    }))
}

/// [`redeem_admin_recovery_code`] at the instant `now`.
pub(crate) async fn redeem_admin_recovery_code_in(
    db: &DatabaseConnection,
    input: RedeemAdminRecoveryCodeInput,
    now: DateTime<Utc>,
) -> AppResult<RedeemAdminRecoveryCodeResult> {
    let payload = match verify_code_in(db, &input.code, now).await? {
        Ok(payload) => payload,
        Err(refusal) => {
            audit_refusal(db, refusal, &input.code).await;
            return Err(refusal.into_error());
        }
    };

    // Only a genuine, current, unused code for this PC reaches the account fields.
    let username = checked_username(&input.username)?;
    let display_name = checked_display_name(&input.display_name)?;
    if input.password != input.password_confirmation {
        return Err(coded(PASSWORD_MISMATCH, "The two passwords do not match."));
    }
    crate::auth::builtin_admin::validate_first_admin_password(db, &input.password).await?;
    let password_hash = crate::auth::password::hash_password(&input.password)?;

    let written = async {
        let txn = begin_write_transaction(db).await?;
        match write_recovery(&txn, &payload, &username, &display_name, &password_hash, now).await? {
            Ok(created) => {
                txn.commit().await?;
                Ok(Ok(created))
            }
            Err(refusal) => {
                txn.rollback().await?;
                Ok::<_, AppError>(Err(refusal))
            }
        }
    }
    .await;
    let created = match written {
        Ok(Ok(created)) => created,
        Ok(Err(refusal)) => {
            if matches!(&refusal, AppError::OrgValidationFailed(issues)
                if issues.first().is_some_and(|i| i.code == Refusal::AlreadyUsed.error_code()))
            {
                audit_refusal(db, Refusal::AlreadyUsed, &input.code).await;
            }
            return Err(refusal);
        }
        Err(err) => {
            // The write failed (e.g. another submission held the database past the busy timeout):
            // if that other one spent the code, say so; otherwise a coded "retry".
            tracing::warn!(error = %err, "admin recovery write failed");
            return Err(match code_already_used(db, &payload.code_id).await {
                Ok(true) => Refusal::AlreadyUsed.into_error(),
                _ => coded(
                    RETRY,
                    "The administrator could not be saved because the database is busy: wait a moment, then retry.",
                ),
            });
        }
    };

    tracing::warn!(
        event = "admin_recovery_code_used",
        code_id = %payload.code_id,
        user_id = created.user_id,
        notified = created.notified,
        "Administrator created with a vendor recovery code"
    );
    // A built-in "admin" never used is retired now that a lasting administrator exists.
    crate::auth::builtin_admin::retire_after_administration_change(db).await;
    Ok(RedeemAdminRecoveryCodeResult { username })
}

/// Signed out: redeems a vendor recovery code by creating one new named administrator.
#[tauri::command]
pub async fn redeem_admin_recovery_code(
    input: RedeemAdminRecoveryCodeInput,
    state: State<'_, AppState>,
) -> AppResult<RedeemAdminRecoveryCodeResult> {
    redeem_admin_recovery_code_in(&state.db, input, Utc::now()).await
}

// ── Notice to the administrators ─────────────────────────────────────────────────────────────────

/// One administrator created with a recovery code, not yet acknowledged by the signed-in one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdminRecoveryNotice {
    pub id: i64,
    /// When the code was redeemed (RFC 3339, UTC).
    pub created_at: String,
    pub created_username: String,
    pub created_display_name: String,
}

/// The notices of `user_id` still to acknowledge, while that account is an administrator.
pub(crate) async fn notices_for(
    conn: &impl ConnectionTrait,
    user_id: Option<i64>,
) -> AppResult<Vec<AdminRecoveryNotice>> {
    let Some(user_id) = user_id else {
        return Ok(Vec::new());
    };
    let Ok(user_id_i32) = i32::try_from(user_id) else {
        return Ok(Vec::new());
    };
    if !crate::auth::rbac::user_has_elevated_admin_access(conn, user_id_i32).await? {
        return Ok(Vec::new());
    }
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, created_at, created_username, created_display_name
               FROM admin_recovery_notices
              WHERE recipient_user_id = ? AND acknowledged_at IS NULL
              ORDER BY id",
            [user_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(AdminRecoveryNotice {
                id: r.try_get("", "id")?,
                created_at: r.try_get("", "created_at")?,
                created_username: r.try_get("", "created_username")?,
                created_display_name: r.try_get("", "created_display_name")?,
            })
        })
        .collect()
}

/// Acknowledges `notice_id` for `user_id` only (another administrator's notice is never touched),
/// with an audit entry in the same transaction.
pub(crate) async fn acknowledge_notice_in(db: &DatabaseConnection, user_id: i64, notice_id: i64) -> AppResult<()> {
    let txn = db.begin().await?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let updated = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE admin_recovery_notices SET acknowledged_at = ?
              WHERE id = ? AND recipient_user_id = ? AND acknowledged_at IS NULL",
            [now.into(), notice_id.into(), user_id.into()],
        ))
        .await?
        .rows_affected();
    if updated != 1 {
        txn.rollback().await?;
        return Err(coded(
            NOTICE_NOT_FOUND,
            "This notice is not yours or was already acknowledged.",
        ));
    }
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code_id, created_user_id, created_username FROM admin_recovery_notices WHERE id = ?",
            [notice_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("acknowledged notice vanished")))?;
    let code_id: String = row.try_get("", "code_id")?;
    let created_user_id: i64 = row.try_get("", "created_user_id")?;
    let created_username: String = row.try_get("", "created_username")?;
    crate::audit::writer::write_audit_event_in(
        &txn,
        crate::audit::writer::AuditEventInput {
            action_code: AUDIT_NOTICE_ACKNOWLEDGED.to_string(),
            target_type: Some("user_account".to_string()),
            target_id: Some(created_user_id.to_string()),
            actor_id: Some(user_id),
            auth_context: "session".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "security".to_string(),
            details_json: Some(serde_json::json!({
                "summary": "Recovery-code administrator notice acknowledged",
                "notice_id": notice_id,
                "code_id": code_id,
                "created_username": created_username,
            })),
        },
    )
    .await?;
    txn.commit().await?;
    Ok(())
}

/// The notices of the signed-in account (read like `last_admin::get_single_administrator_warning`,
/// also while a forced password change is pending); nothing while signed out.
#[tauri::command]
pub async fn get_admin_recovery_notices(state: State<'_, AppState>) -> AppResult<Vec<AdminRecoveryNotice>> {
    let user_id = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .map(|session| i64::from(session.user.user_id));
    notices_for(&state.db, user_id).await
}

/// The signed-in account acknowledges one of its notices.
#[tauri::command]
pub async fn acknowledge_admin_recovery_notice(notice_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = crate::require_session!(state);
    acknowledge_notice_in(&state.db, i64::from(user.user_id), notice_id).await
}

#[cfg(test)]
#[path = "recovery_code_tests.rs"]
mod tests;
