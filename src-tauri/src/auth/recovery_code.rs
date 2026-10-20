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
//! the normal Administrator role plant-wide (tenant scope with no scope reference, so the role
//! synchronises to every PC) with no end date. An existing account is never reset or taken over:
//! the username must be one this PC has never had. In one write transaction: the account and its
//! role, the code recorded as spent, the audit event, and a notice for every other active
//! administrator.
//!
//! **Names**: the username and display name may not contain control, format or default-ignorable
//! characters ([`INVISIBLE_CODE_POINTS`]: bidi controls, zero-width characters, Hangul fillers,
//! variation selectors...), which would let the new account pose as an existing one.
//!
//! **Nothing for strangers**: a signed-out caller learns only "this code is refused, and why"
//! until a genuine, current, unused code for this PC is presented; only then are the username and
//! password examined. No existing account is read back or changed, except the retirement of a
//! never-used built-in "admin", as at first start.
//!
//! **The code's lifetime**: the vendor issues `expires_at = issued_at + 24 h`; a code with a longer
//! window, or one that ends before it starts, is refused ([`Refusal::InvalidWindow`]).
//!
//! **The notice** ([`get_admin_recovery_notices`], [`acknowledge_admin_recovery_notice`]): every
//! administrator active at redemption (the new one excluded) gets one notice, shown after each
//! sign-in until that administrator acknowledges it, on every PC of the plant (CMMS Advisor
//! decision (a): notices and acknowledgements are synchronised). Only the recipient acknowledges
//! their own notice, here and when an acknowledgement arrives by synchronisation
//! ([`inbound_acknowledgement_check`]); nobody edits or deletes a notice ([`inbound_notice_check`],
//! and a trigger refuses any update). The acknowledgement records who, when and on which PC, and is
//! audited. The register of spent codes stays on this PC: a code names one PC's activation.

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
/// Longest window `expires_at - issued_at` accepted: the vendor issues exactly 24 h
/// (`ADMIN_RECOVERY_TTL_HOURS` of the vendor console's `admin-recovery-signer.ts`).
pub const MAX_CODE_LIFETIME_HOURS: i64 = 24;
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
/// Invisible or control characters (Unicode Cc / Cf, bidi controls) in the name or username.
pub const NAME_INVISIBLE_CHARACTERS: &str = "ADMIN_RECOVERY_NAME_INVISIBLE_CHARACTERS";
pub const PASSWORD_MISMATCH: &str = "ADMIN_RECOVERY_PASSWORD_MISMATCH";
pub const RETRY: &str = "ADMIN_RECOVERY_RETRY";
pub const NOTICE_NOT_FOUND: &str = "ADMIN_RECOVERY_NOTICE_NOT_FOUND";

/// Sync conflict codes of received notices and acknowledgements.
pub const CONFLICT_NOTICE_CHANGED: &str = "ADMIN_RECOVERY_NOTICE_CHANGED";
pub const CONFLICT_NOTICE_INVALID: &str = "ADMIN_RECOVERY_NOTICE_INVALID";
pub const CONFLICT_ACK_NOT_RECIPIENT: &str = "ADMIN_RECOVERY_ACK_NOT_RECIPIENT";
pub const CONFLICT_ACK_CHANGED: &str = "ADMIN_RECOVERY_ACK_CHANGED";

/// Entity types (and tables) of the synchronised notices and acknowledgements.
pub const NOTICE_ENTITY: &str = "admin_recovery_notices";
pub const ACK_ENTITY: &str = "admin_recovery_acknowledgements";

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
    /// `expires_at <= issued_at`, or `expires_at - issued_at` longer than
    /// [`MAX_CODE_LIFETIME_HOURS`] (the vendor issues exactly 24 h).
    InvalidWindow,
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
            Self::InvalidWindow => "invalid_window",
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
            Self::InvalidWindow => "ADMIN_RECOVERY_CODE_INVALID_WINDOW",
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
                | Self::InvalidWindow
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

