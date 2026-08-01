use std::collections::BTreeMap;

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::activation::domain::{
    ActivationLineageRecord, ActivationPolicySnapshot, ApplyMachineActivationInput, MachineActivationApplyResult,
    MachineActivationDiagnostics, MachineActivationStatus, OfflineActivationDecision, RebindMachineActivationInput,
    RebindMachineActivationResult, RotateActivationSecretInput, RotateActivationSecretResult,
};
use crate::audit;
use crate::entitlements;
use crate::errors::{AppError, AppResult};
use crate::license::security::{append_license_trace, register_api_exchange, LicenseTraceInput};

const KEYRING_SERVICE: &str = "maintafox-desktop";
const KEYRING_ACTIVATION_SECRET_KEY: &str = "machine-activation-binding-secret";
const KEYRING_ACTIVATION_TOKEN_KEY: &str = "machine-activation-refresh-token";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::ValidationFailed(vec![format!("Failed to decode activation field '{field}': {err}")])
}

fn parse_rfc3339(value: &str, field: &str, errors: &mut Vec<String>) {
    if chrono::DateTime::parse_from_rfc3339(value).is_err() {
        errors.push(format!("{field} must be a valid RFC3339 timestamp."));
    }
}

fn canonical_policy_snapshot(input: &ApplyMachineActivationInput) -> AppResult<ActivationPolicySnapshot> {
    serde_json::from_str::<ActivationPolicySnapshot>(&input.policy_snapshot_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Invalid policy_snapshot_json: {e}")]))
}

fn validate_activation_input(input: &ApplyMachineActivationInput) -> AppResult<()> {
    let mut errors = Vec::new();
    for (name, value) in [
        ("contract_id", input.contract_id.as_str()),
        ("machine_id", input.machine_id.as_str()),
        ("slot_assignment_id", input.slot_assignment_id.as_str()),
        ("response_nonce", input.response_nonce.as_str()),
        ("anchor_hashes_json", input.anchor_hashes_json.as_str()),
        ("policy_snapshot_json", input.policy_snapshot_json.as_str()),
    ] {
        if value.trim().is_empty() {
            errors.push(format!("{name} is required."));
        }
    }
    if input.slot_number <= 0 {
        errors.push("slot_number must be > 0.".to_string());
    }
    if input.slot_limit <= 0 {
        errors.push("slot_limit must be > 0.".to_string());
    }
    if input.slot_number > input.slot_limit {
        errors.push("slot_number cannot exceed slot_limit.".to_string());
    }
    if input.vps_version <= 0 {
        errors.push("vps_version must be > 0.".to_string());
    }
    parse_rfc3339(&input.issued_at, "issued_at", &mut errors);
    parse_rfc3339(&input.expires_at, "expires_at", &mut errors);
    parse_rfc3339(&input.offline_grace_until, "offline_grace_until", &mut errors);
    if !matches!(
        input.revocation_state.as_str(),
        "active" | "pending_revocation" | "revoked"
    ) {
        errors.push("revocation_state must be one of: active, pending_revocation, revoked.".to_string());
    }
    if serde_json::from_str::<BTreeMap<String, String>>(&input.anchor_hashes_json).is_err() {
        errors.push("anchor_hashes_json must decode to { anchor_name: hash }.".to_string());
    }
    if canonical_policy_snapshot(input).is_err() {
        errors.push("policy_snapshot_json must match ActivationPolicySnapshot.".to_string());
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    Ok(())
}

fn read_machine_id_or_fallback() -> String {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("reg")
            .args(["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.trim().starts_with("MachineGuid") {
                    if let Some(guid) = line.split_whitespace().last() {
                        return guid.trim().to_string();
                    }
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(value) = std::fs::read_to_string("/etc/machine-id") {
            return value.trim().to_string();
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(uuid) = line.split('"').nth(3) {
                        return uuid.to_string();
                    }
                }
            }
        }
    }
    "unknown-machine".to_string()
}

fn get_keyring_entry(secret_name: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, secret_name)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("keyring open failed: {e}")))
}

