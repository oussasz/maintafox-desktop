//! The last active administrator: commands, synchronisation, lockout and warning, on real
//! in-memory databases (`db::test_db::seeded`, whose built-in "admin" password was never changed).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::*;
use crate::auth::session_manager::AuthenticatedUser;
use crate::commands::admin_users::{
    assign_role_scope_impl, revoke_assignment_impl, set_user_active_impl, AssignRoleScopeInput, SetUserActiveInput,
};
use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};

async fn scalar_i64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "v")
        .expect("decode")
}

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("exec");
}

async fn role_id(db: &DatabaseConnection, name: &str) -> i64 {
    scalar_i64(db, "SELECT id AS v FROM roles WHERE name = ?", vec![name.into()]).await
}

async fn builtin_id(db: &DatabaseConnection) -> i64 {
    scalar_i64(db, "SELECT id AS v FROM user_accounts WHERE username = 'admin'", vec![]).await
}

/// The seeded built-in account, signed in: it may manage accounts (bootstrap flag), but it is not
/// counted as an administrator while its password was never changed.
async fn builtin_caller(db: &DatabaseConnection) -> AuthenticatedUser {
    AuthenticatedUser {
        user_id: i32::try_from(builtin_id(db).await).expect("fits"),
        username: "admin".into(),
        display_name: None,
        is_admin: true,
        force_password_change: false,
        tenant_id: "tenant-test".into(),
        token_tenant_id: "tenant-test".into(),
    }
}

/// A named, active account (password chosen) holding the Administrator role plant-wide; returns
/// (user id, assignment id).
async fn named_admin(db: &DatabaseConnection, username: &str) -> (i64, i64) {
    let now = chrono::Utc::now().to_rfc3339();
    exec(
        db,
        "INSERT INTO user_accounts \
         (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin, \
          force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version) \
         VALUES (?, ?, 'local', 'x', 1, 0, 0, ?, 0, ?, ?, 1)",
        vec![
            format!("{username}-sync").into(),
            username.into(),
            now.clone().into(),
            now.clone().into(),
            now.clone().into(),
        ],
    )
    .await;
    let user = scalar_i64(
        db,
        "SELECT id AS v FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await;
    let admin_role = role_id(db, "Administrator").await;
    exec(
        db,
        "INSERT INTO user_scope_assignments \
         (entity_sync_id, user_id, role_id, scope_type, scope_reference, created_at, updated_at, row_version) \
         VALUES (?, ?, ?, 'tenant', NULL, ?, ?, 1)",
        vec![
            format!("{username}-assignment").into(),
            user.into(),
            admin_role.into(),
            now.clone().into(),
            now.into(),
        ],
    )
    .await;
    let assignment = scalar_i64(
        db,
        "SELECT id AS v FROM user_scope_assignments WHERE entity_sync_id = ?",
        vec![format!("{username}-assignment").into()],
    )
    .await;
    (user, assignment)
}

fn is_last_admin_refusal(result: &AppResult<impl std::fmt::Debug>) -> bool {
    matches!(result, Err(AppError::OrgValidationFailed(issues))
        if issues.iter().any(|i| i.code == LAST_ADMINISTRATOR_REQUIRED))
}

async fn is_active(db: &DatabaseConnection, user: i64) -> bool {
    scalar_i64(
        db,
        "SELECT is_active AS v FROM user_accounts WHERE id = ?",
        vec![user.into()],
    )
    .await
        == 1
}

fn disable(user_id: i64) -> SetUserActiveInput {
    SetUserActiveInput {
        user_id,
        reason: "Left the plant".into(),
    }
}

fn readonly_at_tenant(user_id: i64, role_id: i64) -> AssignRoleScopeInput {
    AssignRoleScopeInput {
        user_id,
        role_id,
        scope_type: "tenant".into(),
        scope_reference: None,
        valid_from: None,
        valid_to: None,
    }
}

#[tokio::test]
async fn the_builtin_account_with_its_password_never_changed_is_not_counted() {
    let db = crate::db::test_db::seeded().await;
    let builtin = builtin_id(&db).await;
    assert!(
        crate::auth::rbac::user_has_elevated_admin_access(&db, i32::try_from(builtin).unwrap())
            .await
            .unwrap(),
        "the built-in account has administrator access"
    );
    assert_eq!(active_administrator_ids(&db).await.unwrap(), Vec::<i64>::new());

    let (named, _) = named_admin(&db, "named_admin").await;
    assert_eq!(active_administrator_ids(&db).await.unwrap(), vec![named]);

    // Once its password was chosen it is an administrator like any other.
    exec(
        &db,
        "UPDATE user_accounts SET password_changed_at = ?, force_password_change = 0 WHERE id = ?",
        vec![chrono::Utc::now().to_rfc3339().into(), builtin.into()],
    )
    .await;
    assert_eq!(active_administrator_ids(&db).await.unwrap(), vec![builtin, named]);
}

#[tokio::test]
async fn disabling_the_last_active_administrator_is_refused() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = builtin_caller(&db).await;
    let (only, _) = named_admin(&db, "only_admin").await;

    // The never-changed built-in account remains, yet it does not count: refused.
    let refused = set_user_active_impl(&state, &caller, disable(only), false).await;
    assert!(is_last_admin_refusal(&refused), "{refused:?}");
    assert!(is_active(&db, only).await, "rolled back");
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) AS v FROM admin_change_events WHERE action = 'user_deactivated' AND target_user_id = ?",
            vec![only.into()],
        )
        .await,
        0,
        "no ledger entry for a refused change"
    );

    // With a second active administrator the same change is allowed.
    named_admin(&db, "second_admin").await;
    set_user_active_impl(&state, &caller, disable(only), false)
        .await
        .expect("no longer the last one");
    assert!(!is_active(&db, only).await);
}