/// A window the vendor issues (`issued_at < expires_at <= issued_at + MAX_CODE_LIFETIME_HOURS`),
/// current: `now < expires_at` and `issued_at <= now + MAX_CLOCK_SKEW_MINUTES`.
pub fn check_validity_window(payload: &RecoveryPayload, now: DateTime<Utc>) -> Result<(), Refusal> {
    let issued_at = parse_instant(&payload.issued_at)?;
    let expires_at = parse_instant(&payload.expires_at)?;
    if expires_at <= issued_at || expires_at - issued_at > Duration::hours(MAX_CODE_LIFETIME_HOURS) {
        return Err(Refusal::InvalidWindow);
    }
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

#[derive(Deserialize)]
pub struct RedeemAdminRecoveryCodeInput {
    pub code: String,
    pub username: String,
    pub display_name: String,
    pub password: String,
    pub password_confirmation: String,
}

/// The code and both passwords never reach a log, even through `{:?}`.
impl std::fmt::Debug for RedeemAdminRecoveryCodeInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedeemAdminRecoveryCodeInput")
            .field("code", &"<redacted>")
            .field("username", &self.username)
            .field("display_name", &self.display_name)
            .field("password", &"<redacted>")
            .field("password_confirmation", &"<redacted>")
            .finish()
    }
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

/// A genuine vendor code refused on this PC: who issued it, for which code, and why. At most one
/// audit row per (code, reason): a code submitted again and again from the login screen adds
/// nothing more to the (synchronised) audit trail; every attempt is still logged.
async fn audit_refusal(db: &DatabaseConnection, refusal: Refusal, code: &str) {
    let code_id = decode_code(code).ok().map(|d| d.payload.code_id);
    tracing::warn!(
        event = "admin_recovery_code_refused",
        reason = refusal.as_str(),
        code_id = code_id.as_deref().unwrap_or(""),
        "Vendor recovery code refused"
    );
    if !refusal.signed_by_vendor() {
        return;
    }
    if let Err(err) = write_refusal_audit_once(db, refusal, code_id).await {
        tracing::error!(error = %err, "admin recovery refusal could not be audited");
    }
}

async fn write_refusal_audit_once(db: &DatabaseConnection, refusal: Refusal, code_id: Option<String>) -> AppResult<()> {
    // Under the write lock, so two refusals of one code racing write one row.
    let txn = begin_write_transaction(db).await?;
    let already = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT EXISTS (SELECT 1 FROM audit_events
                 WHERE action_code = ? AND target_type = 'admin_recovery_code'
                   AND target_id IS ? AND json_extract(details_json, '$.reason') = ?) AS done",
            [AUDIT_REFUSED.into(), code_id.clone().into(), refusal.as_str().into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "done"))
        .transpose()?
        .unwrap_or(0)
        == 1;
    if already {
        txn.rollback().await?;
        return Ok(());
    }
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
    crate::audit::writer::write_audit_event_in(&txn, input).await?;
    txn.commit().await?;
    Ok(())
}

/// Code points that show nothing (or almost nothing) and let a name pose as another one, as of
/// Unicode 15.1: the union of
/// - general category **Cf** (format characters): soft hyphen, Arabic number signs, bidi controls
///   U+202A-U+202E and U+2066-U+2069, zero-width U+200B-U+200F, word joiner, BOM, interlinear
///   annotation, Egyptian hieroglyph format controls, shorthand format controls, musical symbol
///   format controls, tags; and
/// - **Default_Ignorable_Code_Point** (`DerivedCoreProperties.txt`): adds the combining grapheme
///   joiner U+034F, the Hangul fillers U+115F, U+1160, U+3164 and U+FFA0 (category Lo: they pass
///   `char::is_alphanumeric`), the Khmer inherent vowels U+17B4-U+17B5, the Mongolian free
///   variation selectors U+180B-U+180F, the variation selectors U+FE00-U+FE0F and U+E0100-U+E01EF,
///   and the reserved ranges U+2065, U+FFF0-U+FFF8 and U+E0000-U+E0FFF.
///
/// Each entry: (first, last) inclusive, sorted.
pub const INVISIBLE_CODE_POINTS: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),   // Cf, DI: soft hyphen
    (0x034F, 0x034F),   // DI: combining grapheme joiner
    (0x0600, 0x0605),   // Cf: Arabic number signs
    (0x061C, 0x061C),   // Cf, DI: Arabic letter mark
    (0x06DD, 0x06DD),   // Cf: Arabic end of ayah
    (0x070F, 0x070F),   // Cf: Syriac abbreviation mark
    (0x0890, 0x0891),   // Cf: Arabic pound / piastre mark above
    (0x08E2, 0x08E2),   // Cf: Arabic disputed end of ayah
    (0x115F, 0x1160),   // DI: Hangul choseong / jungseong fillers
    (0x17B4, 0x17B5),   // DI: Khmer inherent vowels
    (0x180B, 0x180F),   // DI (U+180E also Cf): Mongolian free variation selectors, vowel separator
    (0x200B, 0x200F),   // Cf, DI: zero-width space / non-joiner / joiner, LRM, RLM
    (0x202A, 0x202E),   // Cf, DI: bidi embeddings and overrides
    (0x2060, 0x206F),   // Cf, DI: word joiner, invisible operators, bidi isolates, deprecated
    (0x3164, 0x3164),   // DI: Hangul filler
    (0xFE00, 0xFE0F),   // DI: variation selectors 1-16
    (0xFEFF, 0xFEFF),   // Cf, DI: zero-width no-break space (BOM)
    (0xFFA0, 0xFFA0),   // DI: halfwidth Hangul filler
    (0xFFF0, 0xFFFB),   // DI reserved U+FFF0-U+FFF8, Cf interlinear annotation U+FFF9-U+FFFB
    (0x110BD, 0x110BD), // Cf: Kaithi number sign
    (0x110CD, 0x110CD), // Cf: Kaithi number sign above
    (0x13430, 0x1343F), // Cf: Egyptian hieroglyph format controls
    (0x1BCA0, 0x1BCA3), // Cf, DI: shorthand format controls
    (0x1D173, 0x1D17A), // Cf, DI: musical symbol format controls
    (0xE0000, 0xE0FFF), // Cf tags U+E0001, U+E0020-U+E007F; DI: variation selectors 17-256, reserved
];

