//! Session commands on a real in-memory database (`db::test_db::seeded`): the idle lock and the
//! activity report (AU-34).

use chrono::{TimeDelta, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use super::*;
use crate::auth::session_manager::{AuthenticatedUser, SessionTimeouts};

const PASSWORD: &str = "Correct-Pass-2026!";

async fn scalar_i64(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get::<i64>("", "v")
        .expect("decode")
}

/// An active, non-administrator account with a known password; returns its id.
async fn technician(db: &DatabaseConnection, username: &str) -> i32 {
    let now = Utc::now().to_rfc3339();
    let hash = password::hash_password(PASSWORD).expect("hash");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts \
         (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin, \
          force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version) \
         VALUES (?, ?, 'local', ?, 1, 0, 0, ?, 0, ?, ?, 1)",
        [
            format!("{username}-sync").into(),
            username.into(),
            hash.into(),
            now.clone().into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .expect("insert user");
    let id = scalar_i64(
        db,
        "SELECT id AS v FROM user_accounts WHERE username = ?",
        vec![username.into()],
    )
    .await;
    i32::try_from(id).expect("fits")
}

/// Signs `username` in on `state` (in memory and its `app_sessions` row), 10-minute idle policy.
async fn sign_in(state: &AppState, user_id: i32, username: &str) -> String {
    let user = AuthenticatedUser {
        user_id,
        username: username.into(),
        display_name: None,
        is_admin: false,
        force_password_change: false,
        tenant_id: "tenant-test".into(),
        token_tenant_id: "tenant-test".into(),
    };
    let mut guard = state.session.write().await;
    guard.create_session_with_timeouts(user, SessionTimeouts::from_policy(10, 600, 15));
    let current = guard.current.as_ref().expect("session");
    let (id, expires) = (current.session_db_id.clone(), current.expires_at.to_rfc3339());
    session_manager::store_session_or_clear(&state.db, &mut guard, user_id, &id, &expires)
        .await
        .expect("stored session");
    id
}

async fn set_last_activity(state: &AppState, minutes_ago: i64) {
    let mut guard = state.session.write().await;
    guard.current.as_mut().expect("session").last_activity_at = Utc::now() - TimeDelta::minutes(minutes_ago);
}

/// What every protected command (and so every background poll) runs first.
async fn protected_command(state: &AppState) -> AppResult<AuthenticatedUser> {
    Ok(crate::require_session!(state))
}

async fn is_locked(state: &AppState) -> bool {
    state.session.read().await.session_info().is_locked
}

#[tokio::test]
async fn touch_before_the_idle_deadline_extends_the_session() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "touch_tech").await;
    sign_in(&state, id, "touch_tech").await;
    set_last_activity(&state, 9).await;

    let info = touch_session_impl(&state).await.expect("within the idle timeout");
    assert!(info.is_authenticated && !info.is_locked);
    assert_eq!(info.idle_timeout_minutes, Some(10));
    let last = chrono::DateTime::parse_from_rfc3339(info.last_activity_at.as_deref().expect("activity"))
        .expect("date")
        .with_timezone(&Utc);
    assert!(Utc::now() - last < TimeDelta::seconds(5), "activity moved to now");
}

#[tokio::test]
async fn touch_after_the_idle_deadline_is_refused_and_the_session_stays_locked() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "sleep_tech").await;
    sign_in(&state, id, "sleep_tech").await;
    // The PC slept for 45 minutes; the first mouse move after waking sends a touch.
    set_last_activity(&state, 45).await;

    let refused = touch_session_impl(&state).await.unwrap_err();
    assert!(matches!(refused, AppError::SessionLocked(_)), "{refused:?}");
    assert!(is_locked(&state).await);
    // Further input does not reopen it either.
    assert!(matches!(
        touch_session_impl(&state).await.unwrap_err(),
        AppError::SessionLocked(_)
    ));
    assert!(is_locked(&state).await);
}

#[tokio::test]
async fn polls_do_not_keep_the_session_open() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "poll_tech").await;
    sign_in(&state, id, "poll_tech").await;
    set_last_activity(&state, 9).await;
    let before = state.session.read().await.current.as_ref().unwrap().last_activity_at;

    // A protected command, as the notification / presence / statistics polls call them.
    for _ in 0..3 {
        protected_command(&state).await.expect("still open");
    }
    assert_eq!(
        state.session.read().await.current.as_ref().unwrap().last_activity_at,
        before,
        "polls do not move the activity time"
    );

    set_last_activity(&state, 11).await;
    let refused = protected_command(&state).await.unwrap_err();
    assert!(matches!(refused, AppError::SessionLocked(_)), "{refused:?}");
    assert!(is_locked(&state).await);
}

#[tokio::test]
async fn password_unlock_reopens_a_session_locked_after_its_deadline() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "unlock_tech").await;
    sign_in(&state, id, "unlock_tech").await;
    set_last_activity(&state, 45).await;
    assert!(touch_session_impl(&state).await.is_err());

    let info = unlock_session_impl(
        &state,
        UnlockSessionRequest {
            password: PASSWORD.into(),
        },
    )
    .await
    .expect("right password");
    assert!(info.is_authenticated && !info.is_locked);
    touch_session_impl(&state)
        .await
        .expect("open again: input counts again");
}
