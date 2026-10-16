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

/// The policy as loaded, with the stored keys that could not be read (missing, undecodable or
/// out of their rules). Empty when the stored policy is applied as it is.
#[derive(Debug, Clone)]
pub struct LoadedPasswordPolicy {
    pub policy: PasswordPolicy,
    pub unreadable_keys: Vec<String>,
}

impl PasswordPolicy {
    /// The values shipped with the application (migration 033).
    pub const fn shipped() -> Self {
        Self {
            max_age_days: 90,
            warn_days_before_expiry: 14,
            min_length: 8,
            require_uppercase: true,
            require_lowercase: true,
            require_digit: true,
            require_special: false,
        }
    }

    /// The policy to apply (see [`PasswordPolicy::load_checked`]).
    pub async fn load(db: &DatabaseConnection) -> Self {
        Self::load_checked(db).await.policy
    }

    /// Load the stored policy (`rbac_settings`). When every key is readable it applies as
    /// stored. When a key is missing or unreadable, sign-in is not blocked (that would lock
    /// everyone out): each rule takes the stricter of its readable stored value and the shipped
    /// value, an error is logged, and the unreadable keys are returned so Settings can warn the
    /// administrator.
    pub async fn load_checked(db: &DatabaseConnection) -> LoadedPasswordPolicy {
        let mut unreadable = Vec::new();
        let max_age = number_setting(db, "password_max_age_days", 0, 3650, &mut unreadable).await;
        let warn = number_setting(db, "password_warn_days", 0, 3650, &mut unreadable).await;
        let min_length = number_setting(db, "password_min_length", 8, 128, &mut unreadable).await;
        let upper = flag_setting(db, "password_require_uppercase", &mut unreadable).await;
        let lower = flag_setting(db, "password_require_lowercase", &mut unreadable).await;
        let digit = flag_setting(db, "password_require_digit", &mut unreadable).await;
        let special = flag_setting(db, "password_require_special", &mut unreadable).await;
        let shipped = Self::shipped();

        if unreadable.is_empty() {
            return LoadedPasswordPolicy {
                policy: Self {
                    max_age_days: max_age.unwrap_or(shipped.max_age_days),
                    warn_days_before_expiry: warn.unwrap_or(shipped.warn_days_before_expiry),
                    min_length: min_length.map_or(shipped.min_length, |n| usize::try_from(n).unwrap_or(8)),
                    require_uppercase: upper.unwrap_or(shipped.require_uppercase),
                    require_lowercase: lower.unwrap_or(shipped.require_lowercase),
                    require_digit: digit.unwrap_or(shipped.require_digit),
                    require_special: special.unwrap_or(shipped.require_special),
                },
                unreadable_keys: unreadable,
            };
        }

        tracing::error!(
            keys = ?unreadable,
            "stored password policy unreadable: the stricter of the stored and shipped values is applied"
        );
        let shipped_len = i64::try_from(shipped.min_length).unwrap_or(8);
        LoadedPasswordPolicy {
            policy: Self {
                max_age_days: stricter_max_age(max_age, shipped.max_age_days),
                warn_days_before_expiry: warn.map_or(shipped.warn_days_before_expiry, |w| {
                    w.max(shipped.warn_days_before_expiry)
                }),
                min_length: usize::try_from(min_length.map_or(shipped_len, |n| n.max(shipped_len))).unwrap_or(8),
                require_uppercase: upper.unwrap_or(false) || shipped.require_uppercase,
                require_lowercase: lower.unwrap_or(false) || shipped.require_lowercase,
                require_digit: digit.unwrap_or(false) || shipped.require_digit,
                require_special: special.unwrap_or(false) || shipped.require_special,
            },
            unreadable_keys: unreadable,
        }
    }
}

/// The stricter maximum age: the shorter one, `0` (no expiry) being the weakest.
fn stricter_max_age(stored: Option<i64>, shipped: i64) -> i64 {
    match stored {
        None | Some(0) => shipped,
        Some(days) if shipped == 0 => days,
        Some(days) => days.min(shipped),
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
pub fn validate_password_strength(password: &str, policy: &PasswordPolicy) -> Result<(), Vec<String>> {
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

/// Raw stored value of `key` in `rbac_settings`; `None` when missing or unreadable (a database
/// error is logged).
async fn stored_setting(db: &DatabaseConnection, key: &str) -> Option<String> {
    let result = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value FROM rbac_settings WHERE key = ?",
            [key.into()],
        ))
        .await;
    match result {
        Ok(row) => row.and_then(|r| r.try_get::<String>("", "value").ok()),
        Err(err) => {
            tracing::error!(key, error = %err, "password policy setting could not be read");
            None
        }
    }
}

/// A number within its rules, or `None` (and `key` noted as unreadable).
async fn number_setting(
    db: &DatabaseConnection,
    key: &str,
    min: i64,
    max: i64,
    unreadable: &mut Vec<String>,
) -> Option<i64> {
    let value = stored_setting(db, key)
        .await
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|n| (min..=max).contains(n));
    if value.is_none() {
        unreadable.push(key.to_string());
    }
    value
}

/// A `"1"` / `"0"` flag, or `None` (and `key` noted as unreadable).
async fn flag_setting(db: &DatabaseConnection, key: &str, unreadable: &mut Vec<String>) -> Option<bool> {
    let value = match stored_setting(db, key).await.as_deref().map(str::trim) {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    };
    if value.is_none() {
        unreadable.push(key.to_string());
    }
    value
}
