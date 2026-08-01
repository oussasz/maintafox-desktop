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

#[derive(Serialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductLicenseReconciliationPolicy {
    pub retry_attempt: u32,
    pub next_retry_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
}

impl Default for ProductLicenseReconciliationPolicy {
    fn default() -> Self {
        Self {
            retry_attempt: 0,
            next_retry_at: None,
            last_attempt_at: None,
            last_success_at: None,
            last_error_code: None,
            last_error_message: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    let mut value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| AppError::ValidationFailed(vec![format!("claimJson must be valid JSON: {e}")]))?;

    let envelope = match value
        .as_object_mut()
        .and_then(|obj| obj.remove("entitlement_envelope"))
        .filter(|v| !v.is_null())
    {
        Some(env_value) => Some(
            serde_json::from_value::<crate::entitlements::domain::EntitlementEnvelopeInput>(env_value)
                .map_err(|e| AppError::ValidationFailed(vec![format!("entitlement_envelope is invalid: {e}")]))?,
        ),
        None => None,
    };

    if let Some(obj) = value.as_object_mut() {
        sanitize_activation_claim_object(obj);
    }

    let mut claim: ProductActivationClaimRecord = serde_json::from_value(value).map_err(|e| {
        AppError::ValidationFailed(vec![format!(
            "claimJson must be valid ProductActivationClaimRecord JSON: {e}"
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
        // #region agent log
        {
            let line = format!(
                "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"A\",\"location\":\"product_license.rs:apply_claim_entitlement_envelope\",\"message\":\"claim had no entitlement_envelope\",\"data\":{{\"applied\":false}},\"timestamp\":{}}}\n",
                chrono::Utc::now().timestamp_millis()
            );
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(r"c:\Dev\MaintafoxSuite\desktop\maintafox-desktop\debug-a6aab3.log")
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(line.as_bytes())
                });
        }
        // #endregion
        return Ok(());
    };
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
            // #region agent log
            {
                let line = format!(
                    "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"K-trust\",\"location\":\"product_license.rs:apply_claim_entitlement_envelope\",\"message\":\"envelope apply error soft-continued\",\"data\":{{\"error\":\"{}\"}},\"timestamp\":{}}}\n",
                    e.to_string().replace('\\', "\\\\").replace('"', "'"),
                    chrono::Utc::now().timestamp_millis()
                );
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(r"c:\Dev\MaintafoxSuite\desktop\maintafox-desktop\debug-a6aab3.log")
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(line.as_bytes())
                    });
            }
            // #endregion
            // Soft phase: never roll activation back to degraded solely because envelope apply failed.
            tracing::warn!(
                event = "desktop_entitlement_envelope_apply_failed",
                error = %e,
                "Entitlement envelope apply failed; keeping activation success"
            );
            return Ok(());
        }
    };
    // #region agent log
    {
        let line = format!(
            "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"A-D\",\"location\":\"product_license.rs:apply_claim_entitlement_envelope\",\"message\":\"envelope apply result\",\"data\":{{\"applied\":true,\"verified\":{},\"envelopeId\":\"{}\",\"effectiveState\":\"{}\",\"verification\":\"{}\"}},\"timestamp\":{}}}\n",
            result.verified,
            result.envelope_id.replace('"', ""),
            result.effective_state.replace('"', ""),
            result.verification_result.replace('"', ""),
            chrono::Utc::now().timestamp_millis()
        );
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(r"c:\Dev\MaintafoxSuite\desktop\maintafox-desktop\debug-a6aab3.log")
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(line.as_bytes())
            });
    }
    // #endregion
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

fn compute_backoff_at(attempt: u32) -> DateTime<Utc> {
    // 30s, 60s, 120s... capped at 6h
    let exponent = attempt.min(12);
    let seconds = (30_i64 * (1_i64 << exponent)).min(6 * 60 * 60);
    Utc::now() + Duration::seconds(seconds)
}

fn map_denied_status(code: Option<&str>) -> ProductLicenseActivationStatus {
    match code.unwrap_or_default() {
        "license_revoked" => ProductLicenseActivationStatus::DeniedRevoked,
        "license_expired" => ProductLicenseActivationStatus::DeniedExpired,
        "slot_limit_reached" => ProductLicenseActivationStatus::DeniedSlotLimit,
        "force_update_required" => ProductLicenseActivationStatus::DeniedForceUpdateRequired,
        _ => ProductLicenseActivationStatus::DeniedInvalid,
    }
}

