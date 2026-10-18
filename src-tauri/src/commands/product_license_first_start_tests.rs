//! The two first-start actions that run without a session (creating the first administrator,
//! applying a licence reconciliation) are refused once the plant has an administrator, except a
//! refresh of the licence stored here, or a licence change while this PC has no usable licence.

use sea_orm::DatabaseConnection;
use serde_json::json;

use super::{
    apply_product_license_reconciliation_in, bootstrap_initial_tenant_admin_in, get_activation_claim_tenant_id,
    licence_caller, record_activation_claim_for_tests, submit_product_license_key_in, tenant_has_administrator_for_id,
    BootstrapInitialAdminInput, LicenceCaller, ProductLicenseActivationStatus, PRODUCT_LICENSE_ONBOARDING_KEY,
    PRODUCT_LICENSE_SCOPE,
};
use crate::errors::AppError;

const SIGNED_OUT: LicenceCaller = LicenceCaller {
    user_id: None,
    may_administer: false,
    step_up: false,
};
/// An administrator signed in with a fresh identity check.
const ADMIN: LicenceCaller = LicenceCaller {
    user_id: Some(1),
    may_administer: true,
    step_up: true,
};
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
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success(TENANT, "test-licence", json!({})))
        .await
        .expect("same licence");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);

    // Another tenant: refused, nothing stored.
    let other = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success("tenant-b", "lic-b", json!({})))
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
        SIGNED_OUT,
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
    apply_product_license_reconciliation_in(&db, ADMIN, &success("tenant-b", "lic-b", json!({})))
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
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denied)
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
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success(TENANT, "lic-new", json!({})))
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
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success("tenant-b", "lic-b", json!({})))
        .await
        .expect("first start");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some("tenant-b")
    );
}

/// A change of licence alone, or of machine alone, is a change of the activation: refused without
/// an administrator; an administrator must also confirm their identity (step-up).
#[tokio::test]
async fn a_licence_or_machine_change_needs_an_administrator_with_step_up() {
    let db = activated_plant_with_admin().await;
    let other_machine = json!({
        "kind": "success",
        "claim": {
            "tenant_id": TENANT,
            "license_id": "test-licence",
            "machine_fingerprint": "fp-other-pc",
            "activation_token": "token-3",
        },
    })
    .to_string();
    for outcome in [&success(TENANT, "lic-other", json!({})), &other_machine] {
        let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, outcome)
            .await
            .err()
            .expect("refused");
        assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
        let no_step_up = LicenceCaller {
            step_up: false,
            ..ADMIN
        };
        let err = apply_product_license_reconciliation_in(&db, no_step_up, outcome)
            .await
            .err()
            .expect("step-up required");
        assert!(matches!(err, AppError::StepUpRequired), "{err:?}");
    }
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );
}

/// Network and server errors only degrade the licence: recorded without a session. A denial or
/// network outcome that carries a claim never stores it.
#[tokio::test]
async fn errors_are_recorded_signed_out_and_never_carry_a_claim_in() {
    let db = activated_plant_with_admin().await;
    for kind in ["network_error", "http_error", "denied"] {
        let crafted = json!({
            "kind": kind,
            "error_code": "x",
            "error_message": "x",
            "claim": {
                "tenant_id": "tenant-hijack",
                "license_id": "lic-hijack",
                "machine_fingerprint": FINGERPRINT,
                "activation_token": "t",
                "has_demo_data": true,
            },
        })
        .to_string();
        apply_product_license_reconciliation_in(&db, SIGNED_OUT, &crafted)
            .await
            .expect("recorded");
        assert_eq!(
            get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
            Some(TENANT),
            "{kind}"
        );
    }
}

/// After a denial (the licence is unusable, nobody can sign in), a licence change is accepted
/// without a session, as key entry is; the claim stored before stays until then.
#[tokio::test]
async fn after_a_denial_a_licence_change_is_accepted_signed_out() {
    let db = activated_plant_with_admin().await;
    let denied = json!({ "kind": "denied", "error_code": "license_revoked", "error_message": "revoked" }).to_string();
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denied)
        .await
        .expect("denial");
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success(TENANT, "lic-renewed", json!({})))
        .await
        .expect("recovery");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
}

/// Who may administer is read from the roles: a signed-in user without `adm.settings` may not.
#[tokio::test]
async fn the_caller_right_comes_from_the_roles() {
    let db = activated_plant_with_admin().await;
    let admin_id: i32 = 1;
    let caller = licence_caller(&db, Some(admin_id), true).await.expect("caller");
    assert!(caller.may_administer && caller.step_up);
    sea_orm::ConnectionTrait::execute_unprepared(
        &db,
        "INSERT INTO user_accounts (id, entity_sync_id, username, display_name, identity_mode, is_active, is_admin,          force_password_change, created_at, updated_at) VALUES (77, 'user-sync-77', 'tech77', 'Tech', 'local', 1, 0, 0,          '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .await
    .expect("user");
    let caller = licence_caller(&db, Some(77), true).await.expect("caller");
    assert!(!caller.may_administer);
    let signed_out = licence_caller(&db, None, true).await.expect("caller");
    assert!(!signed_out.may_administer && !signed_out.step_up);
}

/// Key entry while the licence is unusable is open (recovery), but never adds demo data to a plant
/// that has an administrator.
#[tokio::test]
async fn key_entry_never_seeds_demo_data_on_an_administered_plant() {
    let db = activated_plant_with_admin().await;
    let denied = json!({ "kind": "denied", "error_code": "license_revoked", "error_message": "revoked" }).to_string();
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denied)
        .await
        .expect("denial");
    let demo_claim = json!({
        "tenant_id": TENANT,
        "license_id": "lic-demo",
        "machine_fingerprint": FINGERPRINT,
        "activation_token": "token-demo",
        "has_demo_data": true,
        "is_initialized": false,
    })
    .to_string();
    let err = submit_product_license_key_in(
        &db,
        SIGNED_OUT,
        "MFX-KEY-DEMO-0001",
        Some(demo_claim.clone()),
        None,
        None,
    )
    .await
    .err()
    .expect("refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    // Without demo data, the recovery key is accepted signed out.
    let plain = demo_claim.replace("\"has_demo_data\":true", "\"has_demo_data\":false");
    submit_product_license_key_in(&db, SIGNED_OUT, "MFX-KEY-PLAIN-0001", Some(plain), None, None)
        .await
        .expect("recovery key");
}
