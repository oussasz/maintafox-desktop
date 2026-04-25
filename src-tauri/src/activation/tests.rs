use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, Statement};
use sea_orm_migration::MigratorTrait;

use crate::activation::domain::{ActivationPolicySnapshot, ApplyMachineActivationInput};
use crate::activation::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
    seed_user_and_permissions(&db).await;
    db
}

async fn seed_user_and_permissions(db: &DatabaseConnection) {
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
}

fn activation_input(contract_id: &str, nonce: &str, slot_assignment_id: &str) -> ApplyMachineActivationInput {
    let anchors = queries::current_anchor_hashes().expect("anchors");
    let policy = ActivationPolicySnapshot {
        fingerprint_max_drift: 1,
        grace_hours: 72,
        offline_allowed_states: vec!["active".to_string(), "grace".to_string()],
        reconnect_revocation_blocking: true,
    };
    ApplyMachineActivationInput {
        contract_id: contract_id.to_string(),
        machine_id: "machine-alpha".to_string(),
        slot_assignment_id: slot_assignment_id.to_string(),
        slot_number: 1,
        slot_limit: 3,
        trust_score: 99,
        vps_version: 10,
        response_nonce: nonce.to_string(),
        issued_at: "2026-04-16T00:00:00Z".to_string(),
        expires_at: "2099-01-01T00:00:00Z".to_string(),
        offline_grace_until: "2099-01-03T00:00:00Z".to_string(),
        revocation_state: "active".to_string(),
        revocation_reason: None,
        anchor_hashes_json: serde_json::to_string(&anchors).expect("anchor json"),
        policy_snapshot_json: serde_json::to_string(&policy).expect("policy json"),
    }
}

#[tokio::test]
async fn activation_contract_applies_and_replay_nonce_is_idempotent() {
    let db = setup_db().await;
    let first = queries::apply_machine_activation(&db, activation_input("act-001", "nonce-001", "slot-001"), Some(1))
        .await
        .expect("apply");
    assert!(!first.replay_rejected);
    let second = queries::apply_machine_activation(&db, activation_input("act-001b", "nonce-001", "slot-001"), Some(1))
        .await
        .expect("replay");
    assert!(second.replay_rejected);
}

#[tokio::test]
async fn drift_policy_rejects_when_expected_hashes_do_not_match() {
    let db = setup_db().await;
    let mut input = activation_input("act-drift", "nonce-drift", "slot-drift");
    input.anchor_hashes_json = r#"{"machine_id":"bad","hostname":"bad","os":"bad","arch":"bad"}"#.to_string();
    let err = queries::apply_machine_activation(&db, input, Some(1))
        .await
        .expect_err("drift rejection");
    assert!(format!("{err}").contains("drift score"));
}

#[tokio::test]
async fn slot_conflict_detected_for_different_machine() {
    let db = setup_db().await;
    queries::apply_machine_activation(&db, activation_input("act-slot-1", "nonce-slot-1", "slot-collision"), Some(1))
        .await
        .expect("apply 1");
    let mut second = activation_input("act-slot-2", "nonce-slot-2", "slot-collision");
    second.machine_id = "machine-beta".to_string();
    let err = queries::apply_machine_activation(&db, second, Some(1))
        .await
        .expect_err("slot conflict");
    assert!(format!("{err}").contains("Slot assignment conflict"));
}

#[tokio::test]
async fn offline_policy_denies_without_bootstrap_and_with_revocation() {
    let db = setup_db().await;
    let decision = queries::evaluate_offline_activation_policy(&db, 1, "fp-unknown")
        .await
        .expect("decision");
    assert!(!decision.allowed);
    assert_eq!(decision.denial_code.as_deref(), Some("bootstrap_required"));

    let fingerprint = crate::auth::device::derive_device_fingerprint().unwrap_or_else(|_| "unknown".to_string());
    crate::auth::device::register_device_trust(&db, 1, &fingerprint, None)
        .await
        .expect("register trust");
    queries::apply_machine_activation(&db, activation_input("act-offline", "nonce-offline", "slot-offline"), Some(1))
        .await
        .expect("activation");
    let trusted_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM trusted_devices WHERE user_id = 1 AND device_fingerprint = ?",
            [fingerprint.clone().into()],
        ))
        .await
        .expect("query trust row")
        .expect("row");
    let trusted_id: String = trusted_row.try_get("", "id").expect("id");
    crate::auth::device::revoke_device_trust(&db, &trusted_id, 1)
        .await
        .expect("revoke trust");
    let denied = queries::evaluate_offline_activation_policy(&db, 1, &fingerprint)
        .await
        .expect("denied decision");
    assert_eq!(denied.denial_code.as_deref(), Some("device_revoked"));
}

#[tokio::test]
async fn reconnect_applies_pending_revocation_immediately() {
    let db = setup_db().await;
    let mut input = activation_input("act-revoke", "nonce-revoke", "slot-revoke");
    input.revocation_state = "pending_revocation".to_string();
    input.revocation_reason = Some("vps revoked slot".to_string());
    queries::apply_machine_activation(&db, input, Some(1))
        .await
        .expect("activation");

    let reason = queries::process_reconnect_revocation(&db)
        .await
        .expect("reconnect")
        .expect("reason");
    assert!(reason.contains("revoked"));

    let status = queries::get_machine_activation_status(&db).await.expect("status");
    assert_eq!(status.revocation_state, "revoked");
}

#[tokio::test]
async fn rebind_request_clears_active_contract_and_sets_rebind_gate() {
    let db = setup_db().await;
    queries::apply_machine_activation(&db, activation_input("act-rebind", "nonce-rebind", "slot-rebind"), Some(1))
        .await
        .expect("activation");

    let result = queries::request_machine_rebind(
        &db,
        crate::activation::domain::RebindMachineActivationInput {
            reason: "hardware replaced".to_string(),
        },
        Some(1),
    )
    .await
    .expect("rebind");
    assert!(result.rebind_required);
    assert_eq!(result.previous_contract_id.as_deref(), Some("act-rebind"));

    let status = queries::get_machine_activation_status(&db).await.expect("status");
    assert!(status.contract_id.is_none());
}

#[tokio::test]
async fn activation_rejects_clock_skewed_or_stale_signed_timestamps() {
    let db = setup_db().await;
    let mut future = activation_input("act-skew-future", "nonce-skew-future", "slot-skew-future");
    future.issued_at = "2099-01-01T00:00:00Z".to_string();
    let future_err = queries::apply_machine_activation(&db, future, Some(1))
        .await
        .expect_err("future skew should fail");
    assert!(format!("{future_err}").contains("clock skew"));

    let mut stale = activation_input("act-skew-stale", "nonce-skew-stale", "slot-skew-stale");
    stale.issued_at = "2020-01-01T00:00:00Z".to_string();
    let stale_err = queries::apply_machine_activation(&db, stale, Some(1))
        .await
        .expect_err("stale signed_at should fail");
    assert!(format!("{stale_err}").contains("anti-replay window"));
}
