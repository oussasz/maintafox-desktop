//! Product (tenant) license key onboarding — persisted in `app_settings`, not the in-app licensing module.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::require_session;
use crate::settings;
use crate::state::AppState;
use crate::sync::domain::TenantConfigSyncPayload;

const PRODUCT_LICENSE_ONBOARDING_KEY: &str = "product.license_onboarding";
const PRODUCT_LICENSE_SCOPE: &str = "device";
const TENANT_BOOTSTRAP_KEY: &str = "tenant.initial_bootstrap";
const TENANT_BOOTSTRAP_SCOPE: &str = "device";

/// Serialises every read → authorise → write of the licence record (key entry, reconciliation), so
/// a change is always authorised against the record it replaces: a signed-out claim authorised
/// against an active record never overwrites a revocation written meanwhile. A process lock, not a
/// SQLite transaction: the app's pool has one connection (`db::connect_sqlcipher`), and the
/// authorisation and the settings write read and write through the pool, so a transaction holding
/// that connection would wait on itself. One process writes the record (single-instance app).
static LICENCE_RECORD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Serialize)]
pub struct ProductLicenseOnboardingState {
    pub complete: bool,
    pub status: ProductLicenseActivationStatus,
    pub pending_online_validation: bool,
    pub deny_reason_code: Option<String>,
    pub deny_message: Option<String>,
    pub degraded_reason: Option<String>,
    pub next_retry_at: Option<String>,
    pub retry_attempt: u32,
    pub last_reconciled_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    /// Activated tenant id from the last successful claim (when present).
    pub tenant_id: Option<String>,
    /// Company / tenant display name from activation (when provided by the control plane).
    pub company_display_name: Option<String>,
    /// Commercial edition from claim (`core`/`professional`/`enterprise`/`development`) when known.
    pub license_edition: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProductLicenseActivationStatus {
    Uninitialized,
    PendingOnlineValidation,
    Active,
    DegradedApiUnavailable,
    DeniedRevoked,
    DeniedExpired,
    DeniedSlotLimit,
    DeniedForceUpdateRequired,
    DeniedInvalid,
    /// No successful contact with the control plane within the licence's `offline_grace_hours`.
    /// Derived on read (see `effective_status`); cleared by the next successful reconciliation.
    DeniedOfflineGraceExpired,
}

/// `Debug` never shows the activation token (see the `impl` below).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProductActivationClaimRecord {
    pub tenant_id: String,
    pub license_id: String,
    pub machine_fingerprint: String,
    pub activation_token: String,
    pub expires_at: Option<String>,
    pub force_min_app_version: Option<String>,
    pub force_update_required: Option<bool>,
    pub update_channel: Option<String>,
    pub offline_grace_hours: Option<u32>,
    pub trust_revocation_disconnects_immediately: Option<bool>,
    pub reconnect_requires_fresh_heartbeat: Option<bool>,
    #[serde(
        default,
        alias = "company_display_name",
        alias = "company_name",
        alias = "tenant_name"
    )]
    pub tenant_display_name: Option<String>,
    #[serde(default)]
    pub tenant_slug: Option<String>,
    #[serde(default, alias = "slot_limit", alias = "seat_limit")]
    pub device_limit: Option<i64>,
    #[serde(default, alias = "tier", alias = "plan", alias = "edition")]
    pub license_tier: Option<String>,
    #[serde(default, alias = "rollout_cohort")]
    pub license_plan: Option<String>,
    #[serde(default)]
    pub license_status: Option<String>,
    #[serde(default)]
    pub activated_device_count: Option<i64>,
    #[serde(default, alias = "demo_data", alias = "is_demo")]
    pub has_demo_data: Option<bool>,
    #[serde(default, alias = "tenant_initialized")]
    pub is_initialized: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductLicenseDiagnosticEvent {
    pub at: String,
    pub kind: String,
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProductLicenseReconciliationPolicy {
    pub retry_attempt: u32,
    pub next_retry_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
}

/// `Debug` never shows the licence key nor the activation token (see the `impl` below).
#[derive(Clone, Serialize, Deserialize)]
pub struct ProductLicenseStateRecord {
    pub schema_version: u32,
    pub key_sha256: String,
    /// Stored for eventual online reconciliation.
    /// NOTE: This is transitional and should move to OS keychain-backed secret storage.
    pub license_key_plaintext: Option<String>,
    pub machine_fingerprint: Option<String>,
    pub app_version: Option<String>,
    pub submitted_at: String,
    pub status: ProductLicenseActivationStatus,
    pub pending_online_validation: bool,
    pub deny_reason_code: Option<String>,
    pub deny_message: Option<String>,
    pub degraded_reason: Option<String>,
    pub activation_claim: Option<ProductActivationClaimRecord>,
    pub last_reconciled_at: Option<String>,
    pub reconciliation: ProductLicenseReconciliationPolicy,
    pub diagnostics: Vec<ProductLicenseDiagnosticEvent>,
    #[serde(default)]
    pub company_display_name: Option<String>,
    /// Whether the forced-update denial recorded here may be lifted without an administrator once
    /// this PC runs the forced minimum version (`forced_update_installed`). Set by
    /// [`forced_update_lift_allowed`] only when a success claim recorded the forced update with a
    /// readable forced minimum version stored with it, and did not replace another denial (unless an
    /// administrator with step-up applied it); `false` for every other status, after a forced-update
    /// denial recorded from a bare denial outcome (no claim, no minimum), and after any denial
    /// outcome. A record written before this field existed reads as `false`: fail-closed, the lift
    /// then needs an administrator.
    #[serde(default)]
    pub forced_update_lift_allowed: bool,
}

/// What replaces a secret field in a `Debug` output: whether it is set, never its value.
fn redacted_debug(value: Option<&str>) -> &'static str {
    match value {
        Some(v) if !v.is_empty() => "[redacted]",
        _ => "[none]",
    }
}

impl std::fmt::Debug for ProductActivationClaimRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductActivationClaimRecord")
            .field("tenant_id", &self.tenant_id)
            .field("license_id", &self.license_id)
            .field("machine_fingerprint", &self.machine_fingerprint)
            .field("activation_token", &redacted_debug(Some(&self.activation_token)))
            .field("expires_at", &self.expires_at)
            .field("force_min_app_version", &self.force_min_app_version)
            .field("force_update_required", &self.force_update_required)
            .field("update_channel", &self.update_channel)
            .field("offline_grace_hours", &self.offline_grace_hours)
            .field(
                "trust_revocation_disconnects_immediately",
                &self.trust_revocation_disconnects_immediately,
            )
            .field(
                "reconnect_requires_fresh_heartbeat",
                &self.reconnect_requires_fresh_heartbeat,
            )
            .field("tenant_display_name", &self.tenant_display_name)
            .field("tenant_slug", &self.tenant_slug)
            .field("device_limit", &self.device_limit)
            .field("license_tier", &self.license_tier)
            .field("license_plan", &self.license_plan)
            .field("license_status", &self.license_status)
            .field("activated_device_count", &self.activated_device_count)
            .field("has_demo_data", &self.has_demo_data)
            .field("is_initialized", &self.is_initialized)
            .finish()
    }
}

impl std::fmt::Debug for ProductLicenseStateRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductLicenseStateRecord")
            .field("schema_version", &self.schema_version)
            .field("key_sha256", &self.key_sha256)
            .field(
                "license_key_plaintext",
                &redacted_debug(self.license_key_plaintext.as_deref()),
            )
            .field("machine_fingerprint", &self.machine_fingerprint)
            .field("app_version", &self.app_version)
            .field("submitted_at", &self.submitted_at)
            .field("status", &self.status)
            .field("pending_online_validation", &self.pending_online_validation)
            .field("deny_reason_code", &self.deny_reason_code)
            .field("deny_message", &self.deny_message)
            .field("degraded_reason", &self.degraded_reason)
            .field("activation_claim", &self.activation_claim)
            .field("last_reconciled_at", &self.last_reconciled_at)
            .field("reconciliation", &self.reconciliation)
            .field("diagnostics", &self.diagnostics)
            .field("company_display_name", &self.company_display_name)
            .field("forced_update_lift_allowed", &self.forced_update_lift_allowed)
            .finish()
    }
}

impl ProductLicenseStateRecord {
    fn from_legacy_json(value: serde_json::Value) -> Option<Self> {
        let key_sha256 = value.get("key_sha256")?.as_str()?.to_string();
        let submitted_at = value
            .get("submitted_at")
            .and_then(serde_json::Value::as_str)
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let pending_online_validation = value
            .get("pending_online_validation")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let activation_claim = value
            .get("activation_claim")
            .cloned()
            .and_then(|claim| serde_json::from_value::<ProductActivationClaimRecord>(claim).ok());
        Some(Self {
            schema_version: 1,
            key_sha256,
            license_key_plaintext: None,
            machine_fingerprint: activation_claim.as_ref().map(|c| c.machine_fingerprint.clone()),
            app_version: None,
            submitted_at,
            status: if pending_online_validation {
                ProductLicenseActivationStatus::PendingOnlineValidation
            } else {
                ProductLicenseActivationStatus::Active
            },
            pending_online_validation,
            deny_reason_code: None,
            deny_message: None,
            degraded_reason: None,
            activation_claim,
            last_reconciled_at: None,
            reconciliation: ProductLicenseReconciliationPolicy::default(),
            diagnostics: vec![],
            company_display_name: None,
            forced_update_lift_allowed: false,
        })
    }

    fn push_diagnostic(&mut self, kind: &str, code: Option<String>, message: String) {
        self.diagnostics.push(ProductLicenseDiagnosticEvent {
            at: Utc::now().to_rfc3339(),
            kind: kind.to_string(),
            message,
            code,
        });
        if self.diagnostics.len() > 64 {
            let keep_from = self.diagnostics.len().saturating_sub(64);
            self.diagnostics = self.diagnostics.split_off(keep_from);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductLicenseReconciliationOutcomeKind {
    Success,
    NetworkError,
    HttpError,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductLicenseReconciliationInput {
    pub kind: ProductLicenseReconciliationOutcomeKind,
    pub claim: Option<ProductActivationClaimRecord>,
    #[serde(default)]
    pub entitlement_envelope: Option<crate::entitlements::domain::EntitlementEnvelopeInput>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductLicenseDiagnostics {
    pub status: ProductLicenseActivationStatus,
    pub deny_reason_code: Option<String>,
    pub deny_message: Option<String>,
    pub pending_online_validation: bool,
    pub last_reconciled_at: Option<String>,
    pub machine_fingerprint: Option<String>,
    pub app_version: Option<String>,
    pub reconciliation: ProductLicenseReconciliationPolicy,
    pub diagnostics: Vec<ProductLicenseDiagnosticEvent>,
    pub has_activation_claim: bool,
}

/// Collapse control-plane / FE alias keys onto canonical `ProductActivationClaimRecord` fields.
///
/// `normalizeActivationPayload` (and some control-plane payloads) emit both a canonical name and
/// its serde `alias` (e.g. `license_tier` + `edition`). Serde rejects that as a duplicate field;
/// this sanitizer keeps one value per field so activation never fails on alias co-presence.
fn sanitize_activation_claim_object(obj: &mut serde_json::Map<String, serde_json::Value>) {
    fn take_non_null(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str) -> Option<serde_json::Value> {
        match obj.remove(key) {
            Some(v) if !v.is_null() => Some(v),
            _ => None,
        }
    }

    fn prefer_canonical(obj: &mut serde_json::Map<String, serde_json::Value>, canonical: &str, aliases: &[&str]) {
        let mut chosen = take_non_null(obj, canonical);
        for alias in aliases {
            if let Some(v) = take_non_null(obj, alias) {
                if chosen.is_none() {
                    chosen = Some(v);
                }
            }
        }
        if let Some(v) = chosen {
            obj.insert(canonical.to_string(), v);
        }
    }

    // Commercial edition: prefer explicit `edition` over legacy tier/plan labels.
    let edition = take_non_null(obj, "edition");
    prefer_canonical(obj, "license_tier", &["tier", "plan"]);
    prefer_canonical(obj, "license_plan", &["rollout_cohort"]);
    if let Some(edition) = edition {
        obj.insert("license_tier".to_string(), edition.clone());
        obj.insert("license_plan".to_string(), edition);
    }

    prefer_canonical(
        obj,
        "tenant_display_name",
        &["company_display_name", "company_name", "tenant_name"],
    );
    prefer_canonical(obj, "device_limit", &["slot_limit", "seat_limit"]);
    prefer_canonical(obj, "has_demo_data", &["demo_data", "is_demo"]);
    prefer_canonical(obj, "is_initialized", &["tenant_initialized"]);

    // FE-only / control-plane noise — not part of ProductActivationClaimRecord.
    for key in [
        "force_update_mode",
        "force_update_reason",
        "force_update_policy_source",
        "capabilities_digest",
        "feature_flags_digest",
        "machine_label",
        "entitlement_envelope",
    ] {
        obj.remove(key);
    }
}

fn extract_claim_and_envelope(
    raw: &str,
) -> AppResult<(
    ProductActivationClaimRecord,
    Option<crate::entitlements::domain::EntitlementEnvelopeInput>,
)> {
    let mut value: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_LICENSE_PAYLOAD_INVALID: claimJson must be valid JSON: {e}"
        )])
    })?;

    let envelope = match value
        .as_object_mut()
        .and_then(|obj| obj.remove("entitlement_envelope"))
        .filter(|v| !v.is_null())
    {
        Some(env_value) => Some(
            serde_json::from_value::<crate::entitlements::domain::EntitlementEnvelopeInput>(env_value).map_err(
                |e| {
                    AppError::ValidationFailed(vec![format!(
                        "GATE_LICENSE_ENVELOPE_INVALID: entitlement_envelope is invalid: {e}"
                    )])
                },
            )?,
        ),
        None => None,
    };

    if let Some(obj) = value.as_object_mut() {
        sanitize_activation_claim_object(obj);
    }

    let mut claim: ProductActivationClaimRecord = serde_json::from_value(value).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_LICENSE_PAYLOAD_INVALID: claimJson must be valid ProductActivationClaimRecord JSON: {e}"
        )])
    })?;
    // Prefer explicit edition fields for display when plan/tier missing.
    if claim.license_plan.is_none() {
        claim.license_plan = claim.license_tier.clone();
    }
    if claim.license_tier.is_none() {
        claim.license_tier = claim.license_plan.clone();
    }
    Ok((claim, envelope))
}

async fn apply_claim_entitlement_envelope(
    db: &DatabaseConnection,
    envelope: Option<crate::entitlements::domain::EntitlementEnvelopeInput>,
) -> AppResult<()> {
    let Some(envelope) = envelope else {
        return Ok(());
    };
    if envelope_would_overwrite_unverified(db, &envelope).await? {
        tracing::warn!(
            event = "desktop_entitlement_envelope_same_id_unverified",
            envelope_id = %envelope.envelope_id,
            "Entitlement envelope reuses a stored envelope id without a valid signature; not applied"
        );
        return Ok(());
    }
    // Ensure trust material exists before verify (older DBs may have an empty table).
    if let Err(e) = crate::license::security::ensure_default_licensing_trust_keys(db).await {
        tracing::warn!(
            event = "desktop_entitlement_trust_seed_failed",
            error = %e,
            "Failed to ensure licensing trust keys before envelope apply"
        );
    }
    let result = match crate::entitlements::queries::apply_entitlement_envelope(db, envelope).await {
        Ok(result) => result,
        Err(e) => {
            // Soft phase: never roll activation back to degraded solely because envelope apply failed.
            tracing::warn!(
                event = "desktop_entitlement_envelope_apply_failed",
                error = %e,
                "Entitlement envelope apply failed; keeping activation success"
            );
            return Ok(());
        }
    };
    if !result.verified {
        tracing::warn!(
            event = "desktop_entitlement_envelope_unverified",
            envelope_id = %result.envelope_id,
            verification_result = %result.verification_result,
            "Entitlement envelope not verified; continuing without active envelope"
        );
        return Ok(());
    }
    tracing::info!(
        event = "desktop_entitlement_envelope_applied",
        envelope_id = %result.envelope_id,
        effective_state = %result.effective_state,
        "Signed entitlement envelope applied from activation claim"
    );
    Ok(())
}

/// Whether `envelope` reuses the id of an envelope stored here while its signature does not verify.
/// Applying it would rewrite that row (the one in force included) with unverified content
/// (`apply_entitlement_envelope` updates a row of the same id in place), so it is never applied.
async fn envelope_would_overwrite_unverified(
    db: &DatabaseConnection,
    envelope: &crate::entitlements::domain::EntitlementEnvelopeInput,
) -> AppResult<bool> {
    if envelope_signature_verified(envelope) {
        return Ok(false);
    }
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT 1 AS found FROM entitlement_envelopes WHERE envelope_id = ? LIMIT 1",
            [envelope.envelope_id.clone().into()],
        ))
        .await?;
    Ok(existing.is_some())
}

fn compute_backoff_at(attempt: u32) -> DateTime<Utc> {
    // 30s, 60s, 120s... capped at 6h
    let exponent = attempt.min(12);
    let seconds = (30_i64 * (1_i64 << exponent)).min(6 * 60 * 60);
    Utc::now() + Duration::seconds(seconds)
}

fn map_denied_status(code: Option<&str>) -> ProductLicenseActivationStatus {
    match code.unwrap_or_default() {
        // Licence revoked, this device revoked by the vendor, or the customer account deactivated:
        // all mean access was withdrawn; `deny_reason_code` keeps the exact server code.
        "license_revoked" | "activation_revoked" | "tenant_inactive" => ProductLicenseActivationStatus::DeniedRevoked,
        "license_expired" => ProductLicenseActivationStatus::DeniedExpired,
        "slot_limit_reached" => ProductLicenseActivationStatus::DeniedSlotLimit,
        "force_update_required" => ProductLicenseActivationStatus::DeniedForceUpdateRequired,
        _ => ProductLicenseActivationStatus::DeniedInvalid,
    }
}

/// How severe a recorded licence status is: the one order in which a denial may be replaced. A
/// denial outcome (which carries no claim and anyone can report signed out) and a success claim
/// accepted without an administrator may only keep the severity recorded here or raise it, never
/// lower it (`apply_product_license_reconciliation_in`, `denial_lifted`):
/// - `0`: not denied by the control plane (active, degraded, pending, uninitialised; the offline
///   grace is derived on read and not stored);
/// - `1`: a forced update (`DeniedForceUpdateRequired`): the only denial this PC can end by itself,
///   by installing the update (`forced_update_installed`);
/// - `2`: a revocation, an expiry, a slot limit or an invalid licence: only the control plane ends
///   these, so on this PC only an administrator with step-up applies the claim that lifts them. They
///   rank equal: replacing one with another changes only the reason shown, never what can be lifted
///   here, so the latest denial the control plane gave is recorded.
///
/// So a revocation, expiry, slot-limit or invalid denial always replaces a forced update, while a
/// forced update (or no denial) never replaces one of them.
fn denial_severity(status: &ProductLicenseActivationStatus) -> u8 {
    match status {
        ProductLicenseActivationStatus::DeniedRevoked
        | ProductLicenseActivationStatus::DeniedExpired
        | ProductLicenseActivationStatus::DeniedSlotLimit
        | ProductLicenseActivationStatus::DeniedInvalid => 2,
        ProductLicenseActivationStatus::DeniedForceUpdateRequired => 1,
        ProductLicenseActivationStatus::Uninitialized
        | ProductLicenseActivationStatus::PendingOnlineValidation
        | ProductLicenseActivationStatus::Active
        | ProductLicenseActivationStatus::DegradedApiUnavailable
        | ProductLicenseActivationStatus::DeniedOfflineGraceExpired => 0,
    }
}

