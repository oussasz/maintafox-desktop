use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use uuid::Uuid;

use crate::activation;
use crate::audit;
use crate::entitlements;
use crate::errors::{AppError, AppResult};
use crate::license::domain::{
    ApplyAdminLicenseActionInput, ApplyAdminLicenseActionResult, ApplyLicensingCompromiseResponseInput,
    ApplyLicensingCompromiseResponseResult, LicenseEnforcementDecision, LicenseRejectionReason, LicenseStatusView,
    LicenseTraceEvent,
};
use crate::license::security::{append_license_trace, mark_key_compromised, LicenseTraceInput};

fn capability_class_for_permission(permission: &str) -> &'static str {
    if permission.ends_with(".view")
        || permission.starts_with("audit.")
        || permission.starts_with("act.view")
        || permission.starts_with("ent.view")
        || permission.starts_with("sync.view")
    {
        "read"
    } else if permission.starts_with("act.") || permission.starts_with("ent.") {
        "activation"
    } else {
        "write"
    }
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::ValidationFailed(vec![format!("Failed to decode license field '{field}': {err}")])
}

async fn is_policy_sync_pending(db: &DatabaseConnection) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT policy_sync_pending FROM license_enforcement_state WHERE id = 1",
            [],
        ))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<Option<i64>>("", "policy_sync_pending").ok())
        .flatten()
        .is_some_and(|v| v == 1))
}

async fn pending_local_writes(db: &DatabaseConnection) -> AppResult<i64> {
    let count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'pending'",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to count pending local writes".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("pending_local_writes", e))?;
    Ok(count)
}

pub async fn evaluate_permission_matrix(
    db: &DatabaseConnection,
    user_id: i32,
    permission: &str,
) -> AppResult<LicenseEnforcementDecision> {
    let capability_class = capability_class_for_permission(permission).to_string();
    let entitlement = entitlements::queries::get_entitlement_summary(db).await?;
    let activation = activation::queries::get_machine_activation_status(db).await?;
    let fingerprint = crate::auth::device::derive_device_fingerprint().unwrap_or_else(|_| "unknown".to_string());
    let mut trust = crate::auth::device::get_device_trust(db, user_id, &fingerprint).await?;
    let mut trust_state = if trust.as_ref().is_some_and(|t| t.is_revoked) {
        "revoked"
    } else if trust.is_some() {
        "trusted"
    } else {
        "untrusted"
    };
    let policy_pending = is_policy_sync_pending(db).await?;

    let mut decision = LicenseEnforcementDecision {
        permission: permission.to_string(),
        capability_class: capability_class.clone(),
        allowed: true,
        degraded_to_read_only: false,
        reason: None,
        entitlement_state: entitlement.effective_state.clone(),
        activation_state: activation.revocation_state.clone(),
        trust_state: trust_state.to_string(),
    };

    if capability_class != "read" {
        if policy_pending {
            decision.allowed = false;
            decision.reason = Some(LicenseRejectionReason {
                code: "policy_sync_pending".to_string(),
                message: "License policy synchronization is pending; write operations are temporarily blocked."
                    .to_string(),
                source: "license_matrix".to_string(),
            });
            return Ok(decision);
        }
        // Self-heal for trusted-device bootstrap:
        // if online and no trust exists for this user+device, register now.
        if trust.is_none() && crate::auth::device::is_network_available() {
            crate::auth::device::register_device_trust(db, user_id, &fingerprint, None).await?;
            trust = crate::auth::device::get_device_trust(db, user_id, &fingerprint).await?;
            trust_state = if trust.as_ref().is_some_and(|t| t.is_revoked) {
                "revoked"
            } else if trust.is_some() {
                "trusted"
            } else {
                "untrusted"
            };
            decision.trust_state = trust_state.to_string();
        }
        if trust_state != "trusted" {
            decision.allowed = false;
            decision.reason = Some(LicenseRejectionReason {
                code: "trust_violation".to_string(),
                message: "Trusted-device verification failed; write operations require an online trusted binding."
                    .to_string(),
                source: "device_trust".to_string(),
            });
            return Ok(decision);
        }
        if matches!(activation.revocation_state.as_str(), "pending_revocation" | "revoked") {
            decision.allowed = false;
            decision.reason = Some(LicenseRejectionReason {
                code: "activation_violation".to_string(),
                message: "Machine activation is revoked or pending revocation; write operations are blocked."
                    .to_string(),
                source: "activation".to_string(),
            });
            return Ok(decision);
        }
    }

    match entitlement.effective_state.as_str() {
        "revoked" | "suspended" => {
            if capability_class == "read" {
                decision.allowed = true;
                decision.degraded_to_read_only = true;
            } else {
                decision.allowed = false;
                decision.reason = Some(LicenseRejectionReason {
                    code: "entitlement_violation".to_string(),
                    message: format!(
                        "Entitlement state '{}' blocks this operation.",
                        entitlement.effective_state
                    ),
                    source: "entitlement".to_string(),
                });
            }
        }
        "expired" => {
            if capability_class == "read" {
                decision.allowed = true;
                decision.degraded_to_read_only = true;
            } else {
                decision.allowed = false;
                decision.reason = Some(LicenseRejectionReason {
                    code: "entitlement_expired".to_string(),
                    message: "Entitlement is expired; operation requires reactivation.".to_string(),
                    source: "entitlement".to_string(),
                });
            }
        }
        _ => {}
    }

    Ok(decision)
}

