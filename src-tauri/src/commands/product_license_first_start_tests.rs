//! The first-start actions that run without a session (creating the first administrator, entering
//! a licence key, applying a licence reconciliation) are refused once the plant has an
//! administrator, except a claim of the very activation stored here (same organisation, licence
//! and machine) that keeps or reduces its terms. Another organisation, licence or machine, and any
//! extension of the terms, need an administrator signed in with a fresh identity check.

use sea_orm::DatabaseConnection;
use serde_json::json;

use super::{
    apply_product_license_reconciliation_in, authorize_signed_out_reset, bootstrap_initial_tenant_admin_in,
    check_product_license_key_entry_in, get_activation_claim_tenant_id, licence_caller,
    record_activation_claim_for_tests, submit_product_license_key_in, tenant_has_administrator_for_id,
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
/// The activation token stored by `activated_plant_with_admin`.
const STORED_TOKEN: &str = "token-1";
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
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
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
        "activation_token": STORED_TOKEN,
    });
    if let (Some(c), Some(e)) = (claim.as_object_mut(), extra.as_object()) {
        c.extend(e.clone());
    }
    json!({ "kind": "success", "claim": claim }).to_string()
}

/// The claim a key entry for `tenant` brings (`submit_product_license_key`'s `claim_json`).
fn key_claim(tenant: &str, licence: &str) -> String {
    json!({
        "tenant_id": tenant,
        "license_id": licence,
        "machine_fingerprint": FINGERPRINT,
        "activation_token": "token-key",
    })
    .to_string()
}

const DENIED: &str = r#"{"kind":"denied","error_code":"license_revoked","error_message":"revoked"}"#;

/// Administrator roles scoped to a named tenant (the first administrator included).
async fn administrator_count(db: &DatabaseConnection) -> i64 {
    sea_orm::ConnectionTrait::query_one(
        db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT COUNT(1) AS c FROM user_scope_assignments usa
               JOIN roles r ON r.id = usa.role_id
              WHERE usa.scope_type = 'tenant' AND r.name IN ('Administrator', 'Superadmin')
                AND TRIM(COALESCE(usa.scope_reference, '')) <> ''"
                .to_string(),
        ),
    )
    .await
    .expect("count")
    .and_then(|row| row.try_get::<i64>("", "c").ok())
    .expect("row")
}

#[tokio::test]
async fn the_first_administrator_is_created_once() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, "token-1")
        .await
        .expect("claim");
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
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

/// Owner test of 0.9.0: the first administrator chose their password on the first-start screen,
/// so the sign-in that follows does not ask them to change it. The account is not marked for a
/// forced change and its password date is recorded, so the sign-in's expiry check does not read
/// a password never set (which would force the change anyway).
#[tokio::test]
async fn the_first_administrator_is_not_asked_to_change_the_password_they_chose() {
    let db = activated_plant_with_admin().await;
    let row = sea_orm::ConnectionTrait::query_one(
        &db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT id, force_password_change, password_changed_at FROM user_accounts WHERE username = 'chef'"
                .to_string(),
        ),
    )
    .await
    .expect("query")
    .expect("the first administrator");
    assert_eq!(row.try_get::<i32>("", "force_password_change").expect("flag"), 0);
    assert!(row
        .try_get::<Option<String>>("", "password_changed_at")
        .expect("date")
        .is_some());

    let user_id: i32 = row.try_get("", "id").expect("id");
    let policy = crate::auth::password_policy::PasswordPolicy::shipped();
    assert!(policy.max_age_days > 0, "the expiry check must really run");
    assert_eq!(
        crate::auth::password_policy::check_password_expiry(&db, user_id, &policy)
            .await
            .expect("expiry"),
        crate::auth::password_policy::PasswordExpiryStatus::Valid
    );
}

#[tokio::test]
async fn a_signed_out_reconciliation_only_refreshes_the_stored_licence() {
    let db = plant_with_terms(base_terms()).await;

    // Refresh of the licence stored here, same terms: accepted without a session.
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(base_terms()))
        .await
        .expect("same licence");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);

    // Another tenant: refused, nothing stored.
    let other = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success("tenant-b", "lic-b", base_terms()))
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
        &refresh(terms_with(json!({ "has_demo_data": true, "is_initialized": false }))),
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
async fn a_denial_is_always_recorded_and_a_pc_without_a_stored_claim_needs_an_administrator() {
    let db = activated_plant_with_admin().await;
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("denial");
    assert_eq!(state.status, ProductLicenseActivationStatus::DeniedRevoked);

    // A record without a claim (a key kept for online validation, written by an earlier version or
    // by an administrator): nothing to compare the organisation and the terms with, so its
    // reconciliation needs an administrator with step-up, for this organisation as for another.
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
    let other = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success("tenant-x", "lic-x", json!({})))
        .await
        .err()
        .expect("another organisation refused");
    assert_eq!(code_of(&other), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{other:?}");
    assert_eq!(get_activation_claim_tenant_id(&db).await.expect("tenant"), None);

    let same = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success(TENANT, "lic-new", json!({})))
        .await
        .err()
        .expect("no stored claim to compare with");
    assert_eq!(code_of(&same), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{same:?}");

    let state = apply_product_license_reconciliation_in(&db, ADMIN, &success(TENANT, "lic-new", json!({})))
        .await
        .expect("administrator with step-up");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );
}

/// On a release install (no built-in "admin" account: review E-1). A PC that still has a never-used
/// built-in account and a stored claim is limited as administered (`auth::builtin_admin_tests`, L3).
#[tokio::test]
async fn before_any_administrator_a_licence_change_needs_no_session() {
    let db = crate::db::test_db::migrated().await;
    crate::db::seeder::seed_system_data_for(&db, crate::auth::builtin_admin::BuildProfile::Release)
        .await
        .expect("release seed");
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

/// After a denial (which anyone can record signed out: the licence is unusable), nothing lifts it
/// without a session: another organisation, licence or machine needs an administrator with
/// step-up, and so does the very activation stored here with the same terms (a lifted denial is an
/// extension).
#[tokio::test]
async fn after_a_denial_nothing_is_lifted_signed_out() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("denial");

    let other = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success("tenant-x", "lic-x", base_terms()))
        .await
        .err()
        .expect("another organisation refused");
    assert_eq!(code_of(&other), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{other:?}");
    let no_step_up = LicenceCaller {
        step_up: false,
        ..ADMIN
    };
    let other = apply_product_license_reconciliation_in(&db, no_step_up, &success("tenant-x", "lic-x", base_terms()))
        .await
        .err()
        .expect("step-up required");
    assert!(matches!(other, AppError::StepUpRequired), "{other:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );

    // Another licence of the same organisation: a change of activation, refused signed out too.
    let licence =
        apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success(TENANT, "lic-renewed", base_terms()))
            .await
            .err()
            .expect("another licence refused");
    assert_eq!(
        code_of(&licence),
        Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
        "{licence:?}"
    );
    assert_eq!(stored_claim(&db).await.license_id, "test-licence");

    let lifted = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(base_terms()))
        .await
        .err()
        .expect("a denial is not lifted signed out");
    assert_eq!(code_of(&lifted), Some("LICENSE_EXTENSION_ADMIN_REQUIRED"), "{lifted:?}");
    assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::DeniedRevoked);
    let state = apply_product_license_reconciliation_in(&db, ADMIN, &refresh(base_terms()))
        .await
        .expect("an administrator with step-up lifts it");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
}

