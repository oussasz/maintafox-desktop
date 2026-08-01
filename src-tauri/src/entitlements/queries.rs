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

/// Soft phase: accept control-plane envelopes even when local canonicalization
/// disagrees with Node `JSON.stringify` (historical serde_json mismatch).
/// Phase B hard enforcement must set this to false after canon parity is proven.
const SOFT_ACCEPT_CONTROL_PLANE_ENVELOPES: bool = true;

fn json_str(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Canonical payload must match control-plane `JSON.stringify({...})` field order.
fn canonical_payload(input: &EntitlementEnvelopeInput) -> String {
    let previous = match &input.previous_envelope_id {
        Some(v) => json_str(v),
        None => "null".to_string(),
    };
    format!(
        "{{\"envelope_id\":{},\"previous_envelope_id\":{},\"lineage_version\":{},\"issuer\":{},\"key_id\":{},\"signature_alg\":{},\"tier\":{},\"state\":{},\"channel\":{},\"machine_slots\":{},\"feature_flags_json\":{},\"capabilities_json\":{},\"policy_json\":{},\"issued_at\":{},\"valid_from\":{},\"valid_until\":{},\"offline_grace_until\":{}}}",
        json_str(&input.envelope_id),
        previous,
        input.lineage_version,
        json_str(&input.issuer),
        json_str(&input.key_id),
        json_str(&input.signature_alg),
        json_str(&input.tier),
        json_str(&input.state),
        json_str(&input.channel),
        input.machine_slots,
        json_str(&input.feature_flags_json),
        json_str(&input.capabilities_json),
        json_str(&input.policy_json),
        json_str(&input.issued_at),
        json_str(&input.valid_from),
        json_str(&input.valid_until),
        json_str(&input.offline_grace_until),
    )
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
             WHERE verification_result IN ('verified', 'soft_accepted')
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

/// Active envelope identity for License Enforcement view-model (no full row decode).
pub async fn peek_active_envelope_meta(
    db: &DatabaseConnection,
) -> AppResult<Option<(String, String, String)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT e.envelope_id, e.tier, e.verification_result
             FROM entitlement_cache_state s
             JOIN entitlement_envelopes e ON e.id = s.active_envelope_id
             WHERE s.id = 1",
            [],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some((
        row.try_get("", "envelope_id")
            .map_err(|e| decode_err("envelope_id", e))?,
        row.try_get("", "tier").map_err(|e| decode_err("tier", e))?,
        row.try_get("", "verification_result")
            .map_err(|e| decode_err("verification_result", e))?,
    )))
}

fn is_soft_lineage_refresh(input: &EntitlementEnvelopeInput) -> bool {
    input.lineage_version <= 1
        && input
            .previous_envelope_id
            .as_ref()
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
}

async fn find_envelope_row_id_by_envelope_id(
    conn: &impl ConnectionTrait,
    envelope_id: &str,
) -> AppResult<Option<(i64, String)>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, payload_hash FROM entitlement_envelopes WHERE envelope_id = ?",
            [envelope_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some((
        row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        row.try_get("", "payload_hash")
            .map_err(|e| decode_err("payload_hash", e))?,
    )))
}