/// Whether the forced-update denial a success claim records may later be lifted on this PC by
/// installing the update (stored as `forced_update_lift_allowed`). Only when the claim forces the
/// update with a forced minimum version this PC can read, and either an administrator with step-up
/// applied it, or the record it replaces holds no denial, or holds a forced update that was itself
/// liftable. A forced update recorded from a bare denial outcome (no minimum) stays not liftable when
/// a claim later keeps it forced; a claim never replaces a revocation, expiry, slot-limit or invalid
/// denial without an administrator (`denial_lifted`), and if one ever did it would not be liftable.
fn forced_update_lift_allowed(
    previous: Option<&ProductLicenseStateRecord>,
    claim: &ProductActivationClaimRecord,
    authority: LicenceAuthority,
) -> bool {
    let minimum_readable = claim.force_min_app_version.as_deref().and_then(version_parts).is_some();
    if claim.force_update_required != Some(true) || !minimum_readable {
        return false;
    }
    if authority == LicenceAuthority::Administrator {
        return true;
    }
    match previous {
        None => true,
        Some(record) => match denial_severity(&record.status) {
            0 => true,
            1 => record.forced_update_lift_allowed,
            _ => false,
        },
    }
}

async fn load_state_record(state: &State<'_, AppState>) -> AppResult<Option<ProductLicenseStateRecord>> {
    load_state_record_from_db(&state.db).await
}

async fn load_state_record_from_db<C: ConnectionTrait>(db: &C) -> AppResult<Option<ProductLicenseStateRecord>> {
    let row = settings::get_setting(db, PRODUCT_LICENSE_ONBOARDING_KEY, PRODUCT_LICENSE_SCOPE).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let parsed: Result<ProductLicenseStateRecord, _> = serde_json::from_str(&row.setting_value_json);
    if let Ok(record) = parsed {
        return Ok(Some(record));
    }
    let legacy: serde_json::Value = serde_json::from_str(&row.setting_value_json)?;
    Ok(ProductLicenseStateRecord::from_legacy_json(legacy))
}

/// The organisation of this PC's current activation (`None`: not activated). Any connection, so a
/// caller's transaction can read it (`sync::plant_join::complete_pull_done`).
pub async fn get_activation_claim_tenant_id<C: ConnectionTrait>(db: &C) -> AppResult<Option<String>> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(None);
    };
    if !record_has_valid_activation_claim(&record) {
        return Ok(None);
    }
    Ok(record.activation_claim.as_ref().map(|claim| claim.tenant_id.clone()))
}

/// When the licence key of the current activation was entered on this PC (`submitted_at`, set by
/// every key entry and kept by reconciliations); `None` when no key was entered.
pub(crate) async fn activation_submitted_at(db: &DatabaseConnection) -> AppResult<Option<String>> {
    Ok(load_state_record_from_db(db).await?.map(|record| record.submitted_at))
}

pub async fn get_activation_claim_record(db: &DatabaseConnection) -> AppResult<Option<ProductActivationClaimRecord>> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(None);
    };
    if !record_has_valid_activation_claim(&record) {
        return Ok(None);
    }
    Ok(record.activation_claim.clone())
}

/// Minimum claim fields required for login, sync bearer token, and tenant scoping.
fn activation_claim_is_complete(claim: &ProductActivationClaimRecord) -> bool {
    !claim.tenant_id.trim().is_empty()
        && !claim.license_id.trim().is_empty()
        && !claim.machine_fingerprint.trim().is_empty()
        && !claim.activation_token.trim().is_empty()
}

/// True when persisted state includes a usable activation claim (not half-activated / corrupted).
pub fn record_has_valid_activation_claim(record: &ProductLicenseStateRecord) -> bool {
    record
        .activation_claim
        .as_ref()
        .is_some_and(activation_claim_is_complete)
}

/// True when this device has a complete product activation claim (tenant is bound).
/// Used to suppress dev-only demo datasets so they are never mixed with a licensed customer tenant.
pub async fn is_product_activation_complete(db: &DatabaseConnection) -> AppResult<bool> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(false);
    };
    Ok(record_has_valid_activation_claim(&record))
}

fn commercial_edition_from_claim(claim: &ProductActivationClaimRecord) -> Option<String> {
    for candidate in [claim.license_tier.as_deref(), claim.license_plan.as_deref()]
        .into_iter()
        .flatten()
    {
        let v = candidate.trim().to_ascii_lowercase();
        if edition_rank(&v).is_some() {
            return Some(v);
        }
    }
    None
}

const OFFLINE_GRACE_EXPIRED_CODE: &str = "offline_grace_expired";

/// Last successful contact with the control plane (falls back to the last reconciliation for
/// records written before `last_success_at` existed).
fn last_contact_at(record: &ProductLicenseStateRecord) -> Option<&str> {
    record
        .reconciliation
        .last_success_at
        .as_deref()
        .or(record.last_reconciled_at.as_deref())
}

/// End of the offline grace window: last successful contact + the licence's `offline_grace_hours`.
/// `None` when either is unknown (nothing to enforce).
fn offline_grace_deadline(record: &ProductLicenseStateRecord) -> Option<DateTime<Utc>> {
    let hours = record.activation_claim.as_ref()?.offline_grace_hours?;
    let last_contact = DateTime::parse_from_rfc3339(last_contact_at(record)?)
        .ok()?
        .with_timezone(&Utc);
    Some(last_contact + Duration::hours(i64::from(hours)))
}

/// Status as enforced now: a usable activation (active or degraded) whose offline grace window has
/// ended is reported as `DeniedOfflineGraceExpired` until the next successful reconciliation.
fn effective_status(record: &ProductLicenseStateRecord, now: DateTime<Utc>) -> ProductLicenseActivationStatus {
    let usable = matches!(
        record.status,
        ProductLicenseActivationStatus::Active | ProductLicenseActivationStatus::DegradedApiUnavailable
    );
    if usable && offline_grace_deadline(record).is_some_and(|deadline| now > deadline) {
        ProductLicenseActivationStatus::DeniedOfflineGraceExpired
    } else {
        record.status.clone()
    }
}

fn onboarding_state_from_record(record: &ProductLicenseStateRecord) -> ProductLicenseOnboardingState {
    let complete = record_has_valid_activation_claim(record);
    let status = effective_status(record, Utc::now());
    let grace_expired = status == ProductLicenseActivationStatus::DeniedOfflineGraceExpired;
    ProductLicenseOnboardingState {
        complete,
        status,
        // Keep retrying: the denial clears as soon as the control plane is reachable again.
        pending_online_validation: record.pending_online_validation || grace_expired,
        deny_reason_code: if grace_expired {
            Some(OFFLINE_GRACE_EXPIRED_CODE.to_string())
        } else {
            record.deny_reason_code.clone()
        },
        deny_message: if grace_expired {
            Some(format!(
                "No contact with the licence server since {}. Connect this device to the internet to continue.",
                last_contact_at(record).unwrap_or("activation")
            ))
        } else {
            record.deny_message.clone()
        },
        degraded_reason: record.degraded_reason.clone(),
        next_retry_at: record.reconciliation.next_retry_at.clone(),
        retry_attempt: record.reconciliation.retry_attempt,
        last_reconciled_at: record.last_reconciled_at.clone(),
        last_error_code: record.reconciliation.last_error_code.clone(),
        last_error_message: record.reconciliation.last_error_message.clone(),
        tenant_id: record.activation_claim.as_ref().map(|c| c.tenant_id.clone()),
        company_display_name: record.company_display_name.clone(),
        license_edition: record.activation_claim.as_ref().and_then(commercial_edition_from_claim),
    }
}

/// Fields for the License Enforcement canonical view-model.
pub async fn product_activation_view_fields(db: &DatabaseConnection) -> AppResult<(String, Option<String>)> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(("uninitialized".to_string(), None));
    };
    let status = serde_json::to_value(effective_status(&record, Utc::now()))
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "uninitialized".to_string());
    let edition = record.activation_claim.as_ref().and_then(commercial_edition_from_claim);
    Ok((status, edition))
}

/// Tenant context for sync exchange JSON (desktop → control plane).
pub async fn tenant_config_sync_payload(db: &DatabaseConnection) -> AppResult<Option<TenantConfigSyncPayload>> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(None);
    };
    if !record_has_valid_activation_claim(&record) {
        return Ok(None);
    }
    let Some(claim) = record.activation_claim.as_ref() else {
        return Ok(None);
    };
    Ok(Some(TenantConfigSyncPayload {
        tenant_id: claim.tenant_id.clone(),
        is_activated: record.status == ProductLicenseActivationStatus::Active,
        company_display_name: record.company_display_name.clone(),
    }))
}

/// True when at least one active user may administer this tenant (`count_tenant_administrators`):
/// through an assignment scoped to `tenant_id` or to every tenant (no scope reference).
pub async fn tenant_has_administrator_for_id(db: &impl ConnectionTrait, tenant_id: &str) -> AppResult<bool> {
    Ok(count_tenant_administrators(db, AdministratorScope::Tenant(tenant_id), AdministratorTime::Ever).await? > 0)
}

/// True when this PC has an administrator, whatever the organisation (same rule as
/// [`tenant_has_administrator_for_id`], any scope). This, not the licence claim stored here, tells
/// a true first start apart: the stored claim can be replaced by a key entry, the administrators
/// cannot. Once an administrator has been created on this PC it stays true even if every
/// administrator is later disabled or deleted (`auth::builtin_admin::first_admin_marker_closes`).
pub(crate) async fn plant_has_administrator(db: &impl ConnectionTrait) -> AppResult<bool> {
    Ok(
        count_tenant_administrators(db, AdministratorScope::Any, AdministratorTime::Ever).await? > 0
            || crate::auth::builtin_admin::first_admin_marker_closes(db).await?,
    )
}

/// Review E-1 (L3): this PC holds a licence claim and is still reached through the built-in "admin"
/// account, active and never used (its forced password change pending, its password never
/// changed: `auth::builtin_admin::BuiltinAdminRow::is_default_password_account`). Such a PC is not
/// administered (`plant_has_administrator`), yet whoever knows the built-in password must not be
/// able to swap its organisation or wipe it while signed out. So signed-out licence changes are
/// limited exactly as on an administered plant (`authorize_licence_change`: keep or reduce the
/// same activation only) and signed-out resets are refused (`authorize_signed_out_reset`).
/// Creating the first administrator stays open (`bootstrap_initial_tenant_admin` reads only
/// `plant_has_administrator`): it is how the built-in account gets replaced.
pub(crate) async fn pre_administered_by_builtin_account(
    db: &impl ConnectionTrait,
    record: Option<&ProductLicenseStateRecord>,
) -> AppResult<bool> {
    let holds_claim = record
        .and_then(|r| r.activation_claim.as_ref())
        .is_some_and(activation_claim_is_complete);
    if !holds_claim {
        return Ok(false);
    }
    Ok(crate::auth::builtin_admin::builtin_admin_row(db)
        .await?
        .is_some_and(|row| row.is_default_password_account()))
}

/// Which assignments of `adm.settings` an administrator count reads.
#[derive(Clone, Copy)]
enum AdministratorScope<'a> {
    /// Any scope (tenant, wildcard, site, entity...).
    Any,
    /// Tenant scope, any reference: what `check_permission` with `PermissionScope::Global` accepts.
    TenantScope,
    /// Tenant scope for this tenant, or for every tenant (no reference).
    Tenant(&'a str),
}

/// When an assignment counts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AdministratorTime {
    /// Not deleted, whatever its validity window or emergency expiry. "The plant has an
    /// administrator" is fail-closed: once an administrator exists, the passing of time alone (an
    /// assignment reaching its end, an emergency grant expiring, or this PC's clock moved) never
    /// reopens the first-start steps; only an administrator's act does (removing the assignment,
    /// disabling or deleting the account).
    Ever,
    /// In force now, as `check_permission` reads it: who can act right now.
    InForce,
}

/// Active users (not deleted, not disabled) holding `adm.settings`, the right the licence rule asks
/// of its caller (`licence_caller`), by the permission model itself: any role that carries it
/// (custom roles included, never a role name), through the assignments `scope` and `time` select.
///
/// The administrator account the system seeder creates on every PC (its own wildcard assignment)
/// does not count while it has never been taken into use: its forced first password change still
/// pending and its password never changed. So a brand-new PC still has its first start. Once its
/// password has been changed it counts like any administrator.
async fn count_tenant_administrators(
    db: &impl ConnectionTrait,
    scope: AdministratorScope<'_>,
    time: AdministratorTime,
) -> AppResult<i64> {
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = Vec::new();
    let time_filter = match time {
        AdministratorTime::Ever => "usa.deleted_at IS NULL",
        AdministratorTime::InForce => {
            values.extend(crate::rbac::resolver::in_force_binds(&now));
            crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
        }
    };
    values.push(crate::rbac::permissions::ADM_SETTINGS.into());
    let scope_filter = match scope {
        AdministratorScope::Tenant(tenant_id) => {
            values.push(tenant_id.into());
            "usa.scope_type = 'tenant' AND (TRIM(COALESCE(usa.scope_reference, '')) = '' OR usa.scope_reference = ?)"
        }
        AdministratorScope::TenantScope => "usa.scope_type = 'tenant'",
        AdministratorScope::Any => "1 = 1",
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT COUNT(DISTINCT ua.id) AS c
                FROM user_accounts ua
                INNER JOIN user_scope_assignments usa ON usa.user_id = ua.id
                INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
                INNER JOIN permissions p ON p.id = rp.permission_id
               WHERE ua.deleted_at IS NULL AND ua.is_active = 1
                 AND {time_filter}
                 AND p.name = ?
                 AND NOT (ua.username = 'admin' AND COALESCE(usa.notes, '') = 'System seeder'
                          AND ua.force_password_change = 1 AND ua.password_changed_at IS NULL)
                 AND {scope_filter}"
            ),
            values,
        ))
        .await?;
    Ok(row.as_ref().and_then(|r| r.try_get::<i64>("", "c").ok()).unwrap_or(0))
}

/// Whether `user_id` holds `adm.settings` through an assignment in force at any scope (same
/// account and assignment rules as [`count_tenant_administrators`]).
async fn user_holds_settings_right_at_any_scope(db: &DatabaseConnection, user_id: i32) -> AppResult<bool> {
    let now = crate::rbac::resolver::assignment_clock();
    let mut values: Vec<sea_orm::Value> = vec![user_id.into()];
    values.extend(crate::rbac::resolver::in_force_binds(&now));
    values.push(crate::rbac::permissions::ADM_SETTINGS.into());
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT COUNT(1) AS c
                FROM user_accounts ua
                INNER JOIN user_scope_assignments usa ON usa.user_id = ua.id
                INNER JOIN role_permissions rp ON rp.role_id = usa.role_id
                INNER JOIN permissions p ON p.id = rp.permission_id
               WHERE ua.id = ? AND ua.deleted_at IS NULL AND ua.is_active = 1
                 AND {}
                 AND p.name = ?",
                crate::rbac::resolver::ASSIGNMENT_IN_FORCE_SQL
            ),
            values,
        ))
        .await?;
    Ok(row.as_ref().and_then(|r| r.try_get::<i64>("", "c").ok()).unwrap_or(0) > 0)
}

pub(crate) async fn reset_local_tenant_runtime_data_impl(db: &DatabaseConnection) -> AppResult<u64> {
    // Keep only global bootstrap/settings tables; wipe operational/runtime data to avoid cross-tenant leakage.
    let keep_tables: HashSet<&str> = HashSet::from([
        "seaql_migrations",
        "system_config",
        "app_settings",
        "settings_change_events",
        "policy_snapshots",
        "secure_secret_refs",
        "connection_profiles",
        // System signing trust anchors — not tenant data; required for entitlement apply after wipe.
        "licensing_trust_keys",
        // Spent vendor recovery codes: a reset must not make a code usable again (`auth::recovery_code`).
        "admin_recovery_codes_used",
    ]);

    // The plant default language is kept across the reset (F11): the wipe below puts its Settings
    // row back to the shipped value, so it is written back right after the reseed (AU-127).
    let plant_language = crate::locale::read_plant_language(db).await?;
    // The rows the migrations create (system roles, permissions, statuses, catalogs, plant
    // settings) are put back after the wipe, as a new installation holds them: the migrations are
    // not run again and sync never sends these rows. Built before anything is deleted.
    let baseline = crate::db::baseline::MigrationBaseline::build().await?;

    tracing::warn!(
        event = "desktop_tenant_runtime_reset_begin",
        "Starting runtime tenant data reset"
    );
    let table_rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'".to_string(),
        ))
        .await?;

    let mut tables_to_wipe = Vec::new();
    for row in table_rows {
        let table_name: String = row.try_get("", "name")?;
        if !keep_tables.contains(table_name.as_str()) {
            tables_to_wipe.push(table_name);
        }
    }
    tracing::warn!(
        event = "desktop_tenant_runtime_reset_tables",
        table_count = tables_to_wipe.len(),
        tables = ?tables_to_wipe,
        "Runtime reset table selection"
    );

    // The wipe and the reseed below are local maintenance, not changes for other devices: capture
    // is off until it is installed again at the end.
    crate::sync::capture::suspend_capture(db).await?;

    // SQLite ignores (or cannot toggle) foreign_keys inside an active transaction. Run OFF on the
    // connection first, then delete inside a transaction, then restore ON. Requires a single-connection
    // pool (see init_db) so this pragma applies to the same connection as `begin()`.
    db.execute_unprepared("PRAGMA foreign_keys = OFF").await?;

    let wipe_result: AppResult<u64> = async {
        let tx = db.begin().await?;
        // The safety rule notices are otherwise never deleted: the reseed below guards them again.
        // Dropped inside the transaction, so a reset that fails keeps them guarded.
        crate::settings::received_notice::drop_notice_guards(&tx).await?;
        let mut wiped_rows_total: u64 = 0;
        for table in &tables_to_wipe {
            let escaped = table.replace('"', "\"\"");
            let sql = format!("DELETE FROM \"{escaped}\"");
            let result = tx.execute(Statement::from_string(DatabaseBackend::Sqlite, sql)).await?;
            wiped_rows_total = wiped_rows_total.saturating_add(result.rows_affected());
        }

        // `system_config` is kept, the accounts and the sync checkpoint are not: "the server
        // confirmed no user accounts" must come from a new complete pull (`sync::plant_join`), and
        // the PC joins the plant again (its next snapshot is a first join).
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "DELETE FROM system_config WHERE key IN (?, ?)",
            [
                crate::sync::plant_join::SERVER_CONFIRMED_NO_ACCOUNTS_KEY.into(),
                crate::sync::plant_join::COMPLETE_PULL_DONE_KEY.into(),
            ],
        ))
        .await?;
        // The roles are wiped: the reseed below gives the editable ones their whole baseline again.
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "DELETE FROM system_config WHERE substr(key, 1, ?) = ?",
            [
                i64::try_from(crate::db::seeder::SEEDED_ROLE_PERMISSIONS_KEY_PREFIX.len())
                    .unwrap_or(i64::MAX)
                    .into(),
                crate::db::seeder::SEEDED_ROLE_PERMISSIONS_KEY_PREFIX.into(),
            ],
        ))
        .await?;

        // Tenant-scoped preferences are tenant-owned context and should not leak across activations.
        tx.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "DELETE FROM app_settings WHERE setting_scope = 'tenant'".to_string(),
        ))
        .await?;

        // What was deleted comes back as the migrations left it on a new database.
        for table in &tables_to_wipe {
            baseline.restore_into(&tx, table, None).await?;
        }
        baseline
            .restore_into(&tx, "app_settings", Some("setting_scope = 'tenant'"))
            .await?;
        crate::db::baseline::ensure_no_dangling_rows(&tx, &tables_to_wipe).await?;

        tx.commit().await?;
        Ok(wiped_rows_total)
    }
    .await;

    if let Err(e) = db.execute_unprepared("PRAGMA foreign_keys = ON").await {
        tracing::error!(
            error = %e,
            "failed to re-enable SQLite foreign_keys after tenant runtime reset"
        );
    }

    let reseeded: AppResult<u64> = async {
        let wiped_rows_total = wipe_result?;
        crate::db::seeder::seed_system_data(db).await?;
        // Without these rows every outbox staging (so every business write) and the sync worker fail.
        crate::sync::queries::ensure_sync_control_rows(db).await?;
        // Migrations are not re-run after wipe (seaql_migrations kept). Restore system
        // reference_* catalogs that seed_system_data does not cover (lookup_* only).
        crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(db).await?;
        // Never fails the reset (the wipe is committed): the language is set again in Settings.
        if let Some(code) = &plant_language {
            if let Err(e) = crate::locale::write_plant_language(db, code).await {
                tracing::warn!(error = %e, language = %code, "reset: the plant language could not be written back");
            }
        }
        Ok(wiped_rows_total)
    }
    .await;
    // Capture comes back whatever happened above, so later business writes are never missed.
    let installed = crate::sync::capture::install_capture(db).await;
    let wiped_rows_total = reseeded?;
    installed?;
    tracing::warn!(
        event = "desktop_tenant_runtime_reset_complete",
        wiped_rows_total,
        "Runtime tenant data reset committed and reseeded"
    );
    Ok(wiped_rows_total)
}