async fn load_state_record(state: &State<'_, AppState>) -> AppResult<Option<ProductLicenseStateRecord>> {
    load_state_record_from_db(&state.db).await
}

async fn load_state_record_from_db(db: &DatabaseConnection) -> AppResult<Option<ProductLicenseStateRecord>> {
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

pub async fn get_activation_claim_tenant_id(db: &DatabaseConnection) -> AppResult<Option<String>> {
    let Some(record) = load_state_record_from_db(db).await? else {
        return Ok(None);
    };
    if !record_has_valid_activation_claim(&record) {
        return Ok(None);
    }
    Ok(record.activation_claim.as_ref().map(|claim| claim.tenant_id.clone()))
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
        .map_or(false, activation_claim_is_complete)
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
        if matches!(v.as_str(), "core" | "professional" | "enterprise" | "development") {
            return Some(v);
        }
    }
    None
}

fn onboarding_state_from_record(record: &ProductLicenseStateRecord) -> ProductLicenseOnboardingState {
    let complete = record_has_valid_activation_claim(record);
    ProductLicenseOnboardingState {
        complete,
        status: record.status.clone(),
        pending_online_validation: record.pending_online_validation,
        deny_reason_code: record.deny_reason_code.clone(),
        deny_message: record.deny_message.clone(),
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
    let status = serde_json::to_value(&record.status)
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

/// True when at least one active user has Administrator or Superadmin explicitly scoped to this tenant.
pub async fn tenant_has_administrator_for_id(db: &DatabaseConnection, tenant_id: &str) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            r"SELECT COUNT(1) AS c
                FROM user_accounts ua
                INNER JOIN user_scope_assignments usa
                  ON usa.user_id = ua.id AND usa.deleted_at IS NULL
                INNER JOIN roles r ON r.id = usa.role_id AND r.deleted_at IS NULL
               WHERE ua.deleted_at IS NULL AND ua.is_active = 1
                 AND usa.scope_type = 'tenant'
                 AND r.name IN ('Administrator', 'Superadmin')
                 AND NOT (ua.username = 'admin' AND COALESCE(usa.notes, '') = 'System seeder')
                 AND TRIM(COALESCE(usa.scope_reference, '')) = ?",
            [tenant_id.into()],
        ))
        .await?;
    let count: i64 = row.as_ref().and_then(|r| r.try_get::<i64>("", "c").ok()).unwrap_or(0);
    Ok(count > 0)
}

async fn reset_local_tenant_runtime_data_impl(state: &State<'_, AppState>) -> AppResult<u64> {
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
    ]);

    tracing::warn!(
        event = "desktop_tenant_runtime_reset_begin",
        "Starting runtime tenant data reset"
    );
    let table_rows = state
        .db
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

    // SQLite ignores (or cannot toggle) foreign_keys inside an active transaction. Run OFF on the
    // connection first, then delete inside a transaction, then restore ON. Requires a single-connection
    // pool (see init_db) so this pragma applies to the same connection as `begin()`.
    state.db.execute_unprepared("PRAGMA foreign_keys = OFF").await?;

    let wipe_result: Result<u64, sea_orm::DbErr> = async {
        let tx = state.db.begin().await?;
        let mut wiped_rows_total: u64 = 0;
        for table in &tables_to_wipe {
            let escaped = table.replace('"', "\"\"");
            let sql = format!("DELETE FROM \"{escaped}\"");
            let result = tx.execute(Statement::from_string(DatabaseBackend::Sqlite, sql)).await?;
            wiped_rows_total = wiped_rows_total.saturating_add(result.rows_affected());
        }

        // Tenant-scoped preferences are tenant-owned context and should not leak across activations.
        tx.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "DELETE FROM app_settings WHERE setting_scope = 'tenant'".to_string(),
        ))
        .await?;

        tx.commit().await?;
        Ok(wiped_rows_total)
    }
    .await;

    if let Err(e) = state.db.execute_unprepared("PRAGMA foreign_keys = ON").await {
        tracing::error!(
            error = %e,
            "failed to re-enable SQLite foreign_keys after tenant runtime reset"
        );
    }

    let wiped_rows_total = wipe_result?;

    crate::db::seeder::seed_system_data(&state.db).await?;
    // Migrations are not re-run after wipe (seaql_migrations kept). Restore system
    // reference_* catalogs that seed_system_data does not cover (lookup_* only).
    crate::reference::system_catalog_integrity::ensure_system_reference_catalog_integrity(&state.db).await?;
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
    let record = load_state_record(&state).await?;
    let Some(record) = record else {
        return Ok(ActivationBootstrapState {
            tenant_id: None,
            company_display_name: None,
            has_tenant_admin: false,
        });
    };
    let tenant_id = record.activation_claim.as_ref().map(|c| c.tenant_id.clone());
    let has_tenant_admin = if let Some(ref tid) = tenant_id {
        tenant_has_administrator_for_id(&state.db, tid).await?
    } else {
        false
    };
    Ok(ActivationBootstrapState {
        tenant_id,
        company_display_name: record.company_display_name,
        has_tenant_admin,
    })
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

