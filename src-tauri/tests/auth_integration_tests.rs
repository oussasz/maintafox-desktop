//! SP04-F04 S2 — Auth integration tests (external).
//!
//! Eight tests that exercise the auth subsystem end-to-end against
//! a real in-memory SQLite database with migrations + seeded data.
//!
//! These are external integration tests referencing `maintafox_lib` as a
//! library crate — they can only access `pub` items.

mod common;

use maintafox_lib::audit;
use maintafox_lib::auth::password;
use maintafox_lib::auth::session_manager::{self, AuthenticatedUser, SessionManager};
use maintafox_lib::errors::AppError;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

// ── T1 — Login success creates a valid session ─────────────────────────

#[tokio::test]
async fn t1_login_success_creates_session() {
    let db = common::create_test_db().await;

    let (user_id, username, display_name, is_admin, force_pw, pw_hash) =
        session_manager::find_active_user(&db, "admin")
            .await
            .expect("query should not fail")
            .expect("T1 FAIL: admin user not seeded");

    let stored_hash = pw_hash.expect("T1 FAIL: admin has no password_hash");
    let ok = password::verify_password("Admin#2026!", &stored_hash).expect("T1 FAIL: verify_password returned Err");
    assert!(ok, "T1 FAIL: correct password did not verify");

    let mut mgr = SessionManager::new();
    let session = mgr.create_session(AuthenticatedUser {
        user_id,
        username,
        display_name,
        is_admin,
        force_password_change: force_pw,
    });

    session_manager::record_successful_login(&db, user_id)
        .await
        .expect("T1 FAIL: record_successful_login failed");
    session_manager::create_session_record(&db, &session.session_db_id, user_id, &session.expires_at.to_rfc3339())
        .await
        .expect("T1 FAIL: create_session_record failed");

    let info = mgr.session_info();
    assert!(info.is_authenticated, "T1 FAIL: not authenticated after login");
    assert_eq!(info.username.as_deref(), Some("admin"), "T1 FAIL: username mismatch");
    assert_eq!(info.is_admin, Some(true), "T1 FAIL: is_admin should be true");
}

// ── T2 — Wrong password gives opaque error ─────────────────────────────

#[tokio::test]
async fn t2_wrong_password_opaque_error() {
    let db = common::create_test_db().await;

    let (_user_id, _username, _display_name, _is_admin, _force_pw, pw_hash) =
        session_manager::find_active_user(&db, "admin")
            .await
            .expect("query")
            .expect("admin must exist");

    let stored_hash = pw_hash.expect("admin must have hash");
    let ok = password::verify_password("wrong_password", &stored_hash).expect("verify should not error on valid hash");
    assert!(!ok, "T2 FAIL: wrong password should not verify");

    let err = AppError::Auth("Identifiant ou mot de passe invalide.".into());
    let msg = err.to_string();
    assert!(!msg.contains("not found"), "T2 FAIL: error leaks user status");
    assert!(!msg.contains("wrong password"), "T2 FAIL: error leaks password status");
    assert!(!msg.contains("locked"), "T2 FAIL: error leaks lock status");

    let serialized = serde_json::to_value(&err).expect("serialize");
    assert_eq!(serialized["code"], "AUTH_ERROR", "T2 FAIL: code mismatch");
}

// ── T3 — Non-existent user gets same opaque error ──────────────────────

#[tokio::test]
async fn t3_nonexistent_user_opaque_error() {
    let db = common::create_test_db().await;

    let result = session_manager::find_active_user(&db, "does_not_exist")
        .await
        .expect("query should not fail");
    assert!(result.is_none(), "T3 FAIL: ghost user should return None");

    let err = AppError::Auth("Identifiant ou mot de passe invalide.".into());
    let serialized = serde_json::to_value(&err).expect("serialize");
    assert_eq!(serialized["code"], "AUTH_ERROR", "T3 FAIL: code mismatch");
}

// ── T4 — Logout clears session ─────────────────────────────────────────

#[tokio::test]
async fn t4_logout_clears_session() {
    let db = common::create_test_db().await;

    let (user_id, username, display_name, is_admin, force_pw, _) = session_manager::find_active_user(&db, "admin")
        .await
        .expect("query")
        .expect("admin exists");

    let mut mgr = SessionManager::new();
    mgr.create_session(AuthenticatedUser {
        user_id,
        username,
        display_name,
        is_admin,
        force_password_change: force_pw,
    });
    assert!(mgr.is_authenticated(), "T4: should be authenticated after login");

    let cleared = mgr.clear_session();
    assert!(cleared.is_some(), "T4 FAIL: clear_session should return session id");

    let info = mgr.session_info();
    assert!(!info.is_authenticated, "T4 FAIL: still authenticated after logout");
    assert!(info.user_id.is_none(), "T4 FAIL: user_id not cleared");
    assert!(info.username.is_none(), "T4 FAIL: username not cleared");
}