#[cfg(not(test))]
fn get_activation_binding_secret() -> AppResult<Option<Vec<u8>>> {
    let entry = get_keyring_entry(KEYRING_ACTIVATION_SECRET_KEY)?;
    match entry.get_password() {
        Ok(value) => {
            let bytes =
                hex::decode(value).map_err(|e| AppError::Internal(anyhow::anyhow!("keyring decode failed: {e}")))?;
            Ok(Some(bytes))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Internal(anyhow::anyhow!("keyring read failed: {e}"))),
    }
}

fn set_activation_binding_secret(raw: &[u8]) -> AppResult<()> {
    let entry = get_keyring_entry(KEYRING_ACTIVATION_SECRET_KEY)?;
    entry
        .set_password(&hex::encode(raw))
        .map_err(|e| AppError::Internal(anyhow::anyhow!("keyring write failed: {e}")))?;
    Ok(())
}

fn get_or_initialize_activation_binding_secret() -> AppResult<Vec<u8>> {
    #[cfg(test)]
    {
        return Ok(vec![0x2A; 32]);
    }

    #[cfg(not(test))]
    {
    if let Some(secret) = get_activation_binding_secret()? {
        return Ok(secret);
    }
    use rand_core::{OsRng, RngCore};
    let mut secret = [0_u8; 32];
    OsRng.fill_bytes(&mut secret);
    set_activation_binding_secret(&secret)?;
    Ok(secret.to_vec())
    }
}

fn derive_anchor_hash(secret: &[u8], anchor_name: &str, anchor_value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret);
    hasher.update(b":");
    hasher.update(anchor_name.as_bytes());
    hasher.update(b":");
    hasher.update(anchor_value.as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) fn current_anchor_hashes() -> AppResult<BTreeMap<String, String>> {
    let secret = get_or_initialize_activation_binding_secret()?;
    let machine_id = read_machine_id_or_fallback();
    let hostname = hostname::get()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown-host".to_string());
    let os_type = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    Ok(BTreeMap::from([
        (
            "machine_id".to_string(),
            derive_anchor_hash(&secret, "machine_id", &machine_id),
        ),
        (
            "hostname".to_string(),
            derive_anchor_hash(&secret, "hostname", &hostname),
        ),
        ("os".to_string(), derive_anchor_hash(&secret, "os", &os_type)),
        ("arch".to_string(), derive_anchor_hash(&secret, "arch", &arch)),
    ]))
}

fn drift_score(expected: &BTreeMap<String, String>, current: &BTreeMap<String, String>) -> i64 {
    expected
        .iter()
        .map(|(k, v)| {
            let same = current.get(k) == Some(v);
            if same { 0_i64 } else { 1_i64 }
        })
        .sum()
}

fn activation_payload_hash(input: &ApplyMachineActivationInput) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.contract_id.as_bytes());
    hasher.update(b"|");
    hasher.update(input.machine_id.as_bytes());
    hasher.update(b"|");
    hasher.update(input.slot_assignment_id.as_bytes());
    hasher.update(b"|");
    hasher.update(input.response_nonce.as_bytes());
    hasher.update(b"|");
    hasher.update(input.anchor_hashes_json.as_bytes());
    hasher.update(b"|");
    hasher.update(input.policy_snapshot_json.as_bytes());
    hex::encode(hasher.finalize())
}

