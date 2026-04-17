use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use sha2::{Digest, Sha256};

use crate::entitlements::domain::{
    EntitlementCapabilityCheck, EntitlementDiagnostics, EntitlementEnvelope, EntitlementEnvelopeInput,
    EntitlementRefreshResult, EntitlementSummary, ENTITLEMENT_SIGNATURE_ALG_V1,
};
use crate::errors::{AppError, AppResult};
use crate::license::security::{append_license_trace, register_api_exchange, verify_trust_key, LicenseTraceInput};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::ValidationFailed(vec![format!("Failed to decode entitlement field '{field}': {err}")])
}

fn trusted_issuer_secrets() -> HashMap<(&'static str, &'static str), &'static str> {
    HashMap::from([
        (("maintafox-vps", "key-v1"), "MAINTAFOX_TRUSTED_ISSUER_KEY_V1"),
        (("maintafox-vps", "key-rotated-v2"), "MAINTAFOX_TRUSTED_ISSUER_KEY_V2"),
    ])
}

fn canonical_payload(input: &EntitlementEnvelopeInput) -> String {
    serde_json::json!({
        "envelope_id": input.envelope_id,
        "previous_envelope_id": input.previous_envelope_id,
        "lineage_version": input.lineage_version,
        "issuer": input.issuer,
        "key_id": input.key_id,
        "signature_alg": input.signature_alg,
        "tier": input.tier,
        "state": input.state,
        "channel": input.channel,
        "machine_slots": input.machine_slots,
        "feature_flags_json": input.feature_flags_json,
        "capabilities_json": input.capabilities_json,
        "policy_json": input.policy_json,
        "issued_at": input.issued_at,
        "valid_from": input.valid_from,
        "valid_until": input.valid_until,
        "offline_grace_until": input.offline_grace_until
    })
    .to_string()
}

fn payload_hash(input: &EntitlementEnvelopeInput) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_payload(input));
    hex::encode(hasher.finalize())
}

fn expected_signature(input: &EntitlementEnvelopeInput) -> Option<String> {
    let issuer_secrets = trusted_issuer_secrets();
    let secret = issuer_secrets.get(&(input.issuer.as_str(), input.key_id.as_str()))?;
    let payload_hash = payload_hash(input);
    let material = format!(
        "{}:{}:{}:{}",
        input.issuer, input.key_id, payload_hash, secret
    );
    let mut hasher = Sha256::new();
    hasher.update(material);
    Some(hex::encode(hasher.finalize()))
}

fn parse_rfc3339(value: &str, field: &str, errors: &mut Vec<String>) {
    if chrono::DateTime::parse_from_rfc3339(value).is_err() {
        errors.push(format!("{field} must be a valid RFC3339 timestamp."));
    }
}