#[derive(Debug, Deserialize)]
pub struct BootstrapInitialAdminInput {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ActivationBootstrapState {
    pub tenant_id: Option<String>,
    pub company_display_name: Option<String>,
    pub has_tenant_admin: bool,
    /// What the first-start screen shows on an activated PC (`sync::plant_join`); `None` when this
    /// PC is not activated.
    pub plant_join: Option<crate::sync::plant_join::PlantJoinState>,
}

#[derive(Debug, Serialize)]
pub struct ActivationLicenseMetadata {
    pub tenant_id: String,
    pub company_display_name: Option<String>,
    pub license_id: String,
    pub expires_at: Option<String>,
    pub device_limit: Option<i64>,
    pub license_tier: Option<String>,
    pub machine_fingerprint: String,
}

#[tauri::command]
pub async fn get_activation_bootstrap_state(state: State<'_, AppState>) -> AppResult<ActivationBootstrapState> {
    get_activation_bootstrap_state_in(&state.db, &state.sync_worker.status()).await
}

/// `worker`: the sync worker's last status, from which the first-start screen tells "joining
/// your plant" from "a connection is needed" (`plant_join::plant_join_state`).
pub(crate) async fn get_activation_bootstrap_state_in(
    db: &DatabaseConnection,
    worker: &crate::sync::transport::SyncWorkerStatus,
) -> AppResult<ActivationBootstrapState> {
    let record = load_state_record_from_db(db).await?;
    let Some(record) = record else {
        return Ok(ActivationBootstrapState {
            tenant_id: None,
            company_display_name: None,
            has_tenant_admin: false,
            plant_join: None,
        });
    };
    let tenant_id = record.activation_claim.as_ref().map(|c| c.tenant_id.clone());
    // As `bootstrap_initial_tenant_admin` decides: an administrator of any organisation on this PC
    // closes the first-administrator step.
    let has_tenant_admin = tenant_id.is_some() && plant_has_administrator(db).await?;
    let plant_join = match tenant_id.as_deref() {
        Some(tenant) => {
            let licence_syncs = crate::sync::plant_join::licence_syncs(db).await?;
            Some(crate::sync::plant_join::plant_join_state(db, tenant, has_tenant_admin, licence_syncs, worker).await?)
        }
        None => None,
    };
    Ok(ActivationBootstrapState {
        tenant_id,
        company_display_name: record.company_display_name,
        has_tenant_admin,
        plant_join,
    })
}

/// The first-start screen joins the plant: the sync worker runs a cycle soon instead of waiting for
/// its next re-check (it reads the activation made a moment ago), and again on "Retry". Signed out,
/// so only while this PC has no administrator (afterwards sync is run by signed-in operators:
/// `run_sync_now`, `sync.view`), and as `SyncCommand::JoinPlant`, not `RunNow`: repeated calls
/// coalesce, keep the failure count and never run before the wake spacing or a server
/// `Retry-After`.
#[tauri::command]
pub async fn request_plant_join(state: State<'_, AppState>) -> AppResult<()> {
    if get_activation_claim_tenant_id(&state.db).await?.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_LICENSE_NOT_ACTIVATED: Device is not activated.".into(),
        ]));
    }
    if plant_has_administrator(&state.db).await? {
        return Err(first_admin_exists());
    }
    state.sync_worker.send(crate::sync::transport::SyncCommand::JoinPlant);
    Ok(())
}

#[tauri::command]
pub async fn get_activation_license_metadata(
    state: State<'_, AppState>,
) -> AppResult<Option<ActivationLicenseMetadata>> {
    let record = load_state_record(&state).await?;
    let Some(record) = record else {
        return Ok(None);
    };
    let Some(claim) = record.activation_claim else {
        return Ok(None);
    };
    if !activation_claim_is_complete(&claim) {
        return Ok(None);
    }
    Ok(Some(ActivationLicenseMetadata {
        tenant_id: claim.tenant_id,
        company_display_name: record.company_display_name,
        license_id: claim.license_id,
        expires_at: claim.expires_at,
        device_limit: claim.device_limit,
        license_tier: claim.license_tier,
        machine_fingerprint: claim.machine_fingerprint,
    }))
}

/// A transaction that holds the database write lock from its start, as SQLite's
/// `BEGIN IMMEDIATE` does: sea-orm only opens a deferred `BEGIN`, and SQLite takes the write lock
/// at the first write of a transaction, so the first statement is a write that changes no row.
/// A second writer waits (busy timeout) until this one commits, then reads what it wrote; it never
/// reads first and fails on the write with a stale snapshot.
async fn begin_write_transaction(db: &DatabaseConnection) -> AppResult<sea_orm::DatabaseTransaction> {
    let txn = db.begin().await?;
    txn.execute_unprepared("UPDATE user_accounts SET id = id WHERE 0")
        .await?;
    Ok(txn)
}

/// A first-start action refused because the plant already has an administrator.
fn first_admin_exists() -> AppError {
    crate::errors::org_validation_failed(
        "FIRST_ADMIN_ALREADY_EXISTS",
        "An administrator already exists for this activation: sign in with that account.",
        std::collections::HashMap::new(),
    )
}

/// A first-start action refused because no complete pull has confirmed that the organisation has
/// no user account on the server (`sync::plant_join`): this PC may belong to a plant whose
/// accounts have not arrived yet.
fn plant_join_required() -> AppError {
    crate::errors::org_validation_failed(
        "FIRST_ADMIN_PLANT_JOIN_REQUIRED",
        "This PC has not joined your plant yet: it must synchronise with the server once to check whether your \
         organisation already has accounts. Connect it to the network, then retry.",
        std::collections::HashMap::new(),
    )
}

/// A first-administrator creation that could not take the database (another one is being written):
/// coded, to be retried.
fn first_admin_retry() -> AppError {
    crate::errors::org_validation_failed(
        "LICENSE_FIRST_ADMIN_RETRY",
        "The first administrator could not be saved because another one is being created at the same time: wait a \
         moment, then retry.",
        std::collections::HashMap::new(),
    )
}

/// Creates the first administrator of the activated tenant. Runs without a session (first start),
/// so it is refused as soon as this PC has an administrator, for this organisation or any other
/// (`plant_has_administrator`): the organisation stored here is not proof of a first start, since
/// it can be replaced, while the administrators stay until local data is reset (which itself needs
/// an administrator once one exists). It is also refused until a complete sync pull has confirmed
/// that the organisation has no user account on the server (`sync::plant_join`, CMMS Advisor
/// decision 2026-10-21): a new PC of an existing plant gets that plant's accounts, never a second
/// first administrator; the creation consumes that fact. A licence without sync (a single-PC
/// plant, `plant_join::licence_syncs`) has no plant to join and skips that condition. The checks
/// and the account, role and scope rows are one write transaction that holds the write lock from
/// its start (`begin_write_transaction`): two submissions at once make one administrator, the
/// other waits and gets the coded refusal.
/// The password meets the plant password policy (`builtin_admin::validate_first_admin_password`);
/// the same transaction records the "first administrator created" marker, which keeps this screen
/// closed for good; once committed, a built-in "admin" never used is retired
/// (`builtin_admin::retire_after_administration_change`).
#[tauri::command]
pub async fn bootstrap_initial_tenant_admin(
    input: BootstrapInitialAdminInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    bootstrap_initial_tenant_admin_in(&state.db, input).await
}

pub(crate) async fn bootstrap_initial_tenant_admin_in(
    db: &DatabaseConnection,
    input: BootstrapInitialAdminInput,
) -> AppResult<()> {
    let tenant_id = get_activation_claim_tenant_id(db).await?.ok_or_else(|| {
        AppError::ValidationFailed(vec!["GATE_LICENSE_NOT_ACTIVATED: Device is not activated.".into()])
    })?;
    if plant_has_administrator(db).await? {
        return Err(first_admin_exists());
    }
    // A single-PC licence (no sync) has no plant to join.
    let must_join = crate::sync::plant_join::licence_syncs(db).await?;
    if must_join && !crate::sync::plant_join::server_confirmed_no_accounts(db, &tenant_id).await? {
        return Err(plant_join_required());
    }

    let username = input.username.trim();
    if username.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_LICENSE_USERNAME_REQUIRED: username is required.".into(),
        ]));
    }
    let email = input.email.trim();
    if email.is_empty() || !email.contains('@') || !email.contains('.') {
        return Err(AppError::ValidationFailed(vec![
            "GATE_LICENSE_EMAIL_REQUIRED: A valid email is required.".into(),
        ]));
    }
    crate::auth::builtin_admin::validate_first_admin_password(db, &input.password).await?;
    let password_hash = crate::auth::password::hash_password(&input.password)?;

    let written = async {
        let txn = begin_write_transaction(db).await?;
        if plant_has_administrator(&txn).await? {
            return Err(first_admin_exists());
        }
        // An account received while the form was being filled in closes the creation here too.
        if must_join && !crate::sync::plant_join::server_confirmed_no_accounts(&txn, &tenant_id).await? {
            return Err(plant_join_required());
        }
        let exists = txn
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE LOWER(username) = LOWER(?) AND deleted_at IS NULL",
                [username.to_string().into()],
            ))
            .await?;
        if exists.is_some() {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_ADMIN_USERNAME_TAKEN: Username '{username}' is already taken"
            )]));
        }

        let role_row = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT id FROM roles WHERE name = 'Administrator' AND deleted_at IS NULL LIMIT 1".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Administrator role not found in seed data")))?;
        let role_id: i64 = role_row.try_get("", "id")?;

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let entity_sync_id = Uuid::new_v4().to_string();
        let display_name = input
            .display_name
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| "Administrator".to_string());

        // The administrator chose this password on this screen, so it is not a password set by
        // someone else: no forced change at the first sign-in (`force_password_change = 0`), and
        // its date is recorded (`password_changed_at`), else the sign-in reads a password never
        // set and forces the change anyway (`password_policy::PasswordExpiryStatus::NeverSet`).
        // Accounts an administrator creates keep their forced change (`admin_users`).
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            r"INSERT INTO user_accounts
                (entity_sync_id, username, display_name, identity_mode, password_hash,
                 is_active, is_admin, force_password_change, password_changed_at, failed_login_attempts,
                 created_at, updated_at, row_version)
             VALUES (?, ?, ?, 'local', ?, 1, 1, 0, ?, 0, ?, ?, 1)",
            vec![
                entity_sync_id.clone().into(),
                username.to_string().into(),
                display_name.into(),
                password_hash.into(),
                Utc::now().to_rfc3339().into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;

        let id_row = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to read new user id")))?;
        let new_id: i64 = id_row.try_get("", "id")?;
        // The administrator chose the password: the account is its author and its own root (who
        // set it, `password_set_*`; `sync::credential_check`).
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE user_accounts SET password_changed_by_id = id, password_set_by_id = id, \
             password_set_at = password_changed_at WHERE id = ?",
            [new_id.into()],
        ))
        .await?;

        let assign_sync = Uuid::new_v4().to_string();
        let now_iso = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            r"INSERT INTO user_scope_assignments
                (entity_sync_id, user_id, role_id, scope_type, scope_reference,
                 valid_from, valid_to, assigned_by_id, notes, created_at, updated_at, row_version)
             VALUES (?, ?, ?, 'tenant', ?, NULL, NULL, NULL, ?, ?, ?, 1)",
            vec![
                assign_sync.into(),
                new_id.into(),
                role_id.into(),
                tenant_id.clone().into(),
                "Bootstrap initial tenant admin".into(),
                now_iso.clone().into(),
                now_iso.into(),
            ],
        ))
        .await?;
        crate::auth::builtin_admin::record_first_administrator_in(&txn, &entity_sync_id).await?;
        crate::sync::plant_join::forget(&txn).await?;
        txn.commit().await?;
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(e) = written {
        // Two first-start submissions at once: the one that lost the write finds the
        // administrator the other one created; if the other one has not committed yet (its write
        // outlasted the busy timeout), a coded "retry", never the raw database error.
        return Err(match plant_has_administrator(db).await {
            Ok(true) => first_admin_exists(),
            Ok(false) if !matches!(e, AppError::Database(_)) => e,
            _ => first_admin_retry(),
        });
    }

    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::USER_CREATED,
            summary: "Bootstrap initial tenant administrator",
            detail_json: Some(
                serde_json::json!({
                    "username": username,
                    "email": email,
                    "tenant_id": tenant_id,
                    "bootstrap": true,
                })
                .to_string(),
            ),
            ..Default::default()
        },
    )
    .await;
    crate::auth::builtin_admin::retire_after_administration_change(db).await;

    Ok(())
}

async fn persist_state_record_in(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    record: &ProductLicenseStateRecord,
    summary: &str,
) -> AppResult<()> {
    let json = serde_json::to_string(record)?;
    settings::set_setting(
        db,
        PRODUCT_LICENSE_ONBOARDING_KEY,
        PRODUCT_LICENSE_SCOPE,
        &json,
        user_id,
        summary,
    )
    .await
}

#[tauri::command]
pub async fn get_product_license_onboarding_state(
    state: State<'_, AppState>,
) -> AppResult<ProductLicenseOnboardingState> {
    let record = load_state_record(&state).await?;
    if let Some(record) = record {
        Ok(onboarding_state_from_record(&record))
    } else {
        Ok(ProductLicenseOnboardingState {
            complete: false,
            status: ProductLicenseActivationStatus::Uninitialized,
            pending_online_validation: false,
            deny_reason_code: None,
            deny_message: None,
            degraded_reason: None,
            next_retry_at: None,
            retry_attempt: 0,
            last_reconciled_at: None,
            last_error_code: None,
            last_error_message: None,
            tenant_id: None,
            company_display_name: None,
            license_edition: None,
        })
    }
}

/// Who asks for a licence change: the signed-in user (if any), whether they hold `adm.settings`
/// plant-wide, and whether their identity re-check (step-up) is valid.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LicenceCaller {
    pub user_id: Option<i32>,
    pub may_administer: bool,
    pub step_up: bool,
}

/// The caller of a licence command, from its session. May administer: `adm.settings` at tenant
/// scope (`check_permission`, global), or, on a plant where nobody holds it at tenant scope right
/// now, `adm.settings` at any scope. Every holder at any scope closes the first-start steps
/// (`plant_has_administrator`), so without that second case a plant whose only administrators are
/// site-scoped would be "administered" with nobody able to approve a licence change. Where a
/// tenant-scope holder exists, only such a holder approves.
pub(crate) async fn licence_caller(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    step_up: bool,
) -> AppResult<LicenceCaller> {
    let may_administer = match user_id {
        Some(user_id) => {
            crate::auth::rbac::check_permission(
                db,
                user_id,
                crate::rbac::permissions::ADM_SETTINGS,
                &crate::auth::rbac::PermissionScope::Global,
            )
            .await?
                || (count_tenant_administrators(db, AdministratorScope::TenantScope, AdministratorTime::InForce)
                    .await?
                    == 0
                    && user_holds_settings_right_at_any_scope(db, user_id).await?)
        }
        None => false,
    };
    Ok(LicenceCaller {
        user_id,
        may_administer,
        step_up: step_up && user_id.is_some(),
    })
}

/// The signed-in user as every other command sees it (`require_session!`: no idle-locked or
/// expired session, no revoked stored session, no disabled account, no pending forced password
/// change).
async fn session_user(state: &State<'_, AppState>) -> AppResult<crate::auth::session_manager::AuthenticatedUser> {
    Ok(require_session!(state))
}

/// The caller of a licence command. A session that fails the checks of `require_session!` counts
/// as signed out (the licence rule then applies its signed-out limits; nothing is refused for it).
async fn caller_of(state: &State<'_, AppState>) -> AppResult<LicenceCaller> {
    let has_session = state.session.read().await.current.is_some();
    let user_id = if has_session {
        session_user(state).await.ok().map(|user| user.user_id)
    } else {
        None
    };
    let step_up = user_id.is_some() && state.session.read().await.is_step_up_valid();
    licence_caller(&state.db, user_id, step_up).await
}

/// How a licence claim reaches this PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LicenceChange {
    /// A licence key entered (pre-login gate or in-app).
    KeyEntry,
    /// The result of the licence check with the control plane (every start, before sign-in).
    Reconciliation,
}

/// A licence change refused without an administrator signed in (coded, translated on screen).
fn reconcile_admin_required() -> AppError {
    crate::errors::org_validation_failed(
        "LICENSE_RECONCILE_ADMIN_REQUIRED",
        "This licence change needs an administrator: sign in as an administrator; the licence screen then asks \
         you to confirm your identity and applies the change.",
        std::collections::HashMap::new(),
    )
}

/// A licence update refused without an administrator signed in because it extends the licence
/// terms or lifts a denial (coded, translated on screen).
fn extension_admin_required() -> AppError {
    crate::errors::org_validation_failed(
        "LICENSE_EXTENSION_ADMIN_REQUIRED",
        "This licence update extends the licence terms or lifts a denial (expiry, devices, edition, \
         entitlements): sign in as an administrator; the licence screen then asks you to confirm your identity \
         and applies the update.",
        std::collections::HashMap::new(),
    )
}

/// The commercial editions, lowest first: the order of what they grant in
/// `src/lib/edition-capability-catalog.ts` (`SYSTEM_EDITION_IDS`): professional adds modules to
/// core, enterprise to professional; development holds every enterprise module and is not a
/// commercial edition, so moving into it ranks as a raise. The one place this order is defined.
const EDITION_ORDER: [&str; 4] = ["core", "professional", "enterprise", "development"];

/// Rank of a commercial edition in [`EDITION_ORDER`]; `None` for an edition this PC does not know.
fn edition_rank(edition: &str) -> Option<usize> {
    let edition = edition.trim().to_ascii_lowercase();
    EDITION_ORDER.iter().position(|known| *known == edition)
}

/// OWNER DECISION (option A, 2026-10-19): a term (expiry, device limit, edition, offline grace)
/// stated on NEITHER side, the stored licence nor the incoming one, is "nothing changed", so it is
/// not an extension and a signed-out refresh may carry it. Flip this one value to make it an
/// extension again (then a licence without that term needs an administrator with step-up at every
/// refresh). A term missing on only one side is always an extension (`term_extended`).
const fn term_missing_on_both_sides_extends() -> bool {
    false
}

/// Whether going from `stored` to `incoming` extends a term, fail-closed: a term missing on one
/// side only is an extension (a licence that states no expiry, device limit, edition or offline
/// grace may mean "unlimited", so it never compares as a keep with one that states it); missing on
/// both sides is decided by [`term_missing_on_both_sides_extends`]; `raises` decides between two
/// values.
fn term_extended<T>(stored: Option<T>, incoming: Option<T>, raises: impl FnOnce(T, T) -> bool) -> bool {
    match (stored, incoming) {
        (Some(stored), Some(incoming)) => raises(stored, incoming),
        (None, None) => term_missing_on_both_sides_extends(),
        _ => true,
    }
}