async fn write_lineage_event(
    db: &impl ConnectionTrait,
    event_code: &str,
    contract_id: Option<&str>,
    slot_assignment_id: Option<&str>,
    detail_json: serde_json::Value,
    actor_user_id: Option<i64>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO machine_activation_lineage (
            id, event_code, contract_id, slot_assignment_id, detail_json, occurred_at, actor_user_id
         ) VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)",
        [
            Uuid::new_v4().to_string().into(),
            event_code.to_string().into(),
            contract_id.map(ToOwned::to_owned).into(),
            slot_assignment_id.map(ToOwned::to_owned).into(),
            detail_json.to_string().into(),
            actor_user_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

fn to_status_row(row: sea_orm::QueryResult) -> AppResult<MachineActivationStatus> {
    Ok(MachineActivationStatus {
        contract_id: row.try_get("", "contract_id").map_err(|e| decode_err("contract_id", e))?,
        machine_id: row.try_get("", "machine_id").map_err(|e| decode_err("machine_id", e))?,
        slot_assignment_id: row
            .try_get("", "slot_assignment_id")
            .map_err(|e| decode_err("slot_assignment_id", e))?,
        slot_number: row
            .try_get("", "slot_number")
            .map_err(|e| decode_err("slot_number", e))?,
        slot_limit: row.try_get("", "slot_limit").map_err(|e| decode_err("slot_limit", e))?,
        trust_score: row
            .try_get("", "trust_score")
            .map_err(|e| decode_err("trust_score", e))?,
        revocation_state: row
            .try_get("", "revocation_state")
            .map_err(|e| decode_err("revocation_state", e))?,
        issued_at: row.try_get("", "issued_at").map_err(|e| decode_err("issued_at", e))?,
        expires_at: row.try_get("", "expires_at").map_err(|e| decode_err("expires_at", e))?,
        offline_grace_until: row
            .try_get("", "offline_grace_until")
            .map_err(|e| decode_err("offline_grace_until", e))?,
        drift_score: 0,
        drift_within_tolerance: true,
        denial_code: None,
        denial_message: None,
    })
}

pub async fn apply_machine_activation(
    db: &DatabaseConnection,
    input: ApplyMachineActivationInput,
    actor_user_id: Option<i64>,
) -> AppResult<MachineActivationApplyResult> {
    validate_activation_input(&input)?;
    if let Err(err) = register_api_exchange(
        db,
        "vps.activation",
        "machine_activation_apply",
        &input.contract_id,
        None,
        Some(&input.response_nonce),
        &input.issued_at,
        &activation_payload_hash(&input),
        None,
        Some(&input.contract_id),
    )
    .await
    {
        if let AppError::LicenseDenied { reason_code, .. } = &err {
            if reason_code == "replay_detected" {
                return Ok(MachineActivationApplyResult {
                    contract_id: input.contract_id,
                    trusted_binding: true,
                    drift_score: 0,
                    slot_assignment_consistent: true,
                    replay_rejected: true,
                });
            }
        }
        return Err(err);
    }
    let expected = serde_json::from_str::<BTreeMap<String, String>>(&input.anchor_hashes_json)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Invalid anchor_hashes_json: {e}")]))?;
    let current = current_anchor_hashes()?;
    let policy = canonical_policy_snapshot(&input)?;
    let drift = drift_score(&expected, &current);
    let trusted_binding = drift <= policy.fingerprint_max_drift;
    if !trusted_binding {
        return Err(AppError::ValidationFailed(vec![format!(
            "Fingerprint drift score {drift} exceeds tolerance {}.",
            policy.fingerprint_max_drift
        )]));
    }

    let tx = db.begin().await?;
    let replay_exists: i64 = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM machine_activation_contracts WHERE response_nonce = ?",
            [input.response_nonce.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed replay check".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("replay_count", e))?;
    if replay_exists > 0 {
        return Ok(MachineActivationApplyResult {
            contract_id: input.contract_id,
            trusted_binding: true,
            drift_score: drift,
            slot_assignment_consistent: true,
            replay_rejected: true,
        });
    }

    let slot_conflict: i64 = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count
             FROM machine_activation_contracts
             WHERE slot_assignment_id = ?
               AND machine_id <> ?
               AND revocation_state <> 'revoked'",
            [
                input.slot_assignment_id.clone().into(),
                input.machine_id.clone().into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed slot consistency check".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("slot_conflict", e))?;
    if slot_conflict > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Slot assignment conflict detected against existing active machine contract.".to_string(),
        ]));
    }

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO machine_activation_contracts (
            contract_id, machine_id, device_fingerprint, slot_assignment_id, slot_number, slot_limit,
            trust_score, vps_version, response_nonce, issued_at, expires_at, offline_grace_until,
            revocation_state, revocation_reason, anchor_hashes_json, policy_snapshot_json, created_at, row_version
         ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1
         )",
        [
            input.contract_id.clone().into(),
            input.machine_id.clone().into(),
            crate::auth::device::derive_device_fingerprint()
                .unwrap_or_else(|_| "unknown-fingerprint".to_string())
                .into(),
            input.slot_assignment_id.clone().into(),
            input.slot_number.into(),
            input.slot_limit.into(),
            input.trust_score.into(),
            input.vps_version.into(),
            input.response_nonce.clone().into(),
            input.issued_at.clone().into(),
            input.expires_at.clone().into(),
            input.offline_grace_until.clone().into(),
            input.revocation_state.clone().into(),
            input.revocation_reason.clone().into(),
            input.anchor_hashes_json.clone().into(),
            input.policy_snapshot_json.clone().into(),
        ],
    ))
    .await?;

    let inserted_id: i64 = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM machine_activation_contracts WHERE contract_id = ?",
            [input.contract_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed inserted activation id lookup".to_string()))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO machine_activation_state (
            id, active_contract_id, last_reconnect_at, last_revocation_applied_at, last_offline_check_at,
            last_offline_denial_code, last_offline_denial_message, updated_at
         ) VALUES (
            1, ?, NULL, NULL, NULL, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now')
         )
         ON CONFLICT(id) DO UPDATE SET
            active_contract_id = excluded.active_contract_id,
            updated_at = excluded.updated_at",
        [inserted_id.into()],
    ))
    .await?;

    write_lineage_event(
        &tx,
        "activation.applied",
        Some(&input.contract_id),
        Some(&input.slot_assignment_id),
        serde_json::json!({
            "drift_score": drift,
            "slot_number": input.slot_number,
            "slot_limit": input.slot_limit,
            "revocation_state": input.revocation_state
        }),
        actor_user_id,
    )
    .await?;
    tx.commit().await?;

    audit::emit(
        db,
        audit::AuditEvent {
            event_type: "activation.applied",
            actor_id: actor_user_id.map(|v| v as i32),
            entity_type: Some("machine_activation_contract"),
            entity_id: Some(&input.contract_id),
            summary: "Machine activation contract applied",
            detail_json: Some(
                serde_json::json!({
                    "slot_assignment_id": input.slot_assignment_id,
                    "slot_number": input.slot_number,
                    "slot_limit": input.slot_limit
                })
                .to_string(),
            ),
            ..Default::default()
        },
    )
    .await;
    let _ = append_license_trace(
        db,
        LicenseTraceInput {
            correlation_id: input.contract_id.clone(),
            event_type: "activation.contract_applied".to_string(),
            source: "activation_runtime".to_string(),
            subject_type: "machine_activation_contract".to_string(),
            subject_id: Some(input.contract_id.clone()),
            reason_code: None,
            outcome: "accepted".to_string(),
            payload_json: serde_json::json!({
                "slot_assignment_id": input.slot_assignment_id,
                "response_nonce": input.response_nonce,
                "revocation_state": input.revocation_state
            })
            .to_string(),
        },
    )
    .await;

    Ok(MachineActivationApplyResult {
        contract_id: input.contract_id,
        trusted_binding,
        drift_score: drift,
        slot_assignment_consistent: true,
        replay_rejected: false,
    })
}

