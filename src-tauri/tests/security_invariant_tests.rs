//! SP04-F04 S2 — Security invariant tests.
//!
//! Six tests that verify compile-time and constant-based security invariants
//! of the auth subsystem. These tests act as guardrails: if someone changes
//! a security-critical constant, the test suite will catch it.

use maintafox_lib::auth::device::MAX_OFFLINE_GRACE_HOURS;
use maintafox_lib::auth::password::hash_password;
use maintafox_lib::auth::session_manager::{
    SessionManager, IDLE_LOCK_MINUTES, SESSION_DURATION_HOURS, STEP_UP_DURATION_SECS,
};

// ── SEC-1 — Session duration constant pinned to 8 hours ────────────────

#[test]
fn sec1_session_duration_is_8_hours() {
    assert_eq!(
        SESSION_DURATION_HOURS, 8,
        "SEC-1 FAIL: Session duration must be 8 hours — change requires security review"
    );
}

// ── SEC-2 — Default SessionInfo is fully unauthenticated ───────────────

#[test]
fn sec2_new_session_is_unauthenticated() {
    let mgr = SessionManager::new();
    let info = mgr.session_info();

    assert!(!info.is_authenticated, "SEC-2 FAIL: is_authenticated must be false");
    assert!(!info.is_locked, "SEC-2 FAIL: is_locked must be false");
    assert!(info.user_id.is_none(), "SEC-2 FAIL: user_id must be None");
    assert!(info.username.is_none(), "SEC-2 FAIL: username must be None");
    assert!(info.display_name.is_none(), "SEC-2 FAIL: display_name must be None");
    assert!(info.is_admin.is_none(), "SEC-2 FAIL: is_admin must be None");
    assert!(
        info.force_password_change.is_none(),
        "SEC-2 FAIL: force_password_change must be None"
    );
    assert!(info.expires_at.is_none(), "SEC-2 FAIL: expires_at must be None");
    assert!(
        info.last_activity_at.is_none(),
        "SEC-2 FAIL: last_activity_at must be None"
    );
}

// ── SEC-3 — Argon2id parameters meet OWASP 2026 baseline ───────────────

#[test]
fn sec3_argon2id_parameters_match_owasp() {
    let hash = hash_password("security_invariant_test").expect("hash should succeed");

    assert!(
        hash.starts_with("$argon2id$"),
        "SEC-3 FAIL: hash must use argon2id algorithm"
    );
    assert!(
        hash.contains("m=65536"),
        "SEC-3 FAIL: memory cost must be 65536 KiB (64 MiB)"
    );
    assert!(
        hash.contains("t=3"),
        "SEC-3 FAIL: time cost must be 3 iterations"
    );
    assert!(
        hash.contains("p=1"),
        "SEC-3 FAIL: parallelism must be 1"
    );
}

// ── SEC-4 — Step-up re-auth window pinned to 120 seconds ───────────────

#[test]
fn sec4_step_up_window_is_120_seconds() {
    assert_eq!(
        STEP_UP_DURATION_SECS, 120,
        "SEC-4 FAIL: Step-up duration must be 120s — change requires security review"
    );
}

// ── SEC-5 — Offline grace maximum pinned to 168 hours (7 days) ─────────

#[test]
fn sec5_max_offline_grace_is_168_hours() {
    assert_eq!(
        MAX_OFFLINE_GRACE_HOURS, 168,
        "SEC-5 FAIL: Max offline grace must be 168h — change requires security review"
    );
}

// ── SEC-6 — Idle lock timeout pinned to 30 minutes ─────────────────────

#[test]
fn sec6_idle_lock_is_30_minutes() {
    assert_eq!(
        IDLE_LOCK_MINUTES, 30,
        "SEC-6 FAIL: Idle lock timeout must be 30 minutes — change requires security review"
    );
}