#[tauri::command]
pub async fn bootstrap_initial_tenant_admin(
    input: BootstrapInitialAdminInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let tenant_id = get_activation_claim_tenant_id(&state.db)
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Device is not activated.".into()]))?;
    if tenant_has_administrator_for_id(&state.db, &tenant_id).await? {
        return Err(AppError::ValidationFailed(vec![
            "A tenant administrator already exists for this activation.".into(),
        ]));
    }

    let username = input.username.trim();
    if username.is_empty() {
        return Err(AppError::ValidationFailed(vec!["username is required.".into()]));
    }
    let email = input.email.trim();
    if email.is_empty() || !email.contains('@') || !email.contains('.') {
        return Err(AppError::ValidationFailed(vec!["A valid email is required.".into()]));
    }
    crate::commands::admin_users::validate_password_strength(&input.password)?;

    let exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE LOWER(username) = LOWER(?) AND deleted_at IS NULL",
            [username.to_string().into()],
        ))
        .await?;
    if exists.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Username '{username}' is already taken"
        )]));
    }

    let role_row = state
        .db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id FROM roles WHERE name = 'Administrator' AND deleted_at IS NULL LIMIT 1".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Administrator role not found in seed data")))?;
    let role_id: i64 = role_row.try_get("", "id")?;

    let password_hash = crate::auth::password::hash_password(&input.password)?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sync_id = Uuid::new_v4().to_string();
    let display_name = input
        .display_name
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string)
        .unwrap_or_else(|| "Administrator".to_string());

    state
        .db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            r"INSERT INTO user_accounts
                (sync_id, username, display_name, identity_mode, password_hash,
                 is_active, is_admin, force_password_change, failed_login_attempts,
                 created_at, updated_at, row_version)
             VALUES (?, ?, ?, 'local', ?, 1, 1, 1, 0, ?, ?, 1)",
            vec![
                sync_id.into(),
                username.to_string().into(),
                display_name.into(),
                password_hash.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;

    let id_row = state
        .db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to read new user id")))?;
    let new_id: i64 = id_row.try_get("", "id")?;

    let assign_sync = Uuid::new_v4().to_string();
    let now_iso = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    state
        .db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            r"INSERT INTO user_scope_assignments
                (sync_id, user_id, role_id, scope_type, scope_reference,
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

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::USER_CREATED,
            summary: "Bootstrap initial tenant administrator",
            detail_json: Some(format!(
                r#"{{"username":"{}","email":"{}","tenant_id":"{}","bootstrap":true}}"#,
                username, email, tenant_id
            )),
            ..Default::default()
        },
    )
    .await;

    Ok(())
}