/// The review's chain, end to end: a denial recorded signed out, then another organisation brought
/// in signed out (by its check, a key, or a reconciliation), then a first administrator created
/// signed out. Every step after the denial is refused; the organisation, the administrators and the
/// first-administrator guard are unchanged. A key without a claim cannot empty the stored claim to
/// get around it.
#[tokio::test]
async fn a_signed_out_denial_never_lets_another_organisation_in() {
    let db = activated_plant_with_admin().await;
    let admins = administrator_count(&db).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("denial");

    let preview = json!({ "tenant_id": "tenant-x", "license_id": "lic-x", "machine_fingerprint": FINGERPRINT });
    let err = check_product_license_key_entry_in(&db, SIGNED_OUT, &preview.to_string())
        .await
        .expect_err("the check refuses another organisation");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &success("tenant-x", "lic-x", json!({})))
        .await
        .err()
        .expect("a reconciliation to another organisation refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");

    let err = submit_product_license_key_in(
        &db,
        SIGNED_OUT,
        "MFX-KEY-OTHER-0001",
        Some(key_claim("tenant-x", "lic-x")),
        None,
        None,
    )
    .await
    .expect_err("another organisation refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );
    assert!(!tenant_has_administrator_for_id(&db, "tenant-x").await.expect("check"));
    assert_eq!(administrator_count(&db).await, admins);
    let again = bootstrap_initial_tenant_admin_in(&db, admin_input("intrus"))
        .await
        .expect_err("an administrator exists");
    assert_eq!(code_of(&again), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{again:?}");

    // A key without a claim would forget the activation stored here: refused signed out too.
    let err = submit_product_license_key_in(&db, SIGNED_OUT, "MFX-KEY-OFFLINE-0001", None, None, None)
        .await
        .expect_err("a key without a claim refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );
    let err = submit_product_license_key_in(
        &db,
        SIGNED_OUT,
        "MFX-KEY-OTHER-0002",
        Some(key_claim("tenant-x", "lic-x")),
        None,
        None,
    )
    .await
    .expect_err("another organisation refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    assert!(!tenant_has_administrator_for_id(&db, "tenant-x").await.expect("check"));
    assert_eq!(administrator_count(&db).await, admins);
}

/// Key entry while the licence is usable needs an administrator with step-up, also to change the
/// organisation; with both, the organisation changes.
#[tokio::test]
async fn a_key_for_another_organisation_while_usable_needs_an_administrator_with_step_up() {
    let db = activated_plant_with_admin().await;
    let err = submit_product_license_key_in(
        &db,
        SIGNED_OUT,
        "MFX-KEY-OTHER-0003",
        Some(key_claim("tenant-x", "lic-x")),
        None,
        None,
    )
    .await
    .expect_err("signed out refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    let no_step_up = LicenceCaller {
        step_up: false,
        ..ADMIN
    };
    let err = submit_product_license_key_in(
        &db,
        no_step_up,
        "MFX-KEY-OTHER-0003",
        Some(key_claim("tenant-x", "lic-x")),
        None,
        None,
    )
    .await
    .expect_err("step-up required");
    assert!(matches!(err, AppError::StepUpRequired), "{err:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );

    submit_product_license_key_in(
        &db,
        ADMIN,
        "MFX-KEY-OTHER-0003",
        Some(key_claim("tenant-x", "lic-x")),
        None,
        None,
    )
    .await
    .expect("administrator with step-up");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some("tenant-x")
    );
}

fn outcome(result: &Result<(), AppError>) -> String {
    match result {
        Ok(()) => "accepted".into(),
        Err(AppError::StepUpRequired) => "step-up".into(),
        Err(err) => code_of(err).map_or_else(|| format!("{err:?}"), str::to_string),
    }
}

/// The check asked before a licence is taken on the server answers exactly as the key entry would
/// (one rule), and changes nothing.
#[tokio::test]
async fn the_key_entry_check_answers_as_the_key_entry() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("denial");
    let no_step_up = LicenceCaller {
        step_up: false,
        ..ADMIN
    };
    let with_ids = |tenant: &str, licence: &str, demo: bool, expires: &str, token: Option<&str>| {
        let mut value = terms_with(json!({
            "tenant_id": tenant,
            "license_id": licence,
            "machine_fingerprint": FINGERPRINT,
            "has_demo_data": demo,
            "is_initialized": false,
            "expires_at": expires,
            "tenant_display_name": "Plant",
        }));
        if let (Some(obj), Some(token)) = (value.as_object_mut(), token) {
            obj.insert("activation_token".into(), json!(token));
        }
        value.to_string()
    };
    const SAME: &str = "2027-06-30T00:00:00Z";
    const LATER: &str = "2029-06-30T00:00:00Z";
    // Refusals first (nothing stored), then the accepted case.
    let cases = [
        (
            SIGNED_OUT,
            "tenant-x",
            "lic-x",
            false,
            SAME,
            "LICENSE_RECONCILE_ADMIN_REQUIRED",
        ),
        (
            SIGNED_OUT,
            TENANT,
            "lic-other",
            false,
            SAME,
            "LICENSE_RECONCILE_ADMIN_REQUIRED",
        ),
        (
            SIGNED_OUT,
            TENANT,
            "test-licence",
            true,
            SAME,
            "LICENSE_RECONCILE_ADMIN_REQUIRED",
        ),
        (
            SIGNED_OUT,
            TENANT,
            "test-licence",
            false,
            LATER,
            "LICENSE_EXTENSION_ADMIN_REQUIRED",
        ),
        (no_step_up, "tenant-x", "lic-x", false, SAME, "step-up"),
        // The denial recorded above: the same terms would lift it.
        (
            SIGNED_OUT,
            TENANT,
            "test-licence",
            false,
            SAME,
            "LICENSE_EXTENSION_ADMIN_REQUIRED",
        ),
        (ADMIN, TENANT, "test-licence", false, SAME, "accepted"),
    ];
    for (caller, tenant, licence, demo, expires, expected) in cases {
        let checked =
            check_product_license_key_entry_in(&db, caller, &with_ids(tenant, licence, demo, expires, None)).await;
        assert_eq!(
            get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
            Some(TENANT),
            "the check changes nothing"
        );
        let claim = with_ids(tenant, licence, demo, expires, Some(STORED_TOKEN));
        let submitted = submit_product_license_key_in(&db, caller, "MFX-KEY-CHECK-0001", Some(claim), None, None).await;
        assert_eq!(outcome(&checked), expected, "{tenant} {licence} demo={demo} {expires}");
        assert_eq!(
            outcome(&submitted),
            expected,
            "{tenant} {licence} demo={demo} {expires}"
        );
    }
    let invalid = check_product_license_key_entry_in(&db, SIGNED_OUT, r#"{"license_id":"x"}"#)
        .await
        .expect_err("tenant, licence and machine are required");
    assert!(matches!(invalid, AppError::ValidationFailed(_)), "{invalid:?}");
}

/// Two first-administrator submissions at once on a database file (two connections, as two
/// windows would): exactly one administrator; the other gets the coded refusal, never a raw
/// database error.
#[tokio::test]
async fn two_first_administrators_at_once_make_one() {
    use sea_orm::ConnectionTrait;
    use sea_orm_migration::MigratorTrait;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plant.db");
    // Built on one connection, as the app opens it (migrations rely on it).
    let setup = crate::db::connect_sqlcipher(&path, None, true)
        .await
        .expect("database file");
    setup.execute_unprepared("PRAGMA journal_mode=WAL;").await.expect("wal");
    crate::migrations::Migrator::up(&setup, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&setup).await.expect("seed");
    record_activation_claim_for_tests(&setup, TENANT, FINGERPRINT, "token-1")
        .await
        .expect("claim");
    crate::sync::plant_join::confirm_empty_plant_for_tests(&setup, TENANT).await;
    setup.close().await.expect("close setup");

    // Two connections, so the two submissions really compete for the write lock.
    let opts = SqliteConnectOptions::new()
        .filename(&path)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .min_connections(2)
        .connect_with(opts)
        .await
        .expect("database file");
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

    let (a, b) = tokio::join!(
        bootstrap_initial_tenant_admin_in(&db, admin_input("chef-a")),
        bootstrap_initial_tenant_admin_in(&db, admin_input("chef-b")),
    );
    let refused: Vec<&AppError> = [&a, &b].into_iter().filter_map(|r| r.as_ref().err()).collect();
    assert_eq!(refused.len(), 1, "exactly one succeeds: {a:?} {b:?}");
    assert_eq!(
        code_of(refused[0]),
        Some("FIRST_ADMIN_ALREADY_EXISTS"),
        "{:?}",
        refused[0]
    );
    assert_eq!(administrator_count(&db).await, 1);
    db.close().await.expect("close");
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

/// Key entry while the licence is denied never adds demo data to a plant that has an
/// administrator, and the recovery key of the same activation lifts the denial, so it needs an
/// administrator with step-up too.
#[tokio::test]
async fn key_entry_never_seeds_demo_data_on_an_administered_plant() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("denial");
    let demo_claim = terms_with(json!({
        "tenant_id": TENANT,
        "license_id": "test-licence",
        "machine_fingerprint": FINGERPRINT,
        "activation_token": STORED_TOKEN,
        "has_demo_data": true,
        "is_initialized": false,
    }))
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
    .expect_err("refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    let plain = demo_claim.replace("\"has_demo_data\":true", "\"has_demo_data\":false");
    let err = submit_product_license_key_in(&db, SIGNED_OUT, "MFX-KEY-PLAIN-0001", Some(plain.clone()), None, None)
        .await
        .expect_err("the recovery key lifts the denial");
    assert_eq!(code_of(&err), Some("LICENSE_EXTENSION_ADMIN_REQUIRED"), "{err:?}");
    submit_product_license_key_in(&db, ADMIN, "MFX-KEY-PLAIN-0001", Some(plain), None, None)
        .await
        .expect("administrator with step-up");
    assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::Active);
}

// ── Licence terms are never extended signed out (S1) ───────────────────────────────────────────

/// The terms stored on the plant before each case: expiry, devices, edition and offline grace.
fn base_terms() -> serde_json::Value {
    json!({
        "expires_at": "2027-06-30T00:00:00Z",
        "device_limit": 5,
        "license_tier": "professional",
        "offline_grace_hours": 72,
    })
}

/// `base_terms` with `changes` applied (`null` removes a term).
fn terms_with(changes: serde_json::Value) -> serde_json::Value {
    let mut terms = base_terms();
    if let (Some(t), Some(c)) = (terms.as_object_mut(), changes.as_object()) {
        for (key, value) in c {
            if value.is_null() {
                t.remove(key);
            } else {
                t.insert(key.clone(), value.clone());
            }
        }
    }
    terms
}

/// A refresh of the activation stored here (same tenant, licence and machine) with `terms`.
fn refresh(terms: serde_json::Value) -> String {
    success(TENANT, "test-licence", terms)
}

/// A plant with an administrator whose stored licence carries `terms` (written by an administrator).
async fn plant_with_terms(terms: serde_json::Value) -> DatabaseConnection {
    let db = activated_plant_with_admin().await;
    apply_product_license_reconciliation_in(&db, ADMIN, &refresh(terms))
        .await
        .expect("administrator sets the terms");
    db
}

async fn stored_status(db: &DatabaseConnection) -> ProductLicenseActivationStatus {
    super::load_state_record_from_db(db)
        .await
        .expect("record")
        .expect("stored record")
        .status
}

async fn stored_claim(db: &DatabaseConnection) -> super::ProductActivationClaimRecord {
    super::get_activation_claim_record(db)
        .await
        .expect("claim")
        .expect("stored claim")
}

/// Refused signed out with the extension refusal and without step-up; nothing stored until an
/// administrator with step-up applies it.
async fn assert_extension_needs_admin(db: &DatabaseConnection, outcome: &str, case: &str) {
    let before = serde_json::to_value(stored_claim(db).await).expect("claim json");
    let err = apply_product_license_reconciliation_in(db, SIGNED_OUT, outcome)
        .await
        .err()
        .unwrap_or_else(|| panic!("{case}: extension accepted signed out"));
    assert_eq!(
        code_of(&err),
        Some("LICENSE_EXTENSION_ADMIN_REQUIRED"),
        "{case}: {err:?}"
    );
    let no_step_up = LicenceCaller {
        step_up: false,
        ..ADMIN
    };
    let err = apply_product_license_reconciliation_in(db, no_step_up, outcome)
        .await
        .err()
        .unwrap_or_else(|| panic!("{case}: extension accepted without step-up"));
    assert!(matches!(err, AppError::StepUpRequired), "{case}: {err:?}");
    assert_eq!(
        serde_json::to_value(stored_claim(db).await).expect("claim json"),
        before,
        "{case}: nothing stored"
    );
    apply_product_license_reconciliation_in(db, ADMIN, outcome)
        .await
        .unwrap_or_else(|e| panic!("{case}: administrator with step-up refused: {e:?}"));
}

#[tokio::test]
async fn a_signed_out_refresh_may_keep_the_terms() {
    let db = plant_with_terms(base_terms()).await;
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(base_terms()))
        .await
        .expect("same terms");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
}

#[tokio::test]
async fn a_signed_out_refresh_may_reduce_the_terms() {
    let db = plant_with_terms(base_terms()).await;
    let reduced = terms_with(json!({
        "expires_at": "2027-01-31T00:00:00Z",
        "device_limit": 3,
        "license_tier": "core",
        "offline_grace_hours": 24,
    }));
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(reduced))
        .await
        .expect("reduced terms");
    let claim = stored_claim(&db).await;
    assert_eq!(claim.expires_at.as_deref(), Some("2027-01-31T00:00:00Z"));
    assert_eq!(claim.device_limit, Some(3));
    assert_eq!(claim.license_tier.as_deref(), Some("core"));
    assert_eq!(claim.offline_grace_hours, Some(24));
}

#[tokio::test]
async fn extending_a_term_needs_an_administrator_with_step_up() {
    for (case, changes) in [
        ("later expiry", json!({ "expires_at": "2028-06-30T00:00:00Z" })),
        ("more devices", json!({ "device_limit": 6 })),
        ("higher edition", json!({ "license_tier": "enterprise" })),
        ("into development", json!({ "license_tier": "development" })),
        ("longer offline grace", json!({ "offline_grace_hours": 96 })),
        ("unknown incoming edition", json!({ "license_tier": "gold" })),
        ("edition missing on the incoming side", json!({ "license_tier": null })),
        (
            "device limit missing on the incoming side",
            json!({ "device_limit": null }),
        ),
        ("expiry missing on the incoming side", json!({ "expires_at": null })),
    ] {
        let db = plant_with_terms(base_terms()).await;
        assert_extension_needs_admin(&db, &refresh(terms_with(changes)), case).await;
    }
}

/// Fail-closed on the stored side too: an unknown stored edition, or a term the stored licence
/// does not state, cannot be compared, so any incoming value extends.
#[tokio::test]
async fn a_term_this_pc_cannot_compare_is_an_extension() {
    for (case, stored, incoming) in [
        (
            "unknown stored edition",
            terms_with(json!({ "license_tier": "gold" })),
            terms_with(json!({ "license_tier": "core" })),
        ),
        (
            "edition missing on the stored side",
            terms_with(json!({ "license_tier": null })),
            base_terms(),
        ),
        (
            "device limit missing on the stored side",
            terms_with(json!({ "device_limit": null })),
            base_terms(),
        ),
        (
            "expiry missing on the stored side",
            terms_with(json!({ "expires_at": null })),
            base_terms(),
        ),
    ] {
        let db = plant_with_terms(stored).await;
        assert_extension_needs_admin(&db, &refresh(incoming), case).await;
    }
}

