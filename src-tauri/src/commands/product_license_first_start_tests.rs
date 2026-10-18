//! The two first-start actions that run without a session (creating the first administrator,
//! applying a licence reconciliation) are refused once the plant has an administrator, except a
//! refresh of the licence stored here, or a licence change while this PC has no usable licence.

use sea_orm::DatabaseConnection;
use serde_json::json;

use super::{
    apply_product_license_reconciliation_in, bootstrap_initial_tenant_admin_in, get_activation_claim_tenant_id,
    record_activation_claim_for_tests, tenant_has_administrator_for_id, BootstrapInitialAdminInput,
    ProductLicenseActivationStatus, PRODUCT_LICENSE_ONBOARDING_KEY, PRODUCT_LICENSE_SCOPE,
};
use crate::errors::AppError;

const TENANT: &str = "tenant-plant-a";
const FINGERPRINT: &str = "fp-pc-1";

fn code_of(err: &AppError) -> Option<&str> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.as_str()),
        _ => None,
    }
}

fn admin_input(username: &str) -> BootstrapInitialAdminInput {
    BootstrapInitialAdminInput {
        username: username.into(),
        email: format!("{username}@plant.example"),
        password: "Chef-Usine-2026!Sûr".into(),
        display_name: None,
    }
}

async fn activated_plant_with_admin() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, "token-1")
        .await
        .expect("claim");
    bootstrap_initial_tenant_admin_in(&db, admin_input("chef"))
        .await
        .expect("first administrator");
    db
}

fn success(tenant: &str, licence: &str, extra: serde_json::Value) -> String {
    let mut claim = json!({
        "tenant_id": tenant,
        "license_id": licence,
        "machine_fingerprint": FINGERPRINT,
        "activation_token": "token-2",
    });
    if let (Some(c), Some(e)) = (claim.as_object_mut(), extra.as_object()) {
        c.extend(e.clone());
    }
    json!({ "kind": "success", "claim": claim }).to_string()
}

#[tokio::test]
async fn the_first_administrator_is_created_once() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, "token-1")
        .await
        .expect("claim");
    assert!(!tenant_has_administrator_for_id(&db, TENANT).await.expect("check"));
    bootstrap_initial_tenant_admin_in(&db, admin_input("chef"))
        .await
        .expect("first administrator");
    assert!(tenant_has_administrator_for_id(&db, TENANT).await.expect("check"));

    let again = bootstrap_initial_tenant_admin_in(&db, admin_input("intrus"))
        .await
        .expect_err("an administrator exists");
    assert_eq!(code_of(&again), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{again:?}");
}

#[tokio::test]
async fn a_signed_out_reconciliation_only_refreshes_the_stored_licence() {
    let db = activated_plant_with_admin().await;

    // Refresh of the licence stored here: accepted without a session.
    let state = apply_product_license_reconciliation_in(&db, None, false, &success(TENANT, "test-licence", json!({})))
        .await
        .expect("same licence");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);

    // Another tenant: refused, nothing stored.
    let other = apply_product_license_reconciliation_in(&db, None, false, &success("tenant-b", "lic-b", json!({})))
        .await
        .err()
        .expect("tenant change refused");
    assert_eq!(code_of(&other), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{other:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );

    // Demo data on an administered plant: refused even for the same licence.
    let demo = apply_product_license_reconciliation_in(
        &db,
        None,
        false,
        &success(
            TENANT,
            "test-licence",
            json!({ "has_demo_data": true, "is_initialized": false }),
        ),
    )
    .await
    .err()
    .expect("demo seeding refused");
    assert_eq!(code_of(&demo), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{demo:?}");

    // A signed-in administrator may change the licence.
    apply_product_license_reconciliation_in(&db, Some(1), true, &success("tenant-b", "lic-b", json!({})))
        .await
        .expect("administrator");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some("tenant-b")
    );
}

#[tokio::test]
async fn a_denial_is_always_recorded_and_a_denied_pc_can_recover() {
    let db = activated_plant_with_admin().await;
    let denied = json!({
        "kind": "denied",
        "error_code": "license_revoked",
        "error_message": "revoked",
    })
    .to_string();
    let state = apply_product_license_reconciliation_in(&db, None, false, &denied)
        .await
        .expect("denial");
    assert_eq!(state.status, ProductLicenseActivationStatus::DeniedRevoked);

    // A new key entered offline at the gate (allowed while the licence is unusable) is stored
    // without a claim; nobody can sign in. Its reconciliation is accepted without a session.
    let pending = json!({
        "schema_version": 1,
        "key_sha256": "new-key",
        "submitted_at": "2026-10-18T08:00:00Z",
        "status": "pending_online_validation",
        "pending_online_validation": true,
        "activation_claim": null,
        "reconciliation": { "retry_attempt": 0 },
        "diagnostics": [],
    });
    crate::settings::set_setting(
        &db,
        PRODUCT_LICENSE_ONBOARDING_KEY,
        PRODUCT_LICENSE_SCOPE,
        &pending.to_string(),
        None,
        "offline key",
    )
    .await
    .expect("pending");
    let state = apply_product_license_reconciliation_in(&db, None, false, &success(TENANT, "lic-new", json!({})))
        .await
        .expect("recovery without a session");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
}

#[tokio::test]
async fn before_any_administrator_a_licence_change_needs_no_session() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, "token-1")
        .await
        .expect("claim");
    apply_product_license_reconciliation_in(&db, None, false, &success("tenant-b", "lic-b", json!({})))
        .await
        .expect("first start");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some("tenant-b")
    );
}
