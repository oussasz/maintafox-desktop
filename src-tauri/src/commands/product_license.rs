//! Product (tenant) license key onboarding — persisted in `app_settings`, not the in-app licensing module.

use chrono::{DateTime, Duration, Utc};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::errors::{AppError, AppResult};
use crate::settings;
use crate::state::AppState;

const PRODUCT_LICENSE_ONBOARDING_KEY: &str = "product.license_onboarding";
const PRODUCT_LICENSE_SCOPE: &str = "device";

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
    Ok(record.activation_claim.map(|claim| claim.tenant_id))
}

async fn persist_state_record(state: &State<'_, AppState>, user_id: i32, record: &ProductLicenseStateRecord, summary: &str) -> AppResult<()> {
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
pub async fn get_product_license_onboarding_state(state: State<'_, AppState>) -> AppResult<ProductLicenseOnboardingState> {
    let record = load_state_record(&state).await?;
    if let Some(record) = record {
        Ok(ProductLicenseOnboardingState {
            complete: true,
            status: record.status,
            pending_online_validation: record.pending_online_validation,
            deny_reason_code: record.deny_reason_code,
            deny_message: record.deny_message,
            degraded_reason: record.degraded_reason,
            next_retry_at: record.reconciliation.next_retry_at,
            retry_attempt: record.reconciliation.retry_attempt,
            last_reconciled_at: record.last_reconciled_at,
            last_error_code: record.reconciliation.last_error_code,
            last_error_message: record.reconciliation.last_error_message,
        })
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
        return Err(AppError::ValidationFailed(vec!["License key must be at least 8 characters.".into()]));
    }
    let parsed_claim = match claim_json {
        Some(raw) => {
            let value: ProductActivationClaimRecord = serde_json::from_str(&raw)
                .map_err(|_| AppError::ValidationFailed(vec!["claimJson must be valid ProductActivationClaimRecord JSON".into()]))?;
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
        "Product license key submit processed"
    );
    persist_state_record(&state, changed_by_id, &record, "product license key submitted (state machine)")
        .await
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
    let outcome: ProductLicenseReconciliationInput = serde_json::from_str(&outcome_json)
        .map_err(|_| AppError::ValidationFailed(vec!["outcomeJson must be valid ProductLicenseReconciliationInput JSON".into()]))?;

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
            record.reconciliation.next_retry_at = Some(compute_backoff_at(record.reconciliation.retry_attempt).to_rfc3339());
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

    Ok(ProductLicenseOnboardingState {
        complete: true,
        status: record.status,
        pending_online_validation: record.pending_online_validation,
        deny_reason_code: record.deny_reason_code,
        deny_message: record.deny_message,
        degraded_reason: record.degraded_reason,
        next_retry_at: record.reconciliation.next_retry_at,
        retry_attempt: record.reconciliation.retry_attempt,
        last_reconciled_at: record.last_reconciled_at,
        last_error_code: record.reconciliation.last_error_code,
        last_error_message: record.reconciliation.last_error_message,
    })
}

#[tauri::command]
pub async fn get_product_license_diagnostics(state: State<'_, AppState>) -> AppResult<Option<ProductLicenseDiagnostics>> {
    let record = load_state_record(&state).await?;
    Ok(record.map(|record| ProductLicenseDiagnostics {
        status: record.status,
        deny_reason_code: record.deny_reason_code,
        deny_message: record.deny_message,
        pending_online_validation: record.pending_online_validation,
        last_reconciled_at: record.last_reconciled_at,
        machine_fingerprint: record.machine_fingerprint,
        app_version: record.app_version,
        reconciliation: record.reconciliation,
        diagnostics: record.diagnostics,
        has_activation_claim: record.activation_claim.is_some(),
    }))
}
