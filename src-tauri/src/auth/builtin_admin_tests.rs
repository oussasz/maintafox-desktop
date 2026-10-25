//! The built-in "admin" account (review E-1): never seeded by a release build, retired once another
//! administrator exists while its password was never changed, kept (with a warning) while it is the
//! only one, disabled when it arrives by synchronisation on a PC that has another administrator or
//! never had it; the first-start screen closed for good once an administrator was created.
//!
//! The test binary is a `debug_assertions` build. The release behaviour is checked through the
//! same functions a release binary runs, given the profile it runs with (`BuildProfile::Release`,
//! what `BuildProfile::current()` returns without `debug_assertions`).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::commands::product_license::{
    apply_product_license_reconciliation_in, authorize_signed_out_reset, bootstrap_initial_tenant_admin_in,
    get_activation_claim_tenant_id, plant_has_administrator, record_activation_claim_for_tests,
    submit_product_license_key_in, BootstrapInitialAdminInput, LicenceCaller,
};

const TENANT: &str = "tenant-plant-e1";

async fn scalar(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .expect("row")
        .try_get_by_index::<i64>(0)
        .expect("integer")
}

async fn exec(db: &impl ConnectionTrait, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// A fresh PC as a release build leaves it after its first start: migrations, then the system seed
/// a release binary runs.
async fn release_fresh_db() -> DatabaseConnection {
    let db = crate::db::test_db::migrated().await;
    crate::db::seeder::seed_system_data_for(&db, BuildProfile::Release)
        .await
        .expect("release seed");
    db
}

fn first_admin(username: &str) -> BootstrapInitialAdminInput {
    BootstrapInitialAdminInput {
        username: username.into(),
        email: format!("{username}@plant.example"),
        password: "Chef-Usine-2026!Sur".into(),
        display_name: None,
    }
}

/// Creates a named administrator through the first-start screen (activation claim first).
async fn create_first_admin(db: &DatabaseConnection, username: &str) {
    record_activation_claim_for_tests(db, TENANT, "fp-e1", "token-e1")
        .await
        .expect("claim");
    crate::sync::plant_join::confirm_empty_plant_for_tests(db, TENANT).await;
    bootstrap_initial_tenant_admin_in(db, first_admin(username))
        .await
        .expect("first administrator");
}

async fn builtin(db: &DatabaseConnection) -> BuiltinAdminRow {
    builtin_admin_row(db)
        .await
        .expect("read")
        .expect("built-in account present")
}

async fn retire_audits(db: &DatabaseConnection, user_id: i64) -> i64 {
    scalar(
        db,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = ? AND target_type = 'user_account'
           AND target_id = ? AND json_extract(details_json, '$.username') = 'admin'",
        vec![RETIRE_ACTION.into(), user_id.to_string().into()],
    )
    .await
}

// ── 1. The seeding decision ───────────────────────────────────────────────────────────────────────

#[test]
fn only_development_builds_seed_or_delete_the_builtin_account() {
    assert!(BuildProfile::Development.seeds_builtin_admin());
    assert!(!BuildProfile::Release.seeds_builtin_admin());
    assert!(BuildProfile::Development.deletes_superseded_builtin_admin());
    assert!(!BuildProfile::Release.deletes_superseded_builtin_admin());
    // The profile is the compile-time switch: this test binary has `debug_assertions`.
    assert_eq!(BuildProfile::current(), BuildProfile::Development);
}

#[tokio::test]
async fn a_release_build_seeds_no_builtin_account() {
    let db = release_fresh_db().await;
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM user_accounts", vec![]).await,
        0,
        "a release build creates no account at all"
    );
    assert!(builtin_admin_row(&db).await.expect("read").is_none());
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) FROM user_scope_assignments", vec![]).await,
        0
    );
    // Starting again (every start runs the seed) still creates none.
    crate::db::seeder::seed_system_data_for(&db, BuildProfile::Release)
        .await
        .expect("seed again");
    assert_eq!(scalar(&db, "SELECT COUNT(*) FROM user_accounts", vec![]).await, 0);
}

#[tokio::test]
async fn a_development_build_seeds_the_dev_account_with_a_forced_change() {
    let db = crate::db::test_db::seeded().await;
    let row = builtin(&db).await;
    assert!(row.is_default_password_account());
    assert!(
        !plant_has_administrator(&db).await.expect("check"),
        "the dev account never closes first start"
    );
}

