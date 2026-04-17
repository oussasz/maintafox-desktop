use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;
use sha2::{Digest, Sha256};

use crate::activation::domain::{ActivationPolicySnapshot, ApplyMachineActivationInput};
use crate::entitlements::domain::{EntitlementEnvelopeInput, ENTITLEMENT_SIGNATURE_ALG_V1};
use crate::license::domain::ApplyAdminLicenseActionInput;
use crate::license::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO user_accounts
            (id, sync_id, username, display_name, identity_mode, password_hash, is_active, is_admin, force_password_change,
             failed_login_attempts, created_at, updated_at)
         VALUES
            (1, 'sync-user-1', 'tester', 'Tester', 'local', 'dummy', 1, 1, 0, 0, strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("seed user");
    db
}

fn sign_envelope(input: &EntitlementEnvelopeInput) -> String {
    let payload = serde_json::json!({
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
    .to_string();
    let mut payload_hasher = Sha256::new();
    payload_hasher.update(payload);
    let payload_hash = hex::encode(payload_hasher.finalize());
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "{}:{}:{}:{}",
        input.issuer, input.key_id, payload_hash, "MAINTAFOX_TRUSTED_ISSUER_KEY_V1"
    ));
    hex::encode(hasher.finalize())
}

async fn seed_entitlement_and_activation(db: &DatabaseConnection) {
    let mut ent = EntitlementEnvelopeInput {
        envelope_id: "env-license-1".to_string(),
        previous_envelope_id: None,
        lineage_version: 1,
        issuer: "maintafox-vps".to_string(),
        key_id: "key-v1".to_string(),
        signature_alg: ENTITLEMENT_SIGNATURE_ALG_V1.to_string(),
        tier: "enterprise".to_string(),
        state: "active".to_string(),
        channel: "stable".to_string(),
        machine_slots: 4,
        feature_flags_json: r#"{"sync_panel":true}"#.to_string(),
        capabilities_json:
            r#"{"inventory.write":true,"finance.write":true,"planning.write":true,"sync.runtime":true}"#
                .to_string(),
        policy_json: r#"{"grace_allowed_capabilities":["sync.runtime","core.read"]}"#.to_string(),
        issued_at: "2026-04-16T00:00:00Z".to_string(),
        valid_from: "2026-04-16T00:00:00Z".to_string(),
        valid_until: "2099-01-01T00:00:00Z".to_string(),
        offline_grace_until: "2099-01-03T00:00:00Z".to_string(),
        signature: String::new(),
    };
    ent.signature = sign_envelope(&ent);
    crate::entitlements::queries::apply_entitlement_envelope(db, ent)
        .await
        .expect("apply entitlement");

    let anchors = crate::activation::queries::current_anchor_hashes().expect("anchors");
    crate::activation::queries::apply_machine_activation(
        db,
        ApplyMachineActivationInput {
            contract_id: "act-license-1".to_string(),
            machine_id: "machine-alpha".to_string(),
            slot_assignment_id: "slot-license-1".to_string(),
            slot_number: 1,
            slot_limit: 4,
            trust_score: 99,
            vps_version: 4,
            response_nonce: "nonce-license-1".to_string(),
            issued_at: "2026-04-16T00:00:00Z".to_string(),
            expires_at: "2099-01-01T00:00:00Z".to_string(),
            offline_grace_until: "2099-01-03T00:00:00Z".to_string(),
            revocation_state: "active".to_string(),
            revocation_reason: None,
            anchor_hashes_json: serde_json::to_string(&anchors).expect("anchors json"),
            policy_snapshot_json: serde_json::to_string(&ActivationPolicySnapshot {
                fingerprint_max_drift: 1,
                grace_hours: 72,
                offline_allowed_states: vec!["active".to_string(), "grace".to_string()],
                reconnect_revocation_blocking: true,
            })
            .expect("policy json"),
        },
        Some(1),
    )
    .await
    .expect("apply activation");

    let fingerprint = crate::auth::device::derive_device_fingerprint().unwrap_or_else(|_| "unknown".to_string());
    crate::auth::device::register_device_trust(db, 1, &fingerprint, None)
        .await
        .expect("trust register");
}

#[tokio::test]
async fn enforcement_matrix_blocks_write_and_allows_safe_read_when_suspended() {
    let db = setup_db().await;
    seed_entitlement_and_activation(&db).await;
    queries::apply_admin_license_action(
        &db,
        ApplyAdminLicenseActionInput {
            action: "suspend".to_string(),
            reason: "billing hold".to_string(),
            expected_entitlement_state: Some("active".to_string()),
            expected_activation_state: Some("active".to_string()),
        },
        Some(1),
    )
    .await
    .expect("suspend");

    let read_decision = queries::evaluate_permission_matrix(&db, 1, "inv.view")
        .await
        .expect("read decision");
    assert!(read_decision.allowed);
    assert!(read_decision.degraded_to_read_only);

    let write_decision = queries::evaluate_permission_matrix(&db, 1, "inv.manage")
        .await
        .expect("write decision");
    assert!(!write_decision.allowed);
    assert_eq!(
        write_decision.reason.as_ref().map(|r| r.code.as_str()),
        Some("policy_sync_pending")
    );
}