pub async fn get_machine_activation_status(db: &DatabaseConnection) -> AppResult<MachineActivationStatus> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT c.contract_id, c.machine_id, c.slot_assignment_id, c.slot_number, c.slot_limit,
                    c.trust_score, COALESCE(c.revocation_state, 'not_activated') AS revocation_state,
                    c.issued_at, c.expires_at, c.offline_grace_until,
                    c.anchor_hashes_json, c.policy_snapshot_json
             FROM machine_activation_state s
             LEFT JOIN machine_activation_contracts c ON c.id = s.active_contract_id
             WHERE s.id = 1",
            [],
        ))
        .await?;
    if let Some(row) = row {
        let mut status = to_status_row(row)?;
        if let Some(contract_id) = status.contract_id.clone() {
            let details = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT anchor_hashes_json, policy_snapshot_json
                     FROM machine_activation_contracts
                     WHERE contract_id = ?",
                    [contract_id.into()],
                ))
                .await?;
            if let Some(details) = details {
                let expected: BTreeMap<String, String> = serde_json::from_str(
                    &details
                        .try_get::<String>("", "anchor_hashes_json")
                        .map_err(|e| decode_err("anchor_hashes_json", e))?,
                )
                .unwrap_or_default();
                let policy: ActivationPolicySnapshot = serde_json::from_str(
                    &details
                        .try_get::<String>("", "policy_snapshot_json")
                        .map_err(|e| decode_err("policy_snapshot_json", e))?,
                )
                .unwrap_or(ActivationPolicySnapshot {
                    fingerprint_max_drift: 1,
                    grace_hours: 72,
                    offline_allowed_states: vec!["active".to_string(), "grace".to_string()],
                    reconnect_revocation_blocking: true,
                });
                let current = current_anchor_hashes().unwrap_or_default();
                status.drift_score = drift_score(&expected, &current);
                status.drift_within_tolerance = status.drift_score <= policy.fingerprint_max_drift;
            }
        }
        return Ok(status);
    }
    Ok(MachineActivationStatus {
        contract_id: None,
        machine_id: None,
        slot_assignment_id: None,
        slot_number: None,
        slot_limit: None,
        trust_score: None,
        revocation_state: "not_activated".to_string(),
        issued_at: None,
        expires_at: None,
        offline_grace_until: None,
        drift_score: 0,
        drift_within_tolerance: true,
        denial_code: None,
        denial_message: None,
    })
}