#[tokio::test]
async fn disabling_is_allowed_once_the_builtin_password_was_changed() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = builtin_caller(&db).await;
    let (only, _) = named_admin(&db, "named_only").await;
    exec(
        &db,
        "UPDATE user_accounts SET password_changed_at = ?, force_password_change = 0 WHERE username = 'admin'",
        vec![chrono::Utc::now().to_rfc3339().into()],
    )
    .await;
    set_user_active_impl(&state, &caller, disable(only), false)
        .await
        .expect("the built-in account, its password chosen, remains an administrator");
}

#[tokio::test]
async fn demoting_or_withdrawing_the_last_administrator_role_is_refused() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = builtin_caller(&db).await;
    let readonly = role_id(&db, "Readonly").await;
    let (only, assignment) = named_admin(&db, "demote_admin").await;

    // Demotion: the administrator role replaced by another at the same scope.
    let demoted = assign_role_scope_impl(&state, &caller, readonly_at_tenant(only, readonly), false, None).await;
    assert!(is_last_admin_refusal(&demoted), "{demoted:?}");
    // Withdrawal of the administrator assignment.
    let revoked = revoke_assignment_impl(&state, &caller, assignment, Some("Handover"), false).await;
    assert!(is_last_admin_refusal(&revoked), "{revoked:?}");
    assert_eq!(
        active_administrator_ids(&db).await.unwrap(),
        vec![only],
        "nothing changed"
    );

    // With a second administrator both are allowed.
    let (second, _) = named_admin(&db, "demote_second").await;
    assign_role_scope_impl(&state, &caller, readonly_at_tenant(only, readonly), false, None)
        .await
        .expect("demotion allowed");
    assert_eq!(active_administrator_ids(&db).await.unwrap(), vec![second]);
}

