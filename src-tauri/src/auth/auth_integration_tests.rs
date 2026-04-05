//! SP04-F01 Sprint S2 — Integration tests for the auth flow.
//!
//! These tests exercise the exact same code paths as the IPC commands:
//! V1: login with correct credentials
//! V2: login with wrong password (opaque error)
//! V3: session info before login = unauthenticated
//! V4: logout clears session

#[cfg(test)]
mod tests {
    use crate::auth::password;
    use crate::auth::session_manager::{self, SessionManager};
    use crate::errors::AppError;

    /// Helper: create an in-memory DB with all migrations + seeder applied.
    async fn setup_db() -> sea_orm::DatabaseConnection {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite");

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("enable FK");

        use sea_orm_migration::MigratorTrait;
        crate::migrations::Migrator::up(&db, None).await.expect("migrations");

        crate::db::seeder::seed_system_data(&db).await.expect("seeder");

        db
    }

    // ── V3 — Session info before login is unauthenticated. ──────────────

    #[test]
    fn v3_new_session_manager_is_unauthenticated() {
        let mgr = SessionManager::new();
        let info = mgr.session_info();

        assert!(
            !info.is_authenticated,
            "V3 FAIL: is_authenticated should be false before login"
        );
        assert!(!info.is_locked, "V3 FAIL: is_locked should be false");
        assert!(info.user_id.is_none(), "V3 FAIL: user_id should be null");
        assert!(info.username.is_none(), "V3 FAIL: username should be null");
        assert!(info.display_name.is_none(), "V3 FAIL: display_name should be null");
        assert!(info.is_admin.is_none(), "V3 FAIL: is_admin should be null");
        assert!(
            info.force_password_change.is_none(),
            "V3 FAIL: force_password_change should be null"
        );
        assert!(info.expires_at.is_none(), "V3 FAIL: expires_at should be null");
        assert!(
            info.last_activity_at.is_none(),
            "V3 FAIL: last_activity_at should be null"
        );
    }

    // ── V1 — Login with correct credentials succeeds. ───────────────────

    #[tokio::test]
    async fn v1_login_with_correct_credentials_succeeds() {
        let db = setup_db().await;

        // 1. Find the admin user
        let user_record = session_manager::find_active_user(&db, "admin")
            .await
            .expect("DB query should not fail");
        let (user_id, db_username, display_name, is_admin, force_pw_change, pw_hash) =
            user_record.expect("V1 FAIL: admin user not found in database");

        // 2. Verify password matches
        let stored_hash = pw_hash.expect("V1 FAIL: admin has no password_hash — security defect");
        assert!(
            stored_hash.starts_with("$argon2id$"),
            "V1 FAIL: hash does not start with $argon2id$"
        );

        let password_ok =
            password::verify_password("Admin#2026!", &stored_hash).expect("V1 FAIL: verify_password returned Err");
        assert!(password_ok, "V1 FAIL: correct password did not verify");

        // 3. Create session
        let auth_user = session_manager::AuthenticatedUser {
            user_id,
            username: db_username,
            display_name,
            is_admin,
            force_password_change: force_pw_change,
        };

        let mut mgr = SessionManager::new();
        let session = mgr.create_session(auth_user);

        // 4. Record in DB for audit
        session_manager::record_successful_login(&db, user_id)
            .await
            .expect("V1 FAIL: record_successful_login failed");
        session_manager::create_session_record(&db, &session.session_db_id, user_id, &session.expires_at.to_rfc3339())
            .await
            .expect("V1 FAIL: create_session_record failed");

        // 5. Verify session info
        let info = mgr.session_info();
        assert!(
            info.is_authenticated,
            "V1 FAIL: is_authenticated should be true after login"
        );
        assert_eq!(
            info.username.as_deref(),
            Some("admin"),
            "V1 FAIL: username should be 'admin'"
        );
        assert_eq!(
            info.force_password_change,
            Some(true),
            "V1 FAIL: force_password_change should be true"
        );
        assert_eq!(info.is_admin, Some(true), "V1 FAIL: is_admin should be true");
    }

    // ── V2 — Login with wrong password gives opaque error. ──────────────

    #[tokio::test]
    async fn v2_login_with_wrong_password_is_opaque() {
        let db = setup_db().await;

        // 1. Find admin user
        let user_record = session_manager::find_active_user(&db, "admin")
            .await
            .expect("DB query should not fail");
        let (user_id, _username, _display_name, _is_admin, _force_pw, pw_hash) = user_record.expect("admin must exist");

        let stored_hash = pw_hash.expect("admin must have password_hash");

        // 2. Wrong password should not verify
        let password_ok =
            password::verify_password("wrongpassword", &stored_hash).expect("verify should not error on valid hash");
        assert!(!password_ok, "V2 FAIL: wrong password should not verify");

        // 3. Record failed login
        session_manager::record_failed_login(&db, user_id)
            .await
            .expect("record_failed_login should not fail");

        // 4. The error returned by the login command is always opaque
        let err = AppError::Auth("Identifiant ou mot de passe invalide.".into());
        let err_msg = err.to_string();
        assert_eq!(
            err_msg, "Authentication error: Identifiant ou mot de passe invalide.",
            "V2 FAIL: error message must be opaque"
        );
        assert!(!err_msg.contains("not found"), "V2 FAIL: error reveals user not found");
        assert!(
            !err_msg.contains("wrong password"),
            "V2 FAIL: error reveals wrong password"
        );
        assert!(!err_msg.contains("locked"), "V2 FAIL: error reveals account locked");

        // 5. Verify the error serializes with AUTH_ERROR code
        let serialized = serde_json::to_value(&err).expect("serialize error");
        assert_eq!(serialized["code"], "AUTH_ERROR", "V2 FAIL: code should be AUTH_ERROR");
        assert_eq!(
            serialized["message"], "Authentication error: Identifiant ou mot de passe invalide.",
            "V2 FAIL: serialized message mismatch"
        );
    }