/// Owner decision A: a term stated on neither side (stored and incoming) is "nothing changed",
/// accepted signed out; stated on one side only (stored only, or incoming only) it extends: refused
/// signed out, accepted for an administrator with step-up. For each term.
#[tokio::test]
async fn a_term_missing_on_both_sides_is_kept_and_on_one_side_only_extends() {
    for term in ["expires_at", "device_limit", "license_tier", "offline_grace_hours"] {
        let mut missing = serde_json::Map::new();
        missing.insert(term.to_string(), serde_json::Value::Null);
        let without = terms_with(serde_json::Value::Object(missing));

        let db = plant_with_terms(without.clone()).await;
        apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(without.clone()))
            .await
            .unwrap_or_else(|e| panic!("{term} missing on both sides: refused signed out: {e:?}"));

        let db = plant_with_terms(base_terms()).await;
        assert_extension_needs_admin(
            &db,
            &refresh(without.clone()),
            &format!("{term} stated on the stored side only"),
        )
        .await;

        let db = plant_with_terms(without).await;
        assert_extension_needs_admin(
            &db,
            &refresh(base_terms()),
            &format!("{term} stated on the incoming side only"),
        )
        .await;
    }
}

/// The policy a claim carries may only get stricter signed out: lifting a forced update, lowering
/// or removing a forced minimum version (or one this PC cannot read), lifting immediate
/// disconnection on trust revocation or the fresh-heartbeat rule, or moving to another update
/// channel needs an administrator with step-up.
#[tokio::test]
async fn loosening_the_licence_policy_needs_an_administrator_with_step_up() {
    let strict = || {
        terms_with(json!({
            "force_update_required": true,
            "force_min_app_version": "2.1.0",
            "trust_revocation_disconnects_immediately": true,
            "reconnect_requires_fresh_heartbeat": true,
            "update_channel": "stable",
        }))
    };
    let with = |changes: serde_json::Value| {
        let mut terms = strict();
        if let (Some(t), Some(c)) = (terms.as_object_mut(), changes.as_object()) {
            for (key, value) in c {
                if value.is_null() {
                    t.remove(key);
                } else {
                    t.insert(key.clone(), value.clone());
                }
            }
        }
        terms
    };
    for (case, changes) in [
        ("forced update lifted", json!({ "force_update_required": false })),
        ("forced update dropped", json!({ "force_update_required": null })),
        (
            "lower forced minimum version",
            json!({ "force_min_app_version": "2.0.9" }),
        ),
        (
            "forced minimum version removed",
            json!({ "force_min_app_version": null }),
        ),
        (
            "unreadable forced minimum version",
            json!({ "force_min_app_version": "latest" }),
        ),
        (
            "revocation no longer disconnects",
            json!({ "trust_revocation_disconnects_immediately": false }),
        ),
        (
            "reconnect without fresh heartbeat",
            json!({ "reconnect_requires_fresh_heartbeat": null }),
        ),
        ("another update channel", json!({ "update_channel": "beta" })),
    ] {
        let db = plant_with_terms(strict()).await;
        assert_extension_needs_admin(&db, &refresh(with(changes)), case).await;
    }

    // Stricter is accepted signed out: policies turned on, a higher forced minimum version.
    let db = plant_with_terms(terms_with(json!({ "update_channel": "stable" }))).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(strict()))
        .await
        .expect("stricter policy");
    apply_product_license_reconciliation_in(
        &db,
        SIGNED_OUT,
        &refresh(with(json!({ "force_min_app_version": "v2.10.0-rc1" }))),
    )
    .await
    .expect("a higher forced minimum version");
    assert_eq!(
        stored_claim(&db).await.force_min_app_version.as_deref(),
        Some("v2.10.0-rc1")
    );
}

/// A key entry is held to the same rule (recovery of the same organisation after a denial).
#[tokio::test]
async fn a_signed_out_recovery_key_never_extends_the_terms() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("denial");
    let mut extended = terms_with(json!({ "expires_at": "2030-01-01T00:00:00Z" }));
    if let Some(obj) = extended.as_object_mut() {
        obj.extend(
            json!({
                "tenant_id": TENANT,
                "license_id": "test-licence",
                "machine_fingerprint": FINGERPRINT,
                "activation_token": STORED_TOKEN,
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        );
    }
    let err = submit_product_license_key_in(
        &db,
        SIGNED_OUT,
        "MFX-KEY-RECOVER-0001",
        Some(extended.to_string()),
        None,
        None,
    )
    .await
    .expect_err("extension refused");
    assert_eq!(code_of(&err), Some("LICENSE_EXTENSION_ADMIN_REQUIRED"), "{err:?}");
    assert_eq!(
        stored_claim(&db).await.expires_at.as_deref(),
        Some("2027-06-30T00:00:00Z")
    );
}

/// Every edition the catalog lists (`SYSTEM_EDITION_IDS`) has a rank, and nothing else does.
#[test]
fn every_catalog_edition_is_ranked() {
    let catalog = include_str!("../../../src/lib/edition-capability-catalog.ts");
    let line = catalog
        .lines()
        .find(|l| l.contains("export const SYSTEM_EDITION_IDS"))
        .expect("SYSTEM_EDITION_IDS in the catalog");
    let list = &line[line.find('[').expect("[")..=line.find(']').expect("]")];
    let editions: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
    assert!(!editions.is_empty(), "{line}");
    for edition in &editions {
        assert!(super::edition_rank(edition).is_some(), "{edition} is not ranked");
    }
    assert_eq!(editions.len(), super::EDITION_ORDER.len(), "{editions:?}");
}

// ── Entitlement envelopes ──────────────────────────────────────────────────────────────────────

/// An envelope signed with the test key (trusted under `cfg(test)` only), issued `minutes_ago`.
fn signed_envelope(
    id: &str,
    minutes_ago: i64,
    tier: &str,
    machine_slots: i64,
) -> crate::entitlements::domain::EntitlementEnvelopeInput {
    let issued = (chrono::Utc::now() - chrono::Duration::minutes(minutes_ago))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    crate::entitlements::test_support::sign_with_test_key(crate::entitlements::domain::EntitlementEnvelopeInput {
        envelope_id: id.into(),
        previous_envelope_id: None,
        lineage_version: 1,
        issuer: String::new(),
        key_id: String::new(),
        signature_alg: String::new(),
        tier: tier.into(),
        state: "active".into(),
        channel: "stable".into(),
        machine_slots,
        feature_flags_json: "{}".into(),
        capabilities_json: r#"{"equipment":true,"planning":true}"#.into(),
        policy_json: "{}".into(),
        issued_at: issued.clone(),
        valid_from: issued,
        valid_until: "2027-06-30T00:00:00Z".into(),
        offline_grace_until: "2027-07-03T00:00:00Z".into(),
        signature: String::new(),
    })
}

fn refresh_with_envelope(
    terms: serde_json::Value,
    envelope: &crate::entitlements::domain::EntitlementEnvelopeInput,
) -> String {
    let mut outcome: serde_json::Value = serde_json::from_str(&refresh(terms)).expect("outcome");
    outcome["entitlement_envelope"] = serde_json::to_value(envelope).expect("envelope");
    outcome.to_string()
}

/// A plant whose stored licence and entitlements were set by an administrator.
async fn plant_with_envelope() -> DatabaseConnection {
    let db = activated_plant_with_admin().await;
    apply_product_license_reconciliation_in(
        &db,
        ADMIN,
        &refresh_with_envelope(base_terms(), &signed_envelope("env-0", 10, "professional", 5)),
    )
    .await
    .expect("administrator sets the entitlements");
    assert!(super::load_active_envelope(&db).await.expect("envelope").is_some());
    db
}

/// A newer signed envelope with the same terms (the control plane's refresh) is accepted signed out.
#[tokio::test]
async fn a_signed_out_refresh_with_a_newer_signed_envelope_of_the_same_terms_is_accepted() {
    let db = plant_with_envelope().await;
    apply_product_license_reconciliation_in(
        &db,
        SIGNED_OUT,
        &refresh_with_envelope(base_terms(), &signed_envelope("env-1", 0, "professional", 5)),
    )
    .await
    .expect("same terms");
}

/// A correctly signed but older envelope (replayed) needs an administrator.
#[tokio::test]
async fn a_replayed_older_signed_envelope_needs_an_administrator() {
    let db = plant_with_envelope().await;
    let outcome = refresh_with_envelope(base_terms(), &signed_envelope("env-old", 20, "professional", 5));
    assert_extension_needs_admin(&db, &outcome, "older envelope").await;
}

/// The envelope names no tenant, licence or device: a correctly signed envelope of another
/// organisation, with its larger entitlements, is caught by its terms and needs an administrator.
#[tokio::test]
async fn a_signed_envelope_of_another_organisation_needs_an_administrator() {
    let db = plant_with_envelope().await;
    let outcome = refresh_with_envelope(base_terms(), &signed_envelope("env-other", 0, "enterprise", 50));
    assert_extension_needs_admin(&db, &outcome, "envelope of another organisation").await;
}

/// The claim fields are not covered by the envelope signature: a signed envelope does not let an
/// extended claim expiry in.
#[tokio::test]
async fn a_signed_envelope_does_not_carry_an_extended_claim_expiry() {
    let db = plant_with_envelope().await;
    let outcome = refresh_with_envelope(
        terms_with(json!({ "expires_at": "2030-01-01T00:00:00Z" })),
        &signed_envelope("env-2", 0, "professional", 5),
    );
    assert_extension_needs_admin(&db, &outcome, "signed envelope with an extended claim expiry").await;
}

/// An envelope whose signature does not verify may only keep or reduce signed out: one with more
/// device slots needs an administrator; one with fewer is recorded but never becomes the envelope
/// in force.
#[tokio::test]
async fn an_unsigned_envelope_may_only_keep_or_reduce() {
    let db = plant_with_envelope().await;
    let mut forged = signed_envelope("env-forged", 0, "professional", 5);
    forged.machine_slots = 500;
    let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh_with_envelope(base_terms(), &forged))
        .await
        .err()
        .expect("forged envelope refused");
    assert_eq!(code_of(&err), Some("LICENSE_EXTENSION_ADMIN_REQUIRED"), "{err:?}");

    let mut unsigned = signed_envelope("env-unsigned", 0, "professional", 4);
    unsigned.signature = "00".into();
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh_with_envelope(base_terms(), &unsigned))
        .await
        .expect("an unsigned envelope that reduces");
    let active = super::load_active_envelope(&db)
        .await
        .expect("envelope")
        .expect("envelope in force");
    assert!(
        active.verified,
        "an unsigned envelope never becomes the envelope in force"
    );
    assert_eq!(active.machine_slots, 5);
}

/// An envelope whose validity starts earlier than the one in force widens it.
#[tokio::test]
async fn an_envelope_valid_from_earlier_needs_an_administrator() {
    let db = plant_with_envelope().await;
    let mut earlier = signed_envelope("env-earlier", 0, "professional", 5);
    earlier.valid_from = "2020-01-01T00:00:00Z".into();
    let earlier = crate::entitlements::test_support::sign_with_test_key(earlier);
    let outcome = refresh_with_envelope(base_terms(), &earlier);
    assert_extension_needs_admin(&db, &outcome, "envelope valid from earlier").await;
}

/// The first administrator is bound to a true first start: once this PC has an administrator,
/// none is created signed out, even after an administrator moved the plant to another organisation
/// (the new organisation has no administrator of its own yet).
#[tokio::test]
async fn no_first_administrator_once_the_plant_has_one_whatever_the_organisation() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, ADMIN, &success("tenant-b", "lic-b", base_terms()))
        .await
        .expect("administrator moves the plant");
    assert!(!tenant_has_administrator_for_id(&db, "tenant-b").await.expect("check"));
    let err = bootstrap_initial_tenant_admin_in(&db, admin_input("intrus"))
        .await
        .expect_err("the plant has an administrator");
    assert_eq!(code_of(&err), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{err:?}");
    assert_eq!(administrator_count(&db).await, 1);
}