#[tokio::test]
async fn admin_action_reconciliation_is_atomic_and_auditable() {
    let db = setup_db().await;
    seed_entitlement_and_activation(&db).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO sync_outbox (idempotency_key, entity_type, entity_sync_id, operation, row_version, payload_json, payload_hash, status, origin_machine_id, created_at, updated_at)
         VALUES ('idem-license-1', 'work_order', 'wo-1', 'upsert', 1, '{\"x\":1}', 'hash-wo-1', 'pending', 'machine-alpha',
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [],
    ))
    .await
    .expect("seed pending write");

    let applied = queries::apply_admin_license_action(
        &db,
        ApplyAdminLicenseActionInput {
            action: "revoke".to_string(),
            reason: "security incident".to_string(),
            expected_entitlement_state: Some("active".to_string()),
            expected_activation_state: Some("active".to_string()),
        },
        Some(1),
    )
    .await
    .expect("revoke");
    assert_eq!(applied.entitlement_state_after, "revoked");
    assert_eq!(applied.activation_state_after, "revoked");
    assert!(applied.queued_local_writes);
    assert_eq!(applied.pending_local_writes, 1);

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT action, pending_local_writes FROM license_admin_actions WHERE id = ?",
            [applied.action_id.into()],
        ))
        .await
        .expect("query action row")
        .expect("row");
    let action: String = row.try_get("", "action").expect("action");
    let pending: i64 = row.try_get("", "pending_local_writes").expect("pending");
    assert_eq!(action, "revoke");
    assert_eq!(pending, 1);
}

#[tokio::test]
async fn reactivation_restores_write_path_and_recovery_status_message() {
    let db = setup_db().await;
    seed_entitlement_and_activation(&db).await;
    queries::apply_admin_license_action(
        &db,
        ApplyAdminLicenseActionInput {
            action: "suspend".to_string(),
            reason: "maintenance freeze".to_string(),
            expected_entitlement_state: Some("active".to_string()),
            expected_activation_state: Some("active".to_string()),
        },
        Some(1),
    )
    .await
    .expect("suspend");
    queries::apply_admin_license_action(
        &db,
        ApplyAdminLicenseActionInput {
            action: "reactivate".to_string(),
            reason: "issue resolved".to_string(),
            expected_entitlement_state: Some("suspended".to_string()),
            expected_activation_state: Some("pending_revocation".to_string()),
        },
        Some(1),
    )
    .await
    .expect("reactivate");

    let write_decision = queries::evaluate_permission_matrix(&db, 1, "inv.manage")
        .await
        .expect("write decision");
    assert!(write_decision.allowed);

    let status = queries::get_license_status_view(&db, 1).await.expect("status");
    assert_eq!(status.entitlement_state, "active");
    assert_eq!(status.activation_state, "active");
    assert!(status.actionable_message.contains("healthy"));
}

#[tokio::test]
async fn compromise_response_sets_sync_pending_and_forces_revocation() {
    let db = setup_db().await;
    seed_entitlement_and_activation(&db).await;
    let result = queries::apply_licensing_compromise_response(
        &db,
        crate::license::domain::ApplyLicensingCompromiseResponseInput {
            issuer: "maintafox-vps".to_string(),
            key_id: "key-v1".to_string(),
            reason: "security drill".to_string(),
            force_revocation: true,
        },
        Some(1),
    )
    .await
    .expect("compromise response");
    assert!(result.policy_sync_pending);
    assert!(result.forced_revocation);

    let status = queries::get_license_status_view(&db, 1).await.expect("status");
    assert!(status.policy_sync_pending);
    assert_eq!(status.entitlement_state, "revoked");
}

#[tokio::test]
async fn immutable_traces_link_admin_action_and_runtime_outcome() {
    let db = setup_db().await;
    seed_entitlement_and_activation(&db).await;
    let applied = queries::apply_admin_license_action(
        &db,
        ApplyAdminLicenseActionInput {
            action: "suspend".to_string(),
            reason: "trace test".to_string(),
            expected_entitlement_state: Some("active".to_string()),
            expected_activation_state: Some("active".to_string()),
        },
        Some(1),
    )
    .await
    .expect("apply");

    let traces = queries::list_license_trace_events(&db, Some(50), Some(applied.action_id.clone()))
        .await
        .expect("traces");
    assert!(!traces.is_empty());
    assert!(traces.iter().any(|t| t.event_type == "license.admin_action_reconciled"));
    let first = traces.first().expect("trace first");
    assert!(!first.event_hash.is_empty());
}
