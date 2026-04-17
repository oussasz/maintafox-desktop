use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

pub const MAX_CLOCK_SKEW_SECS: i64 = 300;
pub const API_EXCHANGE_MAX_AGE_SECS: i64 = 86_400;

#[derive(Debug, Clone)]
pub struct LicenseTraceInput {
    pub correlation_id: String,
    pub event_type: String,
    pub source: String,
    pub subject_type: String,
    pub subject_id: Option<String>,
    pub reason_code: Option<String>,
    pub outcome: String,
    pub payload_json: String,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::ValidationFailed(vec![format!("Failed to decode licensing security field '{field}': {err}")])
}

fn parse_rfc3339(value: &str, field: &str) -> AppResult<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|e| AppError::ValidationFailed(vec![format!("{field} must be RFC3339: {e}")]))
}

pub fn validate_signed_timestamp_window(signed_at: &str, now: chrono::DateTime<Utc>) -> AppResult<()> {
    let signed = parse_rfc3339(signed_at, "signed_at")?;
    let skew_secs = (signed - now).num_seconds();
    if skew_secs > MAX_CLOCK_SKEW_SECS {
        return Err(AppError::ValidationFailed(vec![format!(
            "signed_at exceeds forward clock skew tolerance ({}s > {}s).",
            skew_secs, MAX_CLOCK_SKEW_SECS
        )]));
    }
    let age_secs = (now - signed).num_seconds();
    if age_secs > API_EXCHANGE_MAX_AGE_SECS {
        return Err(AppError::ValidationFailed(vec![format!(
            "signed_at is stale for anti-replay window ({}s > {}s).",
            age_secs, API_EXCHANGE_MAX_AGE_SECS
        )]));
    }
    Ok(())
}

pub async fn verify_trust_key(
    db: &impl ConnectionTrait,
    issuer: &str,
    key_id: &str,
    purpose: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT is_active, is_compromised, purpose
             FROM licensing_trust_keys
             WHERE issuer = ? AND key_id = ?",
            [issuer.to_string().into(), key_id.to_string().into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Untrusted signing key '{}:{}' for purpose '{}'.",
            issuer, key_id, purpose
        )]));
    };
    let is_active: i64 = row.try_get("", "is_active").map_err(|e| decode_err("is_active", e))?;
    let is_compromised: i64 = row
        .try_get("", "is_compromised")
        .map_err(|e| decode_err("is_compromised", e))?;
    let row_purpose: String = row.try_get("", "purpose").map_err(|e| decode_err("purpose", e))?;
    if row_purpose != purpose {
        return Err(AppError::ValidationFailed(vec![format!(
            "Signing key '{}:{}' has purpose '{}' but '{}' was required.",
            issuer, key_id, row_purpose, purpose
        )]));
    }
    if is_compromised == 1 {
        return Err(AppError::LicenseDenied {
            reason_code: "compromised_key".to_string(),
            message: format!("Signing key '{}:{}' is marked compromised.", issuer, key_id),
        });
    }
    if is_active != 1 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Signing key '{}:{}' is inactive.",
            issuer, key_id
        )]));
    }
    Ok(())
}

pub async fn register_api_exchange(
    db: &impl ConnectionTrait,
    channel: &str,
    purpose: &str,
    exchange_id: &str,
    request_nonce: Option<&str>,
    response_nonce: Option<&str>,
    signed_at: &str,
    payload_hash: &str,
    signer_key_id: Option<&str>,
    correlation_id: Option<&str>,
) -> AppResult<()> {
    validate_signed_timestamp_window(signed_at, Utc::now())?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO license_api_exchange_guard (
            id, channel, purpose, exchange_id, request_nonce, response_nonce, signed_at, received_at,
            payload_hash, signer_key_id, verification_result, correlation_id
         ) VALUES (?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, ?, 'accepted', ?)",
        [
            Uuid::new_v4().to_string().into(),
            channel.to_string().into(),
            purpose.to_string().into(),
            exchange_id.to_string().into(),
            request_nonce.map(ToOwned::to_owned).into(),
            response_nonce.map(ToOwned::to_owned).into(),
            signed_at.to_string().into(),
            payload_hash.to_string().into(),
            signer_key_id.map(ToOwned::to_owned).into(),
            correlation_id.map(ToOwned::to_owned).into(),
        ],
    ))
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("UNIQUE constraint failed") {
            AppError::LicenseDenied {
                reason_code: "replay_detected".to_string(),
                message: "Replayed licensing API exchange was rejected by anti-replay guard.".to_string(),
            }
        } else {
            AppError::Database(e)
        }
    })?;
    Ok(())
}

pub async fn append_license_trace(db: &impl ConnectionTrait, input: LicenseTraceInput) -> AppResult<String> {
    let previous_hash: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT event_hash FROM license_event_traces ORDER BY occurred_at DESC, id DESC LIMIT 1",
            [],
        ))
        .await?
        .and_then(|row| row.try_get::<Option<String>>("", "event_hash").ok())
        .flatten();
    let mut hasher = Sha256::new();
    hasher.update(input.correlation_id.as_bytes());
    hasher.update(b"|");
    hasher.update(input.event_type.as_bytes());
    hasher.update(b"|");
    hasher.update(input.source.as_bytes());
    hasher.update(b"|");
    hasher.update(input.subject_type.as_bytes());
    hasher.update(b"|");
    hasher.update(input.subject_id.clone().unwrap_or_default().as_bytes());
    hasher.update(b"|");
    hasher.update(input.reason_code.clone().unwrap_or_default().as_bytes());
    hasher.update(b"|");
    hasher.update(input.outcome.as_bytes());
    hasher.update(b"|");
    hasher.update(input.payload_json.as_bytes());
    hasher.update(b"|");
    hasher.update(previous_hash.clone().unwrap_or_default().as_bytes());
    let event_hash = hex::encode(hasher.finalize());
    let id = Uuid::new_v4().to_string();

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO license_event_traces (
            id, correlation_id, event_type, source, subject_type, subject_id, reason_code, outcome, occurred_at,
            payload_hash, previous_hash, event_hash
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, ?, ?)",
        [
            id.clone().into(),
            input.correlation_id.into(),
            input.event_type.into(),
            input.source.into(),
            input.subject_type.into(),
            input.subject_id.into(),
            input.reason_code.into(),
            input.outcome.into(),
            event_hash.clone().into(),
            previous_hash.into(),
            event_hash.clone().into(),
        ],
    ))
    .await?;
    Ok(event_hash)
}

pub async fn mark_key_compromised(
    db: &impl ConnectionTrait,
    issuer: &str,
    key_id: &str,
    reason: &str,
) -> AppResult<()> {
    if reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "compromise reason is required.".to_string(),
        ]));
    }
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE licensing_trust_keys
         SET is_compromised = 1,
             is_active = 0,
             compromised_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             compromise_reason = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE issuer = ? AND key_id = ?",
        [
            reason.to_string().into(),
            issuer.to_string().into(),
            key_id.to_string().into(),
        ],
    ))
    .await?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO license_enforcement_state (
            id, policy_sync_pending, last_transition_at, last_transition_reason, updated_at
         ) VALUES (
            1, 1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, strftime('%Y-%m-%dT%H:%M:%SZ','now')
         )
         ON CONFLICT(id) DO UPDATE SET
            policy_sync_pending = 1,
            last_transition_at = excluded.last_transition_at,
            last_transition_reason = excluded.last_transition_reason,
            updated_at = excluded.updated_at",
        [format!("compromised_key:{}:{}:{reason}", issuer, key_id).into()],
    ))
    .await?;
    Ok(())
}