pub async fn enforce_permission_matrix(
    db: &DatabaseConnection,
    user_id: i32,
    permission: &str,
) -> AppResult<()> {
    let decision = evaluate_permission_matrix(db, user_id, permission).await?;
    if !decision.allowed {
        let reason = decision.reason.unwrap_or(LicenseRejectionReason {
            code: "license_denied".to_string(),
            message: "Operation blocked by license policy.".to_string(),
            source: "license_matrix".to_string(),
        });
        return Err(AppError::LicenseDenied {
            reason_code: reason.code,
            message: reason.message,
        });
    }
    Ok(())
}

pub async fn get_license_status_view(db: &DatabaseConnection, user_id: i32) -> AppResult<LicenseStatusView> {
    let entitlement = entitlements::queries::get_entitlement_summary(db).await?;
    let activation = activation::queries::get_machine_activation_status(db).await?;
    let fingerprint = crate::auth::device::derive_device_fingerprint().unwrap_or_else(|_| "unknown".to_string());
    let trust = crate::auth::device::get_device_trust(db, user_id, &fingerprint).await?;
    let trust_state = if trust.as_ref().is_some_and(|t| t.is_revoked) {
        "revoked"
    } else if trust.is_some() {
        "trusted"
    } else {
        "untrusted"
    };
    let policy_sync_pending = is_policy_sync_pending(db).await?;
    let pending_writes = pending_local_writes(db).await?;

    let action_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT action, applied_at
             FROM license_admin_actions
             ORDER BY applied_at DESC
             LIMIT 1",
            [],
        ))
        .await?;
    let (last_admin_action, last_admin_action_at) = if let Some(row) = action_row {
        (
            row.try_get::<Option<String>>("", "action")
                .map_err(|e| decode_err("action", e))?,
            row.try_get::<Option<String>>("", "applied_at")
                .map_err(|e| decode_err("applied_at", e))?,
        )
    } else {
        (None, None)
    };

    let actionable_message = if policy_sync_pending {
        "Policy synchronization is pending. Retry policy refresh before write operations.".to_string()
    } else if entitlement.effective_state == "revoked" {
        "License is revoked. Contact your administrator to reactivate and refresh policy.".to_string()
    } else if entitlement.effective_state == "suspended" {
        "License is suspended. Read-only mode is active until admin reactivation.".to_string()
    } else if activation.revocation_state == "pending_revocation" {
        "Activation revocation is pending and will be enforced on reconnect.".to_string()
    } else if activation.revocation_state == "revoked" {
        "Machine activation is revoked. Rebind/reactivate this device with your admin.".to_string()
    } else if trust_state != "trusted" {
        "Device trust is not established. Complete online bootstrap on a trusted device.".to_string()
    } else {
        "License is healthy. Full operations are allowed.".to_string()
    };

    Ok(LicenseStatusView {
        entitlement_state: entitlement.effective_state,
        activation_state: activation.revocation_state,
        trust_state: trust_state.to_string(),
        policy_sync_pending,
        pending_local_writes: pending_writes,
        last_admin_action,
        last_admin_action_at,
        actionable_message,
        recovery_paths: vec![
            "retry_policy_refresh".to_string(),
            "open_support_bundle".to_string(),
            "contact_admin".to_string(),
            "request_reactivation_or_rebind".to_string(),
        ],
    })
}