/// Whether a restriction that is on when `Some(true)` is lifted: on before, not on now. A
/// restriction absent or off before may be turned on (stricter) or stay off.
fn restriction_lifted(stored: Option<bool>, incoming: Option<bool>) -> bool {
    stored == Some(true) && incoming != Some(true)
}

/// The numeric parts of a version (`1.4.2`, `v1.4.2`, `1.4.2-rc1` reads as `1.4.2`); `None` when it
/// is not a dotted number.
fn version_parts(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    if core.is_empty() {
        return None;
    }
    core.split('.').map(|part| part.parse::<u64>().ok()).collect()
}

/// Whether the minimum app version a licence forces is lowered or removed (looser); a forced
/// minimum this PC cannot read, on either side, counts as lowered.
fn minimum_version_lowered(stored: Option<&str>, incoming: Option<&str>) -> bool {
    let stored = stored.map(str::trim).filter(|v| !v.is_empty());
    let incoming = incoming.map(str::trim).filter(|v| !v.is_empty());
    match (stored, incoming) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(stored), Some(incoming)) => match (version_parts(stored), version_parts(incoming)) {
            (Some(mut stored), Some(mut incoming)) => {
                let len = stored.len().max(incoming.len());
                stored.resize(len, 0);
                incoming.resize(len, 0);
                incoming < stored
            }
            _ => true,
        },
    }
}

/// The version of the app running on this PC: the one the app reports everywhere
/// (`commands::app::get_app_info`, read by the licence screen and sent to the control plane), fixed
/// at build time. The in-app updater reports the same version to the update server
/// (`updater::running_version`), so the forced update the licence asks for and the update offered
/// are decided on one version.
pub(crate) fn running_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether `running` meets the forced minimum version `minimum`, fail-closed: both must read as
/// dotted numbers (`version_parts`); a running pre-release (`1.4.0-rc1`) is below its own release,
/// so it meets only a lower minimum.
fn version_meets_minimum(running: &str, minimum: &str) -> bool {
    let (Some(mut run), Some(mut min)) = (version_parts(running), version_parts(minimum)) else {
        return false;
    };
    let len = run.len().max(min.len());
    run.resize(len, 0);
    min.resize(len, 0);
    let prerelease = running.trim().contains(['-', '+']);
    if prerelease {
        run > min
    } else {
        run >= min
    }
}

/// Whether a claim lifting the forced update recorded here only states what this PC can check for
/// itself: the update was installed. That is: the record holds a forced-update denial that a success
/// claim recorded with its forced minimum version (`forced_update_lift_allowed`; never one recorded
/// from a bare denial outcome, which names no minimum, nor one that replaced another denial), the
/// stored claim forces the update with that minimum, the incoming claim says explicitly that no
/// update is forced and keeps the forced minimum version (not lowered, checked apart by
/// `minimum_version_lowered`), and the app running here meets the stored forced minimum
/// ([`running_app_version`]). Then lifting it is not an extension; any other lift is one (no stored
/// minimum, one this PC cannot read, a PC still below it, or a denial not recorded that way).
fn forced_update_installed(record: &ProductLicenseStateRecord, incoming: &LicenceChangeTarget) -> bool {
    record.status == ProductLicenseActivationStatus::DeniedForceUpdateRequired
        && record.forced_update_lift_allowed
        && incoming.force_update_required == Some(false)
        && record.activation_claim.as_ref().is_some_and(|stored| {
            stored.force_update_required == Some(true)
                && stored
                    .force_min_app_version
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .is_some_and(|minimum| version_meets_minimum(running_app_version(), minimum))
        })
}

fn parse_instant(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.trim())
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// A later instant (or one this PC cannot read) extends.
fn instant_raised(stored: &str, incoming: &str) -> bool {
    match (parse_instant(stored), parse_instant(incoming)) {
        (Some(stored), Some(incoming)) => incoming > stored,
        _ => true,
    }
}

/// A higher edition, or an edition this PC does not know on either side, extends.
fn edition_raised(stored: &str, incoming: &str) -> bool {
    match (edition_rank(stored), edition_rank(incoming)) {
        (Some(stored), Some(incoming)) => incoming > stored,
        _ => true,
    }
}

/// The commercial edition a claim states (`license_tier`, else `license_plan`).
fn claim_edition<'a>(license_tier: Option<&'a str>, license_plan: Option<&'a str>) -> Option<&'a str> {
    [license_tier, license_plan]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|v| !v.is_empty())
}

/// Whether the claim terms of `incoming` extend those stored here: a later expiry, more devices, a
/// higher edition or a longer offline grace (any of these missing on either side counts as
/// extended), or a looser policy: a forced update lifted (unless `update_installed`: the record's
/// forced update is lifted by installing it, `forced_update_installed`) or a forced minimum version
/// lowered or removed, immediate disconnection on trust revocation or the fresh-heartbeat rule on
/// reconnect lifted, another update channel. These fields are not covered by any signature, so they
/// always follow keep-or-reduce without an administrator.
fn claim_terms_extended(
    stored: &ProductActivationClaimRecord,
    incoming: &LicenceChangeTarget,
    update_installed: bool,
) -> bool {
    term_extended(
        stored.expires_at.as_deref(),
        incoming.expires_at.as_deref(),
        instant_raised,
    ) || term_extended(stored.device_limit, incoming.device_limit, |s, i| i > s)
        || term_extended(
            claim_edition(stored.license_tier.as_deref(), stored.license_plan.as_deref()),
            claim_edition(incoming.license_tier.as_deref(), incoming.license_plan.as_deref()),
            edition_raised,
        )
        || term_extended(stored.offline_grace_hours, incoming.offline_grace_hours, |s, i| i > s)
        || (restriction_lifted(stored.force_update_required, incoming.force_update_required) && !update_installed)
        || minimum_version_lowered(
            stored.force_min_app_version.as_deref(),
            incoming.force_min_app_version.as_deref(),
        )
        || restriction_lifted(
            stored.trust_revocation_disconnects_immediately,
            incoming.trust_revocation_disconnects_immediately,
        )
        || restriction_lifted(
            stored.reconnect_requires_fresh_heartbeat,
            incoming.reconnect_requires_fresh_heartbeat,
        )
        || stored.update_channel.as_deref().map(str::trim) != incoming.update_channel.as_deref().map(str::trim)
}

/// The entitlement envelope active on this PC, as the terms an incoming one is compared with.
struct StoredEnvelope {
    verified: bool,
    tier: String,
    state: String,
    channel: String,
    machine_slots: i64,
    feature_flags_json: String,
    capabilities_json: String,
    policy_json: String,
    issued_at: String,
    valid_from: String,
    valid_until: String,
    offline_grace_until: String,
}

async fn load_active_envelope(db: &DatabaseConnection) -> AppResult<Option<StoredEnvelope>> {
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT e.verification_result, e.tier, e.state, e.channel, e.machine_slots, e.feature_flags_json,
                    e.capabilities_json, e.policy_json, e.issued_at, e.valid_from, e.valid_until,
                    e.offline_grace_until
               FROM entitlement_cache_state s
               JOIN entitlement_envelopes e ON e.id = s.active_envelope_id
              WHERE s.id = 1"
                .to_string(),
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(StoredEnvelope {
        verified: row.try_get::<String>("", "verification_result")? == "verified",
        tier: row.try_get("", "tier")?,
        state: row.try_get("", "state")?,
        channel: row.try_get("", "channel")?,
        machine_slots: row.try_get("", "machine_slots")?,
        feature_flags_json: row.try_get("", "feature_flags_json")?,
        capabilities_json: row.try_get("", "capabilities_json")?,
        policy_json: row.try_get("", "policy_json")?,
        issued_at: row.try_get("", "issued_at")?,
        valid_from: row.try_get("", "valid_from")?,
        valid_until: row.try_get("", "valid_until")?,
        offline_grace_until: row.try_get("", "offline_grace_until")?,
    }))
}

/// Whether applying `incoming` would extend the entitlements in force here. The envelope carries no
/// tenant, licence or device field, so even a valid signature does not bind it to this PC (a signed
/// envelope of another organisation, or an older one, could be replayed); and an envelope whose
/// signature does not verify is stored without becoming active, but may rewrite the active row
/// when it reuses its id. So every envelope, signed or not, is compared with the verified envelope
/// in force, fail-closed: it extends when no verified envelope is in force, when it is older
/// (`issued_at`, the only ordering it carries that the control plane sets at each issue), when the
/// edition, device slots or validity window (`valid_from` earlier, `valid_until` later) are raised,
/// when its state leaves suspended or revoked (or turns active again), when its channel,
/// capabilities, feature flags or policy differ, or when its offline grace ends after what the
/// stored terms give (the control plane moves it forward at each contact: `now` or the expiry, plus
/// the licence's `offline_grace_hours`).
async fn envelope_extended(
    db: &DatabaseConnection,
    incoming: &crate::entitlements::domain::EntitlementEnvelopeInput,
    stored_grace_hours: Option<u32>,
) -> AppResult<bool> {
    let Some(stored) = load_active_envelope(db).await? else {
        return Ok(true);
    };
    if !stored.verified {
        return Ok(true);
    }
    let older = match (parse_instant(&stored.issued_at), parse_instant(&incoming.issued_at)) {
        (Some(stored), Some(incoming)) => incoming < stored,
        _ => true,
    };
    let state_raised = incoming.state != stored.state && incoming.state == "active";
    let state_unknown_change = incoming.state != stored.state && stored.state != "active";
    let grace_raised = offline_grace_raised(&stored, incoming, stored_grace_hours);
    Ok(older
        || edition_raised(&stored.tier, &incoming.tier)
        || incoming.machine_slots > stored.machine_slots
        || instant_raised(&stored.valid_until, &incoming.valid_until)
        || instant_raised(&incoming.valid_from, &stored.valid_from)
        || state_raised
        || state_unknown_change
        || incoming.channel != stored.channel
        || incoming.capabilities_json != stored.capabilities_json
        || incoming.feature_flags_json != stored.feature_flags_json
        || incoming.policy_json != stored.policy_json
        || grace_raised)
}

/// Whether the incoming envelope's offline grace ends later than the stored terms give. The control
/// plane sets it, from its own clock, to the licence expiry (`valid_until`) or the issue instant
/// (`issued_at`, for a licence without expiry), plus the licence's `offline_grace_hours`; so it is
/// bounded by those instants of the incoming envelope plus the stored hours, or by the grace end
/// already in force. Only instants are compared with each other: this PC's clock is never read, so a
/// PC clock running behind or ahead changes nothing. An unreadable instant extends.
fn offline_grace_raised(
    stored: &StoredEnvelope,
    incoming: &crate::entitlements::domain::EntitlementEnvelopeInput,
    stored_grace_hours: Option<u32>,
) -> bool {
    let Some(incoming_grace) = parse_instant(&incoming.offline_grace_until) else {
        return true;
    };
    let mut bound = parse_instant(&stored.offline_grace_until);
    if let Some(hours) = stored_grace_hours {
        let hours = Duration::hours(i64::from(hours));
        for base in [parse_instant(&incoming.issued_at), parse_instant(&incoming.valid_until)]
            .into_iter()
            .flatten()
        {
            bound = bound.max(Some(base + hours));
        }
    }
    bound.is_none_or(|bound| incoming_grace > bound)
}

/// The licence a change would bring to this PC: read from the claim, or from the control plane's
/// preview of that claim before it is taken (`check_product_license_key_entry`). Only what the
/// rule reads.
#[derive(Clone, Deserialize)]
pub(crate) struct LicenceChangeTarget {
    pub tenant_id: String,
    pub license_id: String,
    pub machine_fingerprint: String,
    /// The activation token the claim brings; absent from the control plane's preview (the check
    /// asked before the claim), so only the claim itself is held to the token rule.
    #[serde(default)]
    pub activation_token: Option<String>,
    #[serde(default)]
    pub has_demo_data: Option<bool>,
    #[serde(default)]
    pub is_initialized: Option<bool>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub device_limit: Option<i64>,
    #[serde(default)]
    pub license_tier: Option<String>,
    #[serde(default)]
    pub license_plan: Option<String>,
    #[serde(default)]
    pub offline_grace_hours: Option<u32>,
    #[serde(default)]
    pub force_update_required: Option<bool>,
    #[serde(default)]
    pub force_min_app_version: Option<String>,
    #[serde(default)]
    pub update_channel: Option<String>,
    #[serde(default)]
    pub trust_revocation_disconnects_immediately: Option<bool>,
    #[serde(default)]
    pub reconnect_requires_fresh_heartbeat: Option<bool>,
    /// The entitlement envelope that comes with the claim, if any.
    #[serde(skip)]
    pub envelope: Option<crate::entitlements::domain::EntitlementEnvelopeInput>,
}

/// Written by hand so a `?target` in a log line can never print the activation token.
impl std::fmt::Debug for LicenceChangeTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LicenceChangeTarget")
            .field("tenant_id", &self.tenant_id)
            .field("license_id", &self.license_id)
            .field("machine_fingerprint", &self.machine_fingerprint)
            .field(
                "activation_token",
                &self.activation_token.as_ref().map(|_| "[redacted]"),
            )
            .field("has_demo_data", &self.has_demo_data)
            .field("is_initialized", &self.is_initialized)
            .field("expires_at", &self.expires_at)
            .field("device_limit", &self.device_limit)
            .field("license_tier", &self.license_tier)
            .field("license_plan", &self.license_plan)
            .field("offline_grace_hours", &self.offline_grace_hours)
            .field("force_update_required", &self.force_update_required)
            .field("force_min_app_version", &self.force_min_app_version)
            .field("update_channel", &self.update_channel)
            .field(
                "trust_revocation_disconnects_immediately",
                &self.trust_revocation_disconnects_immediately,
            )
            .field(
                "reconnect_requires_fresh_heartbeat",
                &self.reconnect_requires_fresh_heartbeat,
            )
            .field("envelope", &self.envelope)
            .finish()
    }
}

impl LicenceChangeTarget {
    fn from_claim(
        claim: &ProductActivationClaimRecord,
        envelope: Option<&crate::entitlements::domain::EntitlementEnvelopeInput>,
    ) -> Self {
        Self {
            tenant_id: claim.tenant_id.clone(),
            license_id: claim.license_id.clone(),
            machine_fingerprint: claim.machine_fingerprint.clone(),
            activation_token: Some(claim.activation_token.clone()),
            has_demo_data: claim.has_demo_data,
            is_initialized: claim.is_initialized,
            expires_at: claim.expires_at.clone(),
            device_limit: claim.device_limit,
            license_tier: claim.license_tier.clone(),
            license_plan: claim.license_plan.clone(),
            offline_grace_hours: claim.offline_grace_hours,
            force_update_required: claim.force_update_required,
            force_min_app_version: claim.force_min_app_version.clone(),
            update_channel: claim.update_channel.clone(),
            trust_revocation_disconnects_immediately: claim.trust_revocation_disconnects_immediately,
            reconnect_requires_fresh_heartbeat: claim.reconnect_requires_fresh_heartbeat,
            envelope: envelope.cloned(),
        }
    }
}

/// Why a licence change needs an administrator signed in with a fresh identity check.
enum AdminNeeded {
    /// Another organisation, another licence or machine, another activation token not bound to
    /// this activation, demo data, a key without a claim.
    Change,
    /// The same activation with extended terms, or a denial lifted.
    Extension,
}

/// Who the licence rule let a change through for: decides what of the claim is taken as it comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LicenceAuthority {
    /// No administrator on this PC yet (first start): the claim is taken as it comes.
    FirstStart,
    /// An administrator signed in with a valid step-up.
    Administrator,
    /// Nobody who may administer: the same activation, same or reduced terms. Display fields
    /// (organisation name, slug) are kept as stored.
    KeepOrReduce,
}

/// A claim refused whatever the caller: an identifier or token empty, or not exactly what will be
/// stored (leading or trailing spaces), so no whitespace variant of the organisation, licence or
/// machine can be stored and compared later.
fn claim_invalid() -> AppError {
    crate::errors::org_validation_failed(
        "LICENSE_CLAIM_INVALID",
        "The licence data received is incomplete or malformed (organisation, licence, machine or activation \
         token): nothing was changed.",
        std::collections::HashMap::new(),
    )
}

/// An identifier as it may be stored: non-empty and without surrounding spaces.
fn canonical_id(value: &str) -> bool {
    !value.is_empty() && value.trim() == value
}

/// The `iss` and `aud` the control plane signs into a device activation token
/// (`maintafox-vendor-console/api/src/auth-tokens.ts`, `signDeviceActivationToken`).
const DEVICE_TOKEN_ISSUER: &str = "maintafox-control-api";
const DEVICE_TOKEN_AUDIENCE: &str = "maintafox-desktop-device";

/// What a device activation token says about itself (its payload, read without the signature: the
/// control plane signs it with a secret this PC does not hold).
struct DeviceTokenPayload {
    tenant_id: String,
    license_id: String,
    machine_fingerprint: String,
    issued_at: i64,
}

fn device_token_payload(token: &str) -> Option<DeviceTokenPayload> {
    use base64::Engine as _;
    let mut parts = token.split('.');
    let (_header, payload, _signature) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    let audience_ok = match value.get("aud") {
        Some(serde_json::Value::String(aud)) => aud == DEVICE_TOKEN_AUDIENCE,
        Some(serde_json::Value::Array(auds)) => auds.iter().any(|a| a.as_str() == Some(DEVICE_TOKEN_AUDIENCE)),
        _ => false,
    };
    if text("kind") != Some("activation") || text("iss") != Some(DEVICE_TOKEN_ISSUER) || !audience_ok {
        return None;
    }
    Some(DeviceTokenPayload {
        tenant_id: text("tenant_id")?.to_string(),
        license_id: text("license_id")?.to_string(),
        machine_fingerprint: text("machine_fingerprint")?.to_string(),
        issued_at: value.get("iat")?.as_i64()?,
    })
}

/// How much later than the entitlement envelope of the same answer a device token may be issued, in
/// seconds. The control plane builds both in one request from its own clock: the claim signs the
/// token before the envelope, the policy refresh builds the envelope first and then rotates the
/// token, so the token's `iat` (whole seconds) can follow the envelope's `issued_at` by the time the
/// request takes. Five minutes covers a slow request; a forged token can be dated at most this far
/// past the newest envelope the control plane signed.
const DEVICE_TOKEN_AFTER_ENVELOPE_SECONDS: i64 = 300;

/// Whether `envelope`'s signature verifies with the trusted keys compiled into this build.
fn envelope_signature_verified(envelope: &crate::entitlements::domain::EntitlementEnvelopeInput) -> bool {
    crate::entitlements::trusted_keys::verify_entitlement_signature(
        &envelope.issuer,
        &envelope.key_id,
        crate::entitlements::queries::canonical_payload(envelope).as_bytes(),
        &envelope.signature,
    ) == crate::entitlements::trusted_keys::SignatureCheck::Verified
}

/// The control plane's clock as the answer carrying a claim states it, in seconds: the issue instant
/// of the entitlement envelope that comes with it, only when its signature verifies (an unsigned
/// envelope's instant is whatever the sender wrote). `None` without such an envelope.
fn server_time_of(envelope: Option<&crate::entitlements::domain::EntitlementEnvelopeInput>) -> Option<i64> {
    let envelope = envelope.filter(|e| envelope_signature_verified(e))?;
    parse_instant(&envelope.issued_at).map(|instant| instant.timestamp())
}