pub async fn evaluate_offline_activation_policy(
    db: &DatabaseConnection,
    user_id: i32,
    fingerprint: &str,
) -> AppResult<OfflineActivationDecision> {
    let trust = crate::auth::device::get_device_trust(db, user_id, fingerprint).await?;
    if trust.is_none() {
        return Ok(OfflineActivationDecision {
            allowed: false,
            denial_code: Some("bootstrap_required".to_string()),
            denial_message: Some(
                "Offline login denied: this device has no prior online bootstrap activation.".to_string(),
            ),
            requires_online_reconnect: true,
            grace_hours_remaining: None,
        });
    }
    if trust.as_ref().is_some_and(|t| t.is_revoked) {
        return Ok(OfflineActivationDecision {
            allowed: false,
            denial_code: Some("device_revoked".to_string()),
            denial_message: Some("Offline login denied: trusted device binding has been revoked.".to_string()),
            requires_online_reconnect: true,
            grace_hours_remaining: None,
        });
    }

    let status = get_machine_activation_status(db).await?;
    if status.contract_id.is_none() {
        return Ok(OfflineActivationDecision {
            allowed: false,
            denial_code: Some("activation_missing".to_string()),
            denial_message: Some("Offline login denied: machine activation contract is missing.".to_string()),
            requires_online_reconnect: true,
            grace_hours_remaining: None,
        });
    }
    if matches!(status.revocation_state.as_str(), "pending_revocation" | "revoked") {
        return Ok(OfflineActivationDecision {
            allowed: false,
            denial_code: Some("activation_revoked".to_string()),
            denial_message: Some("Offline login denied: machine activation has been revoked by policy.".to_string()),
            requires_online_reconnect: true,
            grace_hours_remaining: None,
        });
    }
    if !status.drift_within_tolerance {
        return Ok(OfflineActivationDecision {
            allowed: false,
            denial_code: Some("fingerprint_drift_exceeded".to_string()),
            denial_message: Some("Offline login denied: device fingerprint drift exceeds tolerance policy.".to_string()),
            requires_online_reconnect: true,
            grace_hours_remaining: None,
        });
    }

    let entitlement = entitlements::queries::get_entitlement_summary(db).await?;
    if !matches!(entitlement.effective_state.as_str(), "active" | "grace") {
        return Ok(OfflineActivationDecision {
            allowed: false,
            denial_code: Some("entitlement_state_blocked".to_string()),
            denial_message: Some(format!(
                "Offline login denied: entitlement state '{}' requires reconnect.",
                entitlement.effective_state
            )),
            requires_online_reconnect: true,
            grace_hours_remaining: None,
        });
    }

    let now = Utc::now();
    let grace_until = status
        .offline_grace_until
        .as_deref()
        .ok_or_else(|| AppError::ValidationFailed(vec!["offline_grace_until missing in activation state".to_string()]))?;
    let grace_until_dt = chrono::DateTime::parse_from_rfc3339(grace_until)
        .map_err(|e| AppError::ValidationFailed(vec![format!("Invalid offline_grace_until: {e}")]))?
        .with_timezone(&Utc);
    if now > grace_until_dt {
        return Ok(OfflineActivationDecision {
            allowed: false,
            denial_code: Some("grace_window_expired".to_string()),
            denial_message: Some(
                "Offline login denied: machine activation grace window expired; online reconnect required.".to_string(),
            ),
            requires_online_reconnect: true,
            grace_hours_remaining: Some(0),
        });
    }
    let remaining = (grace_until_dt - now).num_hours().max(0);

    Ok(OfflineActivationDecision {
        allowed: true,
        denial_code: None,
        denial_message: None,
        requires_online_reconnect: false,
        grace_hours_remaining: Some(remaining),
    })
}