// ── T5 — Audit event for login.success is written ──────────────────────

#[tokio::test]
async fn t5_audit_login_success_event() {
    let db = common::create_test_db().await;

    audit::emit(
        &db,
        audit::AuditEvent {
            event_type: audit::event_type::LOGIN_SUCCESS,
            actor_id: Some(1),
            actor_name: Some("admin"),
            summary: "Successful login",
            detail_json: Some(r#"{"offline":false}"#.to_string()),
            ..Default::default()
        },
    )
    .await;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT action_code, actor_id, details_json FROM audit_events WHERE action_code = ?",
            [audit::event_type::LOGIN_SUCCESS.into()],
        ))
        .await
        .expect("query")
        .expect("T5 FAIL: no login.success row in audit_events");

    let code: String = row.try_get("", "action_code").unwrap();
    let actor: i64 = row.try_get("", "actor_id").unwrap();
    let details_raw: String = row.try_get("", "details_json").unwrap();
    let details: serde_json::Value = serde_json::from_str(&details_raw).expect("T5: details_json");

    assert_eq!(code, "login.success", "T5 FAIL: action_code mismatch");
    assert_eq!(actor, 1, "T5 FAIL: actor_id mismatch");
    assert_eq!(
        details["summary"].as_str(),
        Some("Successful login"),
        "T5 FAIL: summary in details_json mismatch"
    );
}

// ── T6 — Audit event for login.failure has summary + detail_json ───────

#[tokio::test]
async fn t6_audit_login_failure_event() {
    let db = common::create_test_db().await;

    audit::emit(
        &db,
        audit::AuditEvent {
            event_type: audit::event_type::LOGIN_FAILURE,
            summary: "Failed login attempt — wrong password",
            detail_json: Some(r#"{"username_provided":true}"#.to_string()),
            ..Default::default()
        },
    )
    .await;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT action_code, details_json FROM audit_events WHERE action_code = ?",
            [audit::event_type::LOGIN_FAILURE.into()],
        ))
        .await
        .expect("query")
        .expect("T6 FAIL: no login.failure row in audit_events");

    let code: String = row.try_get("", "action_code").unwrap();
    assert_eq!(code, "login.failure", "T6 FAIL: action_code mismatch");

    let details_raw: String = row.try_get("", "details_json").unwrap();
    let details: serde_json::Value = serde_json::from_str(&details_raw).expect("T6: details_json");
    assert_eq!(
        details["summary"].as_str(),
        Some("Failed login attempt — wrong password"),
        "T6 FAIL: summary in details_json mismatch"
    );

    let nested = details
        .get("detail")
        .expect("T6 FAIL: expected detail object in details_json");
    assert!(
        nested.to_string().contains("username_provided"),
        "T6 FAIL: nested detail missing username_provided"
    );
}

// ── T7 — Session expires_at is ~8 hours from creation ──────────────────

#[tokio::test]
async fn t7_session_expires_in_8_hours() {
    let db = common::create_test_db().await;

    let (user_id, username, display_name, is_admin, force_pw, _) = session_manager::find_active_user(&db, "admin")
        .await
        .expect("query")
        .expect("admin exists");

    let before = chrono::Utc::now();
    let mut mgr = SessionManager::new();
    let session = mgr.create_session(AuthenticatedUser {
        user_id,
        username,
        display_name,
        is_admin,
        force_password_change: force_pw,
    });
    let after = chrono::Utc::now();

    let expected_min = before + chrono::Duration::hours(8);
    let expected_max = after + chrono::Duration::hours(8);

    assert!(
        session.expires_at >= expected_min && session.expires_at <= expected_max,
        "T7 FAIL: expires_at not within 8h window: {:?}",
        session.expires_at,
    );
}

// ── T8 — Seeder creates at least 56 permissions ────────────────────────

#[tokio::test]
async fn t8_seeder_creates_minimum_permissions() {
    let db = common::create_test_db().await;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM permissions",
            [],
        ))
        .await
        .expect("query should not fail")
        .expect("T8 FAIL: permissions table empty or missing");

    let count: i32 = row.try_get("", "cnt").unwrap();
    assert!(
        count >= 56,
        "T8 FAIL: expected at least 56 seeded permissions, found {count}"
    );
}