/// The audit detail is real JSON whatever the account name holds (quotes, backslashes).
#[tokio::test]
async fn the_first_administrator_audit_detail_is_valid_json() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, "token-1")
        .await
        .expect("claim");
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
    let username = r#"chef "o'neil" \ x"#;
    bootstrap_initial_tenant_admin_in(&db, admin_input(username))
        .await
        .expect("first administrator");
    let row = sea_orm::ConnectionTrait::query_one(
        &db,
        sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT details_json FROM audit_events WHERE action_code = ? ORDER BY id DESC LIMIT 1",
            [crate::audit::event_type::USER_CREATED.into()],
        ),
    )
    .await
    .expect("query")
    .expect("audit row");
    let raw: String = row.try_get("", "details_json").expect("details");
    let details: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(details["detail"]["username"], json!(username), "{raw}");
    assert_eq!(details["detail"]["tenant_id"], json!(TENANT), "{raw}");
    assert_eq!(details["detail"]["bootstrap"], json!(true), "{raw}");
}

// ── Round 2: the activation token and identifiers, denials, envelopes, administrators ──────────

/// A device activation token as the control plane issues it (`signDeviceActivationToken`): only its
/// payload is read on this PC, the signature part is not checked (shared secret of the server).
fn device_token(tenant: &str, licence: &str, machine: &str, issued_at: i64, audience: &str) -> String {
    use base64::Engine as _;
    let b64 = |v: serde_json::Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    format!(
        "{}.{}.c2lnbmF0dXJl",
        b64(json!({ "alg": "HS256", "typ": "JWT" })),
        b64(json!({
            "kind": "activation",
            "tenant_id": tenant,
            "license_id": licence,
            "machine_fingerprint": machine,
            "iss": "maintafox-control-api",
            "aud": audience,
            "iat": issued_at,
            "exp": issued_at + 30 * 24 * 3600,
        }))
    )
}

const DEVICE_AUDIENCE: &str = "maintafox-desktop-device";

/// A refresh of the activation stored here, with the base terms and `claim` overriding fields.
fn refresh_with(claim: serde_json::Value) -> String {
    let mut terms = base_terms();
    if let (Some(t), Some(c)) = (terms.as_object_mut(), claim.as_object()) {
        t.extend(c.clone());
    }
    let mut outcome: serde_json::Value = serde_json::from_str(&refresh(json!({}))).expect("outcome");
    if let Some(stored) = outcome["claim"].as_object_mut() {
        stored.extend(terms.as_object().cloned().unwrap_or_default());
    }
    outcome.to_string()
}

/// An empty or whitespace-only token, or an identifier with surrounding spaces, is refused for
/// every caller and by every path (reconciliation, key entry, its check): nothing stored, and the
/// stored claim stays complete (login keeps working).
#[tokio::test]
async fn an_empty_token_or_an_untrimmed_identifier_is_refused_whatever_the_caller() {
    let db = plant_with_terms(base_terms()).await;
    let variants = [
        ("empty token", json!({ "activation_token": "" })),
        ("whitespace token", json!({ "activation_token": "   " })),
        ("token with a leading space", json!({ "activation_token": " token-1" })),
        (
            "tenant with a leading space",
            json!({ "tenant_id": format!(" {TENANT}") }),
        ),
        (
            "tenant with a trailing space",
            json!({ "tenant_id": format!("{TENANT} ") }),
        ),
        ("licence with a trailing tab", json!({ "license_id": "test-licence\t" })),
        (
            "machine with a leading space",
            json!({ "machine_fingerprint": format!(" {FINGERPRINT}") }),
        ),
        ("empty licence", json!({ "license_id": "" })),
    ];
    for caller in [SIGNED_OUT, ADMIN] {
        for (case, change) in &variants {
            let err = apply_product_license_reconciliation_in(&db, caller, &refresh_with(change.clone()))
                .await
                .err()
                .unwrap_or_else(|| panic!("{case}: accepted"));
            assert_eq!(code_of(&err), Some("LICENSE_CLAIM_INVALID"), "{case}: {err:?}");
            let mut key = serde_json::from_str::<serde_json::Value>(&refresh_with(change.clone())).expect("outcome")
                ["claim"]
                .clone();
            let err =
                submit_product_license_key_in(&db, caller, "MFX-KEY-CHECK-0002", Some(key.to_string()), None, None)
                    .await
                    .expect_err("key entry refused");
            assert_eq!(code_of(&err), Some("LICENSE_CLAIM_INVALID"), "{case} (key): {err:?}");
            if let Some(obj) = key.as_object_mut() {
                obj.remove("activation_token");
            }
            let checked = check_product_license_key_entry_in(&db, caller, &key.to_string()).await;
            if case.contains("token") {
                // The preview carries no token: the check answers on the rest.
                assert!(
                    !matches!(&checked, Err(e) if code_of(e) == Some("LICENSE_CLAIM_INVALID")),
                    "{case}: {checked:?}"
                );
            } else {
                let err = checked.expect_err("check refused");
                assert_eq!(code_of(&err), Some("LICENSE_CLAIM_INVALID"), "{case} (check): {err:?}");
            }
        }
    }
    let claim = stored_claim(&db).await;
    assert_eq!(claim.tenant_id, TENANT);
    assert_eq!(claim.activation_token, STORED_TOKEN);
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT),
        "the stored claim stays complete"
    );
}

/// A signed envelope of the base terms issued at `issued` (the control plane's clock in the answer).
fn envelope_issued_at(
    id: &str,
    issued: chrono::DateTime<chrono::Utc>,
) -> crate::entitlements::domain::EntitlementEnvelopeInput {
    let mut envelope = signed_envelope(id, 0, "professional", 5);
    let at = issued.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    envelope.issued_at = at.clone();
    envelope.valid_from = at;
    crate::entitlements::test_support::sign_with_test_key(envelope)
}

/// A refresh of the activation stored here bringing `token`, with `envelope` when given (the
/// control plane's answer always carries one).
fn token_refresh(token: &str, envelope: Option<&crate::entitlements::domain::EntitlementEnvelopeInput>) -> String {
    let mut outcome: serde_json::Value =
        serde_json::from_str(&refresh_with(json!({ "activation_token": token }))).expect("outcome");
    if let Some(envelope) = envelope {
        outcome["entitlement_envelope"] = serde_json::to_value(envelope).expect("envelope");
    }
    outcome.to_string()
}

/// The control plane rotates the device token on routine refresh: signed out, a new token is
/// accepted only when its payload names this very activation, it is not older than the stored one
/// and not dated past the control plane's clock in the same answer (the signed envelope's issue
/// instant); anything else needs an administrator with step-up.
#[tokio::test]
async fn a_signed_out_token_change_needs_a_token_of_this_activation() {
    let db = plant_with_envelope().await;
    let now = chrono::Utc::now();
    let fresh = |n: u32| envelope_issued_at(&format!("env-rot-{n}"), now);
    let first = device_token(TENANT, "test-licence", FINGERPRINT, 1_000, DEVICE_AUDIENCE);

    // The stored token ("token-1") cannot be read as a device token: no change signed out.
    let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &token_refresh(&first, Some(&fresh(1))))
        .await
        .err()
        .expect("unreadable stored token");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    apply_product_license_reconciliation_in(&db, ADMIN, &token_refresh(&first, Some(&fresh(2))))
        .await
        .expect("administrator with step-up");

    let rotated = device_token(TENANT, "test-licence", FINGERPRINT, 2_000, DEVICE_AUDIENCE);
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &token_refresh(&rotated, Some(&fresh(3))))
        .await
        .expect("a rotated token of this activation");
    assert_eq!(stored_claim(&db).await.activation_token, rotated);

    // Without a signed envelope the answer carries no server time to bound the token by.
    let later = device_token(TENANT, "test-licence", FINGERPRINT, 3_000, DEVICE_AUDIENCE);
    let mut unsigned = fresh(4);
    unsigned.signature = "00".into();
    for (case, envelope) in [
        ("a rotation without an envelope", None),
        ("a rotation with an unsigned envelope", Some(&unsigned)),
    ] {
        let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &token_refresh(&later, envelope))
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}: accepted signed out"));
        assert_eq!(
            code_of(&err),
            Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
            "{case}: {err:?}"
        );
        assert_eq!(
            stored_claim(&db).await.activation_token,
            rotated,
            "{case}: nothing stored"
        );
    }

    for (case, token) in [
        (
            "a token dated past the server time",
            device_token(
                TENANT,
                "test-licence",
                FINGERPRINT,
                now.timestamp() + super::DEVICE_TOKEN_AFTER_ENVELOPE_SECONDS + 60,
                DEVICE_AUDIENCE,
            ),
        ),
        ("an opaque token", "opaque-token-of-somewhere".to_string()),
        (
            "another organisation's token",
            device_token("tenant-x", "test-licence", FINGERPRINT, 3_000, DEVICE_AUDIENCE),
        ),
        (
            "another licence's token",
            device_token(TENANT, "lic-other", FINGERPRINT, 3_000, DEVICE_AUDIENCE),
        ),
        (
            "another machine's token",
            device_token(TENANT, "test-licence", "fp-other", 3_000, DEVICE_AUDIENCE),
        ),
        (
            "an older token",
            device_token(TENANT, "test-licence", FINGERPRINT, 1_500, DEVICE_AUDIENCE),
        ),
        (
            "a token for another audience",
            device_token(TENANT, "test-licence", FINGERPRINT, 3_000, "maintafox-vendor-admin"),
        ),
        ("a token with surrounding spaces", format!("{rotated} ")),
    ] {
        let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &token_refresh(&token, Some(&fresh(5))))
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}: accepted signed out"));
        assert!(
            matches!(
                code_of(&err),
                Some("LICENSE_RECONCILE_ADMIN_REQUIRED" | "LICENSE_CLAIM_INVALID")
            ),
            "{case}: {err:?}"
        );
        assert_eq!(
            stored_claim(&db).await.activation_token,
            rotated,
            "{case}: nothing stored"
        );
    }
    let opaque = json!({ "activation_token": "opaque-token-set-by-admin" });
    let no_step_up = LicenceCaller {
        step_up: false,
        ..ADMIN
    };
    let err = apply_product_license_reconciliation_in(&db, no_step_up, &refresh_with(opaque.clone()))
        .await
        .err()
        .expect("step-up required");
    assert!(matches!(err, AppError::StepUpRequired), "{err:?}");
    apply_product_license_reconciliation_in(&db, ADMIN, &refresh_with(opaque))
        .await
        .expect("administrator with step-up");
}

/// The lock-out the review found: a renewal (later expiry) arriving while the licence is denied, or
/// past its offline grace, is refused signed out and without step-up, and applied by an
/// administrator who confirmed their identity; the licence is then usable again.
#[tokio::test]
async fn an_administrator_with_step_up_applies_a_renewal_while_denied_or_past_grace() {
    let renewal = refresh(terms_with(json!({ "expires_at": "2028-06-30T00:00:00Z" })));
    let no_step_up = LicenceCaller {
        step_up: false,
        ..ADMIN
    };

    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(
        &db,
        SIGNED_OUT,
        r#"{"kind":"denied","error_code":"license_expired","error_message":"expired"}"#,
    )
    .await
    .expect("denial");
    assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::DeniedExpired);

    // Past the offline grace: the last contact is older than the licence's 72 hours.
    let grace = plant_with_terms(base_terms()).await;
    let mut record = super::load_state_record_from_db(&grace)
        .await
        .expect("record")
        .expect("record");
    record.reconciliation.last_success_at = Some("2020-01-01T00:00:00Z".into());
    record.last_reconciled_at = Some("2020-01-01T00:00:00Z".into());
    super::persist_state_record_in(&grace, None, &record, "test: no contact since 2020")
        .await
        .expect("persist");
    assert_eq!(
        super::effective_status(&record, chrono::Utc::now()),
        ProductLicenseActivationStatus::DeniedOfflineGraceExpired
    );

    for (case, db) in [("denied", &db), ("past offline grace", &grace)] {
        let err = apply_product_license_reconciliation_in(db, SIGNED_OUT, &renewal)
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}: renewal accepted signed out"));
        assert_eq!(
            code_of(&err),
            Some("LICENSE_EXTENSION_ADMIN_REQUIRED"),
            "{case}: {err:?}"
        );
        let err = apply_product_license_reconciliation_in(db, no_step_up, &renewal)
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}: renewal accepted without step-up"));
        assert!(matches!(err, AppError::StepUpRequired), "{case}: {err:?}");
        let state = apply_product_license_reconciliation_in(db, ADMIN, &renewal)
            .await
            .unwrap_or_else(|e| panic!("{case}: administrator with step-up refused: {e:?}"));
        assert_eq!(state.status, ProductLicenseActivationStatus::Active, "{case}");
        assert_eq!(
            stored_claim(db).await.expires_at.as_deref(),
            Some("2028-06-30T00:00:00Z"),
            "{case}"
        );
    }
}