pub async fn process_reconnect_revocation(db: &DatabaseConnection) -> AppResult<Option<String>> {
    let now = Utc::now().to_rfc3339();
    let active = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT c.id, c.contract_id, c.revocation_state, c.revocation_reason, c.slot_assignment_id
             FROM machine_activation_state s
             JOIN machine_activation_contracts c ON c.id = s.active_contract_id
             WHERE s.id = 1",
            [],
        ))
        .await?;
    let Some(active) = active else {
        return Ok(None);
    };
    let revocation_state: String = active
        .try_get("", "revocation_state")
        .map_err(|e| decode_err("revocation_state", e))?;
    if revocation_state != "pending_revocation" {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE machine_activation_state
             SET last_reconnect_at = ?, updated_at = ?
             WHERE id = 1",
            [now.clone().into(), now.into()],
        ))
        .await?;
        return Ok(None);
    }

    let contract_id: String = active
        .try_get("", "contract_id")
        .map_err(|e| decode_err("contract_id", e))?;
    let slot_assignment_id: String = active
        .try_get("", "slot_assignment_id")
        .map_err(|e| decode_err("slot_assignment_id", e))?;
    let reason: Option<String> = active
        .try_get("", "revocation_reason")
        .map_err(|e| decode_err("revocation_reason", e))?;

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE machine_activation_contracts
         SET revocation_state = 'revoked',
             row_version = row_version + 1
         WHERE contract_id = ?",
        [contract_id.clone().into()],
    ))
    .await?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE machine_activation_state
         SET last_reconnect_at = ?,
             last_revocation_applied_at = ?,
             last_offline_denial_code = 'activation_revoked',
             last_offline_denial_message = ?,
             updated_at = ?
         WHERE id = 1",
        [
            now.clone().into(),
            now.clone().into(),
            reason
                .clone()
                .unwrap_or_else(|| "Activation revoked on reconnect".to_string())
                .into(),
            now.into(),
        ],
    ))
    .await?;

    write_lineage_event(
        db,
        "activation.revocation_applied_on_reconnect",
        Some(&contract_id),
        Some(&slot_assignment_id),
        serde_json::json!({ "reason": reason }),
        None,
    )
    .await?;
    let _ = append_license_trace(
        db,
        LicenseTraceInput {
            correlation_id: contract_id.clone(),
            event_type: "activation.revocation_applied".to_string(),
            source: "activation_reconnect".to_string(),
            subject_type: "machine_activation_contract".to_string(),
            subject_id: Some(contract_id.clone()),
            reason_code: Some("activation_revoked".to_string()),
            outcome: "applied".to_string(),
            payload_json: serde_json::json!({ "reason": reason }).to_string(),
        },
    )
    .await;

    Ok(Some(
        reason.unwrap_or_else(|| "Machine activation revoked and applied during reconnect.".to_string()),
    ))
}