fn validate_envelope_input(input: &EntitlementEnvelopeInput) -> AppResult<()> {
    let mut errors = Vec::new();
    if input.envelope_id.trim().is_empty() {
        errors.push("envelope_id is required.".to_string());
    }
    if input.issuer.trim().is_empty() {
        errors.push("issuer is required.".to_string());
    }
    if input.key_id.trim().is_empty() {
        errors.push("key_id is required.".to_string());
    }
    if input.signature_alg != ENTITLEMENT_SIGNATURE_ALG_V1 {
        errors.push(format!(
            "Unsupported signature_alg '{}'; supported: {}.",
            input.signature_alg, ENTITLEMENT_SIGNATURE_ALG_V1
        ));
    }
    match input.state.as_str() {
        "active" | "grace" | "expired" | "suspended" | "revoked" => {}
        _ => errors.push("state must be one of: active, grace, expired, suspended, revoked.".to_string()),
    }
    if input.lineage_version <= 0 {
        errors.push("lineage_version must be > 0.".to_string());
    }
    if input.machine_slots < 0 {
        errors.push("machine_slots must be >= 0.".to_string());
    }
    parse_rfc3339(&input.issued_at, "issued_at", &mut errors);
    parse_rfc3339(&input.valid_from, "valid_from", &mut errors);
    parse_rfc3339(&input.valid_until, "valid_until", &mut errors);
    parse_rfc3339(&input.offline_grace_until, "offline_grace_until", &mut errors);
    if serde_json::from_str::<serde_json::Value>(&input.feature_flags_json).is_err() {
        errors.push("feature_flags_json must be valid JSON.".to_string());
    }
    if serde_json::from_str::<serde_json::Value>(&input.capabilities_json).is_err() {
        errors.push("capabilities_json must be valid JSON.".to_string());
    }
    if serde_json::from_str::<serde_json::Value>(&input.policy_json).is_err() {
        errors.push("policy_json must be valid JSON.".to_string());
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    Ok(())
}

fn to_entitlement_envelope(row: &sea_orm::QueryResult) -> AppResult<EntitlementEnvelope> {
    Ok(EntitlementEnvelope {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        envelope_id: row
            .try_get("", "envelope_id")
            .map_err(|e| decode_err("envelope_id", e))?,
        previous_envelope_id: row
            .try_get("", "previous_envelope_id")
            .map_err(|e| decode_err("previous_envelope_id", e))?,
        lineage_version: row
            .try_get("", "lineage_version")
            .map_err(|e| decode_err("lineage_version", e))?,
        issuer: row.try_get("", "issuer").map_err(|e| decode_err("issuer", e))?,
        key_id: row.try_get("", "key_id").map_err(|e| decode_err("key_id", e))?,
        signature_alg: row
            .try_get("", "signature_alg")
            .map_err(|e| decode_err("signature_alg", e))?,
        tier: row.try_get("", "tier").map_err(|e| decode_err("tier", e))?,
        state: row.try_get("", "state").map_err(|e| decode_err("state", e))?,
        channel: row
            .try_get("", "channel")
            .map_err(|e| decode_err("channel", e))?,
        machine_slots: row
            .try_get("", "machine_slots")
            .map_err(|e| decode_err("machine_slots", e))?,
        feature_flags_json: row
            .try_get("", "feature_flags_json")
            .map_err(|e| decode_err("feature_flags_json", e))?,
        capabilities_json: row
            .try_get("", "capabilities_json")
            .map_err(|e| decode_err("capabilities_json", e))?,
        policy_json: row
            .try_get("", "policy_json")
            .map_err(|e| decode_err("policy_json", e))?,
        issued_at: row
            .try_get("", "issued_at")
            .map_err(|e| decode_err("issued_at", e))?,
        valid_from: row
            .try_get("", "valid_from")
            .map_err(|e| decode_err("valid_from", e))?,
        valid_until: row
            .try_get("", "valid_until")
            .map_err(|e| decode_err("valid_until", e))?,
        offline_grace_until: row
            .try_get("", "offline_grace_until")
            .map_err(|e| decode_err("offline_grace_until", e))?,
        payload_hash: row
            .try_get("", "payload_hash")
            .map_err(|e| decode_err("payload_hash", e))?,
        signature: row
            .try_get("", "signature")
            .map_err(|e| decode_err("signature", e))?,
        verified_at: row
            .try_get("", "verified_at")
            .map_err(|e| decode_err("verified_at", e))?,
        verification_result: row
            .try_get("", "verification_result")
            .map_err(|e| decode_err("verification_result", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

fn parse_json_map(raw: &str) -> HashMap<String, bool> {
    serde_json::from_str::<HashMap<String, bool>>(raw).unwrap_or_default()
}

fn compute_effective_state(envelope: &EntitlementEnvelope) -> String {
    if envelope.state == "suspended" || envelope.state == "revoked" {
        return envelope.state.clone();
    }
    let now = chrono::Utc::now();
    let valid_until = chrono::DateTime::parse_from_rfc3339(&envelope.valid_until)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or(now);
    let grace_until = chrono::DateTime::parse_from_rfc3339(&envelope.offline_grace_until)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or(now);

    if now <= valid_until {
        "active".to_string()
    } else if now <= grace_until {
        "grace".to_string()
    } else {
        "expired".to_string()
    }
}

async fn active_envelope(db: &DatabaseConnection) -> AppResult<Option<EntitlementEnvelope>> {
    let state_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT active_envelope_id FROM entitlement_cache_state WHERE id = 1",
            [],
        ))
        .await?;
    let active_id: Option<i64> = state_row
        .as_ref()
        .map(|r| r.try_get::<Option<i64>>("", "active_envelope_id"))
        .transpose()
        .map_err(|e| decode_err("active_envelope_id", e))?
        .flatten();
    if let Some(active_id) = active_id {
        if let Some(row) = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id, envelope_id, previous_envelope_id, lineage_version, issuer, key_id, signature_alg,
                        tier, state, channel, machine_slots, feature_flags_json, capabilities_json, policy_json,
                        issued_at, valid_from, valid_until, offline_grace_until, payload_hash, signature,
                        verified_at, verification_result, created_at
                 FROM entitlement_envelopes
                 WHERE id = ?",
                [active_id.into()],
            ))
            .await?
        {
            return Ok(Some(to_entitlement_envelope(&row)?));
        }
    }
    if let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, envelope_id, previous_envelope_id, lineage_version, issuer, key_id, signature_alg,
                    tier, state, channel, machine_slots, feature_flags_json, capabilities_json, policy_json,
                    issued_at, valid_from, valid_until, offline_grace_until, payload_hash, signature,
                    verified_at, verification_result, created_at
             FROM entitlement_envelopes
             WHERE verification_result = 'verified'
             ORDER BY lineage_version DESC, id DESC
             LIMIT 1",
            [],
        ))
        .await?
    {
        return Ok(Some(to_entitlement_envelope(&row)?));
    }
    Ok(None)
}