/// A network or server error recorded signed out never turns a denial into the usable degraded
/// status; only the retry bookkeeping moves.
#[tokio::test]
async fn a_signed_out_network_error_never_lifts_a_denial() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("denial");
    for kind in ["network_error", "http_error"] {
        let outcome = json!({ "kind": kind, "error_code": "network_unreachable", "error_message": "offline" });
        let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &outcome.to_string())
            .await
            .expect("recorded");
        assert_eq!(state.status, ProductLicenseActivationStatus::DeniedRevoked, "{kind}");
        assert_eq!(
            stored_status(&db).await,
            ProductLicenseActivationStatus::DeniedRevoked,
            "{kind}"
        );
    }
    let record = super::load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("record");
    assert_eq!(record.reconciliation.retry_attempt, 2);
}

/// An envelope reusing the id of the envelope in force, without a valid signature, never rewrites
/// it: signed out (where it reduces, so the claim itself is accepted) or for an administrator.
#[tokio::test]
async fn an_unsigned_envelope_reusing_the_active_id_never_rewrites_it() {
    let db = plant_with_envelope().await;
    let mut unsigned = signed_envelope("env-0", 0, "professional", 4);
    unsigned.signature = "00".into();
    for caller in [SIGNED_OUT, ADMIN] {
        apply_product_license_reconciliation_in(&db, caller, &refresh_with_envelope(base_terms(), &unsigned))
            .await
            .expect("the claim keeps the terms");
        let active = super::load_active_envelope(&db)
            .await
            .expect("envelope")
            .expect("envelope in force");
        assert!(active.verified, "the envelope in force stays verified");
        assert_eq!(active.machine_slots, 5, "the envelope in force is not rewritten");
    }
    // Later refreshes still compare with the verified envelope: a newer signed one of the same
    // terms is accepted signed out.
    apply_product_license_reconciliation_in(
        &db,
        SIGNED_OUT,
        &refresh_with_envelope(base_terms(), &signed_envelope("env-3", 0, "professional", 5)),
    )
    .await
    .expect("routine refresh");
}

/// The offline grace bound reads only the envelope's own instants: an envelope issued by a control
/// plane whose clock is days ahead of this PC (or this PC's clock behind) is compared the same way.
#[tokio::test]
async fn the_offline_grace_rule_never_reads_this_pc_clock() {
    // Direct: the bound comes from the incoming issue instant plus the stored hours.
    let stored = super::StoredEnvelope {
        verified: true,
        tier: "professional".into(),
        state: "active".into(),
        channel: "stable".into(),
        machine_slots: 5,
        feature_flags_json: "{}".into(),
        capabilities_json: "{}".into(),
        policy_json: "{}".into(),
        issued_at: "2030-12-01T00:00:00Z".into(),
        valid_from: "2030-12-01T00:00:00Z".into(),
        valid_until: "2030-12-31T00:00:00Z".into(),
        offline_grace_until: "2031-01-03T00:00:00Z".into(),
    };
    let mut incoming = signed_envelope("env-future", 0, "professional", 5);
    incoming.issued_at = "2031-06-01T00:00:00Z".into();
    incoming.valid_until = "2031-05-01T00:00:00Z".into();
    incoming.offline_grace_until = "2031-06-04T00:00:00Z".into();
    assert!(
        !super::offline_grace_raised(&stored, &incoming, Some(72)),
        "issue + 72 h is kept"
    );
    incoming.offline_grace_until = "2031-06-04T01:00:00Z".into();
    assert!(
        super::offline_grace_raised(&stored, &incoming, Some(72)),
        "issue + 73 h extends"
    );

    // End to end: issued five days ahead of this PC's clock, same terms: accepted signed out.
    let db = plant_with_envelope().await;
    let ahead = (chrono::Utc::now() + chrono::Duration::days(5)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut envelope = signed_envelope("env-ahead", 0, "professional", 5);
    envelope.issued_at = ahead.clone();
    envelope.valid_from = ahead;
    let envelope = crate::entitlements::test_support::sign_with_test_key(envelope);
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh_with_envelope(base_terms(), &envelope))
        .await
        .expect("a control plane clock ahead of this PC changes nothing");
}

async fn insert_account(db: &DatabaseConnection, username: &str) -> i64 {
    sea_orm::ConnectionTrait::execute(
        db,
        sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin, \
             force_password_change, created_at, updated_at) VALUES (?, ?, ?, 'local', 1, 0, 0, \
             '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [format!("{username}-sync").into(), username.into(), username.into()],
        ),
    )
    .await
    .expect("account");
    sea_orm::ConnectionTrait::query_one(
        db,
        sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE username = ?",
            [username.into()],
        ),
    )
    .await
    .expect("query")
    .and_then(|row| row.try_get::<i64>("", "id").ok())
    .expect("id")
}

/// A custom role (no reserved name) carrying `permissions`, assigned to `user` at `scope`.
async fn grant_custom_role(
    db: &DatabaseConnection,
    user: i64,
    role_name: &str,
    permissions: &[&str],
    scope_type: &str,
    scope_reference: Option<&str>,
) {
    let exec = |sql: &str, values: Vec<sea_orm::Value>| {
        sea_orm::ConnectionTrait::execute(
            db,
            sea_orm::Statement::from_sql_and_values(sea_orm::DatabaseBackend::Sqlite, sql, values),
        )
    };
    exec(
        "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version) \
         VALUES (?, ?, 'custom', 'active', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1)",
        vec![format!("{role_name}-sync").into(), role_name.into()],
    )
    .await
    .expect("role");
    for permission in permissions {
        exec(
            "INSERT INTO role_permissions (role_id, permission_id, granted_at) \
             SELECT r.id, p.id, '2026-01-01T00:00:00Z' FROM roles r, permissions p WHERE r.name = ? AND p.name = ?",
            vec![role_name.into(), (*permission).into()],
        )
        .await
        .expect("permission");
    }
    exec(
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference, \
         created_at, updated_at, row_version) \
         SELECT ?, ?, id, ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1 FROM roles WHERE name = ?",
        vec![
            format!("{role_name}-usa-{user}").into(),
            user.into(),
            scope_type.into(),
            scope_reference.map(str::to_string).into(),
            role_name.into(),
        ],
    )
    .await
    .expect("assignment");
}

/// "The plant has an administrator" is read from the permission model: any active user holding
/// `adm.settings` through any role (a custom one here) and any scope (wildcard, site), never a
/// role name. A disabled account, or a role without the right, does not count.
#[tokio::test]
async fn the_plant_is_administered_by_whoever_holds_the_right() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("claim");
    assert!(
        !super::plant_has_administrator(&db).await.expect("check"),
        "a true first start"
    );

    let viewer = insert_account(&db, "viewer").await;
    grant_custom_role(
        &db,
        viewer,
        "Lecture",
        &[crate::rbac::permissions::ADM_USERS],
        "tenant",
        None,
    )
    .await;
    assert!(
        !super::plant_has_administrator(&db).await.expect("check"),
        "no adm.settings"
    );

    let keeper = insert_account(&db, "keeper").await;
    grant_custom_role(
        &db,
        keeper,
        "Gardien",
        &[crate::rbac::permissions::ADM_SETTINGS],
        "tenant",
        None,
    )
    .await;
    assert!(
        super::plant_has_administrator(&db).await.expect("check"),
        "custom role, wildcard scope"
    );
    assert!(tenant_has_administrator_for_id(&db, TENANT).await.expect("check"));
    let err = bootstrap_initial_tenant_admin_in(&db, admin_input("intrus"))
        .await
        .expect_err("administered");
    assert_eq!(code_of(&err), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{err:?}");

    sea_orm::ConnectionTrait::execute_unprepared(
        &db,
        &format!("UPDATE user_accounts SET is_active = 0 WHERE id = {keeper}"),
    )
    .await
    .expect("disable");
    assert!(
        !super::plant_has_administrator(&db).await.expect("check"),
        "a disabled account"
    );

    let site_admin = insert_account(&db, "site-admin").await;
    grant_custom_role(
        &db,
        site_admin,
        "Chef de site",
        &[crate::rbac::permissions::ADM_SETTINGS],
        "site",
        Some("site-north"),
    )
    .await;
    assert!(super::plant_has_administrator(&db).await.expect("check"), "any scope");
    assert!(
        !tenant_has_administrator_for_id(&db, TENANT).await.expect("check"),
        "a site scope does not administer the organisation"
    );
}

async fn root_organisation_name(db: &DatabaseConnection) -> String {
    sea_orm::ConnectionTrait::query_one(
        db,
        sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT name FROM org_nodes WHERE parent_id IS NULL AND deleted_at IS NULL ORDER BY id LIMIT 1".to_string(),
        ),
    )
    .await
    .expect("query")
    .and_then(|row| row.try_get::<String>("", "name").ok())
    .expect("root organisation")
}

/// The organisation's name comes with the unsigned claim: signed out it never renames the root
/// organisation or the company shown; an administrator with step-up may.
#[tokio::test]
async fn a_signed_out_refresh_never_renames_the_organisation() {
    let db = plant_with_terms(terms_with(json!({ "tenant_display_name": "Usine Nord" }))).await;
    assert_eq!(root_organisation_name(&db).await, "Usine Nord");

    let renamed = terms_with(json!({ "tenant_display_name": "Autre Société" }));
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(renamed.clone()))
        .await
        .expect("same terms");
    assert_eq!(root_organisation_name(&db).await, "Usine Nord");
    assert_eq!(
        stored_claim(&db).await.tenant_display_name.as_deref(),
        Some("Usine Nord")
    );
    let record = super::load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("record");
    assert_eq!(record.company_display_name.as_deref(), Some("Usine Nord"));

    apply_product_license_reconciliation_in(&db, ADMIN, &refresh(renamed))
        .await
        .expect("administrator with step-up");
    assert_eq!(root_organisation_name(&db).await, "Autre Société");
}