pub async fn store_activation_refresh_token(refresh_token: &str) -> AppResult<()> {
    let entry = get_keyring_entry(KEYRING_ACTIVATION_TOKEN_KEY)?;
    entry
        .set_password(refresh_token)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("keyring write failed: {e}")))?;
    Ok(())
}

pub async fn rotate_activation_binding_secret(
    db: &DatabaseConnection,
    input: RotateActivationSecretInput,
    actor_user_id: Option<i64>,
) -> AppResult<RotateActivationSecretResult> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "reason is required for activation secret rotation.".to_string(),
        ]));
    }
    use rand_core::{OsRng, RngCore};
    let mut secret = [0_u8; 32];
    OsRng.fill_bytes(&mut secret);
    set_activation_binding_secret(&secret)?;
    let now = Utc::now().to_rfc3339();

    write_lineage_event(
        db,
        "activation.secret_rotated",
        None,
        None,
        serde_json::json!({ "reason": input.reason }),
        actor_user_id,
    )
    .await?;
    audit::emit(
        db,
        audit::AuditEvent {
            event_type: "activation.secret_rotated",
            actor_id: actor_user_id.map(|v| v as i32),
            summary: "Machine activation binding secret rotated",
            detail_json: Some(serde_json::json!({ "reason": input.reason }).to_string()),
            ..Default::default()
        },
    )
    .await;
    let _ = append_license_trace(
        db,
        LicenseTraceInput {
            correlation_id: format!("activation-secret-rotation:{now}"),
            event_type: "activation.secret_rotated".to_string(),
            source: "activation_runtime".to_string(),
            subject_type: "activation_secret".to_string(),
            subject_id: None,
            reason_code: None,
            outcome: "applied".to_string(),
            payload_json: serde_json::json!({ "reason": input.reason }).to_string(),
        },
    )
    .await;
    Ok(RotateActivationSecretResult {
        rotated: true,
        rotated_at: now,
        reason: input.reason,
    })
}

