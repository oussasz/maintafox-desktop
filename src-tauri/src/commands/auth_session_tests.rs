//! Session commands on a real in-memory database (`db::test_db::seeded`): the idle lock and the
//! activity report (AU-34); the account lockout of the unlock and step-up passwords (AU-37).

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
    // Only the session row: sign-in's own reset of the failure count is not part of these tests
    // (sign-in refuses a locked-out account before it).
    session_manager::create_session_record(&state.db, &id, user_id, &expires)
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

// ── AU-37: the unlock and step-up passwords under the sign-in lockout ────────────

async fn failed_attempts(db: &DatabaseConnection, user_id: i32) -> i64 {
    scalar_i64(
        db,
        "SELECT failed_login_attempts AS v FROM user_accounts WHERE id = ?",
        vec![user_id.into()],
    )
    .await
}

async fn locked_out(db: &DatabaseConnection, user_id: i32) -> bool {
    scalar_i64(
        db,
        "SELECT COUNT(*) AS v FROM user_accounts WHERE id = ? AND locked_until IS NOT NULL",
        vec![user_id.into()],
    )
    .await
        == 1
}

async fn session_revoked(db: &DatabaseConnection, session_id: &str) -> bool {
    scalar_i64(
        db,
        "SELECT is_revoked AS v FROM app_sessions WHERE id = ?",
        vec![session_id.into()],
    )
    .await
        == 1
}

/// The lockout ends: the stored lock end is moved into the past.
async fn lockout_ends(db: &DatabaseConnection, user_id: i32) {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET locked_until = '2000-01-01T00:00:00Z' WHERE id = ?",
        [user_id.into()],
    ))
    .await
    .expect("unlock account");
}

async fn unlock(state: &AppState, password: &str) -> AppResult<session_manager::SessionInfo> {
    unlock_session_impl(
        state,
        UnlockSessionRequest {
            password: password.into(),
        },
    )
    .await
}

async fn step_up(state: &AppState, password: &str) -> AppResult<crate::commands::rbac::StepUpResponse> {
    crate::commands::rbac::verify_step_up_impl(
        state,
        crate::commands::rbac::StepUpRequest {
            password: password.into(),
        },
    )
    .await
}

async fn max_attempts(db: &DatabaseConnection) -> i32 {
    let max = crate::auth::lockout::LockoutPolicy::load(db).await.max_attempts;
    assert!(max >= 2, "the policy allows a few attempts ({max})");
    max
}

#[tokio::test]
async fn wrong_unlock_passwords_lock_the_account_and_end_the_session() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "unlock_lockout").await;
    let session_id = sign_in(&state, id, "unlock_lockout").await;
    set_last_activity(&state, 45).await;
    let max = max_attempts(&db).await;

    for attempt in 1..max {
        let refused = unlock(&state, "wrong-password").await.unwrap_err();
        assert!(matches!(refused, AppError::Auth(_)), "attempt {attempt}: {refused:?}");
        assert!(state.session.read().await.current.is_some(), "still locked, not ended");
    }
    let refused = unlock(&state, "wrong-password").await.unwrap_err();
    assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
    assert!(locked_out(&db, id).await);
    assert!(state.session.read().await.current.is_none(), "the session ended");
    assert!(session_revoked(&db, &session_id).await, "its stored row is closed");

    // The right password is refused too while the lockout lasts (a session still open on the
    // account, e.g. one opened before the lockout).
    sign_in(&state, id, "unlock_lockout").await;
    set_last_activity(&state, 45).await;
    let refused = unlock(&state, PASSWORD).await.unwrap_err();
    assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
    assert!(state.session.read().await.current.is_none());

    // Once it ends, the right password unlocks and the count starts afresh.
    lockout_ends(&db, id).await;
    sign_in(&state, id, "unlock_lockout").await;
    set_last_activity(&state, 45).await;
    let info = unlock(&state, PASSWORD).await.expect("lockout over");
    assert!(info.is_authenticated && !info.is_locked);
    assert_eq!(failed_attempts(&db, id).await, 0);
    assert!(!locked_out(&db, id).await);
}