/// Step 7 checklist §9.4, end to end without a session on a fresh database: the first claim (key
/// entry, then its reconciliation) and the first administrator are accepted; afterwards, still
/// signed out, a change of organisation, licence or PC is refused with the coded error (key entry
/// and reconciliation), a second first administrator gets `FIRST_ADMIN_ALREADY_EXISTS`, and the
/// first-start state reports the first-administrator step as closed.
#[tokio::test]
async fn checklist_9_4_first_start_actions_work_then_are_refused_once_an_administrator_exists() {
    let db = crate::db::test_db::seeded().await;
    let first_claim = |tenant: &str, licence: &str, machine: &str| {
        terms_with(json!({
            "tenant_id": tenant,
            "license_id": licence,
            "machine_fingerprint": machine,
            "activation_token": STORED_TOKEN,
        }))
    };

    // (1) The first claim, signed out: key entry, then the control plane's reconciliation of it.
    submit_product_license_key_in(
        &db,
        SIGNED_OUT,
        "MFX-KEY-FIRST-0001",
        Some(first_claim(TENANT, "test-licence", FINGERPRINT).to_string()),
        None,
        None,
    )
    .await
    .expect("first key entry signed out");
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(base_terms()))
        .await
        .expect("first reconciliation signed out");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
    let before =
        super::get_activation_bootstrap_state_in(&db, &crate::sync::transport::SyncWorkerHandle::new().status())
            .await
            .expect("bootstrap state");
    assert_eq!(before.tenant_id.as_deref(), Some(TENANT));
    assert!(!before.has_tenant_admin, "the first-administrator step is open");
    assert_eq!(
        before.plant_join,
        Some(crate::sync::plant_join::PlantJoinState::Joining),
        "no complete pull yet: the screen joins the plant first"
    );

    // (2) The first administrator, signed out.
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
    bootstrap_initial_tenant_admin_in(&db, admin_input("chef"))
        .await
        .expect("first administrator");

    // (3) Still signed out: another organisation, licence or PC is refused, by both paths.
    for (case, tenant, licence, machine) in [
        ("another organisation", "tenant-x", "test-licence", FINGERPRINT),
        ("another licence", TENANT, "lic-other", FINGERPRINT),
        ("another PC", TENANT, "test-licence", "fp-other-pc"),
    ] {
        let claim = first_claim(tenant, licence, machine);
        let outcome = json!({ "kind": "success", "claim": claim }).to_string();
        let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &outcome)
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}: reconciliation accepted"));
        assert_eq!(
            code_of(&err),
            Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
            "{case}: {err:?}"
        );
        let err = submit_product_license_key_in(
            &db,
            SIGNED_OUT,
            "MFX-KEY-OTHER-0009",
            Some(claim.to_string()),
            None,
            None,
        )
        .await
        .expect_err("key entry refused");
        assert_eq!(
            code_of(&err),
            Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
            "{case} (key): {err:?}"
        );
        let stored = stored_claim(&db).await;
        assert_eq!(
            (
                stored.tenant_id.as_str(),
                stored.license_id.as_str(),
                stored.machine_fingerprint.as_str()
            ),
            (TENANT, "test-licence", FINGERPRINT),
            "{case}: nothing stored"
        );
    }

    // (4) A second first administrator.
    let err = bootstrap_initial_tenant_admin_in(&db, admin_input("intrus"))
        .await
        .expect_err("an administrator exists");
    assert_eq!(code_of(&err), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{err:?}");
    assert_eq!(administrator_count(&db).await, 1);

    // (5) The first-start state reports the step as closed.
    let after =
        super::get_activation_bootstrap_state_in(&db, &crate::sync::transport::SyncWorkerHandle::new().status())
            .await
            .expect("bootstrap state");
    assert!(after.has_tenant_admin, "the first-administrator step is closed");
    assert_eq!(after.plant_join, Some(crate::sync::plant_join::PlantJoinState::SignIn));
}

// ── Round 3 ────────────────────────────────────────────────────────────────────────────────────

/// R3-S4: a forged token naming this activation can still be stored signed out (its signature is
/// not checkable here), but only dated up to five minutes past the newest signed envelope; a later
/// genuine rotation is then accepted signed out, never left needing an administrator for good.
#[tokio::test]
async fn a_forged_token_never_blocks_the_next_genuine_rotation() {
    let db = plant_with_envelope().await;
    let now = chrono::Utc::now();
    let first = device_token(
        TENANT,
        "test-licence",
        FINGERPRINT,
        now.timestamp() - 60,
        DEVICE_AUDIENCE,
    );
    apply_product_license_reconciliation_in(
        &db,
        ADMIN,
        &token_refresh(&first, Some(&envelope_issued_at("env-a", now))),
    )
    .await
    .expect("administrator stores a readable token");

    // The review's attack: an inflated iat (far in the future), replaying the newest signed envelope.
    let replayed = envelope_issued_at("env-a", now);
    let inflated = device_token(TENANT, "test-licence", FINGERPRINT, 9_000_000_000_000, DEVICE_AUDIENCE);
    let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &token_refresh(&inflated, Some(&replayed)))
        .await
        .err()
        .expect("an iat past the server time is refused");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");

    // The most a forger can do: an iat at the bound, accepted signed out.
    let at_bound = device_token(
        TENANT,
        "test-licence",
        FINGERPRINT,
        now.timestamp() + super::DEVICE_TOKEN_AFTER_ENVELOPE_SECONDS,
        DEVICE_AUDIENCE,
    );
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &token_refresh(&at_bound, Some(&replayed)))
        .await
        .expect("an iat within the bound");

    // The control plane refuses that token; its next genuine token (later on its clock, carried with
    // a newer signed envelope) is accepted signed out.
    let later = now + chrono::Duration::hours(1);
    let genuine = device_token(TENANT, "test-licence", FINGERPRINT, later.timestamp(), DEVICE_AUDIENCE);
    apply_product_license_reconciliation_in(
        &db,
        SIGNED_OUT,
        &token_refresh(&genuine, Some(&envelope_issued_at("env-b", later))),
    )
    .await
    .expect("the next genuine rotation");
    assert_eq!(stored_claim(&db).await.activation_token, genuine);
}

/// The terms of a forced update: forced or not, with `minimum` as the forced minimum version.
fn forced_update(forced: bool, minimum: &str) -> serde_json::Value {
    terms_with(json!({ "force_update_required": forced, "force_min_app_version": minimum }))
}

/// R3-S1: after the forced update is installed, the control plane's refresh lifts the forced-update
/// denial signed out when the app running here meets the stored minimum; while it does not, lifting
/// it stays an extension, and so does lowering the minimum.
#[tokio::test]
async fn a_forced_update_is_lifted_signed_out_once_this_pc_runs_the_minimum_version() {
    // The version this PC runs is the one the app reports (`get_app_info`), and the rule can read it.
    assert_eq!(super::running_app_version(), env!("CARGO_PKG_VERSION"));
    assert!(super::version_parts(super::running_app_version()).is_some());

    // A minimum below the running version: met.
    let met = "0.0.1";
    let db = plant_with_terms(forced_update(true, met)).await;
    assert_eq!(
        stored_status(&db).await,
        ProductLicenseActivationStatus::DeniedForceUpdateRequired
    );
    // Lowering the minimum at the same time stays an extension.
    assert_extension_needs_admin(&db, &refresh(forced_update(false, "0.0.0")), "minimum lowered").await;
    let db = plant_with_terms(forced_update(true, met)).await;
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(false, met)))
        .await
        .expect("the update is installed: lifted signed out");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
    assert_eq!(stored_claim(&db).await.force_update_required, Some(false));

    // A minimum this PC does not meet, an unreadable one, or none: lifting it is an extension.
    for (case, minimum) in [("PC below the minimum", "99999.0.0"), ("unreadable minimum", "latest")] {
        let db = plant_with_terms(forced_update(true, minimum)).await;
        assert_extension_needs_admin(&db, &refresh(forced_update(false, minimum)), case).await;
    }
    let db = plant_with_terms(terms_with(json!({ "force_update_required": true }))).await;
    assert_extension_needs_admin(
        &db,
        &refresh(terms_with(json!({ "force_update_required": false }))),
        "no stored minimum",
    )
    .await;
    // The flag dropped instead of stated false: an extension.
    let db = plant_with_terms(forced_update(true, met)).await;
    assert_extension_needs_admin(
        &db,
        &refresh(terms_with(json!({ "force_min_app_version": met }))),
        "flag dropped",
    )
    .await;
}

#[test]
fn a_running_version_meets_a_minimum_fail_closed() {
    for (running, minimum, meets) in [
        ("1.4.0", "1.4.0", true),
        ("1.4.1", "1.4", true),
        ("v2.0.0", "1.9.9", true),
        ("1.3.9", "1.4.0", false),
        ("1.4.0-rc1", "1.4.0", false),
        ("1.4.0-dev", "1.3.9", true),
        ("1.4.0", "latest", false),
        ("dev", "1.0.0", false),
    ] {
        assert_eq!(
            super::version_meets_minimum(running, minimum),
            meets,
            "{running} vs {minimum}"
        );
    }
}

/// R3-N4: signed out, a claim never swaps a denial for another kind (a revocation for a forced
/// update, which an update would then lift); a forced update kept forced stays accepted.
#[tokio::test]
async fn a_signed_out_claim_never_swaps_one_denial_for_another() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, DENIED)
        .await
        .expect("revoked");
    let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(true, "0.0.1")))
        .await
        .err()
        .expect("revoked to forced update refused");
    assert_eq!(code_of(&err), Some("LICENSE_EXTENSION_ADMIN_REQUIRED"), "{err:?}");
    assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::DeniedRevoked);

    let db = plant_with_terms(forced_update(true, "99999.0.0")).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(true, "99999.0.0")))
        .await
        .expect("the same forced update, kept");
    assert_eq!(
        stored_status(&db).await,
        ProductLicenseActivationStatus::DeniedForceUpdateRequired
    );
}

/// A denial outcome with the control plane's `code` (no claim, recordable by anyone signed out).
fn denial(code: &str) -> String {
    json!({ "kind": "denied", "error_code": code, "error_message": code }).to_string()
}

async fn stored_deny_reason(db: &DatabaseConnection) -> Option<String> {
    super::load_state_record_from_db(db)
        .await
        .expect("record")
        .expect("stored record")
        .deny_reason_code
}

/// The denials only the control plane ends, with the code that records each.
const CONTROL_PLANE_DENIALS: [(&str, ProductLicenseActivationStatus); 4] = [
    ("license_revoked", ProductLicenseActivationStatus::DeniedRevoked),
    ("license_expired", ProductLicenseActivationStatus::DeniedExpired),
    ("slot_limit_reached", ProductLicenseActivationStatus::DeniedSlotLimit),
    ("license_denied", ProductLicenseActivationStatus::DeniedInvalid),
];

#[test]
fn the_denial_severity_order_puts_a_forced_update_below_every_control_plane_denial() {
    use super::denial_severity;
    let forced = denial_severity(&ProductLicenseActivationStatus::DeniedForceUpdateRequired);
    assert!(forced > denial_severity(&ProductLicenseActivationStatus::Active));
    for (code, status) in CONTROL_PLANE_DENIALS {
        assert!(denial_severity(&status) > forced, "{code}");
        assert_eq!(
            denial_severity(&status),
            denial_severity(&ProductLicenseActivationStatus::DeniedRevoked),
            "{code}: the control-plane denials rank equal"
        );
        assert_eq!(super::map_denied_status(Some(code)), status, "{code}");
    }
}

/// R4-B1: signed out, a revocation, expiry, slot-limit or invalid denial is never laundered into
/// `Active` by (1) a bare forced-update denial, (2) a claim keeping it forced with a low minimum,
/// (3) a claim lifting the forced update the running version meets. Step 1 is kept as a diagnostic
/// only, steps 2 and 3 are refused, the stored forced-update fields never change, and the status is
/// never `Active`.
#[tokio::test]
async fn a_signed_out_denial_is_never_laundered_through_a_forced_update() {
    for (code, status) in CONTROL_PLANE_DENIALS {
        let db = plant_with_terms(base_terms()).await;
        apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial(code))
            .await
            .unwrap_or_else(|e| panic!("{code}: denial recorded: {e:?}"));
        assert_eq!(stored_status(&db).await, status, "{code}");
        let claim_before = serde_json::to_value(stored_claim(&db).await).expect("claim json");

        let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial("force_update_required"))
            .await
            .unwrap_or_else(|e| panic!("{code}: step 1 recorded as a diagnostic: {e:?}"));
        assert_eq!(state.status, status, "{code}: step 1 kept the recorded denial");
        assert_eq!(stored_status(&db).await, status, "{code}: step 1");
        assert_eq!(
            stored_deny_reason(&db).await.as_deref(),
            Some(code),
            "{code}: reason kept"
        );
        let record = super::load_state_record_from_db(&db)
            .await
            .expect("record")
            .expect("record");
        assert!(!record.forced_update_lift_allowed, "{code}");
        assert!(
            record
                .diagnostics
                .last()
                .is_some_and(|d| d.kind == "deny_kept" && d.code.as_deref() == Some("force_update_required")),
            "{code}: diagnostic kept"
        );

        for (step, outcome) in [
            ("step 2", refresh(forced_update(true, "0.0.1"))),
            ("step 3", refresh(forced_update(false, "0.0.1"))),
        ] {
            let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &outcome)
                .await
                .err()
                .unwrap_or_else(|| panic!("{code}: {step} accepted signed out"));
            assert_eq!(
                code_of(&err),
                Some("LICENSE_EXTENSION_ADMIN_REQUIRED"),
                "{code}: {step}: {err:?}"
            );
            assert_eq!(stored_status(&db).await, status, "{code}: {step}");
            assert_ne!(stored_status(&db).await, ProductLicenseActivationStatus::Active);
            assert_eq!(
                serde_json::to_value(stored_claim(&db).await).expect("claim json"),
                claim_before,
                "{code}: {step}: the forced-update fields are not changed signed out"
            );
        }
    }
}