// ── 2. First start ───────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn first_start_creates_the_first_administrator_on_a_fresh_release_pc() {
    let db = release_fresh_db().await;
    assert!(!plant_has_administrator(&db).await.expect("check"));
    create_first_admin(&db, "chef").await;
    assert!(plant_has_administrator(&db).await.expect("check"));
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM user_accounts WHERE username = 'chef' AND is_active = 1",
            vec![]
        )
        .await,
        1
    );
    // The administrator chose the password: the account is its author (`sync::credential_check`).
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM user_accounts WHERE username = 'chef' AND password_changed_by_id = id",
            vec![]
        )
        .await,
        1
    );
    assert!(
        builtin_admin_row(&db).await.expect("read").is_none(),
        "no built-in account appears"
    );
    // The creation is audited.
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = 'user.created'
               AND json_extract(details_json, '$.detail.bootstrap') = 1",
            vec![]
        )
        .await,
        1
    );
}

#[tokio::test]
async fn the_first_admin_password_meets_the_plant_policy() {
    let db = release_fresh_db().await;
    validate_first_admin_password(&db, "Chef-Usine-2026!Sur")
        .await
        .expect("strong password");
    assert!(validate_first_admin_password(&db, "short1A").await.is_err());
    // The plant policy applies on top of the fixed minimum: a plant that asks for 16 characters
    // refuses a 12-character password the fixed rule accepts.
    let twelve = "Abcdefgh123x";
    crate::commands::admin_users::validate_password_strength(twelve).expect("fixed minimum met");
    validate_first_admin_password(&db, twelve)
        .await
        .expect("shipped policy");
    exec(
        &db,
        "INSERT INTO rbac_settings (key, value) VALUES ('password_min_length', '16')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        vec![],
    )
    .await;
    assert!(validate_first_admin_password(&db, twelve).await.is_err());
}

/// The first-start screen itself applies the plant policy: a password the fixed minimum accepts
/// but the plant refuses creates nothing, and first start stays open.
#[tokio::test]
async fn the_first_admin_is_refused_a_password_that_breaks_the_plant_policy() {
    let db = release_fresh_db().await;
    record_activation_claim_for_tests(&db, TENANT, "fp-e1", "token-e1")
        .await
        .expect("claim");
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
    exec(
        &db,
        "INSERT INTO rbac_settings (key, value) VALUES ('password_min_length', '16')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        vec![],
    )
    .await;
    let mut input = first_admin("chef");
    input.password = "Abcdefgh123x".into();
    let err = bootstrap_initial_tenant_admin_in(&db, input)
        .await
        .expect_err("plant policy: 16 characters");
    assert!(matches!(err, AppError::ValidationFailed(_)), "{err:?}");
    assert_eq!(scalar(&db, "SELECT COUNT(*) FROM user_accounts", vec![]).await, 0);
    assert!(!first_admin_marker_closes(&db).await.expect("check"));
    assert!(!plant_has_administrator(&db).await.expect("check"));
    // A password meeting the policy is accepted.
    let mut input = first_admin("chef");
    input.password = "Chef-Usine-2026!Sur-Long".into();
    bootstrap_initial_tenant_admin_in(&db, input).await.expect("policy met");
}

// ── 3. "Closed for good" marker ──────────────────────────────────────────────────────────────────

fn code_of(err: &AppError) -> Option<&str> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.as_str()),
        _ => None,
    }
}

/// The first-start screen refuses a second administrator (`FIRST_ADMIN_ALREADY_EXISTS`).
async fn assert_first_start_closed(db: &DatabaseConnection, case: &str) {
    assert!(plant_has_administrator(db).await.expect("check"), "{case}");
    let err = bootstrap_initial_tenant_admin_in(db, first_admin("intrus"))
        .await
        .expect_err(case);
    assert_eq!(code_of(&err), Some("FIRST_ADMIN_ALREADY_EXISTS"), "{case}: {err:?}");
}

#[tokio::test]
async fn the_marker_keeps_first_start_closed_after_the_last_admin_is_disabled() {
    let db = release_fresh_db().await;
    // The real first-start path records the marker in its own transaction.
    create_first_admin(&db, "chef").await;
    let sync_id: String = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id FROM user_accounts WHERE username = 'chef'".to_string(),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "entity_sync_id")
        .expect("sync id");
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM system_config WHERE key = ? AND value = ?",
            vec![FIRST_ADMIN_MARKER_KEY.into(), sync_id.clone().into()]
        )
        .await,
        1,
        "recorded by bootstrap_initial_tenant_admin"
    );
    assert!(first_admin_marker_closes(&db).await.expect("check"));

    exec(
        &db,
        "UPDATE user_accounts SET is_active = 0 WHERE username = 'chef'",
        vec![],
    )
    .await;
    assert_first_start_closed(&db, "disabling the last administrator does not reopen first start").await;
    // Nor does a restart (the startup seed reads the same marker).
    crate::db::seeder::seed_system_data_for(&db, BuildProfile::Release)
        .await
        .expect("startup seed");
    assert_first_start_closed(&db, "after a restart").await;
    exec(
        &db,
        "UPDATE user_accounts SET deleted_at = '2026-10-19T00:00:00Z' WHERE username = 'chef'",
        vec![],
    )
    .await;
    assert_first_start_closed(&db, "nor does deleting it").await;
    // A second record keeps the valid marker.
    record_first_administrator_in(&db, "another-id").await.expect("record");
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM system_config WHERE key = ? AND value = ?",
            vec![FIRST_ADMIN_MARKER_KEY.into(), sync_id.into()]
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_local_data_reset_which_wipes_the_accounts_reopens_first_start() {
    let db = release_fresh_db().await;
    create_first_admin(&db, "chef").await;
    assert!(
        !backfill_first_admin_marker(&db).await.expect("backfill"),
        "already recorded by the first-start transaction"
    );
    assert!(first_admin_marker_closes(&db).await.expect("check"));
    // The reset deletes every account and keeps system_config.
    exec(&db, "PRAGMA foreign_keys = OFF", vec![]).await;
    exec(&db, "DELETE FROM user_scope_assignments", vec![]).await;
    exec(&db, "DELETE FROM user_accounts", vec![]).await;
    exec(&db, "PRAGMA foreign_keys = ON", vec![]).await;
    assert!(!first_admin_marker_closes(&db).await.expect("check"));
}