#[tokio::test]
async fn wrong_step_up_passwords_lock_the_account_and_end_the_session() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "stepup_lockout").await;
    let session_id = sign_in(&state, id, "stepup_lockout").await;
    let max = max_attempts(&db).await;

    for attempt in 1..max {
        let refused = step_up(&state, "wrong-password").await.unwrap_err();
        assert!(matches!(refused, AppError::Auth(_)), "attempt {attempt}: {refused:?}");
    }
    let refused = step_up(&state, "wrong-password").await.unwrap_err();
    assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
    assert!(state.session.read().await.current.is_none(), "the session ended");
    assert!(session_revoked(&db, &session_id).await);

    sign_in(&state, id, "stepup_lockout").await;
    let refused = step_up(&state, PASSWORD).await.unwrap_err();
    assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
    assert!(state.session.read().await.current.is_none());

    lockout_ends(&db, id).await;
    sign_in(&state, id, "stepup_lockout").await;
    step_up(&state, PASSWORD).await.expect("lockout over");
    assert!(state.session.read().await.is_step_up_valid());
    assert_eq!(failed_attempts(&db, id).await, 0);
}

#[tokio::test]
async fn a_right_password_resets_the_failure_count() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "reset_count").await;
    sign_in(&state, id, "reset_count").await;
    let max = max_attempts(&db).await;

    for _ in 1..max {
        assert!(step_up(&state, "wrong-password").await.is_err());
    }
    assert_eq!(failed_attempts(&db, id).await, i64::from(max - 1));
    step_up(&state, PASSWORD).await.expect("right password");
    assert_eq!(failed_attempts(&db, id).await, 0);

    // Unlock and step-up share the count, as sign-in does: max - 1 more failures still pass.
    set_last_activity(&state, 45).await;
    for _ in 1..max {
        assert!(matches!(unlock(&state, "wrong-password").await, Err(AppError::Auth(_))));
    }
    assert!(!locked_out(&db, id).await);
    unlock(&state, PASSWORD).await.expect("right password");
    assert_eq!(failed_attempts(&db, id).await, 0);
}

// ── Review follow-up: recording the idle lock gives no new PIN tries ──────────

async fn set_user_pin(db: &DatabaseConnection, user_id: i32, pin_value: &str) {
    let hash = pin::hash_pin(pin_value).expect("pin hash");
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts SET pin_hash = ? WHERE id = ?",
        [hash.into(), user_id.into()],
    ))
    .await
    .expect("set pin");
}

async fn pin_unlock(state: &AppState, pin_value: &str) -> AppResult<session_manager::SessionInfo> {
    unlock_session_with_pin_impl(state, PinUnlockInput { pin: pin_value.into() }).await
}

#[tokio::test]
async fn a_protected_call_after_three_wrong_pins_does_not_give_more_pin_tries() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "pin_tech").await;
    sign_in(&state, id, "pin_tech").await;
    set_user_pin(&db, id, "4321").await;
    // Locked by time only: nothing has recorded the lock yet.
    set_last_activity(&state, 45).await;
    assert!(!state.session.read().await.current.as_ref().unwrap().is_locked);

    assert!(matches!(pin_unlock(&state, "1111").await, Err(AppError::Auth(_))));
    assert!(matches!(pin_unlock(&state, "2222").await, Err(AppError::Auth(_))));
    assert!(matches!(pin_unlock(&state, "3333").await, Err(AppError::PinDisabled)));

    // A background poll records the lock.
    assert!(matches!(
        protected_command(&state).await.unwrap_err(),
        AppError::SessionLocked(_)
    ));
    assert!(state.session.read().await.current.as_ref().unwrap().is_locked);

    // PIN unlock stays disabled, even with the right PIN: the password is required.
    assert!(matches!(pin_unlock(&state, "4321").await, Err(AppError::PinDisabled)));
    unlock(&state, PASSWORD).await.expect("password unlock");
}

// ── Every password check inside a session counts toward the lockout ───────────