async fn persist_state_record(
    state: &State<'_, AppState>,
    user_id: i32,
    record: &ProductLicenseStateRecord,
    summary: &str,
) -> AppResult<()> {
    let json = serde_json::to_string(record)?;
    settings::set_setting(
        &state.db,
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

#[tauri::command]
pub async fn submit_product_license_key(
    key: String,
    claim_json: Option<String>,
    machine_fingerprint: Option<String>,
    app_version: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let changed_by_id = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .map(|session| session.user.user_id)
        .unwrap_or(0);
    let trimmed = key.trim();
    if trimmed.len() < 8 {
        return Err(AppError::ValidationFailed(vec![
            "License key must be at least 8 characters.".into(),
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
        company_display_name: parsed_claim.as_ref().and_then(|c| c.tenant_display_name.clone()),
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
        user_id = changed_by_id,
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
    persist_state_record(
        &state,
        changed_by_id,
        &record,
        "product license key submitted (state machine)",
    )
    .await?;
    apply_claim_entitlement_envelope(&state.db, entitlement_envelope).await?;
    crate::db::tenant_bootstrap::bootstrap_from_activation_claim(&state.db, changed_by_id).await?;
    Ok(())
}

#[tauri::command]
pub async fn apply_product_license_reconciliation(
    outcome_json: String,
    state: State<'_, AppState>,
) -> AppResult<ProductLicenseOnboardingState> {
    let changed_by_id = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .map(|session| session.user.user_id)
        .unwrap_or(0);
    let mut record = load_state_record(&state)
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["No local product license state found.".into()]))?;
    let outcome_value: serde_json::Value = serde_json::from_str(&outcome_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("outcomeJson must be valid JSON: {e}")]))?;
    // #region agent log
    {
        let kind = outcome_value.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let has_claim = outcome_value.get("claim").is_some();
        let has_env = outcome_value.get("entitlement_envelope").is_some();
        let line = format!(
            "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"G-serde\",\"location\":\"product_license.rs:apply_reconciliation\",\"message\":\"reconciling outcome\",\"data\":{{\"kind\":\"{kind}\",\"hasClaim\":{has_claim},\"hasEnvelope\":{has_env}}},\"timestamp\":{}}}\n",
            chrono::Utc::now().timestamp_millis()
        );
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(r"c:\Dev\MaintafoxSuite\desktop\maintafox-desktop\debug-a6aab3.log")
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(line.as_bytes())
            });
    }
    // #endregion
    let entitlement_envelope = match outcome_value
        .get("entitlement_envelope")
        .cloned()
        .filter(|v| !v.is_null())
        .map(serde_json::from_value::<crate::entitlements::domain::EntitlementEnvelopeInput>)
        .transpose()
    {
        Ok(v) => v,
        Err(e) => {
            // #region agent log
            {
                let line = format!(
                    "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"G-serde\",\"location\":\"product_license.rs:envelope_parse\",\"message\":\"envelope deserialize failed; continuing without envelope\",\"data\":{{\"error\":\"{}\"}},\"timestamp\":{}}}\n",
                    e.to_string().replace('\\', "\\\\").replace('"', "'"),
                    chrono::Utc::now().timestamp_millis()
                );
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(r"c:\Dev\MaintafoxSuite\desktop\maintafox-desktop\debug-a6aab3.log")
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(line.as_bytes())
                    });
            }
            // #endregion
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
    let mut outcome: ProductLicenseReconciliationInput = serde_json::from_value(outcome_for_meta)
        .map_err(|e| {
            // #region agent log
            {
                let line = format!(
                    "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"G-serde\",\"location\":\"product_license.rs:outcome_parse\",\"message\":\"outcome deserialize failed\",\"data\":{{\"error\":\"{}\"}},\"timestamp\":{}}}\n",
                    e.to_string().replace('\\', "\\\\").replace('"', "'"),
                    chrono::Utc::now().timestamp_millis()
                );
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(r"c:\Dev\MaintafoxSuite\desktop\maintafox-desktop\debug-a6aab3.log")
                    .and_then(|mut f| {
                        use std::io::Write;
                        f.write_all(line.as_bytes())
                    });
            }
            // #endregion
            AppError::ValidationFailed(vec![format!(
                "outcomeJson must be valid ProductLicenseReconciliationInput JSON: {e}"
            )])
        })?;
    outcome.entitlement_envelope = entitlement_envelope;

    let now = Utc::now().to_rfc3339();
    record.reconciliation.last_attempt_at = Some(now.clone());
    if let Some(app_version) = outcome.app_version.clone() {
        record.app_version = Some(app_version);
    }

    match outcome.kind {
        ProductLicenseReconciliationOutcomeKind::Success => {
            let claim = outcome.claim.ok_or_else(|| {
                AppError::ValidationFailed(vec!["Reconciliation success requires claim payload.".into()])
            })?;
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
            record.status = map_denied_status(Some(&denied_code));
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
            record.pending_online_validation = true;
            record.status = ProductLicenseActivationStatus::DegradedApiUnavailable;
            record.degraded_reason = Some(
                outcome
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "Activation API unavailable; running in degraded mode.".into()),
            );
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
        user_id = changed_by_id,
        status = ?record.status,
        pending_online_validation = record.pending_online_validation,
        retry_attempt = record.reconciliation.retry_attempt,
        deny_reason_code = record.deny_reason_code.as_deref().unwrap_or(""),
        last_error_code = record.reconciliation.last_error_code.as_deref().unwrap_or(""),
        "Product license reconciliation outcome applied"
    );

    persist_state_record(
        &state,
        changed_by_id,
        &record,
        "product license reconciliation state updated",
    )
    .await?;
    if matches!(outcome.kind, ProductLicenseReconciliationOutcomeKind::Success) {
        apply_claim_entitlement_envelope(&state.db, outcome.entitlement_envelope).await?;
    }
    crate::db::tenant_bootstrap::bootstrap_from_activation_claim(&state.db, changed_by_id).await?;

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
        let tenant_id = load_state_record(&state)
            .await?
            .and_then(|r| r.activation_claim.as_ref().map(|c| c.tenant_id.clone()));
        let has_admin = if let Some(ref tid) = tenant_id {
            tenant_has_administrator_for_id(&state.db, tid).await?
        } else {
            false
        };
        if has_admin {
            return Err(AppError::Auth(
                "Session requise pour réinitialiser l'activation une fois un administrateur créé.".into(),
            ));
        }
    }

    let changed_by_id = state
        .session
        .read()
        .await
        .current
        .as_ref()
        .map(|session| session.user.user_id)
        .unwrap_or(0);
    let wiped_rows = reset_local_tenant_runtime_data_impl(&state).await?;
    let _ = settings::delete_setting(
        &state.db,
        PRODUCT_LICENSE_ONBOARDING_KEY,
        PRODUCT_LICENSE_SCOPE,
        changed_by_id,
        "product license activation reset (recovery)",
    )
    .await?;
    let _ = settings::delete_setting(
        &state.db,
        TENANT_BOOTSTRAP_KEY,
        TENANT_BOOTSTRAP_SCOPE,
        changed_by_id,
        "tenant bootstrap reset (recovery)",
    )
    .await?;
    {
        let mut session_guard = state.session.write().await;
        session_guard.clear_session();
    }
    tracing::warn!(
        event = "desktop_activation_reset",
        wiped_rows,
        "Product activation state reset and tenant runtime wiped"
    );
    Ok(())
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
        let tenant_id = load_state_record(&state)
            .await?
            .and_then(|r| r.activation_claim.as_ref().map(|c| c.tenant_id.clone()));
        let has_admin = if let Some(ref tid) = tenant_id {
            tenant_has_administrator_for_id(&state.db, tid).await?
        } else {
            false
        };
        if has_admin {
            return Err(AppError::Auth(
                "Session requise pour réinitialiser les données runtime une fois un administrateur créé.".into(),
            ));
        }
    }

    tracing::warn!(
        event = "desktop_tenant_runtime_reset_command",
        "reset_local_tenant_runtime_data command invoked"
    );
    let wiped_rows = reset_local_tenant_runtime_data_impl(&state).await?;
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

/// Bearer token from the last successful activation claim (`/api/v1/activation/claim`).
/// Used by the control-plane sync transport; requires an authenticated desktop session.
#[tauri::command]
pub async fn get_control_plane_activation_bearer_token(state: State<'_, AppState>) -> AppResult<Option<String>> {
    let _session = require_session!(state);
    let record = load_state_record(&state).await?;
    let Some(rec) = record else {
        return Ok(None);
    };
    if rec.status != ProductLicenseActivationStatus::Active {
        return Ok(None);
    }
    if !record_has_valid_activation_claim(&rec) {
        return Ok(None);
    }
    Ok(rec.activation_claim.map(|c| c.activation_token))
}

/// Device-local activation token for policy refresh before/without an app session.
#[tauri::command]
pub async fn get_product_activation_token(state: State<'_, AppState>) -> AppResult<Option<String>> {
    let record = load_state_record(&state).await?;
    let Some(rec) = record else {
        return Ok(None);
    };
    if !record_has_valid_activation_claim(&rec) {
        return Ok(None);
    }
    Ok(rec.activation_claim.map(|c| c.activation_token))
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