pub async fn apply_admin_license_action(
    db: &DatabaseConnection,
    input: ApplyAdminLicenseActionInput,
    actor_user_id: Option<i64>,
) -> AppResult<ApplyAdminLicenseActionResult> {
    let action = input.action.trim().to_lowercase();
    if !matches!(action.as_str(), "suspend" | "revoke" | "reactivate") {
        return Err(AppError::ValidationFailed(vec![
            "action must be one of: suspend, revoke, reactivate.".to_string(),
        ]));
    }
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "reason is required for admin license action.".to_string(),
        ]));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let pending_writes = pending_local_writes(db).await?;
    let queued_local_writes = pending_writes > 0;

    let tx = db.begin().await?;
    let entitlement_state_before: String = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COALESCE(e.state, 'active') AS state
             FROM entitlement_cache_state s
             LEFT JOIN entitlement_envelopes e ON e.id = s.active_envelope_id
             WHERE s.id = 1",
            [],
        ))
        .await?
        .and_then(|row| row.try_get::<Option<String>>("", "state").ok())
        .flatten()
        .unwrap_or_else(|| "active".to_string());
    let activation_state_before: String = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COALESCE(c.revocation_state, 'not_activated') AS revocation_state
             FROM machine_activation_state s
             LEFT JOIN machine_activation_contracts c ON c.id = s.active_contract_id
             WHERE s.id = 1",
            [],
        ))
        .await?
        .and_then(|row| row.try_get::<Option<String>>("", "revocation_state").ok())
        .flatten()
        .unwrap_or_else(|| "not_activated".to_string());

    if let Some(expected) = input.expected_entitlement_state.as_deref() {
        if expected != entitlement_state_before {
            return Err(AppError::ValidationFailed(vec![format!(
                "Entitlement state mismatch: expected '{expected}', found '{}'.",
                entitlement_state_before
            )]));
        }
    }
    if let Some(expected) = input.expected_activation_state.as_deref() {
        if expected != activation_state_before {
            return Err(AppError::ValidationFailed(vec![format!(
                "Activation state mismatch: expected '{expected}', found '{}'.",
                activation_state_before
            )]));
        }
    }

    let (ent_state_after, act_state_after, policy_sync_pending) = match action.as_str() {
        "suspend" => ("suspended", "pending_revocation", 1_i64),
        "revoke" => ("revoked", "revoked", 1_i64),
        "reactivate" => ("active", "active", 0_i64),
        _ => unreachable!(),
    };

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE entitlement_envelopes
         SET state = ?, rowid = rowid
         WHERE id = (SELECT active_envelope_id FROM entitlement_cache_state WHERE id = 1)
           AND verification_result = 'verified'",
        [ent_state_after.into()],
    ))
    .await?;

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE machine_activation_contracts
         SET revocation_state = ?,
             revocation_reason = ?,
             row_version = row_version + 1
         WHERE id = (SELECT active_contract_id FROM machine_activation_state WHERE id = 1)",
        [act_state_after.into(), Some(input.reason.clone()).into()],
    ))
    .await?;

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO license_enforcement_state (
            id, policy_sync_pending, last_transition_at, last_transition_reason, updated_at
         ) VALUES (
            1, ?, ?, ?, ?
         )
         ON CONFLICT(id) DO UPDATE SET
            policy_sync_pending = excluded.policy_sync_pending,
            last_transition_at = excluded.last_transition_at,
            last_transition_reason = excluded.last_transition_reason,
            updated_at = excluded.updated_at",
        [
            policy_sync_pending.into(),
            now.clone().into(),
            input.reason.clone().into(),
            now.clone().into(),
        ],
    ))
    .await?;

    let action_id = Uuid::new_v4().to_string();
    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO license_admin_actions (
            id, action, reason, entitlement_state_before, entitlement_state_after,
            activation_state_before, activation_state_after, pending_local_writes, actor_user_id, applied_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            action_id.clone().into(),
            action.clone().into(),
            input.reason.clone().into(),
            entitlement_state_before.clone().into(),
            ent_state_after.to_string().into(),
            activation_state_before.clone().into(),
            act_state_after.to_string().into(),
            pending_writes.into(),
            actor_user_id.into(),
            now.clone().into(),
        ],
    ))
    .await?;

    tx.commit().await?;

    audit::emit(
        db,
        audit::AuditEvent {
            event_type: "license.admin_action_applied",
            actor_id: actor_user_id.map(|v| v as i32),
            entity_type: Some("license_admin_action"),
            entity_id: Some(&action_id),
            summary: "Admin license action reconciled to local enforcement state",
            detail_json: Some(
                serde_json::json!({
                    "action": action,
                    "pending_local_writes": pending_writes,
                    "queued_local_writes": queued_local_writes
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
            correlation_id: action_id.clone(),
            event_type: "license.admin_action_reconciled".to_string(),
            source: "license_runtime".to_string(),
            subject_type: "license_admin_action".to_string(),
            subject_id: Some(action_id.clone()),
            reason_code: Some(action.clone()),
            outcome: "applied".to_string(),
            payload_json: serde_json::json!({
                "entitlement_state_before": entitlement_state_before,
                "entitlement_state_after": ent_state_after,
                "activation_state_before": activation_state_before,
                "activation_state_after": act_state_after,
                "pending_local_writes": pending_writes
            })
            .to_string(),
        },
    )
    .await;

    Ok(ApplyAdminLicenseActionResult {
        action_id,
        action,
        applied_at: now,
        entitlement_state_after: ent_state_after.to_string(),
        activation_state_after: act_state_after.to_string(),
        pending_local_writes: pending_writes,
        queued_local_writes,
    })
}

pub async fn list_license_trace_events(
    db: &DatabaseConnection,
    limit: Option<i64>,
    correlation_id: Option<String>,
) -> AppResult<Vec<LicenseTraceEvent>> {
    let page_size = limit.unwrap_or(100).clamp(1, 500);
    let rows = if let Some(correlation_id) = correlation_id {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, correlation_id, event_type, source, subject_type, subject_id, reason_code, outcome,
                    occurred_at, payload_hash, previous_hash, event_hash
             FROM license_event_traces
             WHERE correlation_id = ?
             ORDER BY occurred_at DESC, id DESC
             LIMIT ?",
            [correlation_id.into(), page_size.into()],
        ))
        .await?
    } else {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, correlation_id, event_type, source, subject_type, subject_id, reason_code, outcome,
                    occurred_at, payload_hash, previous_hash, event_hash
             FROM license_event_traces
             ORDER BY occurred_at DESC, id DESC
             LIMIT ?",
            [page_size.into()],
        ))
        .await?
    };
    rows.into_iter()
        .map(|row| {
            Ok(LicenseTraceEvent {
                id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
                correlation_id: row
                    .try_get("", "correlation_id")
                    .map_err(|e| decode_err("correlation_id", e))?,
                event_type: row
                    .try_get("", "event_type")
                    .map_err(|e| decode_err("event_type", e))?,
                source: row.try_get("", "source").map_err(|e| decode_err("source", e))?,
                subject_type: row
                    .try_get("", "subject_type")
                    .map_err(|e| decode_err("subject_type", e))?,
                subject_id: row
                    .try_get("", "subject_id")
                    .map_err(|e| decode_err("subject_id", e))?,
                reason_code: row
                    .try_get("", "reason_code")
                    .map_err(|e| decode_err("reason_code", e))?,
                outcome: row.try_get("", "outcome").map_err(|e| decode_err("outcome", e))?,
                occurred_at: row
                    .try_get("", "occurred_at")
                    .map_err(|e| decode_err("occurred_at", e))?,
                payload_hash: row
                    .try_get("", "payload_hash")
                    .map_err(|e| decode_err("payload_hash", e))?,
                previous_hash: row
                    .try_get("", "previous_hash")
                    .map_err(|e| decode_err("previous_hash", e))?,
                event_hash: row
                    .try_get("", "event_hash")
                    .map_err(|e| decode_err("event_hash", e))?,
            })
        })
        .collect()
}