#[tokio::test]
async fn startup_records_the_marker_of_a_pc_administered_before_it_existed() {
    let db = crate::db::test_db::seeded().await;
    // Only the dev account, password never changed: not an administrator someone created.
    assert!(!backfill_first_admin_marker(&db).await.expect("backfill"));
    assert!(!first_admin_marker_closes(&db).await.expect("check"));
    // Once its password has been changed, it is.
    exec(
        &db,
        "UPDATE user_accounts SET force_password_change = 0 WHERE username = 'admin'",
        vec![],
    )
    .await;
    reconcile_at_startup(&db, BuildProfile::Release).await.expect("startup");
    assert!(first_admin_marker_closes(&db).await.expect("check"));
}

// ── 4. Existing PCs: retirement at startup ───────────────────────────────────────────────────────

#[tokio::test]
async fn startup_disables_a_default_password_builtin_admin_once_another_admin_exists() {
    // A PC as its startup leaves it, synchronisation capture installed.
    let db = crate::sync::test_support::started_db().await;
    let before = builtin(&db).await;
    exec(
        &db,
        "INSERT INTO app_sessions (id, user_id, created_at, expires_at, last_activity_at, is_revoked)
         VALUES ('s-admin', CAST(? AS TEXT), '2026-10-19T00:00:00Z', '2099-01-01T00:00:00Z', '2026-10-19T00:00:00Z', 0)",
        vec![before.user_id.into()],
    )
    .await;
    // A named administrator an older version created (before retirement existed).
    insert_administrator(&db, "chef").await;

    // A release build at its next start (upgrade).
    crate::db::seeder::seed_system_data_for(&db, BuildProfile::Release)
        .await
        .expect("startup seed");
    let after = builtin(&db).await;
    assert!(!after.is_active, "disabled");
    assert!(!after.deleted, "a release build only disables it (history kept)");
    assert_eq!(retire_audits(&db, before.user_id).await, 1, "audited");
    assert_eq!(
        scalar(&db, "SELECT is_revoked FROM app_sessions WHERE id = 's-admin'", vec![]).await,
        1,
        "its sessions are closed"
    );
    // Nothing captured for other PCs (each PC decides for itself), while the new administrator is.
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'user_accounts' AND entity_sync_id = ?",
            vec![builtin_admin_entity_sync_id().into()]
        )
        .await,
        0
    );
    assert!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM sync_capture_journal WHERE entity_type = 'user_accounts'",
            vec![]
        )
        .await
            > 0,
        "capture is really on"
    );
    // Starting again changes nothing more.
    crate::db::seeder::seed_system_data_for(&db, BuildProfile::Release)
        .await
        .expect("startup seed");
    assert_eq!(retire_audits(&db, before.user_id).await, 1);
    // The first-start screen stays closed.
    assert!(plant_has_administrator(&db).await.expect("check"));
}

/// Creating the first administrator retires a built-in account never used, at once (this test
/// binary is a development build, so it is deleted; a release build disables it, as at startup).
#[tokio::test]
async fn creating_the_first_admin_retires_an_unused_builtin_admin() {
    let db = crate::db::test_db::seeded().await;
    let before = builtin(&db).await;
    assert!(before.is_default_password_account());
    create_first_admin(&db, "chef").await;
    let row = builtin(&db).await;
    assert!(
        row.deleted && !row.is_active,
        "retired by bootstrap_initial_tenant_admin"
    );
    assert_eq!(retire_audits(&db, row.user_id).await, 1, "audited");
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) FROM audit_events WHERE action_code = ? AND target_id = ?
               AND json_extract(details_json, '$.origin') = 'administrator_created'",
            vec![RETIRE_ACTION.into(), row.user_id.to_string().into()]
        )
        .await,
        1
    );
    // Nothing left to retire.
    assert!(
        retire_builtin_admin_if_superseded(&db, BuildProfile::Development, RetireOrigin::Startup)
            .await
            .expect("retire")
            .is_none()
    );
    // The dev seed does not bring it back.
    crate::db::seeder::seed_system_data(&db).await.expect("seed again");
    let row = builtin(&db).await;
    assert!(row.deleted && !row.is_active);
}