pub async fn apply_entitlement_envelope(
    db: &DatabaseConnection,
    input: EntitlementEnvelopeInput,
) -> AppResult<EntitlementRefreshResult> {
    validate_envelope_input(&input)?;
    verify_trust_key(db, &input.issuer, &input.key_id, "entitlement_signature").await?;
    let payload_hash = payload_hash(&input);
    register_api_exchange(
        db,
        "vps.entitlement",
        "entitlement_envelope_apply",
        &input.envelope_id,
        None,
        None,
        &input.issued_at,
        &payload_hash,
        Some(&input.key_id),
        Some(&input.envelope_id),
    )
    .await?;
    let verification_result = match expected_signature(&input) {
        Some(expected) if expected == input.signature => "verified".to_string(),
        Some(_) => "invalid_signature".to_string(),
        None => "untrusted_issuer".to_string(),
    };
    let verified = verification_result == "verified";
    let tx = db.begin().await?;

    if input.lineage_version > 1 {
        let previous_exists: i64 = tx
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS count
                 FROM entitlement_envelopes
                 WHERE envelope_id = ?",
                [input
                    .previous_envelope_id
                    .clone()
                    .unwrap_or_default()
                    .into()],
            ))
            .await?
            .ok_or_else(|| AppError::ValidationFailed(vec!["Failed to validate previous envelope lineage.".to_string()]))?
            .try_get("", "count")
            .map_err(|e| decode_err("previous_exists", e))?;
        if previous_exists == 0 {
            return Err(AppError::ValidationFailed(vec![
                "previous_envelope_id must reference an existing envelope for lineage continuity.".to_string(),
            ]));
        }
    }

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO entitlement_envelopes (
            envelope_id, previous_envelope_id, lineage_version, issuer, key_id, signature_alg,
            tier, state, channel, machine_slots, feature_flags_json, capabilities_json, policy_json,
            issued_at, valid_from, valid_until, offline_grace_until, payload_hash, signature,
            verified_at, verification_result, created_at
         ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
            CASE WHEN ? = 'verified' THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE NULL END,
            ?, strftime('%Y-%m-%dT%H:%M:%SZ','now')
         )",
        [
            input.envelope_id.clone().into(),
            input.previous_envelope_id.clone().into(),
            input.lineage_version.into(),
            input.issuer.clone().into(),
            input.key_id.clone().into(),
            input.signature_alg.clone().into(),
            input.tier.clone().into(),
            input.state.clone().into(),
            input.channel.clone().into(),
            input.machine_slots.into(),
            input.feature_flags_json.clone().into(),
            input.capabilities_json.clone().into(),
            input.policy_json.clone().into(),
            input.issued_at.clone().into(),
            input.valid_from.clone().into(),
            input.valid_until.clone().into(),
            input.offline_grace_until.clone().into(),
            payload_hash.into(),
            input.signature.clone().into(),
            verification_result.clone().into(),
            verification_result.clone().into(),
        ],
    ))
    .await?;

    let inserted_id: i64 = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM entitlement_envelopes WHERE envelope_id = ?",
            [input.envelope_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to resolve inserted entitlement envelope.".to_string()))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO entitlement_cache_state (
            id, active_envelope_id, last_refresh_at, last_refresh_error, updated_at
         ) VALUES (
            1, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, strftime('%Y-%m-%dT%H:%M:%SZ','now')
         )
         ON CONFLICT(id) DO UPDATE SET
            active_envelope_id = CASE WHEN ? = 'verified' THEN excluded.active_envelope_id ELSE entitlement_cache_state.active_envelope_id END,
            last_refresh_at = excluded.last_refresh_at,
            last_refresh_error = excluded.last_refresh_error,
            updated_at = excluded.updated_at",
        [
            inserted_id.into(),
            if verified {
                sea_orm::Value::String(None)
            } else {
                Some(verification_result.clone()).into()
            },
            verification_result.clone().into(),
        ],
    ))
    .await?;
    tx.commit().await?;

    let trace_payload = serde_json::json!({
        "verification_result": verification_result,
        "lineage_version": input.lineage_version,
        "issuer": input.issuer,
        "key_id": input.key_id,
        "channel": input.channel
    })
    .to_string();
    let _ = append_license_trace(
        db,
        LicenseTraceInput {
            correlation_id: input.envelope_id.clone(),
            event_type: "entitlement.envelope_applied".to_string(),
            source: "entitlement_runtime".to_string(),
            subject_type: "entitlement_envelope".to_string(),
            subject_id: Some(input.envelope_id.clone()),
            reason_code: if verified {
                None
            } else {
                Some(verification_result.clone())
            },
            outcome: if verified { "accepted" } else { "rejected" }.to_string(),
            payload_json: trace_payload,
        },
    )
    .await;

    let summary = get_entitlement_summary(db).await?;
    Ok(EntitlementRefreshResult {
        envelope_id: input.envelope_id,
        verified,
        verification_result,
        effective_state: summary.effective_state,
        active_lineage_version: summary.lineage_version.unwrap_or(0),
    })
}

