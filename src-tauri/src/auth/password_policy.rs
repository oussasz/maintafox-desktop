//! Password policy enforcement — expiry checks and strength validation.
//!
//! Loads configurable rules from `rbac_settings` (seeded by migration 033).
//! Used by the `login` command to detect expired passwords and by the
//! `force_change_password` command to enforce complexity requirements.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::errors::AppResult;

// ── PasswordPolicy struct ─────────────────────────────────────────────────────

/// Configurable password policy loaded from `rbac_settings`.
#[derive(Debug, Clone, Serialize)]
pub struct PasswordPolicy {
    pub max_age_days: i64,
    pub warn_days_before_expiry: i64,
    pub min_length: usize,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_digit: bool,
    pub require_special: bool,
}

impl PasswordPolicy {
    /// Load from `rbac_settings` table with fallback defaults.
    /// If a key is missing or unparseable, the safe default is used.
    pub async fn load(db: &DatabaseConnection) -> Self {
        Self {
            max_age_days: load_setting(db, "password_max_age_days", "90")
                .await
                .parse()
                .unwrap_or(90),
            warn_days_before_expiry: load_setting(db, "password_warn_days", "14")
                .await
                .parse()
                .unwrap_or(14),
            min_length: load_setting(db, "password_min_length", "8")
                .await
                .parse()
                .unwrap_or(8),
            require_uppercase: load_setting(db, "password_require_uppercase", "1").await == "1",
            require_lowercase: load_setting(db, "password_require_lowercase", "1").await == "1",
            require_digit: load_setting(db, "password_require_digit", "1").await == "1",
            require_special: load_setting(db, "password_require_special", "0").await == "1",
        }
    }
}

// ── Expiry check ──────────────────────────────────────────────────────────────

/// Result of checking a user's password age against the policy.
#[derive(Debug, PartialEq)]
pub enum PasswordExpiryStatus {
    /// Password is valid; no action needed.
    Valid,
    /// Password expires within `warn_days_before_expiry`.
    ExpiringSoon { days_remaining: i64 },
    /// Password has exceeded `max_age_days`; must be changed.
    Expired,
    /// No `password_changed_at` recorded (legacy user); treat as expired.
    NeverSet,
}

/// Check if a user's password has expired based on policy.
///
/// - If `max_age_days == 0`, expiry is disabled → always returns `Valid`.
/// - If `password_changed_at` is NULL → returns `NeverSet`.
/// - Otherwise computes age and returns the appropriate status.
pub async fn check_password_expiry(
    db: &DatabaseConnection,
    user_id: i32,
    policy: &PasswordPolicy,
) -> AppResult<PasswordExpiryStatus> {
    // Expiry disabled
    if policy.max_age_days == 0 {
        return Ok(PasswordExpiryStatus::Valid);
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT password_changed_at FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;

    let changed_at: Option<String> = row
        .and_then(|r| r.try_get::<Option<String>>("", "password_changed_at").ok())
        .flatten();

    let changed_at = match changed_at {
        None => return Ok(PasswordExpiryStatus::NeverSet),
        Some(s) => s,
    };

    // Parse the ISO-8601 date
    let changed_dt = match chrono::DateTime::parse_from_rfc3339(&changed_at) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => {
            // Unparseable date — treat as never set
            tracing::warn!(user_id, password_changed_at = %changed_at, "unparseable password_changed_at");
            return Ok(PasswordExpiryStatus::NeverSet);
        }
    };

    let age_days = (chrono::Utc::now() - changed_dt).num_days();
    let warn_start_day = (policy.max_age_days - policy.warn_days_before_expiry).max(0);

    if age_days >= policy.max_age_days {
        Ok(PasswordExpiryStatus::Expired)
    } else if age_days >= warn_start_day {
        Ok(PasswordExpiryStatus::ExpiringSoon {
            days_remaining: policy.max_age_days - age_days,
        })
    } else {
        Ok(PasswordExpiryStatus::Valid)
    }
}

// ── Strength validation ───────────────────────────────────────────────────────

/// Validate password strength against policy rules.
///
/// Returns `Ok(())` if the password meets all requirements, or `Err(violations)`
/// with a list of human-readable messages describing each violated rule.
pub fn validate_password_strength(
    password: &str,
    policy: &PasswordPolicy,
) -> Result<(), Vec<String>> {
    let mut violations = Vec::new();

    if password.len() < policy.min_length {
        violations.push(format!(
            "Le mot de passe doit contenir au moins {} caractères.",
            policy.min_length
        ));
    }

    if policy.require_uppercase && !password.chars().any(|c| c.is_uppercase()) {
        violations.push("Le mot de passe doit contenir au moins une lettre majuscule.".into());
    }

    if policy.require_lowercase && !password.chars().any(|c| c.is_lowercase()) {
        violations.push("Le mot de passe doit contenir au moins une lettre minuscule.".into());
    }

    if policy.require_digit && !password.chars().any(|c| c.is_ascii_digit()) {
        violations.push("Le mot de passe doit contenir au moins un chiffre.".into());
    }

    if policy.require_special && !password.chars().any(|c| !c.is_alphanumeric()) {
        violations.push("Le mot de passe doit contenir au moins un caractère spécial.".into());
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

// ── DB helper ─────────────────────────────────────────────────────────────────

/// Read a single value from `rbac_settings`. Returns `default` on any error.
async fn load_setting(db: &DatabaseConnection, key: &str, default: &str) -> String {
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