/// Whether a new activation token may replace the stored one without an administrator. The control
/// plane rotates the device token on a routine refresh (every 7 days, or when an operator asks) and
/// the old one stops working at once, so a token change alone cannot require an administrator
/// without locking plants out every week. Fail-closed instead: the new token must name, in its
/// payload, exactly the organisation, licence and machine stored here, as a device activation token
/// of the control plane, issued no earlier than the stored token (which must read the same way), and
/// no later than the control plane's own clock in the same answer allows (`server_time`, the issue
/// instant of the signed envelope that comes with it, plus [`DEVICE_TOKEN_AFTER_ENVELOPE_SECONDS`]);
/// this PC's clock is never read. Anything else (a token of another activation, an older one, one
/// this PC cannot read, one dated past the server time, or a rotation without a signed envelope)
/// needs an administrator.
///
/// The token's signature cannot be checked here (shared secret of the control plane), so a forged
/// token naming this activation can still be stored signed out. The control plane refuses it at the
/// next contact (it takes the organisation from the verified token, so no plant data reaches another
/// organisation) and the gate falls back to a fresh claim with the key kept on this PC. The genuine
/// token that claim brings is accepted signed out as long as it is not older than the forged one:
/// the bound above keeps a forged `iat` at most five minutes past the newest envelope the control
/// plane signed, so a genuine token issued more than five minutes after that envelope passes. A
/// genuine claim inside those five minutes is refused (needs an administrator) once; the next check
/// (15 minutes later) gets a newer genuine token, which passes.
fn activation_token_bound(stored: &ProductActivationClaimRecord, incoming: &str, server_time: Option<i64>) -> bool {
    if incoming == stored.activation_token {
        return true;
    }
    let (Some(old), Some(new), Some(server_time)) = (
        device_token_payload(&stored.activation_token),
        device_token_payload(incoming),
        server_time,
    ) else {
        return false;
    };
    let names_this_activation = |p: &DeviceTokenPayload| {
        p.tenant_id == stored.tenant_id
            && p.license_id == stored.license_id
            && p.machine_fingerprint == stored.machine_fingerprint
    };
    names_this_activation(&new)
        && names_this_activation(&old)
        && new.issued_at >= old.issued_at
        && new.issued_at <= server_time.saturating_add(DEVICE_TOKEN_AFTER_ENVELOPE_SECONDS)
}

/// Whether `incoming` would lower the denial recorded here ([`denial_severity`]; the offline grace is
/// read from the clock and cleared by any contact, so it is not a recorded denial). A success claim
/// leaves the licence active (severity 0), or denied for a forced update (1) when it forces one; so
/// from a revocation, an expiry, a slot limit or an invalid licence (2) every claim lowers it (and
/// with it every change of the forced-update fields), and from a forced update only a claim kept
/// forced does not. The one lowering that needs no administrator: the forced update recorded here
/// from a success claim with its minimum, once this PC runs a version that meets that minimum
/// (`forced_update_installed`).
fn denial_lifted(record: &ProductLicenseStateRecord, incoming: &LicenceChangeTarget) -> bool {
    let after = if incoming.force_update_required == Some(true) {
        ProductLicenseActivationStatus::DeniedForceUpdateRequired
    } else {
        ProductLicenseActivationStatus::Active
    };
    denial_severity(&after) < denial_severity(&record.status) && !forced_update_installed(record, incoming)
}

/// A denial recorded from the control plane's answer (the offline grace is derived on read, see
/// `effective_status`).
fn status_is_denied(status: &ProductLicenseActivationStatus) -> bool {
    matches!(
        status,
        ProductLicenseActivationStatus::DeniedRevoked
            | ProductLicenseActivationStatus::DeniedExpired
            | ProductLicenseActivationStatus::DeniedSlotLimit
            | ProductLicenseActivationStatus::DeniedForceUpdateRequired
            | ProductLicenseActivationStatus::DeniedInvalid
    )
}

/// The one rule for storing a licence claim on this PC (key entry, its preview check, and
/// reconciliation).
///
/// Refused whatever the caller: a claim whose organisation, licence, machine or activation token is
/// empty or carries surrounding spaces (`claim_invalid`).
///
/// Needs a signed-in user with `adm.settings` and a valid step-up:
/// - on a plant that has an administrator (`plant_has_administrator`), or that holds a claim and is
///   still reached through the never-used built-in account (`pre_administered_by_builtin_account`),
///   whatever the licence state (usable, denied, offline):
///   - a claim for another activation than the one stored here (another organisation, licence or
///     machine, compared exactly), or any claim when none is stored (nothing to compare it with);
///   - a new activation token not bound to this activation (`activation_token_bound`);
///   - a claim that would seed demo data;
///   - a claim that extends the stored terms (`claim_terms_extended`), brings an entitlement
///     envelope that extends those in force (`envelope_extended`), or would lower a denial recorded
///     here (`denial_lifted`, in the order of `denial_severity`): never extended signed out (the one
///     exception: a forced update recorded from a success claim with its minimum, lifted once this
///     PC runs a version meeting it, `forced_update_installed`);
///   - a key entered without a claim, which would forget the activation stored here;
/// - while this PC has a usable licence (active or degraded with a valid claim): any key entry.
///
/// Everything else is accepted without a session: on an administered plant, a claim of the very
/// activation stored here that keeps or reduces its terms and leaves a denial in place
/// ([`LicenceAuthority::KeepOrReduce`]); and any change before the plant has an administrator
/// (first start). Denials and network / HTTP errors carry no claim and are always recorded, but
/// never lower the denial recorded here (a denial outcome below it is kept as a diagnostic only,
/// `denial_severity`; a network or HTTP error leaves a denial in place). Until
/// the control plane signs the claim, its terms are only trusted to keep or reduce what is stored.
pub(crate) async fn authorize_licence_change(
    db: &DatabaseConnection,
    record: Option<&ProductLicenseStateRecord>,
    target: Option<&LicenceChangeTarget>,
    change: LicenceChange,
    caller: LicenceCaller,
) -> AppResult<LicenceAuthority> {
    if let Some(t) = target {
        let ids_ok = canonical_id(&t.tenant_id) && canonical_id(&t.license_id) && canonical_id(&t.machine_fingerprint);
        let token_ok = t.activation_token.as_deref().is_none_or(canonical_id);
        if !ids_ok || !token_ok {
            return Err(claim_invalid());
        }
    }
    let stored = record
        .and_then(|r| r.activation_claim.as_ref())
        .filter(|c| activation_claim_is_complete(c));
    let usable_activation = record.is_some_and(|r| {
        record_has_valid_activation_claim(r)
            && matches!(
                r.status,
                ProductLicenseActivationStatus::Active | ProductLicenseActivationStatus::DegradedApiUnavailable
            )
    });
    // A PC still reached through the never-used built-in account, with a claim stored, is limited
    // as an administered plant (`pre_administered_by_builtin_account`).
    let administered = plant_has_administrator(db).await? || pre_administered_by_builtin_account(db, record).await?;
    let mut needed: Option<AdminNeeded> = None;
    if administered {
        match (target, stored, record) {
            (Some(t), Some(s), Some(r)) => {
                let seeds_demo = crate::db::tenant_bootstrap::claim_would_seed_demo(
                    db,
                    &t.tenant_id,
                    t.has_demo_data,
                    t.is_initialized,
                )
                .await?;
                let same_activation = s.tenant_id == t.tenant_id
                    && s.license_id == t.license_id
                    && s.machine_fingerprint == t.machine_fingerprint;
                let server_time = server_time_of(t.envelope.as_ref());
                let token_unbound = t
                    .activation_token
                    .as_deref()
                    .is_some_and(|token| !activation_token_bound(s, token, server_time));
                if seeds_demo || !same_activation || token_unbound {
                    needed = Some(AdminNeeded::Change);
                } else {
                    let extended = claim_terms_extended(s, t, forced_update_installed(r, t))
                        || denial_lifted(r, t)
                        || match t.envelope.as_ref() {
                            Some(envelope) => envelope_extended(db, envelope, s.offline_grace_hours).await?,
                            None => false,
                        };
                    if extended {
                        needed = Some(AdminNeeded::Extension);
                    }
                }
            }
            // Nothing stored to compare the organisation and terms with.
            (Some(_), _, _) => needed = Some(AdminNeeded::Change),
            // A key without a claim would forget the activation stored here.
            (None, Some(_), _) if change == LicenceChange::KeyEntry => needed = Some(AdminNeeded::Change),
            (None, _, _) => {}
        }
    }
    if needed.is_none() && usable_activation && change == LicenceChange::KeyEntry {
        needed = Some(AdminNeeded::Change);
    }
    let Some(needed) = needed else {
        return Ok(if !administered {
            LicenceAuthority::FirstStart
        } else if caller.may_administer && caller.step_up {
            LicenceAuthority::Administrator
        } else {
            LicenceAuthority::KeepOrReduce
        });
    };
    if !caller.may_administer {
        return Err(match needed {
            AdminNeeded::Change => reconcile_admin_required(),
            AdminNeeded::Extension => extension_admin_required(),
        });
    }
    if !caller.step_up {
        return Err(AppError::StepUpRequired);
    }
    Ok(LicenceAuthority::Administrator)
}

/// Whether a key entry bringing the licence the control plane previewed would be accepted on this
/// PC, asked before the licence is taken on the server (a device slot) and before local data is
/// wiped. Changes nothing; answers as `submit_product_license_key` would (same rule, same coded
/// refusal), so it reveals nothing the real submit does not.
#[tauri::command]
pub async fn check_product_license_key_entry(preview_json: String, state: State<'_, AppState>) -> AppResult<()> {
    let caller = caller_of(&state).await?;
    check_product_license_key_entry_in(&state.db, caller, &preview_json).await
}

pub(crate) async fn check_product_license_key_entry_in(
    db: &DatabaseConnection,
    caller: LicenceCaller,
    preview_json: &str,
) -> AppResult<()> {
    let mut value: serde_json::Value = serde_json::from_str(preview_json).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_LICENSE_PAYLOAD_INVALID: previewJson must be valid JSON: {e}"
        )])
    })?;
    let envelope = match value
        .as_object_mut()
        .and_then(|obj| obj.remove("entitlement_envelope"))
        .filter(|v| !v.is_null())
    {
        Some(env_value) => Some(
            serde_json::from_value::<crate::entitlements::domain::EntitlementEnvelopeInput>(env_value).map_err(
                |e| {
                    AppError::ValidationFailed(vec![format!(
                        "GATE_LICENSE_ENVELOPE_INVALID: entitlement_envelope is invalid: {e}"
                    )])
                },
            )?,
        ),
        None => None,
    };
    if let Some(obj) = value.as_object_mut() {
        sanitize_activation_claim_object(obj);
    }
    let mut target: LicenceChangeTarget = serde_json::from_value(value).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_LICENSE_PAYLOAD_INVALID: previewJson must name the tenant, licence and machine: {e}"
        )])
    })?;
    // As `extract_claim_and_envelope` does for the claim.
    if target.license_plan.is_none() {
        target.license_plan = target.license_tier.clone();
    }
    if target.license_tier.is_none() {
        target.license_tier = target.license_plan.clone();
    }
    target.envelope = envelope;
    let current = load_state_record_from_db(db).await?;
    authorize_licence_change(db, current.as_ref(), Some(&target), LicenceChange::KeyEntry, caller)
        .await
        .map(|_| ())
}

/// A claim let through signed out ([`LicenceAuthority::KeepOrReduce`]) keeps the organisation's
/// display name and slug stored here: the claim is not signed, so it never renames the plant's root
/// organisation or the company shown on screen.
fn keep_stored_display(claim: &mut ProductActivationClaimRecord, stored: Option<&ProductLicenseStateRecord>) {
    let stored_claim = stored.and_then(|r| r.activation_claim.as_ref());
    claim.tenant_display_name = stored_claim.and_then(|c| c.tenant_display_name.clone());
    claim.tenant_slug = stored_claim.and_then(|c| c.tenant_slug.clone());
}

/// Records the key entry of the claim `claim_product_activation` holds in Rust for this key and
/// organisation (`tenant_id`: the one the webview showed and decided the local data reset on).
/// The webview never passes the claim or its activation token.
#[tauri::command]
pub async fn submit_product_license_key(
    key: String,
    tenant_id: String,
    app_version: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let caller = caller_of(&state).await?;
    submit_pending_product_license_key_in(
        &state.db,
        caller,
        &state.pending_activation_claim,
        &key,
        &tenant_id,
        app_version,
    )
    .await
}

pub(crate) async fn submit_product_license_key_in(
    db: &DatabaseConnection,
    caller: LicenceCaller,
    key: &str,
    claim_json: Option<String>,
    machine_fingerprint: Option<String>,
    app_version: Option<String>,
) -> AppResult<()> {
    let changed_by_id = caller.user_id;
    let _record_lock = LICENCE_RECORD_LOCK.lock().await;
    let trimmed = key.trim();
    if trimmed.len() < 8 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_LICENSE_KEY_SHORT: License key must be at least 8 characters.".into(),
        ]));
    }
    let mut entitlement_envelope: Option<crate::entitlements::domain::EntitlementEnvelopeInput> = None;
    let parsed_claim = match claim_json {
        Some(raw) => {
            let (value, envelope) = extract_claim_and_envelope(&raw)?;
            entitlement_envelope = envelope;
            tracing::info!(
                event = "desktop_activation_submit_claim_received",
                tenant_id = value.tenant_id.as_str(),
                license_id = value.license_id.as_str(),
                machine_fingerprint = value.machine_fingerprint.as_str(),
                activation_token_len = value.activation_token.len(),
                has_entitlement_envelope = entitlement_envelope.is_some(),
                "Activation claim payload received before persistence"
            );
            Some(value)
        }
        None => None,
    };
    let current = load_state_record_from_db(db).await?;
    let target = parsed_claim
        .as_ref()
        .map(|claim| LicenceChangeTarget::from_claim(claim, entitlement_envelope.as_ref()));
    let authority =
        authorize_licence_change(db, current.as_ref(), target.as_ref(), LicenceChange::KeyEntry, caller).await?;
    let mut parsed_claim = parsed_claim;
    if authority == LicenceAuthority::KeepOrReduce {
        if let Some(claim) = parsed_claim.as_mut() {
            keep_stored_display(claim, current.as_ref());
        }
    }

    let mut record = ProductLicenseStateRecord {
        schema_version: 1,
        key_sha256: settings::sha256_hex(trimmed),
        license_key_plaintext: Some(trimmed.to_string()),
        machine_fingerprint,
        app_version,
        submitted_at: Utc::now().to_rfc3339(),
        status: ProductLicenseActivationStatus::PendingOnlineValidation,
        pending_online_validation: parsed_claim.is_none(),
        deny_reason_code: None,
        deny_message: None,
        degraded_reason: None,
        activation_claim: parsed_claim.clone(),
        last_reconciled_at: parsed_claim.as_ref().map(|_| Utc::now().to_rfc3339()),
        reconciliation: ProductLicenseReconciliationPolicy::default(),
        diagnostics: vec![],
        company_display_name: if authority == LicenceAuthority::KeepOrReduce {
            current.as_ref().and_then(|r| r.company_display_name.clone())
        } else {
            parsed_claim.as_ref().and_then(|c| c.tenant_display_name.clone())
        },
        forced_update_lift_allowed: parsed_claim
            .as_ref()
            .is_some_and(|claim| forced_update_lift_allowed(current.as_ref(), claim, authority)),
    };
    if let Some(claim) = parsed_claim {
        if claim.force_update_required.unwrap_or(false) {
            record.status = ProductLicenseActivationStatus::DeniedForceUpdateRequired;
            record.pending_online_validation = false;
            record.deny_reason_code = Some("force_update_required".into());
            record.deny_message = Some("Desktop version does not satisfy forced minimum version policy.".into());
            record.push_diagnostic(
                "deny",
                Some("force_update_required".into()),
                "Activation claim requires forced update.".into(),
            );
        } else {
            record.status = ProductLicenseActivationStatus::Active;
            record.pending_online_validation = false;
            record.reconciliation.last_success_at = Some(Utc::now().to_rfc3339());
            record.push_diagnostic("activation", None, "Activation claim accepted.".into());
        }
    } else {
        record.push_diagnostic(
            "activation",
            Some("pending_online_validation".into()),
            "Saved locally; awaiting online reconciliation.".into(),
        );
    }
    tracing::info!(
        event = "desktop_activation_submit",
        user_id = ?changed_by_id,
        pending_online_validation = record.pending_online_validation,
        status = ?record.status,
        has_claim = record.activation_claim.is_some(),
        persisted_tenant_id = record
            .activation_claim
            .as_ref()
            .map(|c| c.tenant_id.as_str())
            .unwrap_or(""),
        persisted_license_id = record
            .activation_claim
            .as_ref()
            .map(|c| c.license_id.as_str())
            .unwrap_or(""),
        persisted_machine_fingerprint = record
            .activation_claim
            .as_ref()
            .map(|c| c.machine_fingerprint.as_str())
            .unwrap_or(""),
        persisted_activation_token_len = record
            .activation_claim
            .as_ref()
            .map(|c| c.activation_token.len())
            .unwrap_or(0),
        "Product license key submit processed"
    );
    persist_state_record_in(
        db,
        changed_by_id,
        &record,
        "product license key submitted (state machine)",
    )
    .await?;
    apply_claim_entitlement_envelope(db, entitlement_envelope).await?;
    crate::db::tenant_bootstrap::bootstrap_from_activation_claim(db, changed_by_id).await?;
    Ok(())
}

/// Records the outcome of a licence check the webview could not complete (no answer, an error
/// answer, a denial). A success is refused (`GATE_LICENSE_SUCCESS_FROM_SERVER_ONLY`): a claim is
/// recorded only from the answer Rust received (`refresh_product_activation_policy`,
/// `reclaim_product_activation`, `submit_product_license_key`), so no claim or activation token
/// comes from the webview.
#[tauri::command]
pub async fn apply_product_license_reconciliation(
    outcome_json: String,
    state: State<'_, AppState>,
) -> AppResult<ProductLicenseOnboardingState> {
    refuse_success_from_webview(&outcome_json)?;
    let caller = caller_of(&state).await?;
    apply_product_license_reconciliation_in(&state.db, caller, &outcome_json).await
}

/// See [`apply_product_license_reconciliation`]. An outcome that cannot be read is left to
/// [`apply_product_license_reconciliation_in`], which refuses it.
fn refuse_success_from_webview(outcome_json: &str) -> AppResult<()> {
    let kind = serde_json::from_str::<serde_json::Value>(outcome_json)
        .ok()
        .and_then(|v| v.get("kind").cloned())
        .and_then(|k| serde_json::from_value::<ProductLicenseReconciliationOutcomeKind>(k).ok());
    if matches!(kind, Some(ProductLicenseReconciliationOutcomeKind::Success)) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_LICENSE_SUCCESS_FROM_SERVER_ONLY: A licence renewal is recorded only from the licence server's \
             answer received by the app: nothing was changed."
                .into(),
        ]));
    }
    Ok(())
}