/// A code point of [`INVISIBLE_CODE_POINTS`] (Cf or Default_Ignorable_Code_Point).
pub fn is_invisible_code_point(c: char) -> bool {
    let cp = u32::from(c);
    INVISIBLE_CODE_POINTS
        .iter()
        .any(|(first, last)| (*first..=*last).contains(&cp))
}

/// A control (Cc), format (Cf) or default-ignorable character: never part of a name shown to
/// administrators.
pub fn has_invisible_character(value: &str) -> bool {
    value.chars().any(|c| c.is_control() || is_invisible_code_point(c))
}

fn invisible_characters_error() -> AppError {
    coded(
        NAME_INVISIBLE_CHARACTERS,
        "The name and the username must not contain invisible or control characters.",
    )
}

/// The new account's username: the usual username rule, and never a name this PC has had (an
/// account, even disabled or deleted, is never taken over).
fn checked_username(raw: &str) -> AppResult<String> {
    if has_invisible_character(raw.trim()) {
        return Err(invisible_characters_error());
    }
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
    if has_invisible_character(name) {
        return Err(invisible_characters_error());
    }
    if name.is_empty() || name.chars().count() > MAX_DISPLAY_NAME_CHARS {
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

    // Told before the account exists: every administrator active now (the new one excluded).
    let recipients = crate::auth::last_admin::active_administrator_ids(txn).await?;
    let device_code = crate::sync::device_code::this_device_code(txn).await?;

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
    // As at first start: an administrator created on this PC closes the first-start screen for
    // good (keeps a marker that still names an existing account).
    crate::auth::builtin_admin::record_first_administrator_in(txn, &entity_sync_id).await?;

    // Plant-wide, normal Administrator role, no end date: a lasting administrator
    // (`auth::last_admin::lasting_administrator_ids`). Tenant scope with no scope reference, the
    // convention of the seeder and of emergency access (`product_license` counts it for this
    // organisation): such an assignment synchronises, so the new administrator is an administrator
    // on every PC of the plant (a tenant id as reference cannot be sent by sync).
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments
            (entity_sync_id, user_id, role_id, scope_type, scope_reference,
             valid_from, valid_to, assigned_by_id, notes, created_at, updated_at, row_version)
         VALUES (?, ?, ?, 'tenant', NULL, NULL, NULL, NULL, ?, ?, ?, 1)",
        [
            crate::sync::identity::new_entity_sync_id().into(),
            user_id.into(),
            role_id.into(),
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
                "device_code": device_code,
            })),
        },
    )
    .await?;

    // One notice per recipient, synchronised to every PC of the plant.
    for recipient in &recipients {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO admin_recovery_notices
                (entity_sync_id, code_id, recipient_user_id, created_user_id, created_username,
                 created_display_name, redeemed_device_code, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                payload.code_id.clone().into(),
                (*recipient).into(),
                user_id.into(),
                username.into(),
                display_name.into(),
                device_code.clone().into(),
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
    /// Local id of the notice on this PC (what [`acknowledge_admin_recovery_notice`] takes).
    pub id: i64,
    /// When the code was redeemed (RFC 3339, UTC).
    pub created_at: String,
    pub created_username: String,
    pub created_display_name: String,
    /// The PC where the code was redeemed (`sync::device_code`).
    pub redeemed_device_code: String,
}

/// SQL condition: the notice `n` has been acknowledged by its recipient (on any PC).
const ACKNOWLEDGED_BY_RECIPIENT_SQL: &str = "EXISTS (SELECT 1 FROM admin_recovery_acknowledgements a \
     WHERE a.notice_id = n.id AND a.acknowledged_by_id = n.recipient_user_id)";

/// The notices of `user_id` still to acknowledge, while that account is active. Whether it is
/// still an administrator does not matter: a recipient whose role was removed since (for example by
/// the administrator the code created) is exactly who must still be told.
pub(crate) async fn notices_for(
    conn: &impl ConnectionTrait,
    user_id: Option<i64>,
) -> AppResult<Vec<AdminRecoveryNotice>> {
    let Some(user_id) = user_id else {
        return Ok(Vec::new());
    };
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT n.id, n.created_at, n.created_username, n.created_display_name, n.redeemed_device_code
                   FROM admin_recovery_notices n
                   JOIN user_accounts u ON u.id = n.recipient_user_id
                  WHERE n.recipient_user_id = ? AND u.is_active = 1 AND u.deleted_at IS NULL
                    AND NOT {ACKNOWLEDGED_BY_RECIPIENT_SQL}
                  ORDER BY n.id"
            ),
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
                redeemed_device_code: r.try_get("", "redeemed_device_code")?,
            })
        })
        .collect()
}