#[tokio::test]
async fn withdrawing_the_role_is_allowed_with_a_second_administrator() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let caller = builtin_caller(&db).await;
    let (_, assignment) = named_admin(&db, "withdraw_admin").await;
    let (second, _) = named_admin(&db, "withdraw_second").await;
    revoke_assignment_impl(&state, &caller, assignment, Some("Handover"), false)
        .await
        .expect("not the last one");
    assert_eq!(active_administrator_ids(&db).await.unwrap(), vec![second]);
}

// ── Synchronisation ──

async fn payload(
    db: &DatabaseConnection,
    entity_type: &str,
    sync_id: &str,
) -> serde_json::Map<String, serde_json::Value> {
    crate::sync::test_support::current_payload(db, entity_type, sync_id, &Default::default())
        .await
        .expect("payload")
        .expect("row")
}

async fn receive(
    db: &DatabaseConnection,
    entity_type: &str,
    sync_id: &str,
    operation: &str,
    row: serde_json::Map<String, serde_json::Value>,
) -> ApplyOutcome {
    apply_inbound_payload(
        db,
        entity_type,
        sync_id,
        operation,
        2,
        &serde_json::Value::Object(row).to_string(),
    )
    .await
    .expect("apply")
}

fn is_last_admin_conflict(outcome: &ApplyOutcome) -> bool {
    matches!(outcome, ApplyOutcome::Conflict { code, .. } if code == LAST_ADMINISTRATOR_REQUIRED)
}

#[tokio::test]
async fn a_received_change_leaving_no_administrator_is_a_conflict() {
    let db = crate::db::test_db::seeded().await;
    crate::sync::capture::install_capture_in(&db)
        .await
        .expect("capture + seeded identities");
    let (only, _) = named_admin(&db, "sync_admin").await;

    // The account received disabled.
    let mut account = payload(&db, "user_accounts", "sync_admin-sync").await;
    assert!(account.contains_key("is_active"), "{account:?}");
    let disabled = match account.get("is_active") {
        Some(serde_json::Value::Bool(_)) => serde_json::Value::Bool(false),
        _ => serde_json::Value::from(0),
    };
    account.insert("is_active".into(), disabled);
    let outcome = receive(&db, "user_accounts", "sync_admin-sync", "upsert", account.clone()).await;
    assert!(is_last_admin_conflict(&outcome), "{outcome:?}");
    assert!(is_active(&db, only).await, "not applied");

    // The administrator assignment received withdrawn.
    let mut assignment = payload(&db, "user_scope_assignments", "sync_admin-assignment").await;
    assert!(assignment.contains_key("deleted_at"), "{assignment:?}");
    assignment.insert("deleted_at".into(), chrono::Utc::now().to_rfc3339().into());
    let outcome = receive(
        &db,
        "user_scope_assignments",
        "sync_admin-assignment",
        "upsert",
        assignment.clone(),
    )
    .await;
    assert!(is_last_admin_conflict(&outcome), "{outcome:?}");

    // The administrator assignment received deleted.
    let outcome = receive(
        &db,
        "user_scope_assignments",
        "sync_admin-assignment",
        "delete",
        assignment.clone(),
    )
    .await;
    assert!(is_last_admin_conflict(&outcome), "{outcome:?}");
    assert_eq!(
        active_administrator_ids(&db).await.unwrap(),
        vec![only],
        "nothing applied"
    );

    // With a second administrator the same changes are applied.
    named_admin(&db, "sync_second").await;
    let outcome = receive(&db, "user_accounts", "sync_admin-sync", "upsert", account).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(!is_active(&db, only).await, "applied");
}