async fn set_pin_with(state: &AppState, password: &str) -> AppResult<()> {
    set_pin_impl(
        state,
        SetPinInput {
            current_password: password.into(),
            new_pin: "2468".into(),
        },
    )
    .await
}

#[tokio::test]
async fn wrong_passwords_when_setting_the_pin_lock_the_account_and_end_the_session() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "pin_setup").await;
    let session_id = sign_in(&state, id, "pin_setup").await;
    let max = max_attempts(&db).await;

    // A right password resets the count.
    for _ in 1..max {
        assert!(matches!(
            set_pin_with(&state, "wrong-password").await,
            Err(AppError::Auth(_))
        ));
    }
    set_pin_with(&state, PASSWORD).await.expect("right password");
    assert_eq!(failed_attempts(&db, id).await, 0);

    for attempt in 1..max {
        let refused = set_pin_with(&state, "wrong-password").await.unwrap_err();
        assert!(matches!(refused, AppError::Auth(_)), "attempt {attempt}: {refused:?}");
    }
    let refused = set_pin_with(&state, "wrong-password").await.unwrap_err();
    assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
    assert!(state.session.read().await.current.is_none(), "the session ended");
    assert!(session_revoked(&db, &session_id).await);

    // Removing the PIN from a session still open meanwhile is refused too.
    sign_in(&state, id, "pin_setup").await;
    let refused = clear_pin_impl(
        &state,
        ClearPinInput {
            current_password: PASSWORD.into(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
    assert!(state.session.read().await.current.is_none());
}

#[tokio::test]
async fn wrong_current_passwords_at_password_change_lock_the_account() {
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "pw_change").await;
    sign_in(&state, id, "pw_change").await;
    let max = max_attempts(&db).await;
    let change = |current: &'static str| crate::commands::profile::ChangePasswordPayload {
        current_password: current.into(),
        new_password: "Nouveau-Mot-2026!".into(),
    };

    for attempt in 1..max {
        let refused = crate::commands::profile::change_password_impl(&state, change("wrong-password"))
            .await
            .unwrap_err();
        assert!(matches!(refused, AppError::Auth(_)), "attempt {attempt}: {refused:?}");
    }
    let refused = crate::commands::profile::change_password_impl(&state, change("wrong-password"))
        .await
        .unwrap_err();
    assert!(matches!(refused, AppError::AccountLocked { .. }), "{refused:?}");
    assert!(state.session.read().await.current.is_none(), "the session ended");
}

// ── AU-109: no sign-in or unlock while an update is being installed ──────────────

/// What a refused attempt must leave untouched: audit rows, stored sessions, the failure count.
async fn written(db: &DatabaseConnection, user_id: i32) -> (i64, i64, i64) {
    (
        scalar_i64(db, "SELECT COUNT(*) AS v FROM audit_events", vec![]).await,
        scalar_i64(db, "SELECT COUNT(*) AS v FROM app_sessions", vec![]).await,
        failed_attempts(db, user_id).await,
    )
}

fn login_request(username: &str, password: &str) -> LoginRequest {
    LoginRequest {
        username: username.into(),
        password: password.into(),
    }
}

fn is_update_refusal(err: &AppError) -> bool {
    matches!(err, AppError::UpdateInProgress(m) if m.starts_with("AUTH_UPDATE_IN_PROGRESS"))
}

#[tokio::test]
async fn sign_in_is_refused_while_an_update_installs_and_nothing_is_written() {
    use std::sync::atomic::AtomicBool;
    static BARRIER: AtomicBool = AtomicBool::new(true);
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "update_sign_in").await;
    let before = written(&db, id).await;

    for password in ["wrong-password", PASSWORD] {
        let refused = login_impl(&state, login_request("update_sign_in", password), &BARRIER)
            .await
            .unwrap_err();
        assert!(is_update_refusal(&refused), "{password}: {refused:?}");
        assert_eq!(
            serde_json::to_value(&refused).unwrap()["code"],
            "AUTH_UPDATE_IN_PROGRESS"
        );
    }
    assert!(state.session.read().await.current.is_none(), "no session opened");
    assert_eq!(
        written(&db, id).await,
        before,
        "no audit row, stored session or failed attempt"
    );
}