/// Acknowledges `notice_id` for `user_id` only: the notice must be addressed to that account and
/// not acknowledged yet (another administrator's notice is never touched). The acknowledgement
/// (who, when, this PC) is a new synchronised row, audited in the same transaction; the notice
/// itself is never changed.
pub(crate) async fn acknowledge_notice_in(db: &DatabaseConnection, user_id: i64, notice_id: i64) -> AppResult<()> {
    let txn = begin_write_transaction(db).await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT n.code_id, n.created_user_id, n.created_username
                   FROM admin_recovery_notices n
                  WHERE n.id = ? AND n.recipient_user_id = ? AND NOT {ACKNOWLEDGED_BY_RECIPIENT_SQL}"
            ),
            [notice_id.into(), user_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        txn.rollback().await?;
        return Err(coded(
            NOTICE_NOT_FOUND,
            "This notice is not yours or was already acknowledged.",
        ));
    };
    let code_id: String = row.try_get("", "code_id")?;
    let created_user_id: i64 = row.try_get("", "created_user_id")?;
    let created_username: String = row.try_get("", "created_username")?;
    let device_code = crate::sync::device_code::this_device_code(&txn).await?;
    let acknowledged_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let acknowledgement_sync_id = crate::sync::identity::new_entity_sync_id();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO admin_recovery_acknowledgements
            (entity_sync_id, notice_id, acknowledged_by_id, acknowledged_at, device_code)
         VALUES (?, ?, ?, ?, ?)",
        [
            acknowledgement_sync_id.clone().into(),
            notice_id.into(),
            user_id.into(),
            acknowledged_at.clone().into(),
            device_code.clone().into(),
        ],
    ))
    .await?;
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
                "acknowledged_at": acknowledged_at,
                "device_code": device_code,
                "acknowledgement_sync_id": acknowledgement_sync_id,
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

// ── Notices and acknowledgements received by synchronisation ────────────────────────────────────

type Payload = serde_json::Map<String, serde_json::Value>;

fn payload_text<'a>(payload: &'a Payload, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(serde_json::Value::as_str)
}

/// Keys of `received` whose value differs from this PC's copy of the row (keys this version does not
/// know are not compared: they are never written).
fn changed_keys(local: &Payload, received: &Payload) -> Vec<String> {
    received
        .iter()
        .filter(|(key, value)| {
            key.as_str() != crate::sync::row_layout::PAYLOAD_FORMAT_KEY
                && local.get(key.as_str()).is_some_and(|mine| mine != *value)
        })
        .map(|(key, _)| key.clone())
        .collect()
}