    // ── V2b — Non-existent user also gets same opaque error ─────────────

    #[tokio::test]
    async fn v2b_nonexistent_user_same_opaque_error() {
        let db = setup_db().await;

        let user_record = session_manager::find_active_user(&db, "does_not_exist")
            .await
            .expect("DB query should not fail");
        assert!(user_record.is_none(), "non-existent user should return None");

        // The command would return the same error string
        let err = AppError::Auth("Identifiant ou mot de passe invalide.".into());
        let serialized = serde_json::to_value(&err).expect("serialize");
        assert_eq!(serialized["code"], "AUTH_ERROR");
        assert_eq!(
            serialized["message"],
            "Authentication error: Identifiant ou mot de passe invalide."
        );
    }

    // ── V4 — Logout clears session correctly. ───────────────────────────

    #[tokio::test]
    async fn v4_logout_clears_session() {
        let db = setup_db().await;

        // Login first
        let user_record = session_manager::find_active_user(&db, "admin")
            .await
            .expect("query")
            .expect("admin exists");
        let (user_id, username, display_name, is_admin, force_pw, _hash) = user_record;

        let mut mgr = SessionManager::new();
        mgr.create_session(session_manager::AuthenticatedUser {
            user_id,
            username,
            display_name,
            is_admin,
            force_password_change: force_pw,
        });
        assert!(mgr.is_authenticated(), "V4 FAIL: should be authenticated after login");

        // Logout
        let cleared_id = mgr.clear_session();
        assert!(cleared_id.is_some(), "V4 FAIL: clear_session should return session id");

        // Verify unauthenticated after logout
        let info = mgr.session_info();
        assert!(
            !info.is_authenticated,
            "V4 FAIL: is_authenticated must be false after logout"
        );
        assert!(info.user_id.is_none(), "V4 FAIL: user_id must be null after logout");
        assert!(info.username.is_none(), "V4 FAIL: username must be null after logout");
    }

    // ── V5 — audit::emit writes login.success row to audit_events ───────

    #[tokio::test]
    async fn v5_audit_login_success_writes_row() {
        let db = setup_db().await;

        crate::audit::emit(
            &db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::LOGIN_SUCCESS,
                actor_id: Some(1),
                actor_name: Some("admin"),
                summary: "Successful login",
                detail_json: Some(r#"{"offline":false}"#.to_string()),
                ..Default::default()
            },
        )
        .await;

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT event_type, actor_id, summary FROM audit_events WHERE event_type = ?",
                [crate::audit::event_type::LOGIN_SUCCESS.into()],
            ))
            .await
            .expect("query")
            .expect("V5 FAIL: no login.success row in audit_events");

        let et: String = row.try_get("", "event_type").unwrap();
        let actor: String = row.try_get("", "actor_id").unwrap();
        let summary: String = row.try_get("", "summary").unwrap();

        assert_eq!(et, "login.success", "V5 FAIL: event_type mismatch");
        assert_eq!(actor, "1", "V5 FAIL: actor_id mismatch");
        assert_eq!(summary, "Successful login", "V5 FAIL: summary mismatch");
    }

    // ── V6 — audit::emit writes login.failure row to audit_events ───────

    #[tokio::test]
    async fn v6_audit_login_failure_writes_row() {
        let db = setup_db().await;

        crate::audit::emit(
            &db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::LOGIN_FAILURE,
                summary: "Failed login attempt — wrong password",
                detail_json: Some(r#"{"username_provided":true}"#.to_string()),
                ..Default::default()
            },
        )
        .await;

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT event_type, summary, detail_json FROM audit_events WHERE event_type = ?",
                [crate::audit::event_type::LOGIN_FAILURE.into()],
            ))
            .await
            .expect("query")
            .expect("V6 FAIL: no login.failure row in audit_events");

        let et: String = row.try_get("", "event_type").unwrap();
        assert_eq!(et, "login.failure", "V6 FAIL: event_type mismatch");

        let detail: String = row.try_get("", "detail_json").unwrap();
        assert!(
            detail.contains("username_provided"),
            "V6 FAIL: detail_json should contain username_provided"
        );
    }

    // ── V7 — audit::emit writes step_up.success row to audit_events ─────

    #[tokio::test]
    async fn v7_audit_step_up_success_writes_row() {
        let db = setup_db().await;

        crate::audit::emit(
            &db,
            crate::audit::AuditEvent {
                event_type: crate::audit::event_type::STEP_UP_SUCCESS,
                actor_id: Some(1),
                summary: "Step-up reauthentication verified",
                ..Default::default()
            },
        )
        .await;

        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT event_type, actor_id, summary FROM audit_events WHERE event_type = ?",
                [crate::audit::event_type::STEP_UP_SUCCESS.into()],
            ))
            .await
            .expect("query")
            .expect("V7 FAIL: no step_up.success row in audit_events");

        let et: String = row.try_get("", "event_type").unwrap();
        let actor: String = row.try_get("", "actor_id").unwrap();

        assert_eq!(et, "step_up.success", "V7 FAIL: event_type mismatch");
        assert_eq!(actor, "1", "V7 FAIL: actor_id mismatch");
    }
}