#[tokio::test]
async fn an_unrelated_received_change_is_applied() {
    let db = crate::db::test_db::seeded().await;
    crate::sync::capture::install_capture_in(&db)
        .await
        .expect("capture + seeded identities");
    named_admin(&db, "sync_only_admin").await;
    let mut account = payload(&db, "user_accounts", "sync_only_admin-sync").await;
    account.insert("display_name".into(), "Renamed on another PC".into());
    let outcome = receive(&db, "user_accounts", "sync_only_admin-sync", "upsert", account).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

// ── Lockout ──

async fn locked_until(db: &DatabaseConnection, user: i64) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT locked_until FROM user_accounts WHERE id = ?",
        [user.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get::<Option<String>>("", "locked_until")
    .unwrap()
}

fn instant(raw: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .expect("rfc3339")
        .with_timezone(&chrono::Utc)
}

/// A long, non-progressive policy (as stored values may hold): one failed attempt locks.
fn long_policy() -> crate::auth::lockout::LockoutPolicy {
    crate::auth::lockout::LockoutPolicy {
        max_attempts: 1,
        lockout_minutes: 100_000,
        progressive: false,
    }
}

#[tokio::test]
async fn the_last_administrator_lock_is_short_and_expires() {
    let db = crate::db::test_db::seeded().await;
    let (only, _) = named_admin(&db, "lock_admin").await;
    let uid = i32::try_from(only).unwrap();

    crate::auth::lockout::record_failed_attempt(&db, uid, &long_policy())
        .await
        .unwrap();
    let until = locked_until(&db, only).await.expect("locked");
    let bound = chrono::Utc::now() + chrono::Duration::minutes(LAST_ADMIN_MAX_LOCKOUT_MINUTES + 1);
    assert!(instant(&until) <= bound, "{until}");
    assert!(matches!(
        crate::auth::lockout::check_lockout(&db, uid).await,
        Err(AppError::AccountLocked { .. })
    ));

    // A longer lock stored earlier (or unreadable) is brought back to the bound.
    exec(
        &db,
        "UPDATE user_accounts SET locked_until = '2999-01-01T00:00:00Z' WHERE id = ?",
        vec![only.into()],
    )
    .await;
    match crate::auth::lockout::check_lockout(&db, uid).await {
        Err(AppError::AccountLocked { until }) => assert!(instant(&until) <= bound, "{until}"),
        other => panic!("still locked, bounded: {other:?}"),
    }
    assert!(
        instant(&locked_until(&db, only).await.unwrap()) <= bound,
        "stored bounded"
    );

    // Its end passed: the account opens again.
    exec(
        &db,
        "UPDATE user_accounts SET locked_until = '2000-01-01T00:00:00Z' WHERE id = ?",
        vec![only.into()],
    )
    .await;
    crate::auth::lockout::check_lockout(&db, uid)
        .await
        .expect("lock expired");
    assert_eq!(locked_until(&db, only).await, None);
}

#[tokio::test]
async fn another_account_keeps_the_policy_lock() {
    let db = crate::db::test_db::seeded().await;
    let (first, _) = named_admin(&db, "lock_first").await;
    named_admin(&db, "lock_second").await;
    crate::auth::lockout::record_failed_attempt(&db, i32::try_from(first).unwrap(), &long_policy())
        .await
        .unwrap();
    let until = locked_until(&db, first).await.expect("locked");
    assert!(
        instant(&until) > chrono::Utc::now() + chrono::Duration::minutes(1_000),
        "not the last administrator: the policy applies ({until})"
    );
}

// ── Warning ──

#[tokio::test]
async fn the_warning_shows_while_one_administrator_and_hides_with_two() {
    let db = crate::db::test_db::seeded().await;
    let (first, _) = named_admin(&db, "warn_first").await;
    let builtin = builtin_id(&db).await;
    assert!(
        single_administrator_warning_for(&db, Some(first))
            .await
            .unwrap()
            .sole_active_administrator
    );
    assert!(
        !single_administrator_warning_for(&db, Some(builtin))
            .await
            .unwrap()
            .sole_active_administrator
    );
    assert!(
        !single_administrator_warning_for(&db, None)
            .await
            .unwrap()
            .sole_active_administrator
    );

    named_admin(&db, "warn_second").await;
    assert!(
        !single_administrator_warning_for(&db, Some(first))
            .await
            .unwrap()
            .sole_active_administrator
    );
}