/// A 4-character device code (`sync::device_code`: Crockford base 32, capitals).
fn is_device_code(value: &str) -> bool {
    value.len() == crate::sync::device_code::DEVICE_CODE_LEN
        && value.bytes().all(|b| crate::sync::device_code::ALPHABET.contains(&b))
}

/// What a received notice is checked against before it is written.
enum NoticeOrigin {
    /// Matches the created account and the audit record of the redemption.
    Consistent,
    /// Waits: the created account or the redemption's audit record has not arrived yet.
    Wait(String),
    /// Refused, with the reason.
    Inconsistent(String),
}

/// A new notice must describe what was really redeemed. The created account must be known here
/// (otherwise the notice waits). Then:
/// - with the `admin_recovery.used` audit record(s) of the same code here (append-only, written in
///   the redemption's transaction and synchronised before the notices: `audit_events` is a registry
///   parent), the notice is accepted when ANY of them names that account (`target_id`) with the
///   notice's username and display name, and refused otherwise. The notice is compared with the
///   record only, never with the account's current names: both the username and the display name
///   can be edited afterwards (`admin_users::update_user_impl`, Profile), and such an edit must not
///   make the real notices conflict on other PCs. Several records can exist for one code when a peer
///   forged one; a forged one never hides the real one.
/// - without any record here (it has not arrived), a notice naming the account as it is now is
///   accepted; one that differs waits for the record (never refused: the account may have been
///   renamed since the redemption).
async fn notice_origin(conn: &impl ConnectionTrait, payload: &Payload) -> AppResult<NoticeOrigin> {
    let (Some(created_sync_id), Some(code_id), Some(username), Some(display_name)) = (
        payload_text(payload, "created_user_sync_id"),
        payload_text(payload, "code_id"),
        payload_text(payload, "created_username"),
        payload_text(payload, "created_display_name"),
    ) else {
        return Ok(NoticeOrigin::Inconsistent(
            "a recovery notice must name the created account, the code and the names shown".into(),
        ));
    };
    let account = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, username, COALESCE(display_name, '') AS display_name
               FROM user_accounts WHERE entity_sync_id = ?",
            [created_sync_id.into()],
        ))
        .await?;
    let Some(account) = account else {
        return Ok(NoticeOrigin::Wait(format!(
            "created account {created_sync_id} not received yet"
        )));
    };
    let account_id: i64 = account.try_get("", "id")?;
    let records = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT target_id,
                    json_extract(details_json, '$.username') AS username,
                    json_extract(details_json, '$.display_name') AS display_name
               FROM audit_events
              WHERE action_code = ? AND target_type = 'user_account'
                AND json_extract(details_json, '$.code_id') = ?
              ORDER BY id",
            [AUDIT_USED.into(), code_id.into()],
        ))
        .await?;
    if records.is_empty() {
        let account_username: String = account.try_get("", "username")?;
        let account_display_name: String = account.try_get("", "display_name")?;
        return Ok(
            if account_username == username && account_display_name == display_name {
                NoticeOrigin::Consistent
            } else {
                NoticeOrigin::Wait(format!(
                "audit record of recovery code {code_id} not received yet (the names differ from the account's current ones)"
            ))
            },
        );
    }
    let account_id = account_id.to_string();
    for record in &records {
        let target: Option<String> = record.try_get("", "target_id")?;
        let recorded_username: Option<String> = record.try_get("", "username")?;
        let recorded_display_name: Option<String> = record.try_get("", "display_name")?;
        if target.as_deref() == Some(account_id.as_str())
            && recorded_username.as_deref() == Some(username)
            && recorded_display_name.as_deref() == Some(display_name)
        {
            return Ok(NoticeOrigin::Consistent);
        }
    }
    Ok(NoticeOrigin::Inconsistent(format!(
        "the notice does not match the redemption recorded for code {code_id} (account, username or display name)"
    )))
}

