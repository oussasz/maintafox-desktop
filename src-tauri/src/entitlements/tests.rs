use sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use sha2::{Digest, Sha256};

use crate::entitlements::domain::{EntitlementEnvelopeInput, ENTITLEMENT_SIGNATURE_ALG_V1};
use crate::entitlements::queries;
use crate::errors::AppError;

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    db
}

fn sign_envelope(input: &EntitlementEnvelopeInput) -> String {
    let previous = match &input.previous_envelope_id {
        Some(v) => serde_json::to_string(v).unwrap(),
        None => "null".to_string(),
    };
    let payload = format!(
        "{{\"envelope_id\":{},\"previous_envelope_id\":{},\"lineage_version\":{},\"issuer\":{},\"key_id\":{},\"signature_alg\":{},\"tier\":{},\"state\":{},\"channel\":{},\"machine_slots\":{},\"feature_flags_json\":{},\"capabilities_json\":{},\"policy_json\":{},\"issued_at\":{},\"valid_from\":{},\"valid_until\":{},\"offline_grace_until\":{}}}",
        serde_json::to_string(&input.envelope_id).unwrap(),
        previous,
        input.lineage_version,
        serde_json::to_string(&input.issuer).unwrap(),
        serde_json::to_string(&input.key_id).unwrap(),
        serde_json::to_string(&input.signature_alg).unwrap(),
        serde_json::to_string(&input.tier).unwrap(),
        serde_json::to_string(&input.state).unwrap(),
        serde_json::to_string(&input.channel).unwrap(),
        input.machine_slots,
        serde_json::to_string(&input.feature_flags_json).unwrap(),
        serde_json::to_string(&input.capabilities_json).unwrap(),
        serde_json::to_string(&input.policy_json).unwrap(),
        serde_json::to_string(&input.issued_at).unwrap(),
        serde_json::to_string(&input.valid_from).unwrap(),
        serde_json::to_string(&input.valid_until).unwrap(),
        serde_json::to_string(&input.offline_grace_until).unwrap(),
    );
    let mut payload_hasher = Sha256::new();
    payload_hasher.update(payload);
    let payload_hash = hex::encode(payload_hasher.finalize());
    let secret = match input.key_id.as_str() {
        "key-v1" => "MAINTAFOX_TRUSTED_ISSUER_KEY_V1",
        "key-rotated-v2" => "MAINTAFOX_TRUSTED_ISSUER_KEY_V2",
        _ => "UNKNOWN",
    };
    let mut hasher = Sha256::new();
    hasher.update(format!("{}:{}:{}:{}", input.issuer, input.key_id, payload_hash, secret));
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
        feature_flags_json: r#"{}"#.to_string(),
        capabilities_json:
            r#"{"equipment":true,"inventory":true,"finance":true,"planning":true,"sync":true,"pm":true,"personnel":true}"#
                .to_string(),
        policy_json: r#"{"grace_allowed_modules":["sync","equipment","inventory"]}"#.to_string(),
        issued_at: "2026-07-26T00:00:00Z".to_string(),
        valid_from: "2026-07-26T00:00:00Z".to_string(),
        valid_until: "2099-01-01T00:00:00Z".to_string(),
        offline_grace_until: "2099-01-03T00:00:00Z".to_string(),
        signature: String::new(),
    };
    input.signature = sign_envelope(&input);
    input
}

#[tokio::test]
async fn soft_lineage_refresh_supersedes_without_growing_rows() {
    let db = setup_db().await;
    let env1 = base_envelope("env-soft-1", 1, None);
    queries::apply_entitlement_envelope(&db, env1)
        .await
        .expect("apply soft v1");

    let mut env2 = base_envelope("env-soft-2", 1, None);
    env2.signature = sign_envelope(&env2);
    queries::apply_entitlement_envelope(&db, env2)
        .await
        .expect("apply soft refresh");

    let count: i64 = db
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM entitlement_envelopes",
            [],
        ))
        .await
        .expect("count query")
        .expect("count row")
        .try_get("", "count")
        .expect("count");
    assert_eq!(count, 1, "soft-lineage refresh must supersede in place");

    let summary = queries::get_entitlement_summary(&db).await.expect("summary");
    assert_eq!(summary.envelope_id.as_deref(), Some("env-soft-2"));
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

    queries::enforce_capability_for_permission(&db, crate::rbac::permissions::INV_MANAGE)
        .await
        .expect("inventory capability allowed");
    queries::enforce_capability_for_permission(&db, crate::rbac::permissions::INV_VIEW)
        .await
        .expect("inventory view maps to same module");
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
    queries::apply_entitlement_envelope(&db, env1).await.expect("apply v1");
    queries::enforce_capability_for_permission(&db, crate::rbac::permissions::FIN_MANAGE)
        .await
        .expect("finance write allowed in v1");

    let mut env2 = base_envelope("env-101", 2, Some("env-100"));
    env2.state = "suspended".to_string();
    env2.capabilities_json =
        r#"{"equipment":true,"inventory":true,"finance":false,"planning":true,"sync":true}"#.to_string();
    env2.signature = sign_envelope(&env2);
    queries::apply_entitlement_envelope(&db, env2)
        .await
        .expect("apply v2 suspended");

    let blocked = queries::enforce_capability_for_permission(&db, crate::rbac::permissions::FIN_MANAGE)
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
    queries::apply_entitlement_envelope(&db, env1).await.expect("apply v1");
    let env2 = base_envelope("env-diag-2", 2, Some("env-diag-1"));
    queries::apply_entitlement_envelope(&db, env2).await.expect("apply v2");

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
    queries::apply_entitlement_envelope(&db, env1).await.expect("apply v1");

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