#[tokio::test]
async fn the_only_administrator_is_kept_and_warned() {
    let db = crate::db::test_db::seeded().await;
    reconcile_at_startup(&db, BuildProfile::Release).await.expect("startup");
    let row = builtin(&db).await;
    assert!(row.is_active, "never disabled while it is the only administrator");
    assert_eq!(retire_audits(&db, row.user_id).await, 0);

    let warning = builtin_admin_warning_for(&db, Some(row.user_id))
        .await
        .expect("warning");
    assert_eq!(
        warning,
        BuiltinAdminWarning {
            signed_in_as_builtin_admin: true,
            other_administrator_exists: false,
            password_change_pending: true,
        }
    );
    // Nobody else sees it.
    let none = builtin_admin_warning_for(&db, Some(row.user_id + 1000))
        .await
        .expect("warning");
    assert!(!none.signed_in_as_builtin_admin);
    assert!(
        !builtin_admin_warning_for(&db, None)
            .await
            .expect("warning")
            .signed_in_as_builtin_admin
    );
}

#[tokio::test]
async fn a_builtin_admin_whose_password_was_changed_is_not_retired() {
    let db = crate::db::test_db::seeded().await;
    exec(
        &db,
        "UPDATE user_accounts SET force_password_change = 0 WHERE username = 'admin'",
        vec![],
    )
    .await;
    // Another administrator, as user administration creates one.
    insert_administrator(&db, "chef").await;
    assert!(
        retire_builtin_admin_if_superseded(&db, BuildProfile::Release, RetireOrigin::Startup)
            .await
            .expect("retire")
            .is_none()
    );
    assert!(builtin(&db).await.is_active);
}

#[tokio::test]
async fn a_named_admin_called_admin_is_not_the_builtin_account() {
    let db = release_fresh_db().await;
    create_first_admin(&db, "admin").await;
    assert!(builtin_admin_row(&db).await.expect("read").is_none());
    reconcile_at_startup(&db, BuildProfile::Release).await.expect("startup");
    assert_eq!(
        scalar(
            &db,
            "SELECT is_active FROM user_accounts WHERE username = 'admin'",
            vec![]
        )
        .await,
        1
    );
}

/// An active account holding the Administrator role plant-wide (as user administration assigns it).
async fn insert_administrator(db: &DatabaseConnection, username: &str) {
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, password_hash,
             is_active, is_admin, force_password_change, failed_login_attempts, created_at, updated_at, row_version)
         VALUES (?, ?, ?, 'local', 'x', 1, 1, 0, 0, '2026-10-19T00:00:00Z', '2026-10-19T00:00:00Z', 1)",
        vec![format!("sync-{username}").into(), username.into(), username.into()],
    )
    .await;
    exec(
        db,
        "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
             created_at, updated_at, row_version)
         SELECT ?, u.id, r.id, 'tenant', NULL, '2026-10-19T00:00:00Z', '2026-10-19T00:00:00Z', 1
           FROM user_accounts u, roles r
          WHERE u.username = ? AND r.name = 'Administrator' AND r.deleted_at IS NULL",
        vec![format!("usa-{username}").into(), username.into()],
    )
    .await;
}

async fn retire_audits_from(db: &DatabaseConnection, user_id: i64, origin: RetireOrigin) -> i64 {
    scalar(
        db,
        "SELECT COUNT(*) FROM audit_events WHERE action_code = ? AND target_id = ?
           AND json_extract(details_json, '$.origin') = ?",
        vec![RETIRE_ACTION.into(), user_id.to_string().into(), origin.as_str().into()],
    )
    .await
}

// ── 4b. Sign-in and user administration (hunks D and E) ─────────────────────────────────────────

/// Hunk D: an administrator that reached this PC without a start since (synchronisation, an older
/// version) supersedes the built-in account at the next sign-in, before the account is looked up.
#[tokio::test]
async fn a_sign_in_never_finds_a_superseded_builtin_admin() {
    let db = crate::db::test_db::seeded().await;
    assert!(
        find_sign_in_account(&db, "admin").await.expect("lookup").is_some(),
        "the only administrator signs in (warning state)"
    );
    insert_administrator(&db, "chef").await;
    assert!(
        find_sign_in_account(&db, "ADMIN").await.expect("lookup").is_none(),
        "retired before the lookup, whatever the case of the username"
    );
    let row = builtin(&db).await;
    assert!(!row.is_active);
    assert_eq!(retire_audits_from(&db, row.user_id, RetireOrigin::SignIn).await, 1);
    assert!(
        find_sign_in_account(&db, "chef").await.expect("lookup").is_some(),
        "the named administrator signs in"
    );
}