/// A forced update planted signed out before a genuine revocation: the revocation replaces it and
/// still wins against every later signed-out step.
#[tokio::test]
async fn a_genuine_revocation_wins_over_a_forced_update_planted_before_it() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(true, "0.0.1")))
        .await
        .expect("a stricter forced update is accepted signed out");
    assert_eq!(
        stored_status(&db).await,
        ProductLicenseActivationStatus::DeniedForceUpdateRequired
    );
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial("license_revoked"))
        .await
        .expect("the genuine revocation replaces the forced update");
    assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::DeniedRevoked);

    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial("force_update_required"))
        .await
        .expect("kept as a diagnostic");
    assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::DeniedRevoked);
    for outcome in [
        refresh(forced_update(false, "0.0.1")),
        refresh(forced_update(true, "0.0.1")),
    ] {
        let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &outcome)
            .await
            .err()
            .expect("refused signed out");
        assert_eq!(code_of(&err), Some("LICENSE_EXTENSION_ADMIN_REQUIRED"), "{err:?}");
        assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::DeniedRevoked);
    }
}

/// The genuine path, all signed out: the control plane's refresh forces the update with a minimum,
/// this PC runs a version that meets it (a development build is a pre-release, below its own
/// release, so a lower minimum stands for "installed"), and the next refresh lifting the forced
/// update is accepted.
#[tokio::test]
async fn a_genuine_forced_update_is_installed_and_lifted_signed_out() {
    let minimum = "0.0.1";
    assert!(super::version_meets_minimum(super::running_app_version(), minimum));
    let db = plant_with_terms(base_terms()).await;
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(true, minimum)))
        .await
        .expect("forced update recorded");
    assert_eq!(state.status, ProductLicenseActivationStatus::DeniedForceUpdateRequired);
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(false, minimum)))
        .await
        .expect("installed: lifted signed out");
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
}

/// R4-B1 (b): a forced update recorded from a bare denial outcome names no minimum, so it is never
/// lifted by "installing" it signed out, even after a claim keeps it forced with a minimum.
#[tokio::test]
async fn a_forced_update_from_a_bare_denial_is_never_lifted_by_an_update() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial("force_update_required"))
        .await
        .expect("bare forced-update denial");
    assert_eq!(
        stored_status(&db).await,
        ProductLicenseActivationStatus::DeniedForceUpdateRequired
    );
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(true, "0.0.1")))
        .await
        .expect("kept forced with a minimum");
    assert_extension_needs_admin(&db, &refresh(forced_update(false, "0.0.1")), "bare denial then lift").await;
    assert_eq!(stored_status(&db).await, ProductLicenseActivationStatus::Active);
}

/// R4-S1: a forced minimum version (with or without a forced update) planted signed out cannot be
/// refused, since a genuine push must pass; it makes the next genuine refresh (which lowers it) need
/// an administrator, and an administrator with step-up always clears it.
#[tokio::test]
async fn an_administrator_with_step_up_clears_a_planted_forced_minimum() {
    for (case, planted) in [
        (
            "minimum only",
            terms_with(json!({ "force_min_app_version": "99999.0.0" })),
        ),
        ("forced update", forced_update(true, "99999.0.0")),
    ] {
        let db = plant_with_terms(base_terms()).await;
        apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(planted))
            .await
            .unwrap_or_else(|e| panic!("{case}: stricter terms accepted signed out: {e:?}"));
        assert_extension_needs_admin(&db, &refresh(base_terms()), case).await;
        assert_eq!(
            stored_status(&db).await,
            ProductLicenseActivationStatus::Active,
            "{case}"
        );
        let claim = stored_claim(&db).await;
        assert_eq!(claim.force_min_app_version, None, "{case}");
        assert_eq!(claim.force_update_required, None, "{case}");
    }
}

async fn set_seeded_admin(db: &DatabaseConnection, force_password_change: i32, password_changed_at: Option<&str>) {
    sea_orm::ConnectionTrait::execute(
        db,
        sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "UPDATE user_accounts SET force_password_change = ?, password_changed_at = ? WHERE username = 'admin'",
            [
                force_password_change.into(),
                password_changed_at.map(str::to_string).into(),
            ],
        ),
    )
    .await
    .expect("seeded admin");
}

/// R3-S3: the administrator the seeder creates on every PC does not count while never used (a
/// brand-new PC keeps its first start), and counts once its password has been changed.
#[tokio::test]
async fn the_seeded_administrator_counts_once_taken_into_use() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("claim");
    assert!(
        !super::plant_has_administrator(&db).await.expect("check"),
        "never used: a first start"
    );

    for (case, force, changed_at) in [
        ("forced change done", 0, Some("2026-10-01T00:00:00Z")),
        ("password reset, change pending", 1, Some("2026-10-01T00:00:00Z")),
        ("flag cleared by an administrator", 0, None),
    ] {
        set_seeded_admin(&db, force, changed_at).await;
        assert!(super::plant_has_administrator(&db).await.expect("check"), "{case}");
        let err = bootstrap_initial_tenant_admin_in(&db, admin_input("intrus"))
            .await
            .expect_err("administered");
        assert_eq!(code_of(&err), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{case}: {err:?}");
    }

    set_seeded_admin(&db, 1, None).await;
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
    bootstrap_initial_tenant_admin_in(&db, admin_input("chef"))
        .await
        .expect("never used again: the first administrator is created");
}

/// R3-N5: holding `adm.settings` at any scope closes the first-start steps, so such a holder may
/// approve a licence change when nobody holds it at tenant scope; where a tenant-scope holder
/// exists, only such a holder approves.
#[tokio::test]
async fn a_plant_whose_only_administrators_are_site_scoped_can_approve() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("claim");
    let site_admin = insert_account(&db, "site-admin").await;
    grant_custom_role(
        &db,
        site_admin,
        "Chef de site",
        &[crate::rbac::permissions::ADM_SETTINGS],
        "site",
        Some("site-north"),
    )
    .await;
    let site_admin = i32::try_from(site_admin).expect("id");
    assert!(super::plant_has_administrator(&db).await.expect("check"));
    let caller = licence_caller(&db, Some(site_admin), true).await.expect("caller");
    assert!(caller.may_administer, "the only administrators are site-scoped");
    let viewer = i32::try_from(insert_account(&db, "viewer").await).expect("id");
    assert!(
        !licence_caller(&db, Some(viewer), true)
            .await
            .expect("caller")
            .may_administer
    );

    let keeper = insert_account(&db, "keeper").await;
    grant_custom_role(
        &db,
        keeper,
        "Gardien",
        &[crate::rbac::permissions::ADM_SETTINGS],
        "tenant",
        None,
    )
    .await;
    let keeper = i32::try_from(keeper).expect("id");
    assert!(
        licence_caller(&db, Some(keeper), true)
            .await
            .expect("caller")
            .may_administer
    );
    assert!(
        !licence_caller(&db, Some(site_admin), true)
            .await
            .expect("caller")
            .may_administer,
        "a tenant-scope holder exists"
    );
}

/// R3-N6: fail-closed, the plant stays administered when its administrators' assignments end by
/// time alone (validity window, emergency grant expiry); only an administrator's act (removing the
/// assignment) changes it.
#[tokio::test]
async fn the_plant_stays_administered_when_assignments_end_by_time() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("claim");
    let keeper = insert_account(&db, "keeper").await;
    grant_custom_role(
        &db,
        keeper,
        "Gardien",
        &[crate::rbac::permissions::ADM_SETTINGS],
        "tenant",
        None,
    )
    .await;
    for (case, sql) in [
        (
            "validity ended",
            format!("UPDATE user_scope_assignments SET valid_to = '2020-01-01T00:00:00Z' WHERE user_id = {keeper}"),
        ),
        (
            "emergency grant expired",
            format!(
                "UPDATE user_scope_assignments SET valid_to = NULL, is_emergency = 1, \
                 emergency_expires_at = '2020-01-01T00:00:00Z' WHERE user_id = {keeper}"
            ),
        ),
    ] {
        sea_orm::ConnectionTrait::execute_unprepared(&db, &sql)
            .await
            .expect(case);
        assert!(super::plant_has_administrator(&db).await.expect("check"), "{case}");
        let err = bootstrap_initial_tenant_admin_in(&db, admin_input("intrus"))
            .await
            .expect_err("administered");
        assert_eq!(code_of(&err), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{case}: {err:?}");
    }

    sea_orm::ConnectionTrait::execute_unprepared(
        &db,
        &format!("UPDATE user_scope_assignments SET deleted_at = '2026-10-01T00:00:00Z' WHERE user_id = {keeper}"),
    )
    .await
    .expect("removed");
    assert!(!super::plant_has_administrator(&db).await.expect("check"), "removed");
}

async fn stored_lift_allowed(db: &DatabaseConnection) -> bool {
    super::load_state_record_from_db(db)
        .await
        .expect("record")
        .expect("stored record")
        .forced_update_lift_allowed
}

/// `terms` naming the activation stored here (without its token).
fn with_stored_ids(terms: serde_json::Value) -> serde_json::Value {
    let mut claim = terms;
    if let Some(obj) = claim.as_object_mut() {
        obj.insert("tenant_id".into(), json!(TENANT));
        obj.insert("license_id".into(), json!("test-licence"));
        obj.insert("machine_fingerprint".into(), json!(FINGERPRINT));
    }
    claim
}

/// R5-S1 (a): signed out, at each control-plane denial, the key entry and its check refuse a claim
/// that forces an update with a low minimum and one that lifts it, exactly as the reconciliation
/// does; nothing stored changes.
#[tokio::test]
async fn a_signed_out_key_entry_never_launders_a_denial_through_a_forced_update() {
    for (code, status) in CONTROL_PLANE_DENIALS {
        let db = plant_with_terms(base_terms()).await;
        apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial(code))
            .await
            .unwrap_or_else(|e| panic!("{code}: denial recorded: {e:?}"));
        let claim_before = serde_json::to_value(stored_claim(&db).await).expect("claim json");
        for (step, terms) in [
            ("forced claim", forced_update(true, "0.0.1")),
            ("lifting claim", forced_update(false, "0.0.1")),
        ] {
            let mut claim = with_stored_ids(terms);
            let checked = check_product_license_key_entry_in(&db, SIGNED_OUT, &claim.to_string()).await;
            if let Some(obj) = claim.as_object_mut() {
                obj.insert("activation_token".into(), json!(STORED_TOKEN));
            }
            let submitted = submit_product_license_key_in(
                &db,
                SIGNED_OUT,
                "MFX-KEY-LAUNDER-0001",
                Some(claim.to_string()),
                None,
                None,
            )
            .await;
            for (route, result) in [("check", &checked), ("key entry", &submitted)] {
                assert_eq!(
                    outcome(result),
                    "LICENSE_EXTENSION_ADMIN_REQUIRED",
                    "{code}: {step}: {route}"
                );
            }
            assert_eq!(stored_status(&db).await, status, "{code}: {step}");
            assert_eq!(stored_deny_reason(&db).await.as_deref(), Some(code), "{code}: {step}");
            assert!(!stored_lift_allowed(&db).await, "{code}: {step}");
            assert_eq!(
                serde_json::to_value(stored_claim(&db).await).expect("claim json"),
                claim_before,
                "{code}: {step}: nothing stored"
            );
        }
    }
}