pub(crate) async fn apply_product_license_reconciliation_in(
    db: &DatabaseConnection,
    caller: LicenceCaller,
    outcome_json: &str,
) -> AppResult<ProductLicenseOnboardingState> {
    let changed_by_id = caller.user_id;
    let _record_lock = LICENCE_RECORD_LOCK.lock().await;
    let mut record = load_state_record_from_db(db).await?.ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "GATE_LICENSE_NO_STATE: No local product license state found.".into()
        ])
    })?;
    let outcome_value: serde_json::Value = serde_json::from_str(outcome_json).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_LICENSE_PAYLOAD_INVALID: outcomeJson must be valid JSON: {e}"
        )])
    })?;
    let entitlement_envelope = match outcome_value
        .get("entitlement_envelope")
        .cloned()
        .filter(|v| !v.is_null())
        .map(serde_json::from_value::<crate::entitlements::domain::EntitlementEnvelopeInput>)
        .transpose()
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                event = "desktop_entitlement_envelope_parse_failed",
                error = %e,
                "Ignoring invalid entitlement_envelope during reconciliation (soft phase)"
            );
            None
        }
    };
    // Strip envelope from claim object before typed deserialize (unknown nested shapes must not fail claim).
    let mut claim_value = outcome_value.get("claim").cloned().unwrap_or(serde_json::Value::Null);
    if let Some(obj) = claim_value.as_object_mut() {
        obj.remove("entitlement_envelope");
    }
    let mut outcome_for_meta = outcome_value.clone();
    if let Some(obj) = outcome_for_meta.as_object_mut() {
        obj.remove("entitlement_envelope");
        obj.insert("claim".into(), claim_value);
    }
    let mut outcome: ProductLicenseReconciliationInput = serde_json::from_value(outcome_for_meta).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "GATE_LICENSE_PAYLOAD_INVALID: outcomeJson must be valid ProductLicenseReconciliationInput JSON: {e}"
        )])
    })?;
    outcome.entitlement_envelope = entitlement_envelope;
    let mut forced_update_liftable = false;
    if let (ProductLicenseReconciliationOutcomeKind::Success, Some(claim)) = (&outcome.kind, outcome.claim.as_mut()) {
        let target = LicenceChangeTarget::from_claim(claim, outcome.entitlement_envelope.as_ref());
        let authority =
            authorize_licence_change(db, Some(&record), Some(&target), LicenceChange::Reconciliation, caller).await?;
        if authority == LicenceAuthority::KeepOrReduce {
            keep_stored_display(claim, Some(&record));
        }
        forced_update_liftable = forced_update_lift_allowed(Some(&record), claim, authority);
    }

    let now = Utc::now().to_rfc3339();
    record.reconciliation.last_attempt_at = Some(now.clone());
    if let Some(app_version) = outcome.app_version.clone() {
        record.app_version = Some(app_version);
    }

    match outcome.kind {
        ProductLicenseReconciliationOutcomeKind::Success => {
            let claim = outcome.claim.ok_or_else(|| {
                AppError::ValidationFailed(vec![
                    "GATE_LICENSE_PAYLOAD_INVALID: Reconciliation success requires claim payload.".into(),
                ])
            })?;
            // Signed out, `keep_stored_display` already put back the stored name (None when none
            // was stored), so the company name shown is only replaced by a name the claim states.
            record.activation_claim = Some(claim.clone());
            if let Some(name) = claim.tenant_display_name.clone() {
                record.company_display_name = Some(name);
            }
            record.machine_fingerprint = Some(claim.machine_fingerprint.clone());
            record.last_reconciled_at = Some(now.clone());
            record.pending_online_validation = false;
            record.degraded_reason = None;
            record.reconciliation.retry_attempt = 0;
            record.reconciliation.next_retry_at = None;
            record.reconciliation.last_success_at = Some(now.clone());
            record.reconciliation.last_error_code = None;
            record.reconciliation.last_error_message = None;
            record.forced_update_lift_allowed = forced_update_liftable;
            if claim.force_update_required.unwrap_or(false) {
                record.status = ProductLicenseActivationStatus::DeniedForceUpdateRequired;
                record.deny_reason_code = Some("force_update_required".into());
                record.deny_message = Some("Desktop version does not satisfy forced minimum version policy.".into());
                record.push_diagnostic(
                    "deny",
                    Some("force_update_required".into()),
                    "Reconciliation denied: force update required.".into(),
                );
            } else {
                record.status = ProductLicenseActivationStatus::Active;
                record.deny_reason_code = None;
                record.deny_message = None;
                record.push_diagnostic("reconciliation", None, "Online reconciliation succeeded.".into());
            }
        }
        ProductLicenseReconciliationOutcomeKind::Denied => {
            let denied_code = outcome.error_code.clone().unwrap_or_else(|| "license_denied".into());
            let denied_status = map_denied_status(Some(&denied_code));
            if denial_severity(&denied_status) < denial_severity(&record.status) {
                // A denial outcome carries no claim and anyone can report one signed out: it never
                // lowers the denial recorded here (a forced update, which an update would lift, in
                // place of a revocation, an expiry, a slot limit or an invalid licence). The recorded
                // denial, its reason and the stored claim stay as they are; only a diagnostic is kept.
                tracing::warn!(
                    event = "desktop_activation_denial_downgrade_ignored",
                    recorded = ?record.status,
                    received = ?denied_status,
                    "Denial outcome less severe than the recorded denial ignored"
                );
                record.push_diagnostic(
                    "deny_kept",
                    Some(denied_code),
                    format!(
                        "Denial ignored: it is less severe than the denial recorded here ({:?}), which is kept.",
                        record.status
                    ),
                );
                persist_state_record_in(
                    db,
                    changed_by_id,
                    &record,
                    "product license reconciliation: less severe denial ignored",
                )
                .await?;
                return Ok(onboarding_state_from_record(&record));
            }
            record.status = denied_status;
            // A forced update recorded from a bare denial names no minimum: never lifted by an update.
            record.forced_update_lift_allowed = false;
            record.pending_online_validation = false;
            record.deny_reason_code = Some(denied_code.clone());
            record.deny_message = Some(
                outcome
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "Control plane denied activation for this device.".into()),
            );
            record.degraded_reason = None;
            record.last_reconciled_at = Some(now.clone());
            record.reconciliation.next_retry_at = None;
            record.reconciliation.last_error_code = Some(denied_code.clone());
            record.reconciliation.last_error_message = outcome.error_message.clone();
            record.push_diagnostic(
                "deny",
                Some(denied_code),
                outcome
                    .error_message
                    .unwrap_or_else(|| "Reconciliation denied by control plane.".into()),
            );
        }
        ProductLicenseReconciliationOutcomeKind::NetworkError | ProductLicenseReconciliationOutcomeKind::HttpError => {
            // An unreachable control plane never lifts a denial recorded here (anyone can report
            // one signed out): only the retry bookkeeping is updated then.
            if !status_is_denied(&record.status) {
                record.pending_online_validation = true;
                record.status = ProductLicenseActivationStatus::DegradedApiUnavailable;
                record.degraded_reason = Some(
                    outcome
                        .error_message
                        .clone()
                        .unwrap_or_else(|| "Activation API unavailable; running in degraded mode.".into()),
                );
            }
            record.reconciliation.retry_attempt = record.reconciliation.retry_attempt.saturating_add(1);
            record.reconciliation.next_retry_at =
                Some(compute_backoff_at(record.reconciliation.retry_attempt).to_rfc3339());
            record.reconciliation.last_error_code = outcome.error_code.clone();
            record.reconciliation.last_error_message = outcome.error_message.clone();
            record.push_diagnostic(
                "reconciliation",
                outcome.error_code.clone(),
                outcome
                    .error_message
                    .unwrap_or_else(|| "Online reconciliation failed; retry scheduled.".into()),
            );
        }
    }

    tracing::info!(
        event = "desktop_activation_reconciliation",
        user_id = ?changed_by_id,
        status = ?record.status,
        pending_online_validation = record.pending_online_validation,
        retry_attempt = record.reconciliation.retry_attempt,
        deny_reason_code = record.deny_reason_code.as_deref().unwrap_or(""),
        last_error_code = record.reconciliation.last_error_code.as_deref().unwrap_or(""),
        "Product license reconciliation outcome applied"
    );

    persist_state_record_in(
        db,
        changed_by_id,
        &record,
        "product license reconciliation state updated",
    )
    .await?;
    if matches!(outcome.kind, ProductLicenseReconciliationOutcomeKind::Success) {
        apply_claim_entitlement_envelope(db, outcome.entitlement_envelope).await?;
    }
    crate::db::tenant_bootstrap::bootstrap_from_activation_claim(db, changed_by_id).await?;

    Ok(onboarding_state_from_record(&record))
}

#[tauri::command]
pub async fn get_product_license_diagnostics(
    state: State<'_, AppState>,
) -> AppResult<Option<ProductLicenseDiagnostics>> {
    let record = load_state_record(&state).await?;
    Ok(record.map(|record| {
        let has_activation_claim = record_has_valid_activation_claim(&record);
        ProductLicenseDiagnostics {
            status: record.status,
            deny_reason_code: record.deny_reason_code,
            deny_message: record.deny_message,
            pending_online_validation: record.pending_online_validation,
            last_reconciled_at: record.last_reconciled_at,
            machine_fingerprint: record.machine_fingerprint,
            app_version: record.app_version,
            reconciliation: record.reconciliation,
            diagnostics: record.diagnostics,
            has_activation_claim,
        }
    }))
}

/// A signed-out local reset refused because this PC holds a licence and is still reached through
/// its never-used built-in administrator account (coded, translated on screen).
fn reset_builtin_admin_pending() -> AppError {
    crate::errors::org_validation_failed(
        "LICENSE_RESET_BUILTIN_ADMIN_PENDING",
        "This PC holds a licence and is still reached through its built-in administrator account: create the \
         first administrator, then sign in with that account to reset local data.",
        std::collections::HashMap::new(),
    )
}

/// A signed-out local reset refused because the plant has an administrator: the two routes that
/// remain are an administrator signed in with a fresh identity check, or a vendor recovery code
/// (coded, translated on screen).
fn reset_requires_admin_signin() -> AppError {
    crate::errors::org_validation_failed(
        "LICENSE_RESET_REQUIRES_ADMIN_SIGNIN",
        "This PC already has an administrator: sign in as an administrator (the password is asked again) to \
         change the licence key, or use a vendor recovery code.",
        std::collections::HashMap::new(),
    )
}

/// The rule for a reset asked without a session (`reset_product_license_activation`,
/// `reset_local_tenant_runtime_data`): allowed only on a true first start. The administrators
/// decide, not the stored claim (which a signed-out key entry can empty): refused once the plant
/// has an administrator (`reset_requires_admin_signin`), and refused on a PC that holds a claim and
/// is still reached through the never-used built-in account (`pre_administered_by_builtin_account`).
pub(crate) async fn authorize_signed_out_reset(db: &DatabaseConnection) -> AppResult<()> {
    if plant_has_administrator(db).await? {
        return Err(reset_requires_admin_signin());
    }
    let record = load_state_record_from_db(db).await?;
    if pre_administered_by_builtin_account(db, record.as_ref()).await? {
        return Err(reset_builtin_admin_pending());
    }
    Ok(())
}

/// Wipes device-scoped product license onboarding (SQLite `app_settings`).
///
/// Authorization:
/// - Authenticated session → `adm.settings` + step-up
/// - No session → only allowed before a tenant administrator exists (bootstrap recovery)
#[tauri::command]
pub async fn reset_product_license_activation(state: State<'_, AppState>) -> AppResult<()> {
    let has_session = state.session.read().await.current.is_some();
    if has_session {
        let user = require_session!(state);
        crate::require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_SETTINGS,
            crate::auth::rbac::PermissionScope::Global
        );
        crate::require_step_up!(state);
    } else {
        authorize_signed_out_reset(&state.db).await?;
    }

    let wiped_rows = reset_to_first_run(&state, "recovery").await?;
    tracing::warn!(
        event = "desktop_activation_reset",
        wiped_rows,
        "Product activation state reset and tenant runtime wiped"
    );
    Ok(())
}

/// Returns the device to its first-run state: wipes tenant runtime data, forgets the product
/// license onboarding and tenant bootstrap, and ends the session. Callers enforce authorization.
pub(crate) async fn reset_to_first_run(state: &State<'_, AppState>, reason: &str) -> AppResult<u64> {
    let changed_by_id = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .map(|session| session.user.user_id);
    // Same lock as key entry and reconciliation, so a check in flight cannot write the
    // licence record back after it is forgotten here.
    let _record_lock = LICENCE_RECORD_LOCK.lock().await;
    let wiped_rows = reset_local_tenant_runtime_data_impl(&state.db).await?;
    let _ = settings::delete_setting(
        &state.db,
        PRODUCT_LICENSE_ONBOARDING_KEY,
        PRODUCT_LICENSE_SCOPE,
        changed_by_id,
        &format!("product license activation reset ({reason})"),
    )
    .await?;
    let _ = settings::delete_setting(
        &state.db,
        TENANT_BOOTSTRAP_KEY,
        TENANT_BOOTSTRAP_SCOPE,
        changed_by_id,
        &format!("tenant bootstrap reset ({reason})"),
    )
    .await?;
    {
        let mut session_guard = state.session.write().await;
        session_guard.clear_session();
    }
    Ok(wiped_rows)
}

#[tauri::command]
pub async fn reset_local_tenant_runtime_data(state: State<'_, AppState>) -> AppResult<u64> {
    let has_session = state.session.read().await.current.is_some();
    if has_session {
        let user = require_session!(state);
        crate::require_permission!(
            state,
            &user,
            crate::rbac::permissions::ADM_SETTINGS,
            crate::auth::rbac::PermissionScope::Global
        );
        crate::require_step_up!(state);
    } else {
        authorize_signed_out_reset(&state.db).await?;
    }

    tracing::warn!(
        event = "desktop_tenant_runtime_reset_command",
        "reset_local_tenant_runtime_data command invoked"
    );
    let wiped_rows = reset_local_tenant_runtime_data_impl(&state.db).await?;
    {
        let mut session_guard = state.session.write().await;
        session_guard.clear_session();
    }
    tracing::warn!(
        event = "desktop_tenant_runtime_reset",
        wiped_rows,
        "Tenant runtime data reset completed"
    );
    Ok(wiped_rows)
}

/// Bearer token from the last successful activation claim (`/api/v1/activation/claim`) for the
/// Rust sync worker. No command returns the stored token: sync, updates and the licence policy
/// refresh (`refresh_product_activation_policy`) all read it here, in Rust. `None` unless the
/// activation is currently active. The activation claim and the tenant initialisation are sent
/// from Rust too (`claim_product_activation`, `mark_activation_tenant_initialized`).
pub async fn sync_activation_bearer_token(db: &DatabaseConnection) -> AppResult<Option<String>> {
    activation_bearer_token_when(db, &[ProductLicenseActivationStatus::Active]).await
}

/// Bearer token for the update manifest (`updater::ask_manifest`). As for sync, plus one denial: a
/// forced update (`DeniedForceUpdateRequired`) is ended on this PC only by installing the update,
/// so the update server must still be asked while the licence blocks everything else (the server
/// accepts the token: a forced update is not a revoked activation). Any other denial, the offline
/// grace expired, or no activation: none (the server would refuse the device anyway).
pub async fn update_manifest_bearer_token(db: &DatabaseConnection) -> AppResult<Option<String>> {
    activation_bearer_token_when(
        db,
        &[
            ProductLicenseActivationStatus::Active,
            ProductLicenseActivationStatus::DeniedForceUpdateRequired,
        ],
    )
    .await
}

/// The stored activation token when the effective licence status is one of `allowed` and the
/// stored claim is a valid activation claim.
async fn activation_bearer_token_when(
    db: &DatabaseConnection,
    allowed: &[ProductLicenseActivationStatus],
) -> AppResult<Option<String>> {
    let Some(rec) = load_state_record_from_db(db).await? else {
        return Ok(None);
    };
    if !allowed.contains(&effective_status(&rec, Utc::now())) {
        return Ok(None);
    }
    if !record_has_valid_activation_claim(&rec) {
        return Ok(None);
    }
    Ok(rec.activation_claim.map(|c| c.activation_token))
}

/// Licence policy refresh on the control API: the stored activation renewed without taking a
/// device slot.
const POLICY_REFRESH_PATH: &str = "/api/v1/activation/policy-refresh";
/// Longest app version the licence server accepts.
const APP_VERSION_MAX_LEN: usize = 32;
/// Longest server error body kept in a policy-refresh result (the technical details on screen).
const POLICY_REFRESH_DETAILS_MAX_CHARS: usize = 4000;

/// What one licence policy refresh gave (`refresh_product_activation_policy`). Never carries the
/// activation token: the request is sent from here and the token stays in Rust.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyRefreshResult {
    /// No valid activation claim is stored on this PC: nothing was sent.
    NoToken,
    /// The server renewed the activation; its claim was recorded as a successful reconciliation.
    Refreshed { state: ProductLicenseOnboardingState },
    /// The server answered an error status. `code` and `message` are read from its body
    /// (`policy_refresh_error_fields`, as for every activation call); `details` is the body.
    HttpError {
        status: u16,
        code: Option<String>,
        message: Option<String>,
        details: Option<String>,
    },
    /// The server answered 2xx with a body that is not a licence claim, or an answer too large to
    /// read (`RAW_ANSWER_MAX_BYTES`), or the request could not be built. Nothing was recorded.
    InvalidResponse { message: String },
    /// No answer from the server (offline, connection refused, interrupted).
    NetworkError { message: String },
}

/// The version sent to the licence server, compared there against a forced-update minimum:
/// MAJOR.MINOR.PATCH with an optional pre-release ("0.4.0-dev" is below "0.4.0"), at most 32
/// characters as the server accepts.
fn well_formed_app_version(version: &str) -> bool {
    if version.len() > APP_VERSION_MAX_LEN {
        return false;
    }
    let (core, pre_release) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && pre_release
            .is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
}

/// What replaces an activation token in a text that leaves Rust.
const TOKEN_MARKER: &str = "[activation token]";
/// What replaces the licence key in a text that leaves Rust.
const KEY_MARKER: &str = "[licence key]";

/// The secrets sent with an activation call (`(value, marker)`) replaced by their marker in a text
/// that leaves the call (result, logs).
fn without_secrets(text: String, secrets: &[(&str, &str)]) -> String {
    secrets
        .iter()
        .filter(|(value, _)| !value.is_empty())
        .fold(text, |text, (value, marker)| text.replace(value, marker))
}

/// The first of `keys` present and not null, as JavaScript's `a ?? b` reads them.
fn first_present(raw: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Option<serde_json::Value> {
    keys.iter().find_map(|k| raw.get(*k).filter(|v| !v.is_null()).cloned())
}

/// The value at `path` inside nested objects.
fn nested_value<'a>(
    raw: &'a serde_json::Map<String, serde_json::Value>,
    path: &[&str],
) -> Option<&'a serde_json::Value> {
    let (first, rest) = path.split_first()?;
    let mut current = raw.get(*first)?;
    for segment in rest {
        current = current.as_object()?.get(*segment)?;
    }
    Some(current)
}

/// `readNestedString`: a string at `path`.
fn nested_string(raw: &serde_json::Map<String, serde_json::Value>, path: &[&str]) -> Option<serde_json::Value> {
    nested_value(raw, path)
        .and_then(serde_json::Value::as_str)
        .map(serde_json::Value::from)
}

/// `readNestedNumber`: a number at `path`, or a string JavaScript's `Number()` reads as a finite
/// number (an empty string reads as 0, as there).
fn nested_number(raw: &serde_json::Map<String, serde_json::Value>, path: &[&str]) -> Option<serde_json::Value> {
    match nested_value(raw, path)? {
        serde_json::Value::Number(n) => Some(serde_json::Value::Number(n.clone())),
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            let parsed = if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse::<f64>().ok().filter(|n| n.is_finite())?
            };
            // An integral value stays an integer, so it reads into the claim's integer fields.
            if parsed.fract() == 0.0 && parsed.abs() < 9.0e15 {
                Some(serde_json::Value::from(parsed as i64))
            } else {
                serde_json::Number::from_f64(parsed).map(serde_json::Value::Number)
            }
        }
        _ => None,
    }
}