/// The caller of a user-administration command (as `require_session!` returns it).
async fn session_of(db: &DatabaseConnection, username: &str) -> crate::auth::session_manager::AuthenticatedUser {
    let id = scalar(
        db,
        "SELECT id FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await;
    crate::auth::session_manager::AuthenticatedUser {
        user_id: i32::try_from(id).expect("fits"),
        username: username.into(),
        display_name: None,
        is_admin: true,
        force_password_change: false,
        tenant_id: TENANT.into(),
        token_tenant_id: TENANT.into(),
    }
}

async fn administrator_role(db: &DatabaseConnection) -> i64 {
    scalar(
        db,
        "SELECT id FROM roles WHERE name = 'Administrator' AND deleted_at IS NULL",
        vec![],
    )
    .await
}

/// Hunk E (`admin_users::create_user`): creating an administrator retires the unused built-in
/// account at once, not only at the next sign-in or start.
#[tokio::test]
async fn creating_an_administrator_in_user_administration_retires_the_builtin_admin() {
    let db = crate::db::test_db::seeded().await;
    // An administrator that arrived without a start or sign-in since (nothing retired yet).
    insert_administrator(&db, "chef").await;
    assert!(builtin(&db).await.is_default_password_account());
    let state = crate::state::AppState::new(db.clone());
    let caller = session_of(&db, "chef").await;
    let role_id = administrator_role(&db).await;
    crate::commands::admin_users::create_user_then_retire_builtin_admin(
        &state,
        &caller,
        crate::commands::admin_users::CreateUserInput {
            username: "adjoint".into(),
            identity_mode: "local".into(),
            role_id,
            scope_node_id: None,
            personnel_id: None,
            initial_password: Some("Adjoint-Usine-2026!".into()),
            force_password_change: None,
        },
    )
    .await
    .expect("created");
    let row = builtin(&db).await;
    assert!(!row.is_active, "retired by the creation");
    assert_eq!(
        retire_audits_from(&db, row.user_id, RetireOrigin::AdministratorCreated).await,
        1
    );
}

/// Hunk E (`admin_users::assign_role_scope`): giving an account an administrator role retires the
/// unused built-in account at once.
#[tokio::test]
async fn assigning_an_administrator_role_retires_the_builtin_admin() {
    let db = crate::db::test_db::seeded().await;
    insert_administrator(&db, "chef").await;
    exec(
        &db,
        "INSERT INTO user_accounts (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin,
             force_password_change, failed_login_attempts, created_at, updated_at, row_version)
         VALUES ('sync-adjoint', 'adjoint', 'local', 'x', 1, 0, 0, 0, '2026-10-19T00:00:00Z', '2026-10-19T00:00:00Z', 1)",
        vec![],
    )
    .await;
    let adjoint = scalar(&db, "SELECT id FROM user_accounts WHERE username = 'adjoint'", vec![]).await;
    let state = crate::state::AppState::new(db.clone());
    let caller = session_of(&db, "chef").await;
    let role_id = administrator_role(&db).await;
    crate::commands::admin_users::assign_role_scope_then_retire_builtin_admin(
        &state,
        &caller,
        crate::commands::admin_users::AssignRoleScopeInput {
            user_id: adjoint,
            role_id,
            scope_type: "tenant".into(),
            scope_reference: None,
            valid_from: None,
            valid_to: None,
        },
        None,
    )
    .await
    .expect("assigned");
    let row = builtin(&db).await;
    assert!(!row.is_active, "retired by the assignment");
    assert_eq!(
        retire_audits_from(&db, row.user_id, RetireOrigin::AdministratorCreated).await,
        1
    );
}

