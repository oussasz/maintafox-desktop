//! RBAC IPC commands.
//!
//! Exposes permission queries and step-up verification to the frontend.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::password_policy::PasswordPolicy;
use crate::auth::rbac;
use crate::auth::rbac::PermissionScope;
use crate::auth::session_manager;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

/// Return the effective permission set for the currently authenticated user.
///
/// The frontend calls this after login to pre-load permissions into the
/// `usePermissions` hook so that `<PermissionGate>` can work without
/// round-tripping to the backend for every check.
#[tauri::command]
pub async fn get_my_permissions(state: State<'_, AppState>) -> AppResult<Vec<rbac::PermissionRecord>> {
    let user = require_session!(state);
    rbac::get_user_permissions(&state.db, user.user_id).await
}

/// Where the current user holds each permission (step 6): plant-wide, or only on some org nodes
/// (and below). `get_my_permissions` stays the "held anywhere" list used to show modules.
#[tauri::command]
pub async fn get_my_access(state: State<'_, AppState>) -> AppResult<crate::auth::scope::MyAccess> {
    let user = require_session!(state);
    let map = crate::auth::scope::access_map_cached(&state.db, &state.permission_cache, user.user_id).await?;
    Ok(crate::auth::scope::my_access(&map))
}

/// The permissions that apply to the current user at one org node (plant-wide ones plus those
/// held on the node or above it); `None` for an administrator, who holds every permission.
#[tauri::command]
pub async fn get_my_access_at(org_node_id: i64, state: State<'_, AppState>) -> AppResult<Option<Vec<String>>> {
    let user = require_session!(state);
    let map = crate::auth::scope::access_map_cached(&state.db, &state.permission_cache, user.user_id).await?;
    crate::auth::scope::permissions_at_node(&state.db, &map, org_node_id).await
}

/// Input for the `verify_step_up` command.
#[derive(Debug, Deserialize)]
pub struct StepUpRequest {
    pub password: String,
}

/// Response from a successful step-up verification.
#[derive(Debug, Serialize)]
pub struct StepUpResponse {
    pub success: bool,
    pub expires_at: String,
}

/// Generic key/value row returned from `rbac_settings`.
#[derive(Debug, Serialize)]
pub struct RbacSettingEntry {
    pub key: String,
    pub value: String,
    pub description: Option<String>,
}

/// Password policy settings consumed by the admin panel.
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordPolicySettings {
    pub max_age_days: i64,
    pub warn_days_before_expiry: i64,
    pub min_length: usize,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_digit: bool,
    pub require_special: bool,
    /// Stored keys that could not be read: the values above are then the stricter of the
    /// stored and shipped ones, and Settings asks the administrator to save the policy again.
    /// Ignored on input.
    #[serde(default)]
    pub unreadable_keys: Vec<String>,
}

/// The policy in force, as the admin panel shows it (see `PasswordPolicy::load_checked`).
pub(crate) async fn password_policy_settings(db: &sea_orm::DatabaseConnection) -> PasswordPolicySettings {
    let loaded = PasswordPolicy::load_checked(db).await;
    let policy = loaded.policy;
    PasswordPolicySettings {
        max_age_days: policy.max_age_days,
        warn_days_before_expiry: policy.warn_days_before_expiry,
        min_length: policy.min_length,
        require_uppercase: policy.require_uppercase,
        require_lowercase: policy.require_lowercase,
        require_digit: policy.require_digit,
        require_special: policy.require_special,
        unreadable_keys: loaded.unreadable_keys,
    }
}

/// Verify the user's password for a dangerous-action step-up.
///
/// On success, records the verification timestamp in the in-memory session.
/// The step-up window (the session policy `step_up_window_minutes`) starts from this moment.
#[tauri::command]
pub async fn verify_step_up(payload: StepUpRequest, state: State<'_, AppState>) -> AppResult<StepUpResponse> {
    verify_step_up_impl(state.inner(), payload).await
}

/// The password check runs under the sign-in lockout (`commands::auth::verify_password_in_session`):
/// too many wrong passwords lock the account and end the session (`ACCOUNT_LOCKED`).
pub(crate) async fn verify_step_up_impl(state: &AppState, payload: StepUpRequest) -> AppResult<StepUpResponse> {
    let user = require_session!(state);
    let session_db_id = crate::commands::auth::current_session_db_id(state).await?;

    crate::commands::auth::verify_password_in_session(
        state,
        &user,
        &session_db_id,
        &payload.password,
        "Step-up reauthentication failed: wrong password",
    )
    .await?;

    // Record the step-up timestamp in the session
    let mut sm = state.session.write().await;
    sm.record_step_up();
    let window_minutes = sm
        .current
        .as_ref()
        .map_or(session_manager::STEP_UP_WINDOW_MINUTES, |s| s.step_up_window_minutes);

    crate::audit::emit(
        &state.db,
        crate::audit::AuditEvent {
            event_type: crate::audit::event_type::STEP_UP_SUCCESS,
            actor_id: Some(user.user_id),
            summary: "Step-up reauthentication verified",
            ..Default::default()
        },
    )
    .await;

    let expires_at = (chrono::Utc::now() + chrono::TimeDelta::minutes(window_minutes)).to_rfc3339();

    Ok(StepUpResponse {
        success: true,
        expires_at,
    })
}