/// The claim a policy-refresh answer gives, in the canonical shape of
/// `ProductActivationClaimRecord`: the fields and fallbacks of `normalizeActivationPayload`
/// followed by `toDesktopActivationClaimRecord` (product-license-service.ts), which the webview
/// applied to this answer before the refresh moved here. An omitted or empty `activation_token` is
/// the stored one (the server may leave it out of a refresh).
fn policy_refresh_claim(
    raw: &serde_json::Map<String, serde_json::Value>,
    stored_token: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let get = |key: &str| first_present(raw, &[key]);
    let edition = first_present(raw, &["edition", "license_tier", "license_plan", "tier", "plan"])
        .or_else(|| nested_string(raw, &["claim", "tier"]))
        .or_else(|| nested_string(raw, &["entitlement", "tier"]));
    let activation_token = raw
        .get("activation_token")
        .and_then(serde_json::Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .map_or_else(|| stored_token.to_string(), str::to_string);
    let fields = [
        ("tenant_id", get("tenant_id")),
        ("license_id", get("license_id")),
        ("machine_fingerprint", get("machine_fingerprint")),
        ("activation_token", Some(serde_json::Value::from(activation_token))),
        ("expires_at", get("expires_at")),
        ("force_min_app_version", get("force_min_app_version")),
        ("force_update_required", get("force_update_required")),
        ("update_channel", get("update_channel")),
        ("offline_grace_hours", get("offline_grace_hours")),
        (
            "trust_revocation_disconnects_immediately",
            get("trust_revocation_disconnects_immediately"),
        ),
        (
            "reconnect_requires_fresh_heartbeat",
            get("reconnect_requires_fresh_heartbeat"),
        ),
        (
            "tenant_display_name",
            first_present(
                raw,
                &[
                    "tenant_display_name",
                    "company_display_name",
                    "company_name",
                    "tenant_name",
                ],
            )
            .or_else(|| nested_string(raw, &["tenant", "display_name"]))
            .or_else(|| nested_string(raw, &["tenant", "name"]))
            .or_else(|| nested_string(raw, &["company", "display_name"])),
        ),
        (
            "tenant_slug",
            first_present(raw, &["tenant_slug", "slug"]).or_else(|| nested_string(raw, &["tenant", "slug"])),
        ),
        (
            "device_limit",
            first_present(
                raw,
                &["device_limit", "slot_limit", "seat_limit", "deviceLimit", "slotLimit"],
            )
            .or_else(|| nested_number(raw, &["claim", "slot_limit"]))
            .or_else(|| nested_number(raw, &["entitlement", "slot_limit"]))
            .or_else(|| nested_number(raw, &["license", "slot_limit"])),
        ),
        ("license_tier", edition.clone()),
        ("license_plan", edition),
        // As `normalizeActivationPayload`: a 2xx answer that states no licence status is active.
        (
            "license_status",
            Some(get("license_status").unwrap_or_else(|| serde_json::Value::from("active"))),
        ),
        (
            "activated_device_count",
            first_present(raw, &["activated_device_count", "active_device_count"])
                .or_else(|| nested_number(raw, &["limits", "activated"])),
        ),
        (
            "has_demo_data",
            first_present(raw, &["has_demo_data", "demo_data", "is_demo"]),
        ),
        (
            "is_initialized",
            get("is_initialized").or_else(|| {
                nested_value(raw, &["tenant", "is_initialized"])
                    .filter(|v| !v.is_null())
                    .cloned()
            }),
        ),
    ];
    fields
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.unwrap_or(serde_json::Value::Null)))
        .collect()
}

/// The claim and entitlement envelope of a 2xx claim or policy-refresh answer (`answer` names it in
/// the error), or why the answer is not a licence claim.
fn claim_of_answer(
    body: &str,
    stored_token: &str,
    answer: &str,
) -> Result<
    (
        ProductActivationClaimRecord,
        Option<crate::entitlements::domain::EntitlementEnvelopeInput>,
    ),
    String,
> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("The {answer} answer is not JSON: {e}"))?;
    let raw = value
        .as_object()
        .ok_or_else(|| format!("The {answer} answer is not a JSON object."))?;
    let envelope = match raw.get("entitlement_envelope").filter(|v| !v.is_null()) {
        Some(v) => Some(
            serde_json::from_value::<crate::entitlements::domain::EntitlementEnvelopeInput>(v.clone())
                .map_err(|e| format!("The {answer} answer has an invalid entitlement envelope: {e}"))?,
        ),
        None => None,
    };
    let claim: ProductActivationClaimRecord =
        serde_json::from_value(serde_json::Value::Object(policy_refresh_claim(raw, stored_token)))
            .map_err(|e| format!("The {answer} answer is not a licence claim: {e}"))?;
    Ok((claim, envelope))
}

/// The successful reconciliation outcome (`apply_product_license_reconciliation_in`) recording
/// `claim`.
fn success_outcome_json(
    claim: &ProductActivationClaimRecord,
    envelope: Option<&crate::entitlements::domain::EntitlementEnvelopeInput>,
    app_version: Option<&str>,
) -> String {
    serde_json::json!({
        "kind": "success",
        "claim": claim,
        "entitlement_envelope": envelope,
        "app_version": app_version,
    })
    .to_string()
}

/// The reconciliation outcome of a 2xx policy-refresh answer, or why the answer is not a licence
/// claim.
fn policy_refresh_outcome_json(body: &str, stored_token: &str, app_version: Option<&str>) -> Result<String, String> {
    let (claim, envelope) = claim_of_answer(body, stored_token, "policy refresh")?;
    Ok(success_outcome_json(&claim, envelope.as_ref(), app_version))
}

/// The code and message of an activation call's error answer, as the webview read them before the
/// calls moved to Rust: `code`, else `error` when a string, else `error.code`; `message`, else
/// `detail`, `error_description`, `error.message`; a body that is not JSON is the message. What a status means when the body says
/// nothing (a 409 is a slot limit) is decided by the webview, as for every activation error.
fn policy_refresh_error_fields(body: &str) -> (Option<String>, Option<String>) {
    let text = |v: Option<&serde_json::Value>| v.and_then(serde_json::Value::as_str).map(str::to_string);
    let mut code = None;
    let mut message = None;
    let trimmed = body.trim();
    if !trimmed.is_empty() {
        match serde_json::from_str::<serde_json::Value>(body) {
            Ok(serde_json::Value::Object(obj)) => {
                let nested = obj.get("error").and_then(serde_json::Value::as_object);
                code = text(obj.get("code"))
                    .or_else(|| text(obj.get("error")))
                    .or_else(|| nested.and_then(|n| text(n.get("code"))));
                message = text(obj.get("message"))
                    .or_else(|| text(obj.get("detail")))
                    .or_else(|| text(obj.get("error_description")))
                    .or_else(|| nested.and_then(|n| text(n.get("message"))));
            }
            // JSON `null` fails there too (reading a field of null), so the body is the message.
            Ok(serde_json::Value::Null) | Err(_) => message = Some(trimmed.to_string()),
            Ok(_) => {}
        }
    }
    (code, message)
}

/// Why an activation call to the control API gave no answer to read. Its texts never carry a
/// secret sent with the call (`without_secrets`).
#[derive(Debug)]
enum ActivationCallFailure {
    /// The server answered an error status. `code` and `message` are read from its body
    /// (`policy_refresh_error_fields`); `details` is the body.
    HttpError {
        status: u16,
        code: Option<String>,
        message: Option<String>,
        details: Option<String>,
    },
    /// An answer too large to read (`RAW_ANSWER_MAX_BYTES`), a request that could not be built, or
    /// (set by the caller) a 2xx answer that is not what the call expects.
    InvalidResponse { message: String },
    /// No answer from the server (offline, connection refused, interrupted).
    NetworkError { message: String },
}

impl From<ActivationCallFailure> for PolicyRefreshResult {
    fn from(failure: ActivationCallFailure) -> Self {
        match failure {
            ActivationCallFailure::HttpError {
                status,
                code,
                message,
                details,
            } => Self::HttpError {
                status,
                code,
                message,
                details,
            },
            ActivationCallFailure::InvalidResponse { message } => Self::InvalidResponse { message },
            ActivationCallFailure::NetworkError { message } => Self::NetworkError { message },
        }
    }
}

/// `POST {path}` on the control API at `api_base`, with `bearer` when given: the body of a 2xx
/// answer, or why there is none. Every text of a failure has the `secrets` sent replaced by their
/// marker; the 2xx body is returned as received (the caller reads it and removes what must not
/// leave Rust).
async fn post_activation_call(
    api_base: &str,
    path: &str,
    bearer: Option<&str>,
    body: &serde_json::Value,
    secrets: &[(&str, &str)],
) -> AppResult<Result<String, ActivationCallFailure>> {
    let client = crate::sync::transport::client::ControlPlaneClient::new(
        api_base,
        crate::sync::transport::client::ClientTimeouts::interactive(),
    )
    .map_err(AppError::SyncError)?;
    let (status, answer) = match client.post_json_raw(path, bearer, body.to_string()).await {
        Ok(answer) => answer,
        // An answer too large to read (`RAW_ANSWER_MAX_BYTES`), or a request that could not be
        // built: no licence answer, but not the network either.
        Err(e @ crate::sync::transport::client::TransportError::Contract(_)) => {
            let message = without_secrets(e.message(), secrets);
            tracing::warn!(
                event = "desktop_activation_call_invalid_answer",
                path,
                error = %message,
                "Activation call answer could not be read"
            );
            return Ok(Err(ActivationCallFailure::InvalidResponse { message }));
        }
        Err(e) => {
            let message = without_secrets(e.message(), secrets);
            tracing::warn!(
                event = "desktop_activation_call_unreachable",
                path,
                error = %message,
                "Activation call got no answer"
            );
            return Ok(Err(ActivationCallFailure::NetworkError { message }));
        }
    };
    if (200..300).contains(&status) {
        return Ok(Ok(answer));
    }
    let (code, message) = policy_refresh_error_fields(&answer);
    let code = code.map(|c| without_secrets(c, secrets));
    let message = message.map(|m| without_secrets(m, secrets));
    // Redacted before it is shortened: a secret cut at the limit would no longer be found.
    let details = Some(answer.trim()).filter(|b| !b.is_empty()).map(|b| {
        without_secrets(b.to_string(), secrets)
            .chars()
            .take(POLICY_REFRESH_DETAILS_MAX_CHARS)
            .collect::<String>()
    });
    tracing::warn!(
        event = "desktop_activation_call_refused",
        path,
        status,
        code = code.as_deref().unwrap_or(""),
        "Activation call answered an error"
    );
    Ok(Err(ActivationCallFailure::HttpError {
        status,
        code,
        message,
        details,
    }))
}

/// A 2xx answer the caller could not read: logged, returned as `InvalidResponse`.
fn invalid_answer(path: &str, why: String, secrets: &[(&str, &str)]) -> ActivationCallFailure {
    let message = without_secrets(why, secrets);
    tracing::warn!(
        event = "desktop_activation_call_invalid_answer",
        path,
        error = %message,
        "Activation call answer is not what the call expects"
    );
    ActivationCallFailure::InvalidResponse { message }
}

/// Licence policy refresh from Rust: sends the stored activation token to the control API at
/// `api_base`, then records a 2xx answer as a successful reconciliation for `caller` (refusals of
/// that change, such as `LICENSE_RECONCILE_ADMIN_REQUIRED` or a step-up, are returned as errors).
/// `app_version` is sent only when well formed (`well_formed_app_version`); without it the server
/// applies a forced update as if this PC were out of date.
pub(crate) async fn refresh_product_activation_policy_in(
    db: &DatabaseConnection,
    caller: LicenceCaller,
    api_base: &str,
    app_version: Option<&str>,
) -> AppResult<PolicyRefreshResult> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(PolicyRefreshResult::NoToken);
    };
    if !record_has_valid_activation_claim(&record) {
        return Ok(PolicyRefreshResult::NoToken);
    }
    let Some(token) = record.activation_claim.map(|c| c.activation_token.trim().to_string()) else {
        return Ok(PolicyRefreshResult::NoToken);
    };
    let version = app_version.map(str::trim).filter(|v| !v.is_empty());
    let mut request_body = match version.filter(|v| well_formed_app_version(v)) {
        Some(v) => serde_json::json!({ "app_version": v }),
        None => serde_json::json!({}),
    };
    // The phone names its class on every licence call (server: absent = desktop).
    if let Some(class) = crate::sync::transport::wire::device_class_if_not_desktop() {
        request_body["device_class"] = serde_json::Value::from(class);
    }
    let secrets = [(token.as_str(), TOKEN_MARKER)];
    let body = match post_activation_call(api_base, POLICY_REFRESH_PATH, Some(&token), &request_body, &secrets).await? {
        Ok(body) => body,
        Err(failure) => return Ok(failure.into()),
    };
    let outcome_json = match policy_refresh_outcome_json(&body, &token, version) {
        Ok(json) => json,
        Err(why) => return Ok(invalid_answer(POLICY_REFRESH_PATH, why, &secrets).into()),
    };
    let state = apply_product_license_reconciliation_in(db, caller, &outcome_json).await?;
    tracing::info!(
        event = "desktop_policy_refresh_applied",
        status = ?state.status,
        "Licence policy refresh recorded"
    );
    Ok(PolicyRefreshResult::Refreshed { state })
}

/// Renews this PC's licence with the control API (policy refresh, no device slot taken) and records
/// the answer. Runs before sign-in (the licence gate); the activation token never leaves Rust. The
/// control API origin is the one sync and updates use (`build.rs`).
#[tauri::command]
pub async fn refresh_product_activation_policy(
    app_version: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<PolicyRefreshResult> {
    let caller = caller_of(&state).await?;
    refresh_product_activation_policy_in(
        &state.db,
        caller,
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        app_version.as_deref(),
    )
    .await
}

/// Licence check without taking a device slot: the organisation and terms of a key.
const ACTIVATION_PREVIEW_PATH: &str = "/api/v1/activation/preview";
/// Takes a device slot for a key and issues this PC's activation token.
const ACTIVATION_CLAIM_PATH: &str = "/api/v1/activation/claim";
/// Marks the activated organisation as initialised (its first PC set it up).
const TENANT_INITIALIZE_PATH: &str = "/api/v1/activation/tenant-status/initialize";

/// What one activation call sent from Rust gave the webview. Never carries the activation token
/// or the stored licence key: the request is sent from here and they stay in Rust.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivationCallResult<T> {
    /// The server answered; `answer` is what the call returns of it.
    Answered { answer: T },
    /// Nothing to send from this PC (no activation token, no licence key stored, or, for the tenant
    /// initialisation, an organisation the stored claim already says is initialised): nothing was
    /// sent.
    NotActivated,
    /// As `PolicyRefreshResult::HttpError`.
    HttpError {
        status: u16,
        code: Option<String>,
        message: Option<String>,
        details: Option<String>,
    },
    /// As `PolicyRefreshResult::InvalidResponse`.
    InvalidResponse { message: String },
    /// As `PolicyRefreshResult::NetworkError`.
    NetworkError { message: String },
}

impl<T> From<ActivationCallFailure> for ActivationCallResult<T> {
    fn from(failure: ActivationCallFailure) -> Self {
        match failure {
            ActivationCallFailure::HttpError {
                status,
                code,
                message,
                details,
            } => Self::HttpError {
                status,
                code,
                message,
                details,
            },
            ActivationCallFailure::InvalidResponse { message } => Self::InvalidResponse { message },
            ActivationCallFailure::NetworkError { message } => Self::NetworkError { message },
        }
    }
}

/// A claim taken on the control API (`claim_product_activation`), held in Rust with its
/// activation token until the key entry (`submit_product_license_key`) records it. One at a time:
/// a new claim attempt replaces it, a recorded key entry clears it. In memory only.
#[derive(Clone)]
pub struct PendingActivationClaim {
    /// SHA-256 of the licence key the claim was taken with (`settings::sha256_hex`).
    key_sha256: String,
    /// The machine fingerprint the claim was taken for.
    machine_fingerprint: String,
    claim: ProductActivationClaimRecord,
    envelope: Option<crate::entitlements::domain::EntitlementEnvelopeInput>,
}

impl std::fmt::Debug for PendingActivationClaim {
    // Never the activation token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingActivationClaim")
            .field("tenant_id", &self.claim.tenant_id)
            .field("license_id", &self.claim.license_id)
            .field("machine_fingerprint", &self.machine_fingerprint)
            .finish_non_exhaustive()
    }
}

/// Where the pending claim is held (`AppState::pending_activation_claim`).
pub type PendingActivationClaimSlot = std::sync::Arc<tokio::sync::Mutex<Option<PendingActivationClaim>>>;

/// The key entry has no claim waiting in Rust (none taken, or already recorded).
fn claim_not_pending() -> AppError {
    AppError::ValidationFailed(vec![
        "GATE_LICENSE_CLAIM_NOT_PENDING: No licence claim is waiting on this PC: validate the key and activate again."
            .into(),
    ])
}

/// The key entry names another key or organisation than the claim waiting in Rust.
fn claim_mismatch() -> AppError {
    AppError::ValidationFailed(vec![
        "GATE_LICENSE_CLAIM_MISMATCH: The licence claim waiting on this PC is for another key or organisation: \
         validate the key and activate again."
            .into(),
    ])
}

/// The body of a preview or claim, as the webview sent it: the label only when given, the app
/// version only when well formed (`well_formed_app_version`).
fn activation_request_body(
    license_key: &str,
    machine_fingerprint: &str,
    machine_label: Option<&str>,
    app_version: Option<&str>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "license_key": license_key,
        "machine_fingerprint": machine_fingerprint,
    });
    if let Some(label) = machine_label.map(str::trim).filter(|l| !l.is_empty()) {
        body["machine_label"] = serde_json::Value::from(label);
    }
    if let Some(version) = app_version.map(str::trim).filter(|v| well_formed_app_version(v)) {
        body["app_version"] = serde_json::Value::from(version);
    }
    // The Android field app claims a mobile slot (its own count on the licence). Desktop requests
    // stay exactly as before: no field, which the server reads as `desktop`.
    if let Some(class) = crate::sync::transport::wire::device_class_if_not_desktop() {
        body["device_class"] = serde_json::Value::from(class);
    }
    body
}

/// The licence key and machine fingerprint the webview gave for a preview or claim, trimmed:
/// refused as the key entry refuses them (`GATE_LICENSE_KEY_SHORT`, `LICENSE_CLAIM_INVALID`).
fn key_and_fingerprint<'a>(license_key: &'a str, machine_fingerprint: &'a str) -> AppResult<(&'a str, &'a str)> {
    let key = license_key.trim();
    if key.len() < 8 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_LICENSE_KEY_SHORT: License key must be at least 8 characters.".into(),
        ]));
    }
    let fingerprint = machine_fingerprint.trim();
    if fingerprint.is_empty() {
        return Err(claim_invalid());
    }
    Ok((key, fingerprint))
}

/// An activation answer as the webview may read it: every `activation_token` field removed, at
/// any depth, and `token` replaced wherever else it appears.
fn without_activation_token(value: serde_json::Value, token: &str) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(k, _)| k != "activation_token")
                .map(|(k, v)| (k, without_activation_token(v, token)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(|v| without_activation_token(v, token)).collect())
        }
        serde_json::Value::String(s) => serde_json::Value::String(without_secrets(s, &[(token, TOKEN_MARKER)])),
        other => other,
    }
}