/// Review SF-2: only an administrator of the whole plant without an end date (the rule of the
/// last-administrator guard) supersedes the built-in account. A 2-hour emergency elevation, a
/// site-scoped administrator role or a plant-wide one with an end date never retires it: the plant
/// would lose its only real administrator when that right ends or does not reach account management.
#[tokio::test]
async fn only_a_lasting_plant_administrator_retires_the_builtin_admin() {
    let db = crate::db::test_db::seeded().await;
    let state = crate::state::AppState::new(db.clone());
    let role_id = administrator_role(&db).await;
    for username in ["chef_de_quart", "chef_de_site", "prestataire"] {
        exec(
            &db,
            "INSERT INTO user_accounts (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin,
                 force_password_change, failed_login_attempts, created_at, updated_at, row_version)
             VALUES (?, ?, 'local', 'x', 1, 0, 0, 0, '2026-10-19T00:00:00Z', '2026-10-19T00:00:00Z', 1)",
            vec![format!("sync-{username}").into(), username.into()],
        )
        .await;
    }
    let user = |name: &'static str| {
        let db = db.clone();
        async move {
            scalar(
                &db,
                "SELECT id FROM user_accounts WHERE username = ?",
                vec![name.into()],
            )
            .await
        }
    };

    // A 2-hour emergency elevation to Administrator, granted by the built-in account.
    crate::commands::admin_users::grant_emergency_elevation_impl(
        &state,
        &session_of(&db, "admin").await,
        crate::commands::admin_users::GrantEmergencyElevationInput {
            user_id: user("chef_de_quart").await,
            role_id,
            scope_type: "tenant".into(),
            scope_reference: None,
            reason: "Night shift cover".into(),
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339(),
        },
    )
    .await
    .expect("emergency elevation");
    // Administrator on one site only, and plant-wide with an end date.
    let end = (chrono::Utc::now() + chrono::Duration::days(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    for (username, scope_type, scope_reference, valid_to) in [
        ("chef_de_site", "site", Some("SITE-1".to_string()), None),
        ("prestataire", "tenant", None, Some(end)),
    ] {
        exec(
            &db,
            "INSERT INTO user_scope_assignments (entity_sync_id, user_id, role_id, scope_type, scope_reference,
                 valid_to, created_at, updated_at, row_version)
             VALUES (?, ?, ?, ?, ?, ?, '2026-10-19T00:00:00Z', '2026-10-19T00:00:00Z', 1)",
            vec![
                format!("usa-{username}").into(),
                user(username).await.into(),
                role_id.into(),
                scope_type.into(),
                scope_reference.map_or(sea_orm::Value::String(None), Into::into),
                valid_to.map_or(sea_orm::Value::String(None), Into::into),
            ],
        )
        .await;
    }

    assert!(
        retire_builtin_admin_if_superseded(&db, BuildProfile::Release, RetireOrigin::Startup)
            .await
            .expect("retire")
            .is_none(),
        "not superseded"
    );
    assert!(
        find_sign_in_account(&db, "admin").await.expect("lookup").is_some(),
        "still the way in"
    );
    let row = builtin(&db).await;
    assert!(row.is_active);
    assert_eq!(retire_audits(&db, row.user_id).await, 0);

    // A lasting plant administrator: retired.
    insert_administrator(&db, "chef").await;
    assert!(
        retire_builtin_admin_if_superseded(&db, BuildProfile::Release, RetireOrigin::Startup)
            .await
            .expect("retire")
            .is_some(),
        "superseded"
    );
    assert!(!builtin(&db).await.is_active);
}

// ── 5. L3: a licensed PC still reached through the never-used built-in account ───────────────────

const SIGNED_OUT: LicenceCaller = LicenceCaller {
    user_id: None,
    may_administer: false,
    step_up: false,
};
const ADMINISTERED_REFUSAL: &str = "administered";

/// A licence check result for `tenant` / `licence` on this machine, no terms stated.
fn reconciliation(tenant: &str, licence: &str) -> String {
    serde_json::json!({
        "kind": "success",
        "claim": {
            "tenant_id": tenant,
            "license_id": licence,
            "machine_fingerprint": "fp-e1",
            "activation_token": "token-e1",
        },
    })
    .to_string()
}

/// A PC an older version installed (dev seed: the built-in account, never used) that holds a
/// licence claim: not administered, yet limited as if it were.
async fn licensed_pc_with_unused_builtin_admin() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, "fp-e1", "token-e1")
        .await
        .expect("claim");
    assert!(builtin(&db).await.is_default_password_account());
    assert!(!plant_has_administrator(&db).await.expect("check"), "not administered");
    db
}

#[tokio::test]
async fn l3_a_signed_out_tenant_change_is_refused() {
    let db = licensed_pc_with_unused_builtin_admin().await;
    let err = apply_product_license_reconciliation_in(&db, SIGNED_OUT, &reconciliation("tenant-other", "lic-other"))
        .await
        .err()
        .expect("another organisation");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );
}