/// List RBAC settings by key prefix. Requires `adm.settings`.
#[tauri::command]
pub async fn get_rbac_settings(prefix: String, state: State<'_, AppState>) -> AppResult<Vec<RbacSettingEntry>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );

    let pattern = format!("{}%", prefix.trim());
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT key, value, description
               FROM rbac_settings
               WHERE key LIKE ?
               ORDER BY key",
            [pattern.into()],
        ))
        .await?;

    Ok(rows
        .into_iter()
        .map(|row| RbacSettingEntry {
            key: row.try_get::<String>("", "key").unwrap_or_default(),
            value: row.try_get::<String>("", "value").unwrap_or_default(),
            description: row.try_get::<Option<String>>("", "description").unwrap_or(None),
        })
        .collect())
}

/// Security keys of `rbac_settings` an administrator may change, with their allowed values.
/// Every other key is refused (unknown, or stored but read by nothing).
#[derive(Debug, Clone, Copy)]
enum RbacSettingRule {
    Range(i64, i64),
    Flag,
}

const EDITABLE_RBAC_SETTINGS: &[(&str, RbacSettingRule)] = &[
    ("password_min_length", RbacSettingRule::Range(8, 128)),
    ("password_max_age_days", RbacSettingRule::Range(0, 3650)),
    ("password_warn_days", RbacSettingRule::Range(0, 3650)),
    ("password_require_uppercase", RbacSettingRule::Flag),
    ("password_require_lowercase", RbacSettingRule::Flag),
    ("password_require_digit", RbacSettingRule::Flag),
    ("password_require_special", RbacSettingRule::Flag),
    // At least 3 attempts: fewer locks people out on a single typo.
    ("lockout_max_attempts", RbacSettingRule::Range(3, 20)),
    ("lockout_base_minutes", RbacSettingRule::Range(1, 1440)),
    ("lockout_progressive", RbacSettingRule::Flag),
];

/// Canonical stored text of `value` for `key`, or the reason it is refused.
fn validate_rbac_setting(key: &str, value: &str) -> AppResult<String> {
    let Some((_, rule)) = EDITABLE_RBAC_SETTINGS.iter().find(|(k, _)| *k == key) else {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_RBAC_SETTING_LOCKED: The security setting '{key}' cannot be changed here."
        )]));
    };
    let value = value.trim();
    match rule {
        RbacSettingRule::Flag => match value {
            "0" | "1" => Ok(value.to_string()),
            "true" => Ok("1".into()),
            "false" => Ok("0".into()),
            _ => Err(AppError::ValidationFailed(vec![format!(
                "GATE_RBAC_SETTING_FLAG: {key} must be 0 or 1."
            )])),
        },
        RbacSettingRule::Range(min, max) => match value.parse::<i64>() {
            Ok(n) if (*min..=*max).contains(&n) => Ok(n.to_string()),
            _ => Err(AppError::ValidationFailed(vec![format!(
                "GATE_RBAC_SETTING_RANGE:min={min},max={max}: {key} must be a whole number between {min} and {max}."
            )])),
        },
    }
}

/// Write validated security settings in one transaction, with one history row per changed key
/// (`rbac.<key>`, old → new). The caller has checked `adm.settings` and step-up.
async fn write_rbac_settings(
    db: &sea_orm::DatabaseConnection,
    changes: &[(&str, String)],
    actor_id: i32,
) -> AppResult<()> {
    use sea_orm::TransactionTrait;
    let now = chrono::Utc::now().to_rfc3339();
    let tx = db.begin().await?;
    for (key, value) in changes {
        let old: Option<String> = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT value FROM rbac_settings WHERE key = ?",
                [(*key).into()],
            ))
            .await?
            .and_then(|r| r.try_get("", "value").ok());
        if old.as_deref() == Some(value.as_str()) {
            continue;
        }
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO rbac_settings (key, value, description)
              VALUES (?, ?, NULL)
              ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [(*key).into(), value.clone().into()],
        ))
        .await?;
        crate::settings::insert_change_event(
            &tx,
            &format!("rbac.{key}"),
            &format!("Security setting '{key}' changed"),
            old.as_deref(),
            Some(value.as_str()),
            old.as_deref().map(crate::settings::sha256_hex),
            Some(crate::settings::sha256_hex(value)),
            Some(i64::from(actor_id)),
            &now,
            true,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn stored_rbac_number(db: &sea_orm::DatabaseConnection, key: &str) -> AppResult<Option<i64>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value FROM rbac_settings WHERE key = ?",
            [key.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<String>("", "value").ok())
        .and_then(|v| v.trim().parse().ok()))
}