/// A received notice (`sync::apply_hooks::before_upsert_checked`): a notice already here is never
/// changed (any difference is a conflict, whoever sent it). A new one never names the created
/// administrator as its recipient; carries no invisible characters in the fields it shows; has a
/// device code and an RFC 3339 date; and matches the created account and the redemption's audit
/// record ([`notice_origin`], waiting for them while they have not arrived).
pub(crate) async fn inbound_notice_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    payload: &Payload,
) -> AppResult<Option<crate::settings::site::InboundVerdict>> {
    use crate::settings::site::InboundVerdict;
    if let Some(local) = crate::permit::sync_check::local_wire(conn, NOTICE_ENTITY, entity_sync_id).await? {
        let changed = changed_keys(&local, payload);
        return Ok((!changed.is_empty()).then(|| InboundVerdict::Conflict {
            code: CONFLICT_NOTICE_CHANGED,
            message: format!(
                "An administrator recovery notice is never changed; received changes to: {}",
                changed.join(", ")
            ),
        }));
    }
    let recipient = payload_text(payload, "recipient_user_sync_id");
    if recipient.is_none() || recipient == payload_text(payload, "created_user_sync_id") {
        return Ok(Some(InboundVerdict::Conflict {
            code: CONFLICT_NOTICE_INVALID,
            message: "A recovery notice must be addressed to an administrator other than the one created".into(),
        }));
    }
    if ["created_username", "created_display_name", "redeemed_device_code"]
        .iter()
        .any(|key| payload_text(payload, key).is_some_and(has_invisible_character))
    {
        return Ok(Some(InboundVerdict::Conflict {
            code: CONFLICT_NOTICE_INVALID,
            message: "A recovery notice names the administrator with invisible or control characters".into(),
        }));
    }
    if !payload_text(payload, "redeemed_device_code").is_some_and(is_device_code)
        || payload_text(payload, "created_at").is_none_or(|t| DateTime::parse_from_rfc3339(t).is_err())
    {
        return Ok(Some(InboundVerdict::Conflict {
            code: CONFLICT_NOTICE_INVALID,
            message: "A recovery notice must carry a 4-character device code and an RFC 3339 date".into(),
        }));
    }
    Ok(match notice_origin(conn, payload).await? {
        NoticeOrigin::Consistent => None,
        NoticeOrigin::Wait(message) => Some(InboundVerdict::Defer(message)),
        NoticeOrigin::Inconsistent(reason) => Some(InboundVerdict::Conflict {
            code: CONFLICT_NOTICE_INVALID,
            message: format!("A recovery notice does not match what was redeemed: {reason}"),
        }),
    })
}

/// A received acknowledgement: only the notice's recipient acknowledges it (an acknowledgement
/// made by anyone else is a conflict), and an acknowledgement already here is never changed. One
/// whose notice has not arrived yet waits for it.
pub(crate) async fn inbound_acknowledgement_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    payload: &Payload,
) -> AppResult<Option<crate::settings::site::InboundVerdict>> {
    use crate::settings::site::InboundVerdict;
    if let Some(local) = crate::permit::sync_check::local_wire(conn, ACK_ENTITY, entity_sync_id).await? {
        let changed = changed_keys(&local, payload);
        return Ok((!changed.is_empty()).then(|| InboundVerdict::Conflict {
            code: CONFLICT_ACK_CHANGED,
            message: format!(
                "An acknowledgement of a recovery notice is never changed; received changes to: {}",
                changed.join(", ")
            ),
        }));
    }
    let Some(notice_sync_id) = payload_text(payload, "notice_sync_id") else {
        return Ok(Some(InboundVerdict::Conflict {
            code: CONFLICT_ACK_NOT_RECIPIENT,
            message: "An acknowledgement must name its recovery notice".into(),
        }));
    };
    let recipient: Option<String> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT u.entity_sync_id AS recipient
               FROM admin_recovery_notices n JOIN user_accounts u ON u.id = n.recipient_user_id
              WHERE n.entity_sync_id = ?",
            [notice_sync_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "recipient"))
        .transpose()?
        .flatten();
    let Some(recipient) = recipient else {
        return Ok(Some(InboundVerdict::Defer(format!(
            "recovery notice {notice_sync_id} not received yet"
        ))));
    };
    if payload_text(payload, "acknowledged_by_sync_id") != Some(recipient.as_str()) {
        return Ok(Some(InboundVerdict::Conflict {
            code: CONFLICT_ACK_NOT_RECIPIENT,
            message: "Only the administrator a recovery notice is addressed to can acknowledge it".into(),
        }));
    }
    Ok(None)
}

#[cfg(test)]
#[path = "recovery_code_tests.rs"]
mod tests;