#[tokio::test]
async fn sign_in_is_checked_as_before_when_no_update_installs() {
    use std::sync::atomic::AtomicBool;
    static BARRIER: AtomicBool = AtomicBool::new(false);
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "no_update_sign_in").await;

    let refused = login_impl(&state, login_request("no_update_sign_in", "wrong-password"), &BARRIER)
        .await
        .unwrap_err();
    assert!(matches!(refused, AppError::Auth(_)), "{refused:?}");
    assert_eq!(
        failed_attempts(&db, id).await,
        1,
        "the wrong password is counted as before"
    );
    if let Err(err) = login_impl(&state, login_request("no_update_sign_in", PASSWORD), &BARRIER).await {
        assert!(!is_update_refusal(&err), "{err:?}");
    }
}

#[tokio::test]
async fn unlock_is_refused_while_an_update_installs_and_nothing_is_written() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BARRIER: AtomicBool = AtomicBool::new(true);
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "update_unlock").await;
    let session_id = sign_in(&state, id, "update_unlock").await;
    set_user_pin(&db, id, "4321").await;
    set_last_activity(&state, 45).await;
    assert!(touch_session_impl(&state).await.is_err(), "locked by the idle deadline");
    let before = written(&db, id).await;

    for password in ["wrong-password", PASSWORD] {
        let refused = unlock_session_under(
            &state,
            UnlockSessionRequest {
                password: password.into(),
            },
            &BARRIER,
        )
        .await
        .unwrap_err();
        assert!(is_update_refusal(&refused), "password {password}: {refused:?}");
    }
    for pin_value in ["1111", "4321"] {
        let refused = unlock_session_with_pin_under(&state, PinUnlockInput { pin: pin_value.into() }, &BARRIER)
            .await
            .unwrap_err();
        assert!(is_update_refusal(&refused), "PIN {pin_value}: {refused:?}");
    }
    {
        let guard = state.session.read().await;
        let session = guard.current.as_ref().expect("the session is kept");
        assert!(guard.session_info().is_locked, "still locked");
        assert_eq!(session.pin_failed_attempts, 0, "no PIN attempt used up");
    }
    assert_eq!(
        written(&db, id).await,
        before,
        "no audit row, stored session or failed attempt"
    );
    assert!(!session_revoked(&db, &session_id).await);

    // The restart did not come (the install stopped): unlocking works again.
    BARRIER.store(false, Ordering::SeqCst);
    let info = unlock_session_under(
        &state,
        UnlockSessionRequest {
            password: PASSWORD.into(),
        },
        &BARRIER,
    )
    .await
    .expect("unlock once the barrier is down");
    assert!(info.is_authenticated && !info.is_locked);
}

/// Signing out is never blocked by an update: it does not read the barrier, so it ends the session
/// (one audit row) while sign-in and unlock are refused.
#[tokio::test]
async fn sign_out_still_works_while_an_update_installs() {
    use std::sync::atomic::AtomicBool;
    static BARRIER: AtomicBool = AtomicBool::new(true);
    let db = crate::db::test_db::seeded().await;
    let state = AppState::new(db.clone());
    let id = technician(&db, "update_sign_out").await;
    sign_in(&state, id, "update_sign_out").await;
    set_last_activity(&state, 45).await;
    assert!(unlock_session_under(
        &state,
        UnlockSessionRequest {
            password: PASSWORD.into(),
        },
        &BARRIER,
    )
    .await
    .is_err_and(|e| is_update_refusal(&e)));
    let audits = scalar_i64(&db, "SELECT COUNT(*) AS v FROM audit_events", vec![]).await;

    logout_impl(&state).await.expect("sign-out");
    assert!(state.session.read().await.current.is_none(), "the session ended");
    assert_eq!(
        scalar_i64(&db, "SELECT COUNT(*) AS v FROM audit_events", vec![]).await,
        audits + 1,
        "its one audit row"
    );
    assert!(login_impl(&state, login_request("update_sign_out", PASSWORD), &BARRIER)
        .await
        .is_err_and(|e| is_update_refusal(&e)));
}