/// Update one security setting of `rbac_settings`. Requires `adm.settings` and step-up; only
/// the keys the login and password checks read are accepted, each within its range.
#[tauri::command]
pub async fn update_rbac_setting(key: String, value: String, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    require_step_up!(state);

    let key = key.trim();
    let value = validate_rbac_setting(key, &value)?;
    // The warning period and the maximum age are checked together, whichever one changes.
    let number = || -> AppResult<i64> {
        value
            .parse()
            .map_err(|_| AppError::ValidationFailed(vec![format!("GATE_RBAC_SETTING_NUMBER: {key} must be a number.")]))
    };
    if key == "password_warn_days" {
        if let Some(max_age) = stored_rbac_number(&state.db, "password_max_age_days").await? {
            check_warn_within_max_age(number()?, max_age)?;
        }
    }
    if key == "password_max_age_days" {
        if let Some(warn) = stored_rbac_number(&state.db, "password_warn_days").await? {
            check_warn_within_max_age(warn, number()?)?;
        }
    }
    write_rbac_settings(&state.db, &[(key, value)], user.user_id).await
}

fn check_warn_within_max_age(warn_days: i64, max_age_days: i64) -> AppResult<()> {
    if max_age_days > 0 && warn_days > max_age_days {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RBAC_WARN_AFTER_MAX_AGE: The expiry warning cannot start before the password's maximum age.".into(),
        ]));
    }
    Ok(())
}

/// Replace the whole password policy at once (validated together). Requires `adm.settings`
/// and step-up; each changed value is recorded in the settings history.
#[tauri::command]
pub async fn set_password_policy(input: PasswordPolicySettings, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    require_step_up!(state);
    let changes = password_policy_changes(&input)?;
    write_rbac_settings(&state.db, &changes, user.user_id).await
}

fn password_policy_changes(input: &PasswordPolicySettings) -> AppResult<Vec<(&'static str, String)>> {
    let flag = |b: bool| if b { "1".to_string() } else { "0".to_string() };
    let changes = vec![
        (
            "password_min_length",
            validate_rbac_setting("password_min_length", &input.min_length.to_string())?,
        ),
        (
            "password_max_age_days",
            validate_rbac_setting("password_max_age_days", &input.max_age_days.to_string())?,
        ),
        (
            "password_warn_days",
            validate_rbac_setting("password_warn_days", &input.warn_days_before_expiry.to_string())?,
        ),
        ("password_require_uppercase", flag(input.require_uppercase)),
        ("password_require_lowercase", flag(input.require_lowercase)),
        ("password_require_digit", flag(input.require_digit)),
        ("password_require_special", flag(input.require_special)),
    ];
    check_warn_within_max_age(input.warn_days_before_expiry, input.max_age_days)?;
    Ok(changes)
}

/// Sign-in lockout thresholds (read by `auth::lockout` at every failed sign-in).
#[derive(Debug, Serialize, Deserialize)]
pub struct LockoutPolicySettings {
    pub max_attempts: i64,
    pub lockout_minutes: i64,
    pub progressive: bool,
}

/// The lockout thresholds the sign-in applies. Requires `adm.settings`.
#[tauri::command]
pub async fn get_lockout_policy(state: State<'_, AppState>) -> AppResult<LockoutPolicySettings> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    let policy = crate::auth::lockout::LockoutPolicy::load(&state.db).await;
    Ok(LockoutPolicySettings {
        max_attempts: i64::from(policy.max_attempts),
        lockout_minutes: policy.lockout_minutes,
        progressive: policy.progressive,
    })
}

/// Replace the lockout thresholds (validated, step-up, recorded in the settings history).
#[tauri::command]
pub async fn set_lockout_policy(input: LockoutPolicySettings, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );
    require_step_up!(state);
    let changes = vec![
        (
            "lockout_max_attempts",
            validate_rbac_setting("lockout_max_attempts", &input.max_attempts.to_string())?,
        ),
        (
            "lockout_base_minutes",
            validate_rbac_setting("lockout_base_minutes", &input.lockout_minutes.to_string())?,
        ),
        (
            "lockout_progressive",
            if input.progressive {
                "1".to_string()
            } else {
                "0".to_string()
            },
        ),
    ];
    write_rbac_settings(&state.db, &changes, user.user_id).await
}