/// The `activation_token` text of a 2xx claim answer, to redact it from what leaves Rust even when
/// the answer is not a complete claim.
fn answered_token(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("activation_token")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// A 2xx claim answer read as a complete activation claim (organisation, licence, machine and
/// activation token, each as it may be stored: `canonical_id`, as the key entry requires), or why
/// it is not one. Checked before the webview gets the claim, so local data is never wiped for a
/// claim the key entry would then refuse as malformed. The machine must be the one the claim was
/// sent for (`sent_fingerprint`): a slot taken for another machine is not this PC's activation.
fn complete_claim_of_answer(
    body: &str,
    sent_fingerprint: &str,
) -> Result<
    (
        ProductActivationClaimRecord,
        Option<crate::entitlements::domain::EntitlementEnvelopeInput>,
    ),
    String,
> {
    let (claim, envelope) = claim_of_answer(body, "", "activation claim")?;
    let complete = [
        &claim.tenant_id,
        &claim.license_id,
        &claim.machine_fingerprint,
        &claim.activation_token,
    ]
    .into_iter()
    .all(|value| canonical_id(value));
    if !complete {
        return Err(
            "The activation claim answer misses the organisation, licence, machine or activation token.".to_string(),
        );
    }
    if claim.machine_fingerprint != sent_fingerprint {
        return Err(
            "The activation claim answer names another machine than the one the claim was sent for.".to_string(),
        );
    }
    Ok((claim, envelope))
}

/// Licence check from Rust (no device slot taken): posts the key to the control API at `api_base`
/// and returns its answer as received (the webview reads it), any `activation_token` removed
/// (a preview issues none; one sent anyway never reaches the webview).
pub(crate) async fn preview_product_activation_in(
    api_base: &str,
    license_key: &str,
    machine_fingerprint: &str,
    machine_label: Option<&str>,
    app_version: Option<&str>,
) -> AppResult<ActivationCallResult<serde_json::Value>> {
    let (key, fingerprint) = key_and_fingerprint(license_key, machine_fingerprint)?;
    let body = activation_request_body(key, fingerprint, machine_label, app_version);
    let secrets = [(key, KEY_MARKER)];
    let answer = match post_activation_call(api_base, ACTIVATION_PREVIEW_PATH, None, &body, &secrets).await? {
        Ok(answer) => answer,
        Err(failure) => return Ok(failure.into()),
    };
    match serde_json::from_str::<serde_json::Value>(&answer) {
        Ok(value @ serde_json::Value::Object(_)) => Ok(ActivationCallResult::Answered {
            answer: without_activation_token(value, &answered_token(&answer)),
        }),
        Ok(_) => Ok(invalid_answer(
            ACTIVATION_PREVIEW_PATH,
            "The licence preview answer is not a JSON object.".to_string(),
            &secrets,
        )
        .into()),
        Err(e) => Ok(invalid_answer(
            ACTIVATION_PREVIEW_PATH,
            format!("The licence preview answer is not JSON: {e}"),
            &secrets,
        )
        .into()),
    }
}

/// Validates a licence key with the control API without taking a device slot (the licence gate's
/// first step). The answer has the organisation and licence terms; the request is sent from Rust.
#[tauri::command]
pub async fn preview_product_activation(
    license_key: String,
    machine_fingerprint: String,
    machine_label: Option<String>,
    app_version: Option<String>,
) -> AppResult<ActivationCallResult<serde_json::Value>> {
    preview_product_activation_in(
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        &license_key,
        &machine_fingerprint,
        machine_label.as_deref(),
        app_version.as_deref(),
    )
    .await
}

/// Activation claim from Rust: takes a device slot for the key on the control API at `api_base`.
/// A complete claim is held in `pending` (with its activation token) for the key entry; the
/// webview gets the answer without the token. Any earlier pending claim is dropped first, so a
/// failed attempt leaves none.
pub(crate) async fn claim_product_activation_in(
    pending: &PendingActivationClaimSlot,
    api_base: &str,
    license_key: &str,
    machine_fingerprint: &str,
    machine_label: Option<&str>,
    app_version: Option<&str>,
) -> AppResult<ActivationCallResult<serde_json::Value>> {
    let (key, fingerprint) = key_and_fingerprint(license_key, machine_fingerprint)?;
    let mut slot = pending.lock().await;
    *slot = None;
    let body = activation_request_body(key, fingerprint, machine_label, app_version);
    let answer = match post_activation_call(api_base, ACTIVATION_CLAIM_PATH, None, &body, &[(key, KEY_MARKER)]).await? {
        Ok(answer) => answer,
        Err(failure) => return Ok(failure.into()),
    };
    let token = answered_token(&answer);
    let secrets = [(key, KEY_MARKER), (token.as_str(), TOKEN_MARKER)];
    let (claim, envelope) = match complete_claim_of_answer(&answer, fingerprint) {
        Ok(parts) => parts,
        Err(why) => return Ok(invalid_answer(ACTIVATION_CLAIM_PATH, why, &secrets).into()),
    };
    // `complete_claim_of_answer` read it as a JSON object.
    let view = without_activation_token(serde_json::from_str(&answer)?, &claim.activation_token);
    tracing::info!(
        event = "desktop_activation_claim_pending",
        tenant_id = claim.tenant_id.as_str(),
        license_id = claim.license_id.as_str(),
        has_entitlement_envelope = envelope.is_some(),
        "Activation claim taken; held until the key entry records it"
    );
    *slot = Some(PendingActivationClaim {
        key_sha256: settings::sha256_hex(key),
        machine_fingerprint: fingerprint.to_string(),
        claim,
        envelope,
    });
    Ok(ActivationCallResult::Answered { answer: view })
}

/// Activates this PC with a licence key (takes a device slot on the control API). The claim, with
/// its activation token, stays in Rust until `submit_product_license_key` records it; the webview
/// gets the organisation and licence terms of the answer, never the token.
#[tauri::command]
pub async fn claim_product_activation(
    license_key: String,
    machine_fingerprint: String,
    machine_label: Option<String>,
    app_version: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<ActivationCallResult<serde_json::Value>> {
    claim_product_activation_in(
        &state.pending_activation_claim,
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        &license_key,
        &machine_fingerprint,
        machine_label.as_deref(),
        app_version.as_deref(),
    )
    .await
}

/// The key entry of the claim waiting in `pending`: refused unless one is waiting
/// (`GATE_LICENSE_CLAIM_NOT_PENDING`) for this key and organisation (`GATE_LICENSE_CLAIM_MISMATCH`);
/// then recorded with every check of [`submit_product_license_key_in`] (the machine fingerprint
/// stored is the one the claim was taken for). A recorded entry clears the pending claim; a refused
/// one keeps it.
pub(crate) async fn submit_pending_product_license_key_in(
    db: &DatabaseConnection,
    caller: LicenceCaller,
    pending: &PendingActivationClaimSlot,
    key: &str,
    tenant_id: &str,
    app_version: Option<String>,
) -> AppResult<()> {
    let mut slot = pending.lock().await;
    let Some(waiting) = slot.as_ref() else {
        return Err(claim_not_pending());
    };
    if settings::sha256_hex(key.trim()) != waiting.key_sha256 || waiting.claim.tenant_id != tenant_id.trim() {
        return Err(claim_mismatch());
    }
    let mut claim_json = serde_json::to_value(&waiting.claim)?;
    if let (Some(obj), Some(envelope)) = (claim_json.as_object_mut(), waiting.envelope.as_ref()) {
        obj.insert("entitlement_envelope".into(), serde_json::to_value(envelope)?);
    }
    submit_product_license_key_in(
        db,
        caller,
        key,
        Some(claim_json.to_string()),
        Some(waiting.machine_fingerprint.clone()),
        app_version,
    )
    .await?;
    *slot = None;
    Ok(())
}

/// The activation token the tenant initialisation uses: the stored activation's, when it is a valid
/// claim. Never a pending claim's: one not recorded yet may be for another organisation than the
/// one stored here (a claim taken, then its key entry abandoned). The gate initialises only after a
/// recorded key entry, which stored the claim and cleared the pending one. Sent even when the
/// stored claim already says initialised: the server answers it idempotently, and a retry whose
/// first answer was lost (a refresh having since stored `is_initialized: true`) must still finish.
async fn tenant_call_token(db: &DatabaseConnection) -> AppResult<Option<String>> {
    let token = load_state_record_from_db(db)
        .await?
        .filter(record_has_valid_activation_claim)
        .and_then(|record| record.activation_claim)
        .map(|claim| claim.activation_token);
    Ok(token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()))
}

/// The activated organisation's state on the control API.
#[derive(Debug, Serialize, Deserialize)]
pub struct ActivationTenantStatus {
    pub tenant_id: String,
    pub is_initialized: bool,
}

/// Marks the activated organisation initialised on the control API at `api_base`, with the token
/// of [`tenant_call_token`]. `NotActivated` (nothing sent) only when this PC holds no valid stored
/// activation; a claim already marked initialised is sent again (the call is idempotent).
pub(crate) async fn mark_activation_tenant_initialized_in(
    db: &DatabaseConnection,
    api_base: &str,
) -> AppResult<ActivationCallResult<ActivationTenantStatus>> {
    let Some(token) = tenant_call_token(db).await? else {
        return Ok(ActivationCallResult::NotActivated);
    };
    let secrets = [(token.as_str(), TOKEN_MARKER)];
    let answer = match post_activation_call(
        api_base,
        TENANT_INITIALIZE_PATH,
        Some(&token),
        &serde_json::json!({}),
        &secrets,
    )
    .await?
    {
        Ok(answer) => answer,
        Err(failure) => return Ok(failure.into()),
    };
    match serde_json::from_str::<ActivationTenantStatus>(&answer) {
        Ok(status) => Ok(ActivationCallResult::Answered { answer: status }),
        Err(e) => Ok(invalid_answer(
            TENANT_INITIALIZE_PATH,
            format!("The tenant initialisation answer is not a tenant status: {e}"),
            &secrets,
        )
        .into()),
    }
}

/// Marks the activated organisation as initialised on the control API (after the first activation
/// of an organisation not yet initialised). Sent from Rust with the stored activation token.
#[tauri::command]
pub async fn mark_activation_tenant_initialized(
    state: State<'_, AppState>,
) -> AppResult<ActivationCallResult<ActivationTenantStatus>> {
    mark_activation_tenant_initialized_in(&state.db, env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL")).await
}

/// Claims this PC's activation again with the licence key stored at the key entry, when its
/// activation token is unusable or absent (the webview asks after a policy refresh gave one of
/// those answers), and records the new claim as a successful reconciliation for `caller` (its
/// refusals are returned as errors, as for the refresh). The machine fingerprint is the stored
/// one. `NotActivated` when no key or fingerprint is stored (nothing sent).
pub(crate) async fn reclaim_product_activation_in(
    db: &DatabaseConnection,
    caller: LicenceCaller,
    api_base: &str,
    machine_label: Option<&str>,
    app_version: Option<&str>,
) -> AppResult<ActivationCallResult<ProductLicenseOnboardingState>> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(ActivationCallResult::NotActivated);
    };
    let Some(key) = record
        .license_key_plaintext
        .as_deref()
        .map(str::trim)
        .filter(|k| k.len() >= 8)
    else {
        return Ok(ActivationCallResult::NotActivated);
    };
    let Some(fingerprint) = record
        .machine_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .or_else(|| {
            record
                .activation_claim
                .as_ref()
                .map(|c| c.machine_fingerprint.trim())
                .filter(|f| !f.is_empty())
        })
    else {
        return Ok(ActivationCallResult::NotActivated);
    };
    let body = activation_request_body(key, fingerprint, machine_label, app_version);
    let answer = match post_activation_call(api_base, ACTIVATION_CLAIM_PATH, None, &body, &[(key, KEY_MARKER)]).await? {
        Ok(answer) => answer,
        Err(failure) => return Ok(failure.into()),
    };
    let token = answered_token(&answer);
    let secrets = [(key, KEY_MARKER), (token.as_str(), TOKEN_MARKER)];
    let (claim, envelope) = match complete_claim_of_answer(&answer, fingerprint) {
        Ok(parts) => parts,
        Err(why) => return Ok(invalid_answer(ACTIVATION_CLAIM_PATH, why, &secrets).into()),
    };
    let version = app_version.map(str::trim).filter(|v| !v.is_empty());
    let outcome_json = success_outcome_json(&claim, envelope.as_ref(), version);
    let state = apply_product_license_reconciliation_in(db, caller, &outcome_json).await?;
    tracing::info!(
        event = "desktop_activation_reclaim_applied",
        status = ?state.status,
        "Activation claimed again with the stored key and recorded"
    );
    Ok(ActivationCallResult::Answered { answer: state })
}

/// Claims this PC's activation again with the licence key stored here (its activation token is
/// unusable or absent) and records it. Sent from Rust: neither the key nor the token reaches the
/// webview.
#[tauri::command]
pub async fn reclaim_product_activation(
    machine_label: Option<String>,
    app_version: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<ActivationCallResult<ProductLicenseOnboardingState>> {
    let caller = caller_of(&state).await?;
    reclaim_product_activation_in(
        &state.db,
        caller,
        env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"),
        machine_label.as_deref(),
        app_version.as_deref(),
    )
    .await
}

#[cfg(test)]
mod claim_json_tests {
    use super::{extract_claim_and_envelope, sanitize_activation_claim_object};

    #[test]
    fn raw_alias_co_presence_fails_without_sanitize() {
        // Documents the production failure mode before sanitize_activation_claim_object.
        let raw = r#"{
            "tenant_id": "t-1",
            "license_id": "lic-1",
            "machine_fingerprint": "fp-1",
            "activation_token": "tok-1",
            "license_tier": "professional",
            "edition": "core"
        }"#;
        let value: serde_json::Value = serde_json::from_str(raw).unwrap();
        let err = serde_json::from_value::<super::ProductActivationClaimRecord>(value)
            .expect_err("serde must reject edition+license_tier co-presence");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("duplicate") || msg.contains("edition") || msg.contains("license_tier"),
            "unexpected serde error: {err}"
        );
    }

    #[test]
    fn accepts_claim_with_alias_and_canonical_co_present() {
        // Mirrors FE normalizeActivationPayload which sets license_tier + edition (+ company aliases).
        let raw = r#"{
            "tenant_id": "t-1",
            "license_id": "lic-1",
            "machine_fingerprint": "fp-1",
            "activation_token": "tok-1",
            "license_tier": "professional",
            "license_plan": "professional",
            "edition": "core",
            "company_display_name": "Acme",
            "tenant_display_name": "Acme Corp",
            "slot_limit": 3,
            "device_limit": 5,
            "force_update_mode": "off",
            "capabilities_digest": "abc",
            "entitlement_envelope": null
        }"#;
        let (claim, envelope) = extract_claim_and_envelope(raw).expect("claim should parse");
        assert!(envelope.is_none());
        assert_eq!(claim.tenant_id, "t-1");
        assert_eq!(claim.activation_token, "tok-1");
        // edition wins for commercial edition fields
        assert_eq!(claim.license_tier.as_deref(), Some("core"));
        assert_eq!(claim.license_plan.as_deref(), Some("core"));
        assert_eq!(claim.tenant_display_name.as_deref(), Some("Acme Corp"));
        assert_eq!(claim.device_limit, Some(5));
    }

    #[test]
    fn rejects_invalid_json_with_detail() {
        let err = extract_claim_and_envelope("{not-json").expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains("claimJson must be valid JSON"), "{msg}");
    }

    #[test]
    fn sanitize_promotes_aliases_when_canonical_missing() {
        let mut value = serde_json::json!({
            "edition": "enterprise",
            "company_name": "Beta SA",
            "seat_limit": 10
        });
        let obj = value.as_object_mut().unwrap();
        sanitize_activation_claim_object(obj);
        assert_eq!(obj.get("license_tier").and_then(|v| v.as_str()), Some("enterprise"));
        assert_eq!(obj.get("license_plan").and_then(|v| v.as_str()), Some("enterprise"));
        assert_eq!(obj.get("tenant_display_name").and_then(|v| v.as_str()), Some("Beta SA"));
        assert_eq!(obj.get("device_limit").and_then(|v| v.as_i64()), Some(10));
        assert!(!obj.contains_key("edition"));
        assert!(!obj.contains_key("company_name"));
        assert!(!obj.contains_key("seat_limit"));
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn record(
        status: ProductLicenseActivationStatus,
        last_success: &str,
        grace_hours: Option<u32>,
    ) -> ProductLicenseStateRecord {
        serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "key_sha256": "abc",
            "license_key_plaintext": null,
            "machine_fingerprint": "fp-1",
            "app_version": "1.0.0",
            "submitted_at": "2026-10-01T00:00:00Z",
            "status": status,
            "pending_online_validation": false,
            "deny_reason_code": null,
            "deny_message": null,
            "degraded_reason": null,
            "activation_claim": {
                "tenant_id": "t-1",
                "license_id": "lic-1",
                "machine_fingerprint": "fp-1",
                "activation_token": "tok-1",
                "offline_grace_hours": grace_hours,
            },
            "last_reconciled_at": last_success,
            "reconciliation": { "retry_attempt": 0, "last_success_at": last_success },
            "diagnostics": [],
        }))
        .expect("record fixture")
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn revocation_codes_map_to_denied_revoked() {
        for code in ["license_revoked", "activation_revoked", "tenant_inactive"] {
            assert_eq!(
                map_denied_status(Some(code)),
                ProductLicenseActivationStatus::DeniedRevoked,
                "{code}"
            );
        }
        assert_eq!(
            map_denied_status(Some("license_expired")),
            ProductLicenseActivationStatus::DeniedExpired
        );
        assert_eq!(
            map_denied_status(Some("something_else")),
            ProductLicenseActivationStatus::DeniedInvalid
        );
    }

    #[test]
    fn active_within_grace_stays_active() {
        let r = record(ProductLicenseActivationStatus::Active, "2026-10-10T00:00:00Z", Some(72));
        assert_eq!(
            effective_status(&r, at("2026-10-12T23:00:00Z")),
            ProductLicenseActivationStatus::Active
        );
    }

    #[test]
    fn active_or_degraded_past_grace_is_denied() {
        for status in [
            ProductLicenseActivationStatus::Active,
            ProductLicenseActivationStatus::DegradedApiUnavailable,
        ] {
            let r = record(status, "2026-10-10T00:00:00Z", Some(72));
            assert_eq!(
                effective_status(&r, at("2026-10-13T00:00:01Z")),
                ProductLicenseActivationStatus::DeniedOfflineGraceExpired
            );
        }
    }

    #[test]
    fn grace_not_enforced_without_server_value_and_other_denials_are_kept() {
        let r = record(ProductLicenseActivationStatus::Active, "2020-01-01T00:00:00Z", None);
        assert_eq!(
            effective_status(&r, at("2026-10-13T00:00:00Z")),
            ProductLicenseActivationStatus::Active
        );
        let r = record(
            ProductLicenseActivationStatus::DeniedRevoked,
            "2020-01-01T00:00:00Z",
            Some(1),
        );
        assert_eq!(
            effective_status(&r, at("2026-10-13T00:00:00Z")),
            ProductLicenseActivationStatus::DeniedRevoked
        );
    }

    #[test]
    fn onboarding_state_reports_grace_expiry() {
        let r = record(ProductLicenseActivationStatus::Active, "2020-01-01T00:00:00Z", Some(72));
        let view = onboarding_state_from_record(&r);
        assert_eq!(view.status, ProductLicenseActivationStatus::DeniedOfflineGraceExpired);
        assert_eq!(view.deny_reason_code.as_deref(), Some(OFFLINE_GRACE_EXPIRED_CODE));
        assert!(view.pending_online_validation, "must keep retrying when back online");
    }
}

/// Tests and the multi-device harness: record a complete activation claim for `tenant_id`, as a
/// successful activation does, so tenant scoping (media object keys, `tenant_config`) is real.
#[cfg(test)]
pub(crate) async fn record_activation_claim_for_tests(
    db: &DatabaseConnection,
    tenant_id: &str,
    machine_fingerprint: &str,
    activation_token: &str,
) -> AppResult<()> {
    let record = serde_json::json!({
        "schema_version": 1,
        "key_sha256": "test",
        "submitted_at": Utc::now().to_rfc3339(),
        "status": "active",
        "pending_online_validation": false,
        "activation_claim": {
            "tenant_id": tenant_id,
            "license_id": "test-licence",
            "machine_fingerprint": machine_fingerprint,
            "activation_token": activation_token,
        },
        "reconciliation": { "retry_attempt": 0 },
        "diagnostics": [],
    });
    settings::set_setting(
        db,
        PRODUCT_LICENSE_ONBOARDING_KEY,
        PRODUCT_LICENSE_SCOPE,
        &record.to_string(),
        None,
        "test activation",
    )
    .await
}

#[cfg(test)]
#[path = "product_license_first_start_tests.rs"]
mod first_start_tests;

#[cfg(test)]
#[path = "product_license_policy_refresh_tests.rs"]
mod policy_refresh_tests;

#[cfg(test)]
#[path = "product_license_activation_calls_tests.rs"]
mod activation_calls_tests;