pub async fn get_entitlement_summary(db: &DatabaseConnection) -> AppResult<EntitlementSummary> {
    if let Some(envelope) = active_envelope(db).await? {
        let effective_state = compute_effective_state(&envelope);
        return Ok(EntitlementSummary {
            envelope_id: Some(envelope.envelope_id),
            state: envelope.state,
            effective_state,
            tier: Some(envelope.tier),
            channel: Some(envelope.channel),
            lineage_version: Some(envelope.lineage_version),
            valid_until: Some(envelope.valid_until),
            offline_grace_until: Some(envelope.offline_grace_until),
            last_verified_at: envelope.verified_at,
            capability_map_json: envelope.capabilities_json,
            feature_flag_map_json: envelope.feature_flags_json,
        });
    }
    // Legacy-safe fallback: allow runtime until first signed entitlement arrives.
    Ok(EntitlementSummary {
        envelope_id: None,
        state: "active".to_string(),
        effective_state: "active".to_string(),
        tier: Some("legacy".to_string()),
        channel: Some("stable".to_string()),
        lineage_version: None,
        valid_until: None,
        offline_grace_until: None,
        last_verified_at: None,
        capability_map_json: "{}".to_string(),
        feature_flag_map_json: "{}".to_string(),
    })
}

pub async fn check_entitlement_capability(
    db: &DatabaseConnection,
    capability: String,
) -> AppResult<EntitlementCapabilityCheck> {
    let summary = get_entitlement_summary(db).await?;
    let capabilities = parse_json_map(&summary.capability_map_json);
    let policy_json: Option<String> = if let Some(envelope) = active_envelope(db).await? {
        Some(envelope.policy_json)
    } else {
        None
    };
    let allow_in_grace = policy_json
        .as_ref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get("grace_allowed_capabilities").cloned())
        .and_then(|value| serde_json::from_value::<Vec<String>>(value).ok())
        .unwrap_or_default();

    let allowed = match summary.effective_state.as_str() {
        "suspended" | "revoked" | "expired" => false,
        "grace" => {
            if capability == "core.read" {
                true
            } else {
                allow_in_grace.iter().any(|cap| cap == &capability)
            }
        }
        _ => capabilities.get(&capability).copied().unwrap_or(true),
    };
    let reason = if allowed {
        "capability_allowed".to_string()
    } else {
        format!(
            "capability '{}' blocked in entitlement state '{}'",
            capability, summary.effective_state
        )
    };
    Ok(EntitlementCapabilityCheck {
        capability,
        allowed,
        reason,
        effective_state: summary.effective_state,
        envelope_id: summary.envelope_id,
    })
}