#[cfg(test)]
mod rbac_settings_tests {
    use super::*;

    #[test]
    fn security_settings_are_validated() {
        assert_eq!(validate_rbac_setting("password_min_length", "12").unwrap(), "12");
        assert!(
            validate_rbac_setting("password_min_length", "6").is_err(),
            "never below 8"
        );
        assert!(validate_rbac_setting("password_min_length", "200").is_err());
        assert_eq!(validate_rbac_setting("password_require_special", "true").unwrap(), "1");
        assert!(validate_rbac_setting("password_require_special", "yes").is_err());
        assert!(
            validate_rbac_setting("deny_all_fallback", "0").is_err(),
            "unused keys are refused"
        );
        assert!(validate_rbac_setting("lockout_max_attempts", "0").is_err());
        assert!(
            validate_rbac_setting("lockout_max_attempts", "2").is_err(),
            "at least 3"
        );
        assert_eq!(validate_rbac_setting("lockout_max_attempts", "3").unwrap(), "3");
        assert!(check_warn_within_max_age(30, 14).is_err());
        assert!(check_warn_within_max_age(30, 0).is_ok(), "no expiry: no constraint");
    }

    #[tokio::test]
    async fn policy_changes_are_written_with_history() {
        let db = crate::db::test_db::seeded().await;
        let input = PasswordPolicySettings {
            max_age_days: 60,
            warn_days_before_expiry: 7,
            min_length: 12,
            require_uppercase: true,
            require_lowercase: true,
            require_digit: true,
            require_special: true,
            unreadable_keys: Vec::new(),
        };
        let changes = password_policy_changes(&input).unwrap();
        write_rbac_settings(&db, &changes, 1).await.unwrap();
        assert_eq!(PasswordPolicy::load(&db).await.min_length, 12);
        let history = crate::settings::list_change_events(&db, 50, Some("rbac.password_min_length"))
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].new_value_json.as_deref(), Some("12"));
        assert!(history[0].required_step_up);
        // Saving the same values again records nothing.
        write_rbac_settings(&db, &changes, 1).await.unwrap();
        assert_eq!(
            crate::settings::list_change_events(&db, 50, Some("rbac.password_min_length"))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    async fn set_raw(db: &sea_orm::DatabaseConnection, key: &str, value: &str) {
        use sea_orm::ConnectionTrait;
        db.execute(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Sqlite,
            "UPDATE rbac_settings SET value = ? WHERE key = ?",
            [value.into(), key.into()],
        ))
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn an_unreadable_stored_policy_applies_the_stricter_values_and_says_so() {
        let db = crate::db::test_db::seeded().await;
        let readable = password_policy_settings(&db).await;
        assert!(readable.unreadable_keys.is_empty(), "{:?}", readable.unreadable_keys);

        // A weak but readable stored policy is applied as stored while everything is readable.
        set_raw(&db, "password_max_age_days", "0").await;
        set_raw(&db, "password_require_uppercase", "0").await;
        let weak = PasswordPolicy::load(&db).await;
        assert_eq!(weak.max_age_days, 0);
        assert!(!weak.require_uppercase);

        // One unreadable key: sign-in keeps working with the stricter of stored and shipped.
        set_raw(&db, "password_min_length", "douze").await;
        set_raw(&db, "password_require_special", "1").await;
        let settings = password_policy_settings(&db).await;
        assert_eq!(settings.unreadable_keys, vec!["password_min_length".to_string()]);
        assert_eq!(settings.min_length, 8, "shipped minimum for the unreadable key");
        assert_eq!(
            settings.max_age_days, 90,
            "no expiry is weaker than the shipped 90 days"
        );
        assert!(settings.require_uppercase, "shipped rule kept");
        assert!(settings.require_special, "a stricter stored rule is kept");

        // A readable stored value stricter than the shipped one wins.
        set_raw(&db, "password_min_length", "14").await;
        set_raw(&db, "password_require_digit", "yes").await;
        let settings = password_policy_settings(&db).await;
        assert_eq!(settings.unreadable_keys, vec!["password_require_digit".to_string()]);
        assert_eq!(settings.min_length, 14);
        assert!(settings.require_digit);

        // Saving the policy again (the fix action) clears the warning.
        let changes = password_policy_changes(&settings).unwrap();
        write_rbac_settings(&db, &changes, 1).await.unwrap();
        assert!(password_policy_settings(&db).await.unreadable_keys.is_empty());
    }
}

/// Return the resolved password policy used by login/password-change flows.
/// Requires `adm.settings`.
#[tauri::command]
pub async fn get_password_policy(state: State<'_, AppState>) -> AppResult<PasswordPolicySettings> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::ADM_SETTINGS,
        PermissionScope::Global
    );

    Ok(password_policy_settings(&state.db).await)
}
