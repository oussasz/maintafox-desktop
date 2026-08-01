//! Account lockout enforcement — OWASP brute-force protection.
//!
//! `user_accounts` has `failed_login_attempts`, `locked_until`, and
//! `consecutive_lockouts` columns. This module enforces lockout policy
//! by checking/updating those columns at login time.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

use tracing::info;

// ── Lockout policy ────────────────────────────────────────────────────────────

/// Configurable lockout thresholds loaded from `rbac_settings`.
pub struct LockoutPolicy {
    pub max_attempts: i32,
    pub lockout_minutes: i64,
    pub progressive: bool,
}

impl LockoutPolicy {
    /// Load from `rbac_settings` table with fallback defaults.
    /// If the table doesn't exist yet (pre-migration-031), returns defaults.
    pub async fn load(db: &sea_orm::DatabaseConnection) -> Self {
        let max = load_setting(db, "lockout_max_attempts", "5").await;
        let base = load_setting(db, "lockout_base_minutes", "15").await;
        let prog = load_setting(db, "lockout_progressive", "1").await;

        Self {
            max_attempts: max.parse().unwrap_or(5),
            lockout_minutes: base.parse().unwrap_or(15),
            progressive: prog == "1",
        }
    }
}

/// Read a single value from `rbac_settings`. Returns `default` on any error
/// (table missing, key missing, DB error).
async fn load_setting(
    db: &sea_orm::DatabaseConnection,
    key: &str,
    default: &str,
) -> String {
    let result = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value FROM rbac_settings WHERE key = ?",
            [key.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => row
            .try_get::<String>("", "value")
            .unwrap_or_else(|_| default.to_string()),
        _ => default.to_string(),
    }
}

// ── Check lockout ─────────────────────────────────────────────────────────────

/// Check whether the account is currently locked.
///
/// - If `locked_until` is in the future → `Err(AccountLocked { until })`.
/// - If `locked_until` is in the past → auto-unlock (reset counter) and return `Ok`.
/// - If `locked_until` is NULL → `Ok`.
pub async fn check_lockout(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT locked_until FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;

    let locked_until: Option<String> = row
        .as_ref()
        .and_then(|r| r.try_get::<Option<String>>("", "locked_until").ok())
        .flatten();

    let Some(until) = locked_until else {
        return Ok(());
    };

    // Compare with current UTC time
    let now = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    if until > now {
        // Still locked
        return Err(AppError::AccountLocked { until });
    }

    // Expired lock → auto-unlock (but keep consecutive_lockouts for progressive)
    let ts = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts \
         SET locked_until = NULL, failed_login_attempts = 0, updated_at = ? \
         WHERE id = ?",
        [ts.into(), user_id.into()],
    ))
    .await?;

    Ok(())
}

// ── Record failed attempt ─────────────────────────────────────────────────────

/// Increment `failed_login_attempts`. If threshold reached, lock the account.
///
/// Progressive lockout doubles the duration on each consecutive lockout,
/// capped at 24 hours (1 440 minutes).
pub async fn record_failed_attempt(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    policy: &LockoutPolicy,
) -> AppResult<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // Step 1: increment counter
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE user_accounts \
         SET failed_login_attempts = failed_login_attempts + 1, updated_at = ? \
         WHERE id = ?",
        [now.into(), user_id.into()],
    ))
    .await?;

    // Step 2: read updated values
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT failed_login_attempts, consecutive_lockouts \
             FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;

    let (attempts, consecutive) = match row {
        Some(r) => (
            r.try_get::<i32>("", "failed_login_attempts").unwrap_or(0),
            r.try_get::<i32>("", "consecutive_lockouts").unwrap_or(0),
        ),
        None => return Ok(()),
    };

    // Step 3: lock if threshold reached
    info!(
        user_id = user_id,
        attempts = attempts,
        max = policy.max_attempts,
        "lockout::record_failed_attempt check"
    );
    if attempts >= policy.max_attempts {
        let base = policy.lockout_minutes;
        let duration_minutes = if policy.progressive && consecutive > 0 {
            // 2^consecutive, capped at 24h
            let multiplier = 1i64 << consecutive.min(10) as u32; // cap exponent at 10
            (base * multiplier).min(1440)
        } else {
            base
        };

        let lock_until = chrono::Utc::now()
            + chrono::Duration::minutes(duration_minutes);
        let lock_until_str = lock_until.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let lock_ts = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE user_accounts \
             SET locked_until = ?, \
                 consecutive_lockouts = consecutive_lockouts + 1, \
                 updated_at = ? \
             WHERE id = ?",
            [lock_until_str.into(), lock_ts.clone().into(), user_id.into()],
        ))
        .await?;

        // Audit event: account_locked
        let _ = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO admin_change_events \
                 (action, target_user_id, summary, step_up_used) \
                 VALUES ('account_locked', ?, ?, 0)",
                [
                    user_id.into(),
                    format!(
                        "Account locked after {} failed attempts. Duration: {} minutes.",
                        attempts, duration_minutes
                    )
                    .into(),
                ],
            ))
            .await;
    }

    Ok(())
}