#[tokio::test]
async fn l3_a_signed_out_keep_or_reduce_refresh_is_still_allowed() {
    let db = licensed_pc_with_unused_builtin_admin().await;
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &reconciliation(TENANT, "test-licence"))
        .await
        .expect("refresh of the activation stored here");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );
    // While not usable (denied), a key for another organisation is refused too (before L3 a denied,
    // unadministered PC accepted it signed out).
    apply_product_license_reconciliation_in(
        &db,
        SIGNED_OUT,
        r#"{"kind":"denied","error_code":"license_revoked","error_message":"revoked"}"#,
    )
    .await
    .expect("denial recorded");
    let claim = serde_json::json!({
        "tenant_id": "tenant-other",
        "license_id": "lic-other",
        "machine_fingerprint": "fp-e1",
        "activation_token": "token-key",
    })
    .to_string();
    let err = submit_product_license_key_in(&db, SIGNED_OUT, "KEY-OTHER-0001", Some(claim), None, None)
        .await
        .expect_err("another organisation by key");
    assert_eq!(code_of(&err), Some("LICENSE_RECONCILE_ADMIN_REQUIRED"), "{err:?}");
    assert_eq!(
        get_activation_claim_tenant_id(&db).await.expect("tenant").as_deref(),
        Some(TENANT)
    );
}

#[tokio::test]
async fn l3_a_signed_out_reset_is_refused() {
    let db = licensed_pc_with_unused_builtin_admin().await;
    let err = authorize_signed_out_reset(&db, ADMINISTERED_REFUSAL)
        .await
        .expect_err("pre-administered");
    assert_eq!(code_of(&err), Some("LICENSE_RESET_BUILTIN_ADMIN_PENDING"), "{err:?}");
}

#[tokio::test]
async fn l3_the_first_admin_is_still_allowed_and_replaces_the_builtin_account() {
    let db = licensed_pc_with_unused_builtin_admin().await;
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
    bootstrap_initial_tenant_admin_in(&db, first_admin("chef"))
        .await
        .expect("first start stays open");
    assert!(!builtin(&db).await.is_active, "the built-in account is retired");
    assert!(plant_has_administrator(&db).await.expect("check"));
    // Now an ordinary administered plant: the existing refusal applies.
    let err = authorize_signed_out_reset(&db, ADMINISTERED_REFUSAL)
        .await
        .expect_err("administered");
    assert!(
        matches!(&err, AppError::Auth(m) if m == ADMINISTERED_REFUSAL),
        "{err:?}"
    );
}

/// The L3 limits need both conditions: without a stored claim (a true first start) or without the
/// built-in account (a release install), signed-out first-start actions stay open.
#[tokio::test]
async fn l3_does_not_apply_to_a_true_first_start() {
    // Built-in account, no claim.
    let db = crate::db::test_db::seeded().await;
    authorize_signed_out_reset(&db, ADMINISTERED_REFUSAL)
        .await
        .expect("no claim: first start");
    // Claim, no built-in account (release install).
    let db = release_fresh_db().await;
    record_activation_claim_for_tests(&db, TENANT, "fp-e1", "token-e1")
        .await
        .expect("claim");
    authorize_signed_out_reset(&db, ADMINISTERED_REFUSAL)
        .await
        .expect("no built-in account: first start");
    apply_product_license_reconciliation_in(&db, SIGNED_OUT, &reconciliation("tenant-other", "lic-other"))
        .await
        .expect("first start: no session needed");
    // Built-in account disabled (retired): no longer a way in, so no longer limiting.
    let db = licensed_pc_with_unused_builtin_admin().await;
    exec(
        &db,
        "UPDATE user_accounts SET is_active = 0 WHERE username = 'admin'",
        vec![],
    )
    .await;
    authorize_signed_out_reset(&db, ADMINISTERED_REFUSAL)
        .await
        .expect("disabled built-in account");
}

// ── 6. Synchronisation ───────────────────────────────────────────────────────────────────────────

mod sync_arrival {
    use super::*;
    use crate::sync::domain::{ApplySyncBatchInput, SyncInboundItemInput, SYNC_PROTOCOL_VERSION_V1};

    /// The built-in account as another PC sends it (its stored payload), with a later version.
    async fn sent_by_another_pc(row_version: i64, display_name: &str) -> SyncInboundItemInput {
        let other = crate::sync::test_support::started_db().await;
        let mut payload = crate::sync::test_support::current_payload(
            &other,
            "user_accounts",
            &builtin_admin_entity_sync_id(),
            &serde_json::Map::new(),
        )
        .await
        .expect("payload")
        .expect("the other PC has the built-in account");
        payload.insert("display_name".into(), display_name.into());
        assert_eq!(payload.get("force_password_change"), Some(&serde_json::json!(1)));
        SyncInboundItemInput {
            entity_type: "user_accounts".into(),
            entity_sync_id: builtin_admin_entity_sync_id(),
            operation: "upsert".into(),
            row_version,
            payload_json: serde_json::Value::Object(payload).to_string(),
            ..Default::default()
        }
    }