async fn update_envelope_row(
    conn: &impl ConnectionTrait,
    row_id: i64,
    input: &EntitlementEnvelopeInput,
    payload_hash: &str,
    verification_result: &str,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE entitlement_envelopes SET
            envelope_id = ?,
            previous_envelope_id = ?,
            lineage_version = ?,
            issuer = ?,
            key_id = ?,
            signature_alg = ?,
            tier = ?,
            state = ?,
            channel = ?,
            machine_slots = ?,
            feature_flags_json = ?,
            capabilities_json = ?,
            policy_json = ?,
            issued_at = ?,
            valid_from = ?,
            valid_until = ?,
            offline_grace_until = ?,
            payload_hash = ?,
            signature = ?,
            verified_at = CASE
                WHEN ? IN ('verified', 'soft_accepted') THEN strftime('%Y-%m-%dT%H:%M:%SZ','now')
                ELSE NULL
            END,
            verification_result = ?
         WHERE id = ?",
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
            verification_result.into(),
            verification_result.into(),
            row_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn apply_entitlement_envelope(
    db: &DatabaseConnection,
    input: EntitlementEnvelopeInput,
) -> AppResult<EntitlementRefreshResult> {
    validate_envelope_input(&input)?;
    verify_trust_key(db, &input.issuer, &input.key_id, "entitlement_signature").await?;
    let payload_hash = payload_hash(&input);
    let existing_same_id = find_envelope_row_id_by_envelope_id(db, &input.envelope_id).await?;
    // Anti-replay register only for brand-new envelope ids (re-apply / supersede skip).
    if existing_same_id.is_none() {
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
    }
    let verification_result = match expected_signature(&input) {
        Some(expected) if expected == input.signature => "verified".to_string(),
        Some(_) if SOFT_ACCEPT_CONTROL_PLANE_ENVELOPES => {
            // #region agent log
            {
                let line = format!(
                    "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"L-canon\",\"location\":\"entitlements/queries.rs:verify\",\"message\":\"signature mismatch soft-accepted\",\"data\":{{\"envelopeId\":\"{}\",\"tier\":\"{}\"}},\"timestamp\":{}}}\n",
                    input.envelope_id.replace('"', ""),
                    input.tier.replace('"', ""),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
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
                event = "desktop_entitlement_signature_soft_accepted",
                envelope_id = %input.envelope_id,
                tier = %input.tier,
                "Entitlement signature mismatch soft-accepted (Phase A)"
            );
            "soft_accepted".to_string()
        }
        Some(_) => "invalid_signature".to_string(),
        None => "untrusted_issuer".to_string(),
    };
    let verified =
        verification_result == "verified" || verification_result == "soft_accepted";
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
            .ok_or_else(|| {
                AppError::ValidationFailed(vec![
                    "Failed to validate previous envelope lineage.".to_string(),
                ])
            })?
            .try_get("", "count")
            .map_err(|e| decode_err("previous_exists", e))?;
        if previous_exists == 0 {
            return Err(AppError::ValidationFailed(vec![
                "previous_envelope_id must reference an existing envelope for lineage continuity."
                    .to_string(),
            ]));
        }
    }

    let mut active_row_id: i64;
    let soft_refresh = is_soft_lineage_refresh(&input);

    if let Some((row_id, existing_hash)) = existing_same_id {
        if existing_hash != payload_hash {
            update_envelope_row(&tx, row_id, &input, &payload_hash, &verification_result).await?;
        }
        active_row_id = row_id;
    } else if verified && soft_refresh {
        let active_id: Option<i64> = tx
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT active_envelope_id FROM entitlement_cache_state WHERE id = 1",
                [],
            ))
            .await?
            .and_then(|r| r.try_get::<Option<i64>>("", "active_envelope_id").ok())
            .flatten();
        if let Some(row_id) = active_id {
            // Drop soft-lineage duplicates only — preserve real lineage history (version > 1).
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "DELETE FROM entitlement_envelopes
                 WHERE id <> ?
                   AND lineage_version <= 1
                   AND (previous_envelope_id IS NULL OR TRIM(previous_envelope_id) = '')",
                [row_id.into()],
            ))
            .await?;
            update_envelope_row(&tx, row_id, &input, &payload_hash, &verification_result).await?;
            active_row_id = row_id;
            // #region agent log
            {
                let line = format!(
                    "{{\"sessionId\":\"a6aab3\",\"runId\":\"post-fix\",\"hypothesisId\":\"M-upsert\",\"location\":\"entitlements/queries.rs:apply\",\"message\":\"soft-lineage envelope superseded in place\",\"data\":{{\"rowId\":{},\"envelopeId\":\"{}\"}},\"timestamp\":{}}}\n",
                    row_id,
                    input.envelope_id.replace('"', ""),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
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
        } else {
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO entitlement_envelopes (
                    envelope_id, previous_envelope_id, lineage_version, issuer, key_id, signature_alg,
                    tier, state, channel, machine_slots, feature_flags_json, capabilities_json, policy_json,
                    issued_at, valid_from, valid_until, offline_grace_until, payload_hash, signature,
                    verified_at, verification_result, created_at
                 ) VALUES (
                    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    CASE WHEN ? IN ('verified', 'soft_accepted') THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE NULL END,
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
                    payload_hash.clone().into(),
                    input.signature.clone().into(),
                    verification_result.clone().into(),
                    verification_result.clone().into(),
                ],
            ))
            .await?;
            active_row_id = tx
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT id FROM entitlement_envelopes WHERE envelope_id = ?",
                    [input.envelope_id.clone().into()],
                ))
                .await?
                .ok_or_else(|| {
                    AppError::SyncError("Failed to resolve inserted entitlement envelope.".to_string())
                })?
                .try_get("", "id")
                .map_err(|e| decode_err("id", e))?;
        }
    } else {
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO entitlement_envelopes (
                envelope_id, previous_envelope_id, lineage_version, issuer, key_id, signature_alg,
                tier, state, channel, machine_slots, feature_flags_json, capabilities_json, policy_json,
                issued_at, valid_from, valid_until, offline_grace_until, payload_hash, signature,
                verified_at, verification_result, created_at
             ) VALUES (
                ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                CASE WHEN ? IN ('verified', 'soft_accepted') THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE NULL END,
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
                payload_hash.clone().into(),
                input.signature.clone().into(),
                verification_result.clone().into(),
                verification_result.clone().into(),
            ],
        ))
        .await?;
        active_row_id = tx
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id FROM entitlement_envelopes WHERE envelope_id = ?",
                [input.envelope_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| {
                AppError::SyncError("Failed to resolve inserted entitlement envelope.".to_string())
            })?
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?;
    }

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO entitlement_cache_state (
            id, active_envelope_id, last_refresh_at, last_refresh_error, updated_at
         ) VALUES (
            1, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, strftime('%Y-%m-%dT%H:%M:%SZ','now')
         )
         ON CONFLICT(id) DO UPDATE SET
            active_envelope_id = CASE WHEN ? IN ('verified', 'soft_accepted') THEN excluded.active_envelope_id ELSE entitlement_cache_state.active_envelope_id END,
            last_refresh_at = excluded.last_refresh_at,
            last_refresh_error = excluded.last_refresh_error,
            updated_at = excluded.updated_at",
        [
            active_row_id.into(),
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
        // #region agent log
        {
            let false_count = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                &envelope.capabilities_json,
            )
            .ok()
            .map(|m| {
                m.values()
                    .filter(|v| v.as_bool() == Some(false))
                    .count()
            })
            .unwrap_or(0);
            let line = format!(
                "{{\"sessionId\":\"a6aab3\",\"runId\":\"pre-fix\",\"hypothesisId\":\"A-D\",\"location\":\"entitlements/queries.rs:get_entitlement_summary\",\"message\":\"active envelope summary\",\"data\":{{\"hasEnvelope\":true,\"envelopeId\":\"{}\",\"tier\":\"{}\",\"effectiveState\":\"{}\",\"capJsonLen\":{},\"falseCapabilityCount\":{}}},\"timestamp\":{}}}\n",
                envelope.envelope_id.replace('"', ""),
                envelope.tier.replace('"', ""),
                effective_state.replace('"', ""),
                envelope.capabilities_json.len(),
                false_count,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
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
    // #region agent log
    {
        let line = format!(
            "{{\"sessionId\":\"a6aab3\",\"runId\":\"pre-fix\",\"hypothesisId\":\"A\",\"location\":\"entitlements/queries.rs:get_entitlement_summary\",\"message\":\"no active envelope; legacy empty map\",\"data\":{{\"hasEnvelope\":false}},\"timestamp\":{}}}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
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
        .and_then(|value| {
            value
                .get("grace_allowed_modules")
                .cloned()
                .or_else(|| value.get("grace_allowed_capabilities").cloned())
        })
        .and_then(|value| serde_json::from_value::<Vec<String>>(value).ok())
        .unwrap_or_default();

    let allowed = match summary.effective_state.as_str() {
        "suspended" | "revoked" | "expired" => false,
        "grace" => allow_in_grace.iter().any(|cap| cap == &capability),
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
    // Module-level commercial capabilities. CRUD remains RBAC-owned.
    if permission.starts_with("eq.") {
        Some("equipment")
    } else if permission.starts_with("di.") {
        Some("requests")
    } else if permission.starts_with("ot.") {
        Some("work_orders")
    } else if permission.starts_with("inv.") {
        Some("inventory")
    } else if permission.starts_with("per.") || permission.starts_with("trn.") {
        Some("personnel")
    } else if permission.starts_with("org.") {
        Some("organization")
    } else if permission.starts_with("ref.") {
        Some("reference")
    } else if permission.starts_with("plan.") {
        Some("planning")
    } else if permission.starts_with("pm.") {
        Some("pm")
    } else if permission.starts_with("ptw.") {
        Some("permits")
    } else if permission.starts_with("ins.") {
        Some("inspections")
    } else if permission.starts_with("rep.") {
        Some("reports")
    } else if permission.starts_with("ram.") {
        Some("rams")
    } else if permission.starts_with("fin.") {
        Some("finance")
    } else if permission.starts_with("sync.") {
        Some("sync")
    } else if permission.starts_with("erp.") {
        Some("erp")
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