pub async fn apply_licensing_compromise_response(
    db: &DatabaseConnection,
    input: ApplyLicensingCompromiseResponseInput,
    actor_user_id: Option<i64>,
) -> AppResult<ApplyLicensingCompromiseResponseResult> {
    mark_key_compromised(db, &input.issuer, &input.key_id, &input.reason).await?;
    if input.force_revocation {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE entitlement_envelopes
             SET state = 'revoked'
             WHERE id = (SELECT active_envelope_id FROM entitlement_cache_state WHERE id = 1)",
            [],
        ))
        .await?;
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE machine_activation_contracts
             SET revocation_state = 'revoked',
                 revocation_reason = ?
             WHERE id = (SELECT active_contract_id FROM machine_activation_state WHERE id = 1)",
            [Some(format!("compromise_response:{}", input.reason)).into()],
        ))
        .await?;
    }
    let now = chrono::Utc::now().to_rfc3339();
    let _ = append_license_trace(
        db,
        LicenseTraceInput {
            correlation_id: format!("compromise:{}:{}", input.issuer, input.key_id),
            event_type: "license.compromise_response_applied".to_string(),
            source: "license_security".to_string(),
            subject_type: "licensing_trust_key".to_string(),
            subject_id: Some(format!("{}:{}", input.issuer, input.key_id)),
            reason_code: Some("compromised_key".to_string()),
            outcome: "applied".to_string(),
            payload_json: serde_json::json!({
                "reason": input.reason,
                "force_revocation": input.force_revocation
            })
            .to_string(),
        },
    )
    .await;
    audit::emit(
        db,
        audit::AuditEvent {
            event_type: "license.compromise_response_applied",
            actor_id: actor_user_id.map(|v| v as i32),
            entity_type: Some("licensing_trust_key"),
            entity_id: Some(&format!("{}:{}", input.issuer, input.key_id)),
            summary: "Licensing compromise response applied",
            detail_json: Some(
                serde_json::json!({
                    "reason": input.reason,
                    "force_revocation": input.force_revocation
                })
                .to_string(),
            ),
            ..Default::default()
        },
    )
    .await;
    Ok(ApplyLicensingCompromiseResponseResult {
        issuer: input.issuer,
        key_id: input.key_id,
        policy_sync_pending: true,
        forced_revocation: input.force_revocation,
        applied_at: now,
    })
}