pub async fn request_machine_rebind(
    db: &DatabaseConnection,
    input: RebindMachineActivationInput,
    actor_user_id: Option<i64>,
) -> AppResult<RebindMachineActivationResult> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "reason is required for machine activation rebind.".to_string(),
        ]));
    }
    let now = Utc::now().to_rfc3339();
    let previous_contract_id: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT c.contract_id
             FROM machine_activation_state s
             LEFT JOIN machine_activation_contracts c ON c.id = s.active_contract_id
             WHERE s.id = 1",
            [],
        ))
        .await?
        .and_then(|row| row.try_get::<Option<String>>("", "contract_id").ok())
        .flatten();

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE machine_activation_state
         SET active_contract_id = NULL,
             last_offline_denial_code = 'rebind_required',
             last_offline_denial_message = ?,
             updated_at = ?
         WHERE id = 1",
        [
            format!("Machine activation rebind requested: {}", input.reason).into(),
            now.clone().into(),
        ],
    ))
    .await?;
    write_lineage_event(
        db,
        "activation.rebind_requested",
        previous_contract_id.as_deref(),
        None,
        serde_json::json!({ "reason": input.reason }),
        actor_user_id,
    )
    .await?;
    audit::emit(
        db,
        audit::AuditEvent {
            event_type: "activation.rebind_requested",
            actor_id: actor_user_id.map(|v| v as i32),
            entity_type: Some("machine_activation_contract"),
            entity_id: previous_contract_id.as_deref(),
            summary: "Machine activation rebind requested",
            detail_json: Some(serde_json::json!({ "reason": input.reason }).to_string()),
            ..Default::default()
        },
    )
    .await;
    let _ = append_license_trace(
        db,
        LicenseTraceInput {
            correlation_id: previous_contract_id
                .clone()
                .unwrap_or_else(|| format!("activation-rebind:{now}")),
            event_type: "activation.rebind_requested".to_string(),
            source: "activation_runtime".to_string(),
            subject_type: "machine_activation_contract".to_string(),
            subject_id: previous_contract_id.clone(),
            reason_code: Some("rebind_required".to_string()),
            outcome: "requested".to_string(),
            payload_json: serde_json::json!({ "reason": input.reason }).to_string(),
        },
    )
    .await;

    Ok(RebindMachineActivationResult {
        previous_contract_id,
        rebind_required: true,
        rebind_requested_at: now,
        reason: input.reason,
    })
}

pub async fn get_machine_activation_diagnostics(
    db: &DatabaseConnection,
    limit: Option<i64>,
) -> AppResult<MachineActivationDiagnostics> {
    let page_size = limit.unwrap_or(25).clamp(1, 200);
    let status = get_machine_activation_status(db).await?;
    let state = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT last_reconnect_at, last_revocation_applied_at
             FROM machine_activation_state
             WHERE id = 1",
            [],
        ))
        .await?;
    let last_reconnect_at: Option<String> = state
        .as_ref()
        .map(|r| r.try_get::<Option<String>>("", "last_reconnect_at"))
        .transpose()
        .map_err(|e| decode_err("last_reconnect_at", e))?
        .flatten();
    let last_revocation_applied_at: Option<String> = state
        .as_ref()
        .map(|r| r.try_get::<Option<String>>("", "last_revocation_applied_at"))
        .transpose()
        .map_err(|e| decode_err("last_revocation_applied_at", e))?
        .flatten();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, event_code, contract_id, slot_assignment_id, detail_json, occurred_at, actor_user_id
             FROM machine_activation_lineage
             ORDER BY occurred_at DESC
             LIMIT ?",
            [page_size.into()],
        ))
        .await?;
    let lineage = rows
        .into_iter()
        .map(|row| {
            Ok(ActivationLineageRecord {
                id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
                event_code: row
                    .try_get("", "event_code")
                    .map_err(|e| decode_err("event_code", e))?,
                contract_id: row
                    .try_get("", "contract_id")
                    .map_err(|e| decode_err("contract_id", e))?,
                slot_assignment_id: row
                    .try_get("", "slot_assignment_id")
                    .map_err(|e| decode_err("slot_assignment_id", e))?,
                detail_json: row
                    .try_get("", "detail_json")
                    .map_err(|e| decode_err("detail_json", e))?,
                occurred_at: row
                    .try_get("", "occurred_at")
                    .map_err(|e| decode_err("occurred_at", e))?,
                actor_user_id: row
                    .try_get("", "actor_user_id")
                    .map_err(|e| decode_err("actor_user_id", e))?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    Ok(MachineActivationDiagnostics {
        status,
        last_reconnect_at,
        last_revocation_applied_at,
        lineage,
        runbook_links: vec![
            "https://docs.maintafox.com/runbooks/activation/slot-assignment-consistency".to_string(),
            "https://docs.maintafox.com/runbooks/activation/offline-policy-denials".to_string(),
            "https://docs.maintafox.com/runbooks/activation/reconnect-revocation-application".to_string(),
        ],
    })
}