fn capability_from_permission(permission: &str) -> Option<&'static str> {
    if permission.ends_with(".view") || permission.starts_with("audit.") {
        Some("core.read")
    } else if permission.starts_with("inv.") {
        Some("inventory.write")
    } else if permission.starts_with("fin.") {
        Some("finance.write")
    } else if permission.starts_with("plan.") {
        Some("planning.write")
    } else if permission.starts_with("pm.") {
        Some("pm.write")
    } else if permission.starts_with("sync.") {
        Some("sync.runtime")
    } else if permission.starts_with("per.") {
        Some("personnel.write")
    } else if permission.starts_with("erp.") {
        Some("erp.connector")
    } else {
        None
    }
}

pub async fn enforce_capability_for_permission(
    db: &DatabaseConnection,
    permission: &str,
) -> AppResult<()> {
    let Some(capability) = capability_from_permission(permission) else {
        return Ok(());
    };
    let check = check_entitlement_capability(db, capability.to_string()).await?;
    if !check.allowed {
        return Err(AppError::PermissionDenied(format!(
            "Entitlement capability blocked: {} ({})",
            check.capability, check.reason
        )));
    }
    Ok(())
}

pub async fn get_entitlement_diagnostics(db: &DatabaseConnection, limit: Option<i64>) -> AppResult<EntitlementDiagnostics> {
    let page_size = limit.unwrap_or(25).clamp(1, 200);
    let summary = get_entitlement_summary(db).await?;
    let cache_state = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT last_refresh_at, last_refresh_error
             FROM entitlement_cache_state
             WHERE id = 1",
            [],
        ))
        .await?;
    let last_refresh_at: Option<String> = cache_state
        .as_ref()
        .map(|r| r.try_get::<Option<String>>("", "last_refresh_at"))
        .transpose()
        .map_err(|e| decode_err("last_refresh_at", e))?
        .flatten();
    let last_refresh_error: Option<String> = cache_state
        .as_ref()
        .map(|r| r.try_get::<Option<String>>("", "last_refresh_error"))
        .transpose()
        .map_err(|e| decode_err("last_refresh_error", e))?
        .flatten();

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, envelope_id, previous_envelope_id, lineage_version, issuer, key_id, signature_alg,
                    tier, state, channel, machine_slots, feature_flags_json, capabilities_json, policy_json,
                    issued_at, valid_from, valid_until, offline_grace_until, payload_hash, signature,
                    verified_at, verification_result, created_at
             FROM entitlement_envelopes
             ORDER BY lineage_version DESC, id DESC
             LIMIT ?",
            [page_size.into()],
        ))
        .await?;
    let lineage = rows
        .into_iter()
        .map(|row| to_entitlement_envelope(&row))
        .collect::<AppResult<Vec<_>>>()?;

    Ok(EntitlementDiagnostics {
        summary,
        last_refresh_at,
        last_refresh_error,
        lineage,
        runbook_links: vec![
            "https://docs.maintafox.com/runbooks/entitlements/state-transitions".to_string(),
            "https://docs.maintafox.com/runbooks/entitlements/offline-fallback".to_string(),
            "https://docs.maintafox.com/runbooks/entitlements/capability-gating".to_string(),
        ],
    })
}
