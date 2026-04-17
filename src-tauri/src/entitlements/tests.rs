use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use sha2::{Digest, Sha256};

use crate::entitlements::domain::{EntitlementEnvelopeInput, ENTITLEMENT_SIGNATURE_ALG_V1};
use crate::errors::AppError;
use crate::entitlements::queries;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None)
        .await
        .expect("migrations");
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
    let secret = match input.key_id.as_str() {
        "key-v1" => "MAINTAFOX_TRUSTED_ISSUER_KEY_V1",
        "key-rotated-v2" => "MAINTAFOX_TRUSTED_ISSUER_KEY_V2",
        _ => "UNKNOWN",
    };
    let mut hasher = Sha256::new();
    hasher.update(format!(
        "{}:{}:{}:{}",
        input.issuer, input.key_id, payload_hash, secret
    ));
    hex::encode(hasher.finalize())
}

fn base_envelope(envelope_id: &str, lineage_version: i64, previous: Option<&str>) -> EntitlementEnvelopeInput {
    let mut input = EntitlementEnvelopeInput {
        envelope_id: envelope_id.to_string(),
        previous_envelope_id: previous.map(|v| v.to_string()),
        lineage_version,
        issuer: "maintafox-vps".to_string(),
        key_id: "key-v1".to_string(),
        signature_alg: ENTITLEMENT_SIGNATURE_ALG_V1.to_string(),
        tier: "enterprise".to_string(),
        state: "active".to_string(),
        channel: "stable".to_string(),
        machine_slots: 5,
        feature_flags_json: r#"{"sync_panel":true,"advanced_budget":true}"#.to_string(),
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
    input.signature = sign_envelope(&input);
    input
}

#[tokio::test]
async fn signed_envelope_persists_and_capability_check_is_enforced() {
    let db = setup_db().await;
    let envelope = base_envelope("env-001", 1, None);
    let applied = queries::apply_entitlement_envelope(&db, envelope)
        .await
        .expect("apply signed envelope");
    assert!(applied.verified);

    let summary = queries::get_entitlement_summary(&db).await.expect("summary");
    assert_eq!(summary.effective_state, "active");
    assert_eq!(summary.envelope_id.as_deref(), Some("env-001"));

    queries::enforce_capability_for_permission(&db, "inv.manage")
        .await
        .expect("inventory capability allowed");
}

#[tokio::test]
async fn malformed_signature_is_rejected_and_does_not_become_active() {
    let db = setup_db().await;
    let mut envelope = base_envelope("env-bad", 1, None);
    envelope.signature = "tampered-signature".to_string();
    let applied = queries::apply_entitlement_envelope(&db, envelope)
        .await
        .expect("store tampered envelope");
    assert!(!applied.verified);
    assert_eq!(applied.verification_result, "invalid_signature");

    let summary = queries::get_entitlement_summary(&db).await.expect("summary");
    // Legacy-safe fallback remains active when no valid envelope is available.
    assert_eq!(summary.effective_state, "active");
    assert!(summary.envelope_id.is_none());
}

#[tokio::test]
async fn state_transitions_and_mid_session_policy_refresh_are_consistent() {
    let db = setup_db().await;
    let env1 = base_envelope("env-100", 1, None);
    queries::apply_entitlement_envelope(&db, env1)
        .await
        .expect("apply v1");
    queries::enforce_capability_for_permission(&db, "fin.manage")
        .await
        .expect("finance write allowed in v1");

    let mut env2 = base_envelope("env-101", 2, Some("env-100"));
    env2.state = "suspended".to_string();
    env2.capabilities_json = r#"{"inventory.write":true,"finance.write":false,"planning.write":true,"sync.runtime":true}"#
        .to_string();
    env2.signature = sign_envelope(&env2);
    queries::apply_entitlement_envelope(&db, env2)
        .await
        .expect("apply v2 suspended");

    let blocked = queries::enforce_capability_for_permission(&db, "fin.manage")
        .await
        .expect_err("finance write must be blocked when suspended");
    assert!(format!("{blocked}").contains("Entitlement capability blocked"));

    let summary = queries::get_entitlement_summary(&db).await.expect("summary");
    assert_eq!(summary.state, "suspended");
    assert_eq!(summary.effective_state, "suspended");
}

#[tokio::test]
async fn entitlement_diagnostics_include_lineage_and_runbooks() {
    let db = setup_db().await;
    let env1 = base_envelope("env-diag-1", 1, None);
    queries::apply_entitlement_envelope(&db, env1)
        .await
        .expect("apply v1");
    let env2 = base_envelope("env-diag-2", 2, Some("env-diag-1"));
    queries::apply_entitlement_envelope(&db, env2)
        .await
        .expect("apply v2");

    let diagnostics = queries::get_entitlement_diagnostics(&db, Some(10))
        .await
        .expect("diagnostics");
    assert!(diagnostics.lineage.len() >= 2);
    assert!(!diagnostics.runbook_links.is_empty());
    assert_eq!(diagnostics.summary.envelope_id.as_deref(), Some("env-diag-2"));
}

#[tokio::test]
async fn lineage_requires_previous_reference_for_new_versions() {
    let db = setup_db().await;
    let env1 = base_envelope("env-lineage-1", 1, None);
    queries::apply_entitlement_envelope(&db, env1)
        .await
        .expect("apply v1");

    let mut broken = base_envelope("env-lineage-2", 2, Some("missing-parent"));
    broken.signature = sign_envelope(&broken);
    let err = queries::apply_entitlement_envelope(&db, broken)
        .await
        .expect_err("lineage should fail");
    assert!(format!("{err}").contains("previous_envelope_id"));
}

#[tokio::test]
async fn compromised_signing_key_blocks_entitlement_acceptance() {
    let db = setup_db().await;
    crate::license::security::mark_key_compromised(&db, "maintafox-vps", "key-v1", "incident drill")
        .await
        .expect("mark compromised");
    let envelope = base_envelope("env-compromised", 1, None);
    let err = queries::apply_entitlement_envelope(&db, envelope)
        .await
        .expect_err("compromised key must be rejected");
    match err {
        AppError::LicenseDenied { reason_code, .. } => assert_eq!(reason_code, "compromised_key"),
        other => panic!("expected compromised key denial, got: {other}"),
    }
}