/// R5-S1 (b): revoked, then a network error (which never leaves a denial), then a forced update
/// planted signed out, then its lift: the revocation stays.
#[tokio::test]
async fn a_network_error_never_opens_a_way_from_a_revocation_to_a_forced_update() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial("license_revoked"))
        .await
        .expect("revoked");
    let network_error = json!({ "kind": "network_error", "error_code": "network", "error_message": "offline" });
    let state = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &network_error.to_string())
        .await
        .expect("network error recorded");
    assert_eq!(state.status, ProductLicenseActivationStatus::DeniedRevoked);
    for (step, outcome) in [
        ("planted forced update", refresh(forced_update(true, "0.0.1"))),
        ("lift", refresh(forced_update(false, "0.0.1"))),
    ] {
        let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &outcome)
            .await
            .err()
            .unwrap_or_else(|| panic!("{step}: accepted signed out"));
        assert_eq!(
            code_of(&err),
            Some("LICENSE_EXTENSION_ADMIN_REQUIRED"),
            "{step}: {err:?}"
        );
        assert_eq!(
            stored_status(&db).await,
            ProductLicenseActivationStatus::DeniedRevoked,
            "{step}"
        );
    }
}

/// R5-S1 (c): a liftable forced update, then a bare forced-update denial (which resets the lift),
/// then the lift: it needs an administrator with step-up.
#[tokio::test]
async fn a_bare_denial_after_a_liftable_forced_update_makes_the_lift_need_an_administrator() {
    let db = plant_with_terms(base_terms()).await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(true, "0.0.1")))
        .await
        .expect("forced update recorded");
    assert!(stored_lift_allowed(&db).await, "liftable");
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial("force_update_required"))
        .await
        .expect("bare denial recorded");
    assert_eq!(
        stored_status(&db).await,
        ProductLicenseActivationStatus::DeniedForceUpdateRequired
    );
    assert!(!stored_lift_allowed(&db).await, "the bare denial resets the lift");
    assert_extension_needs_admin(&db, &refresh(forced_update(false, "0.0.1")), "lift after a bare denial").await;
}

/// R5-S1 (d): a forced-update record written before `forced_update_lift_allowed` existed is read
/// with its stored status (not through the legacy fallback, which would read it as active) and the
/// lift not allowed, so the signed-out lift is refused. The same record stating the lift allowed is
/// accepted, so the refusal comes from the serde default alone.
#[tokio::test]
async fn an_old_forced_update_record_reads_as_not_liftable() {
    for (case, lift_allowed) in [("old record", None), ("lift stated", Some(true))] {
        let db = activated_plant_with_admin().await;
        let mut claim = with_stored_ids(forced_update(true, "0.0.1"));
        if let Some(obj) = claim.as_object_mut() {
            obj.insert("activation_token".into(), json!(STORED_TOKEN));
        }
        let mut record = json!({
            "schema_version": 1,
            "key_sha256": "test",
            "submitted_at": "2026-10-01T08:00:00Z",
            "status": "denied_force_update_required",
            "pending_online_validation": false,
            "deny_reason_code": "force_update_required",
            "activation_claim": claim,
            "reconciliation": { "retry_attempt": 0 },
            "diagnostics": [],
        });
        if let (Some(obj), Some(flag)) = (record.as_object_mut(), lift_allowed) {
            obj.insert("forced_update_lift_allowed".into(), json!(flag));
        }
        crate::settings::set_setting(
            &db,
            PRODUCT_LICENSE_ONBOARDING_KEY,
            PRODUCT_LICENSE_SCOPE,
            &record.to_string(),
            None,
            case,
        )
        .await
        .expect("record");
        assert_eq!(
            stored_status(&db).await,
            ProductLicenseActivationStatus::DeniedForceUpdateRequired,
            "{case}: read as stored, not through the legacy fallback"
        );
        let accepted = lift_allowed.is_some();
        assert_eq!(stored_lift_allowed(&db).await, accepted, "{case}");

        let lifted =
            apply_product_license_reconciliation_in(&db, SIGNED_OUT, &refresh(forced_update(false, "0.0.1"))).await;
        match lifted {
            Ok(state) => {
                assert!(accepted, "{case}: lifted signed out");
                assert_eq!(state.status, ProductLicenseActivationStatus::Active, "{case}");
            }
            Err(err) => {
                assert!(!accepted, "{case}: refused: {err:?}");
                assert_eq!(
                    code_of(&err),
                    Some("LICENSE_EXTENSION_ADMIN_REQUIRED"),
                    "{case}: {err:?}"
                );
                assert_eq!(
                    stored_status(&db).await,
                    ProductLicenseActivationStatus::DeniedForceUpdateRequired,
                    "{case}"
                );
            }
        }
    }
}

/// R5-N1: a genuine revocation and a signed-out claim reported at the same moment on a database file
/// with two connections: the claim is authorised against the record it replaces, so the revocation
/// is never overwritten (if it lands first, the claim is refused; if the claim lands first, the
/// revocation replaces it).
#[tokio::test]
async fn a_concurrent_signed_out_claim_never_overwrites_a_revocation() {
    use sea_orm::ConnectionTrait;
    use sea_orm_migration::MigratorTrait;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("plant.db");
    let setup = crate::db::connect_sqlcipher(&path, None, true)
        .await
        .expect("database file");
    setup.execute_unprepared("PRAGMA journal_mode=WAL;").await.expect("wal");
    crate::migrations::Migrator::up(&setup, None).await.expect("migrations");
    crate::db::seeder::seed_system_data(&setup).await.expect("seed");
    record_activation_claim_for_tests(&setup, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("claim");
    crate::sync::plant_join::confirm_empty_plant_for_tests(&setup, TENANT).await;
    bootstrap_initial_tenant_admin_in(&setup, admin_input("chef"))
        .await
        .expect("first administrator");
    setup.close().await.expect("close setup");

    let opts = SqliteConnectOptions::new()
        .filename(&path)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .min_connections(2)
        .connect_with(opts)
        .await
        .expect("database file");
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);

    let same_activation = success(TENANT, "test-licence", json!({}));
    let revocation = denial("license_revoked");
    // Alone, the signed-out claim is accepted, so a refusal in the race below comes from the
    // revocation recorded first, not from the claim itself.
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("active");
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &same_activation)
        .await
        .expect("the same activation is accepted signed out on its own");
    assert_eq!(
        stored_status(&db).await,
        ProductLicenseActivationStatus::Active,
        "the claim alone keeps the licence active"
    );
    for round in 0..10 {
        record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
            .await
            .expect("active again");
        let (revoked, claimed) = tokio::join!(
            apply_product_license_reconciliation_in(&db, SIGNED_OUT, &revocation),
            apply_product_license_reconciliation_in(&db, SIGNED_OUT, &same_activation),
        );
        revoked.unwrap_or_else(|e| panic!("round {round}: revocation recorded: {e:?}"));
        if let Err(err) = &claimed {
            assert_eq!(
                code_of(err),
                Some("LICENSE_EXTENSION_ADMIN_REQUIRED"),
                "round {round}: {err:?}"
            );
        }
        assert_eq!(
            stored_status(&db).await,
            ProductLicenseActivationStatus::DeniedRevoked,
            "round {round}: the revocation is kept"
        );
    }
    db.close().await.expect("close");
}

/// Release chain: while the licence requires an update (`DeniedForceUpdateRequired`), sync stays
/// closed but the update manifest is still asked with this PC's activation token, so the required
/// version can be offered and installed (the only way this PC ends that denial). Any denial only
/// the control plane ends closes the manifest too.
#[tokio::test]
async fn the_update_manifest_stays_reachable_while_the_licence_requires_an_update() {
    let db = plant_with_terms(forced_update(true, "99999.0.0")).await;
    assert_eq!(
        stored_status(&db).await,
        ProductLicenseActivationStatus::DeniedForceUpdateRequired
    );
    let token = stored_claim(&db).await.activation_token;
    assert_eq!(
        super::sync_activation_bearer_token(&db).await.expect("sync token"),
        None,
        "sync stays closed while the update is required"
    );
    assert_eq!(
        super::update_manifest_bearer_token(&db).await.expect("manifest token"),
        Some(token),
        "the update server is still asked for the required version"
    );

    for (code, status) in CONTROL_PLANE_DENIALS {
        let db = plant_with_terms(base_terms()).await;
        apply_product_license_reconciliation_in(&db, SIGNED_OUT, &denial(code))
            .await
            .expect("denial recorded");
        assert_eq!(stored_status(&db).await, status, "{code}");
        assert_eq!(
            super::update_manifest_bearer_token(&db).await.expect("manifest token"),
            None,
            "{code}: no update check for a licence only the control plane can end"
        );
    }

    let db = plant_with_terms(base_terms()).await;
    let active = super::sync_activation_bearer_token(&db).await.expect("sync token");
    assert!(active.is_some());
    assert_eq!(
        super::update_manifest_bearer_token(&db).await.expect("manifest token"),
        active,
        "an active licence: the same token as sync"
    );
}

/// F8: a signed-out reset ("Réinitialiser l'activation / autre clé") on a plant with an
/// administrator is refused with a coded reason the login screen translates (sign in as an
/// administrator, or a vendor recovery code), not a bare authentication error; nothing is reset.
/// Before any administrator exists, the reset stays open as before.
#[tokio::test]
async fn a_signed_out_reset_on_an_administered_plant_names_the_two_routes() {
    let db = activated_plant_with_admin().await;
    let err = authorize_signed_out_reset(&db).await.expect_err("administered");
    assert_eq!(code_of(&err), Some("LICENSE_RESET_REQUIRES_ADMIN_SIGNIN"), "{err:?}");
    assert!(!matches!(err, AppError::Auth(_)), "{err:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT),
        "the licence is kept"
    );
    assert!(tenant_has_administrator_for_id(&db, TENANT).await.expect("check"));

    // No administrator and no licence held yet (a true first start): still allowed.
    let first_start = crate::db::test_db::seeded().await;
    authorize_signed_out_reset(&first_start)
        .await
        .expect("no administrator: first start");
}

/// AU-127 (F11): the plant default language set in Settings is kept by an activation reset, in
/// both places it is read from (the Settings row the reset puts back to the shipped "fr", and the
/// runtime default), so the screen and the language in use agree after the reset.
#[tokio::test]
async fn the_plant_language_is_kept_across_a_reset() {
    let db = crate::sync::test_support::started_db().await;
    // As the Settings screen sets it: the Settings row, then the runtime default.
    crate::settings::set_setting(
        &db,
        "locale.primary_language",
        "tenant",
        r#""en""#,
        None,
        "plant language",
    )
    .await
    .expect("setting");
    crate::locale::write_locale_config(&db, "locale.default_language", "en")
        .await
        .expect("default");

    super::reset_local_tenant_runtime_data_impl(&db).await.expect("reset");

    let row = crate::settings::get_setting(&db, "locale.primary_language", "tenant")
        .await
        .expect("read")
        .expect("seeded again");
    assert_eq!(row.setting_value_json, r#""en""#, "the Settings row is written back");
    assert_eq!(
        crate::locale::read_locale_config(&db, "locale.default_language")
            .await
            .expect("read")
            .as_deref(),
        Some("en")
    );
    assert_eq!(
        crate::locale::resolve_locale_preference(&db)
            .await
            .expect("resolve")
            .tenant_locale
            .as_deref(),
        Some("en")
    );
}