    async fn receive(db: &DatabaseConnection, item: SyncInboundItemInput, batch: &str) {
        crate::sync::queries::apply_sync_batch(
            db,
            ApplySyncBatchInput {
                protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
                server_batch_id: batch.into(),
                checkpoint_token: format!("ckpt-{batch}"),
                acknowledged_items: vec![],
                rejected_items: vec![],
                inbound_items: vec![item],
                conflict_items: vec![],
                policy_metadata_json: None,
            },
        )
        .await
        .expect("ingest");
        let result = crate::sync::materialize::materialize_inbox_batch(db, Some(10))
            .await
            .expect("materialize");
        assert!(result.errors.is_empty(), "{result:?}");
        assert_eq!(result.applied, 1, "{result:?}");
    }

    #[tokio::test]
    async fn received_on_a_pc_that_never_had_it_it_arrives_disabled() {
        let db = release_fresh_db().await;
        crate::sync::capture::install_capture(&db).await.expect("capture");
        receive(&db, sent_by_another_pc(2, "Administrateur Maintafox").await, "b1").await;
        let row = builtin(&db).await;
        assert!(
            !row.is_active,
            "a PC without it does not gain a default-password administrator"
        );
        assert_eq!(retire_audits(&db, row.user_id).await, 1);
    }

    #[tokio::test]
    async fn received_where_another_admin_exists_it_is_disabled() {
        let db = crate::sync::test_support::started_db().await;
        insert_administrator(&db, "chef").await;
        // A later version from another PC, still with its password never changed and active.
        receive(&db, sent_by_another_pc(5, "Admin (PC A)").await, "b2").await;
        let row = builtin(&db).await;
        assert!(!row.is_active);
        assert_eq!(retire_audits(&db, row.user_id).await, 1);
    }

    #[tokio::test]
    async fn received_where_it_is_the_only_admin_it_stays() {
        let db = crate::sync::test_support::started_db().await;
        receive(&db, sent_by_another_pc(5, "Admin (PC A)").await, "b3").await;
        let row = builtin(&db).await;
        assert!(
            row.is_active,
            "the only administrator of this PC is kept (warning state)"
        );
        assert_eq!(retire_audits(&db, row.user_id).await, 0);
    }

    /// Review SF-1, scenario A: a release PC that never had the account receives it (disabled on
    /// arrival), then a later version of the same row, which carries `is_active = 1`, while this PC
    /// has no other active administrator. The account stays disabled.
    #[tokio::test]
    async fn a_later_version_never_re_enables_it_on_a_pc_that_never_had_it() {
        let db = release_fresh_db().await;
        crate::sync::capture::install_capture(&db).await.expect("capture");
        receive(&db, sent_by_another_pc(2, "Administrateur Maintafox").await, "b4").await;
        assert!(!builtin(&db).await.is_active);
        assert!(
            builtin_admin_retired_here(&db).await.expect("marker"),
            "retirement recorded"
        );

        receive(&db, sent_by_another_pc(3, "Admin (renamed on PC A)").await, "b5").await;
        let row = builtin(&db).await;
        assert!(
            !other_active_administrator_exists(&db, Some(row.user_id))
                .await
                .expect("check"),
            "no other administrator on this PC at that moment"
        );
        assert!(
            !row.is_active,
            "a later version never re-enables a default-password administrator this PC retired"
        );
        // The later version's re-enable names nobody: held back on arrival
        // (`sync::credential_check`) and audited there, so the account is never active again here
        // and needs no second retirement.
        assert_eq!(retire_audits(&db, row.user_id).await, 1, "the first arrival retired it");
        assert_eq!(
            scalar(
                &db,
                "SELECT COUNT(*) FROM audit_events WHERE action_code = ?",
                vec![crate::sync::credential_check::ACCOUNT_REENABLE_UNAUTHORISED.into()],
            )
            .await,
            1,
            "the second arrival's re-enable audited"
        );
    }

    /// Review SF-1, scenario B: an upgraded PC retired its own built-in account because a named
    /// administrator existed; that administrator is later disabled; a later version of the
    /// built-in row arrives. The account stays retired.
    #[tokio::test]
    async fn a_later_version_never_re_enables_it_after_a_local_retirement() {
        let db = crate::sync::test_support::started_db().await;
        insert_administrator(&db, "chef").await;
        reconcile_at_startup(&db, BuildProfile::Release).await.expect("startup");
        let retired = builtin(&db).await;
        assert!(!retired.is_active, "retired at startup");
        assert!(builtin_admin_retired_here(&db).await.expect("marker"));

        exec(
            &db,
            "UPDATE user_accounts SET is_active = 0 WHERE username = 'chef'",
            vec![],
        )
        .await;
        assert!(!other_active_administrator_exists(&db, Some(retired.user_id))
            .await
            .expect("check"));

        receive(&db, sent_by_another_pc(5, "Admin (PC A)").await, "b6").await;
        let row = builtin(&db).await;
        assert!(!row.is_active, "still retired with no other active administrator");
        assert!(
            find_sign_in_account(&db, "admin").await.expect("lookup").is_none(),
            "and it cannot sign in"
        );
    }
}
