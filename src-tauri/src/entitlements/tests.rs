use sea_orm::{ConnectionTrait, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::entitlements::domain::{EntitlementEnvelopeInput, ENTITLEMENT_SIGNATURE_ALG};
use crate::entitlements::queries;
use crate::entitlements::test_support::{now_rfc3339, sign_envelope};
use crate::entitlements::trusted_keys::TEST_ENTITLEMENT_KEY_ID;
use crate::errors::AppError;

async fn setup_db() -> DatabaseConnection {
    let db = crate::db::test_db::connect_in_memory().await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    crate::license::security::ensure_default_licensing_trust_keys(&db)
        .await
        .expect("seed trusted signing keys (as startup.rs does)");
    db
}

fn base_envelope(envelope_id: &str, lineage_version: i64, previous: Option<&str>) -> EntitlementEnvelopeInput {
    let mut input = EntitlementEnvelopeInput {
        envelope_id: envelope_id.to_string(),
        previous_envelope_id: previous.map(|v| v.to_string()),
        lineage_version,
        issuer: "maintafox-vps".to_string(),
        key_id: TEST_ENTITLEMENT_KEY_ID.to_string(),
        signature_alg: ENTITLEMENT_SIGNATURE_ALG.to_string(),
        tier: "enterprise".to_string(),
        state: "active".to_string(),
        channel: "stable".to_string(),
        machine_slots: 5,
        feature_flags_json: r#"{}"#.to_string(),
        capabilities_json:
            r#"{"equipment":true,"inventory":true,"finance":true,"planning":true,"sync":true,"pm":true,"personnel":true}"#
                .to_string(),
        policy_json: r#"{"grace_allowed_modules":["sync","equipment","inventory"]}"#.to_string(),
        // Signed "now": the verifier enforces a 24h anti-replay window on issued_at, so a fixed
        // date turned these tests into time bombs once it was older than a day.
        issued_at: now_rfc3339(),
        valid_from: now_rfc3339(),
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
    crate::license::security::mark_key_compromised(&db, "maintafox-vps", TEST_ENTITLEMENT_KEY_ID, "incident drill")
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

/// Cross-language contract: `testdata/ed25519_vector.json` is produced by the control plane's
/// real signer (`maintafox-vendor-console/api/scripts/entitlement-ed25519-vector.mjs`).
/// Regenerate it with that script whenever the canonical payload changes on either side.
#[test]
fn server_signed_vector_verifies_and_canonical_payload_matches_byte_for_byte() {
    use crate::entitlements::trusted_keys::{verify_entitlement_signature, SignatureCheck};

    let vector: serde_json::Value =
        serde_json::from_str(include_str!("testdata/ed25519_vector.json")).expect("vector json");
    assert_eq!(
        vector["public_key_hex"].as_str(),
        Some(crate::entitlements::test_support::test_public_key_hex().as_str()),
        "server and desktop must derive the same TEST-ONLY key"
    );
    let envelope: EntitlementEnvelopeInput =
        serde_json::from_value(vector["envelope"].clone()).expect("vector envelope");

    let canonical = queries::canonical_payload(&envelope);
    assert_eq!(
        canonical,
        vector["canonical_payload"].as_str().expect("canonical"),
        "Rust canonical_payload must equal Node canonicalPayload byte-for-byte"
    );
    assert_eq!(
        verify_entitlement_signature(
            &envelope.issuer,
            &envelope.key_id,
            canonical.as_bytes(),
            &envelope.signature
        ),
        SignatureCheck::Verified
    );

    let mut tampered = envelope.clone();
    tampered.machine_slots += 1;
    assert_eq!(
        verify_entitlement_signature(
            &tampered.issuer,
            &tampered.key_id,
            queries::canonical_payload(&tampered).as_bytes(),
            &tampered.signature
        ),
        SignatureCheck::InvalidSignature
    );
}
