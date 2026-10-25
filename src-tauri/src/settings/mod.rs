//! Settings service.
//!
//! Provides typed access to `app_settings`, `policy_snapshots`, and
//! `settings_change_events`. All writes through this module emit an audit event.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod branding;
pub mod catalog;
pub(crate) mod received_notice;
#[cfg(test)]
mod received_notice_tests;
pub mod site;
#[cfg(test)]
mod sync_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSetting {
    pub id: i64,
    pub setting_key: String,
    pub setting_scope: String,
    pub setting_value_json: String,
    pub category: String,
    pub setting_risk: String,
    pub validation_status: String,
    pub secret_ref_id: Option<i64>,
    pub last_modified_by_id: Option<i64>,
    pub last_modified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySnapshot {
    pub id: i64,
    pub policy_domain: String,
    pub version_no: i64,
    pub snapshot_json: String,
    pub is_active: bool,
    pub activated_at: Option<String>,
    pub activated_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsChangeEvent {
    pub id: i64,
    pub setting_key_or_domain: String,
    pub change_summary: String,
    pub old_value_hash: Option<String>,
    pub new_value_hash: Option<String>,
    pub changed_by_id: Option<i64>,
    /// Display name (or username) of `changed_by_id`.
    pub changed_by_name: Option<String>,
    pub changed_at: String,
    pub required_step_up: bool,
    pub apply_result: String,
    /// Value before / after the change (events written before migration 213 have hashes only).
    pub old_value_json: Option<String>,
    pub new_value_json: Option<String>,
}

/// Session policy loaded from the `policy_snapshots` table.
/// Used by the session manager on startup and after policy activation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPolicy {
    pub idle_timeout_minutes: u32,
    pub absolute_session_minutes: u32,
    pub offline_grace_hours: u32,
    pub step_up_window_minutes: u32,
    pub max_failed_attempts: u32,
    pub lockout_minutes: u32,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            idle_timeout_minutes: 30,
            absolute_session_minutes: 480,
            offline_grace_hours: 72,
            step_up_window_minutes: 15,
            max_failed_attempts: 5,
            lockout_minutes: 15,
        }
    }
}

/// Produce a SHA-256 hex digest of a value string.
pub fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub async fn get_setting<C: ConnectionTrait>(db: &C, key: &str, scope: &str) -> AppResult<Option<AppSetting>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   setting_key,
                   setting_scope,
                   setting_value_json,
                   category,
                   setting_risk,
                   validation_status,
                   secret_ref_id,
                   last_modified_by_id,
                   COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', last_modified_at), last_modified_at) AS last_modified_at
               FROM app_settings
               WHERE setting_key = ? AND setting_scope = ?",
            [key.into(), scope.into()],
        ))
        .await?;

    row.map(map_app_setting).transpose()
}

pub async fn list_all_settings(db: &DatabaseConnection) -> AppResult<Vec<AppSetting>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   setting_key,
                   setting_scope,
                   setting_value_json,
                   category,
                   setting_risk,
                   validation_status,
                   secret_ref_id,
                   last_modified_by_id,
                   COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', last_modified_at), last_modified_at) AS last_modified_at
               FROM app_settings
               ORDER BY category, setting_key"
                .to_string(),
        ))
        .await?;

    rows.into_iter().map(map_app_setting).collect()
}

pub async fn list_settings_by_category(db: &DatabaseConnection, category: &str) -> AppResult<Vec<AppSetting>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   setting_key,
                   setting_scope,
                   setting_value_json,
                   category,
                   setting_risk,
                   validation_status,
                   secret_ref_id,
                   last_modified_by_id,
                   COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', last_modified_at), last_modified_at) AS last_modified_at
               FROM app_settings
               WHERE category = ?
               ORDER BY setting_key",
            [category.into()],
        ))
        .await?;

    rows.into_iter().map(map_app_setting).collect()
}

pub async fn list_settings_categories(db: &DatabaseConnection) -> AppResult<Vec<String>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT category FROM app_settings ORDER BY category".to_string(),
        ))
        .await?;

    rows.into_iter()
        .map(|r| {
            r.try_get::<String>("", "category")
                .map_err(|e| decode_err("category", e))
        })
        .collect()
}

pub async fn get_active_policy(db: &DatabaseConnection, domain: &str) -> AppResult<Option<PolicySnapshot>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   policy_domain,
                   version_no,
                   snapshot_json,
                   is_active,
                   CASE
                       WHEN activated_at IS NULL THEN NULL
                       ELSE COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', activated_at), activated_at)
                   END AS activated_at,
                   activated_by_id
               FROM policy_snapshots
               WHERE policy_domain = ? AND is_active = 1
               ORDER BY version_no DESC
               LIMIT 1",
            [domain.into()],
        ))
        .await?;

    row.map(map_policy_snapshot).transpose()
}

/// Load the session policy at startup.
/// Falls back to safe defaults whenever the DB policy is missing or malformed.
pub async fn load_session_policy(db: &DatabaseConnection) -> SessionPolicy {
    match get_active_policy(db, "session").await {
        Ok(Some(snap)) => serde_json::from_str::<SessionPolicy>(&snap.snapshot_json).unwrap_or_else(|e| {
            tracing::warn!("session policy snapshot is malformed, using defaults: {}", e);
            SessionPolicy::default()
        }),
        Ok(None) => {
            tracing::debug!("no active session policy snapshot found, using defaults");
            SessionPolicy::default()
        }
        Err(e) => {
            tracing::error!("failed to load session policy from DB: {}, using defaults", e);
            SessionPolicy::default()
        }
    }
}

/// Write a setting value and append a corresponding audit event.
pub async fn set_setting(
    db: &DatabaseConnection,
    key: &str,
    scope: &str,
    value_json: &str,
    changed_by_id: Option<i32>,
    change_summary: &str,
) -> AppResult<()> {
    set_setting_recorded(db, key, scope, value_json, changed_by_id, change_summary, false).await
}

/// [`set_setting`], recording whether a fresh step-up was required for the change. The lock
/// checks (fiscal start month, tenant currency), the write and its history row are one
/// transaction, so a posting cannot slip in between the check and the change.
pub async fn set_setting_recorded(
    db: &DatabaseConnection,
    key: &str,
    scope: &str,
    value_json: &str,
    changed_by_id: Option<i32>,
    change_summary: &str,
    required_step_up: bool,
) -> AppResult<()> {
    if value_json.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_SET_VALUE_EMPTY:key={}: setting_value_json for '{}' must not be empty",
            key, key
        )]));
    }

    let _: serde_json::Value = serde_json::from_str(value_json).map_err(|_| {
        AppError::ValidationFailed(vec![format!(
            "GATE_SET_VALUE_JSON_INVALID:key={}: setting_value_json for '{}' is not valid JSON",
            key, key
        )])
    })?;
    let tx = db.begin().await?;
    // Per-key checks: the fiscal-year start month and the tenant currency (stored postings
    // depend on them).
    crate::finance::fiscal::guard_fiscal_start_write(&tx, key, value_json).await?;
    crate::inventory::fx::guard_currency_write(&tx, key, value_json).await?;

    let old_value = get_setting(&tx, key, scope).await?.map(|s| s.setting_value_json);
    let old_hash = old_value.as_deref().map(sha256_hex);

    let new_hash = sha256_hex(value_json);
    let now = chrono::Utc::now().to_rfc3339();
    let (category, risk) = catalog::definition(key).map_or(("general", "low"), |d| (d.category, d.risk));
    // An optional setting set blank (`null`) has no row: blank is never stored as a value.
    let blank = value_json.trim() == "null"
        && catalog::definition(key).is_some_and(|d| matches!(d.kind, catalog::SettingKind::OptionalDecimal { .. }));
    if blank {
        if old_value.is_none() {
            // Already blank: nothing changes, nothing to record.
            return Ok(());
        }
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM app_settings WHERE setting_key = ? AND setting_scope = ?",
            [key.into(), scope.into()],
        ))
        .await?;
        insert_change_event(
            &tx,
            key,
            change_summary,
            old_value.as_deref(),
            None,
            old_hash,
            None,
            changed_by_id.map(i64::from),
            &now,
            required_step_up,
        )
        .await?;
        tx.commit().await?;
        tracing::info!(setting_key = key, scope = scope, actor = ?changed_by_id, "setting cleared");
        return Ok(());
    }

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO app_settings
               (setting_key, setting_scope, setting_value_json, category,
                setting_risk, validation_status, last_modified_by_id, last_modified_at)
           VALUES (?, ?, ?, ?, ?, 'valid', ?, ?)
           ON CONFLICT(setting_key, setting_scope)
           DO UPDATE SET
               setting_value_json = excluded.setting_value_json,
               last_modified_by_id = excluded.last_modified_by_id,
               last_modified_at = excluded.last_modified_at",
        [
            key.into(),
            scope.into(),
            value_json.into(),
            category.into(),
            risk.into(),
            changed_by_id.map(i64::from).into(),
            now.clone().into(),
        ],
    ))
    .await?;

    insert_change_event(
        &tx,
        key,
        change_summary,
        old_value.as_deref(),
        Some(value_json),
        old_hash,
        Some(new_hash),
        changed_by_id.map(i64::from),
        &now,
        required_step_up,
    )
    .await?;
    tx.commit().await?;

    tracing::info!(
        setting_key = key,
        scope = scope,
        actor = ?changed_by_id,
        "setting updated"
    );

    Ok(())
}

/// Remove a row from `app_settings` and append a settings change event (delete semantics).
pub async fn delete_setting(
    db: &DatabaseConnection,
    key: &str,
    scope: &str,
    changed_by_id: Option<i32>,
    change_summary: &str,
) -> AppResult<bool> {
    let old = get_setting(db, key, scope).await?;
    let Some(old_row) = old else {
        return Ok(false);
    };
    let old_hash = sha256_hex(&old_row.setting_value_json);
    let now = chrono::Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM app_settings WHERE setting_key = ? AND setting_scope = ?",
        [key.into(), scope.into()],
    ))
    .await?;

    insert_change_event(
        db,
        key,
        change_summary,
        Some(&old_row.setting_value_json),
        None,
        Some(old_hash),
        None,
        changed_by_id.map(i64::from),
        &now,
        false,
    )
    .await?;

    tracing::info!(
        setting_key = key,
        scope = scope,
        actor = ?changed_by_id,
        "setting deleted"
    );

    Ok(true)
}

/// Append one row to the settings change history (hashes and, from migration 213, values).
/// Values are kept only for plant parameters (`catalog::keeps_value_history`): module and device
/// state (licence onboarding with its activation token, bootstrap, rating scales, notification
/// state) is recorded by hash only, because the history is synced and readable in Settings.
pub(crate) async fn insert_change_event<C: ConnectionTrait>(
    db: &C,
    key_or_domain: &str,
    change_summary: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
    old_hash: Option<String>,
    new_hash: Option<String>,
    changed_by_id: Option<i64>,
    changed_at: &str,
    required_step_up: bool,
) -> AppResult<()> {
    let (old_value, new_value) = if catalog::keeps_value_history(key_or_domain) {
        (old_value, new_value)
    } else {
        (None, None)
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO settings_change_events
               (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                changed_by_id, changed_at, required_step_up, apply_result,
                old_value_json, new_value_json)
           VALUES (?, ?, ?, ?, ?, ?, ?, 'applied', ?, ?)",
        [
            key_or_domain.into(),
            change_summary.into(),
            old_hash.into(),
            new_hash.into(),
            changed_by_id.into(),
            changed_at.into(),
            i32::from(required_step_up).into(),
            old_value.map(str::to_string).into(),
            new_value.map(str::to_string).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Clear the values recorded for keys whose history must stay hash-only (see
/// [`catalog::keeps_value_history`]). Run by migration 213 and at every startup (a database that
/// applied an earlier build of 213 may hold some); a no-op once clean. Returns the rows cleared.
pub async fn scrub_hash_only_history<C: ConnectionTrait>(db: &C) -> AppResult<u64> {
    let mut allowed: Vec<&str> = catalog::SETTINGS_CATALOG
        .iter()
        .map(|d| d.key)
        .filter(|k| catalog::is_plant_parameter(k))
        .collect();
    allowed.extend_from_slice(catalog::POLICY_DOMAINS_WITH_VALUE_HISTORY);
    allowed.extend_from_slice(catalog::RETIRED_PLANT_PARAMETERS);
    let placeholders = vec!["?"; allowed.len()].join(",");
    let sql = format!(
        "UPDATE settings_change_events SET old_value_json = NULL, new_value_json = NULL \
         WHERE (old_value_json IS NOT NULL OR new_value_json IS NOT NULL) \
           AND setting_key_or_domain NOT LIKE 'rbac.%' \
           AND setting_key_or_domain NOT IN ({placeholders})"
    );
    let values: Vec<sea_orm::Value> = allowed.iter().map(|k| (*k).into()).collect();
    let result = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?;
    Ok(result.rows_affected())
}

/// Most recent settings changes, optionally for one key or policy domain only.
///
/// Without a key, the history leaves out the machine-written rows of the licence checks
/// (`product.*`) and of the first-start tenant set-up (`tenant.initial_bootstrap`): they are
/// recorded for the audit trail and would push people's own changes out of the list.
pub async fn list_change_events(
    db: &DatabaseConnection,
    limit: i64,
    key_or_domain: Option<&str>,
) -> AppResult<Vec<SettingsChangeEvent>> {
    let safe_limit = limit.clamp(1, 500);
    let key = key_or_domain.map(str::trim).filter(|k| !k.is_empty());
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                   e.id,
                   e.setting_key_or_domain,
                   e.change_summary,
                   e.old_value_hash,
                   e.new_value_hash,
                   e.changed_by_id,
                   COALESCE(u.display_name, u.username) AS changed_by_name,
                   COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', e.changed_at), e.changed_at) AS changed_at,
                   e.required_step_up,
                   e.apply_result,
                   e.old_value_json,
                   e.new_value_json
               FROM settings_change_events e
               LEFT JOIN user_accounts u ON u.id = e.changed_by_id
               WHERE (? IS NULL OR e.setting_key_or_domain = ?)
                 AND (? IS NOT NULL
                      OR (e.setting_key_or_domain NOT LIKE 'product.%'
                          AND e.setting_key_or_domain <> 'tenant.initial_bootstrap'))
               ORDER BY e.changed_at DESC, e.id DESC
               LIMIT ?",
            [
                key.map(str::to_string).into(),
                key.map(str::to_string).into(),
                key.map(str::to_string).into(),
                safe_limit.into(),
            ],
        ))
        .await?;

    rows.into_iter()
        .map(|row| {
            let mut event = map_settings_change_event(row)?;
            // Rows recorded by an older build (or received from one) may carry values of keys
            // whose history must stay hash-only.
            if !catalog::keeps_value_history(&event.setting_key_or_domain) {
                event.old_value_json = None;
                event.new_value_json = None;
            }
            Ok(event)
        })
        .collect()
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("settings row decode failed for column '{column}': {e}"))
}

fn map_app_setting(row: QueryResult) -> AppResult<AppSetting> {
    Ok(AppSetting {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        setting_key: row
            .try_get::<String>("", "setting_key")
            .map_err(|e| decode_err("setting_key", e))?,
        setting_scope: row
            .try_get::<String>("", "setting_scope")
            .map_err(|e| decode_err("setting_scope", e))?,
        setting_value_json: row
            .try_get::<String>("", "setting_value_json")
            .map_err(|e| decode_err("setting_value_json", e))?,
        category: row
            .try_get::<String>("", "category")
            .map_err(|e| decode_err("category", e))?,
        setting_risk: row
            .try_get::<String>("", "setting_risk")
            .map_err(|e| decode_err("setting_risk", e))?,
        validation_status: row
            .try_get::<String>("", "validation_status")
            .map_err(|e| decode_err("validation_status", e))?,
        secret_ref_id: row
            .try_get::<Option<i64>>("", "secret_ref_id")
            .map_err(|e| decode_err("secret_ref_id", e))?,
        last_modified_by_id: row
            .try_get::<Option<i64>>("", "last_modified_by_id")
            .map_err(|e| decode_err("last_modified_by_id", e))?,
        last_modified_at: row
            .try_get::<String>("", "last_modified_at")
            .map_err(|e| decode_err("last_modified_at", e))?,
    })
}

fn map_policy_snapshot(row: QueryResult) -> AppResult<PolicySnapshot> {
    let is_active = row
        .try_get::<i64>("", "is_active")
        .map_err(|e| decode_err("is_active", e))?
        != 0;

    Ok(PolicySnapshot {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        policy_domain: row
            .try_get::<String>("", "policy_domain")
            .map_err(|e| decode_err("policy_domain", e))?,
        version_no: row
            .try_get::<i64>("", "version_no")
            .map_err(|e| decode_err("version_no", e))?,
        snapshot_json: row
            .try_get::<String>("", "snapshot_json")
            .map_err(|e| decode_err("snapshot_json", e))?,
        is_active,
        activated_at: row
            .try_get::<Option<String>>("", "activated_at")
            .map_err(|e| decode_err("activated_at", e))?,
        activated_by_id: row
            .try_get::<Option<i64>>("", "activated_by_id")
            .map_err(|e| decode_err("activated_by_id", e))?,
    })
}

fn map_settings_change_event(row: QueryResult) -> AppResult<SettingsChangeEvent> {
    let required_step_up = row
        .try_get::<i64>("", "required_step_up")
        .map_err(|e| decode_err("required_step_up", e))?
        != 0;

    Ok(SettingsChangeEvent {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        setting_key_or_domain: row
            .try_get::<String>("", "setting_key_or_domain")
            .map_err(|e| decode_err("setting_key_or_domain", e))?,
        change_summary: row
            .try_get::<String>("", "change_summary")
            .map_err(|e| decode_err("change_summary", e))?,
        old_value_hash: row
            .try_get::<Option<String>>("", "old_value_hash")
            .map_err(|e| decode_err("old_value_hash", e))?,
        new_value_hash: row
            .try_get::<Option<String>>("", "new_value_hash")
            .map_err(|e| decode_err("new_value_hash", e))?,
        changed_by_id: row
            .try_get::<Option<i64>>("", "changed_by_id")
            .map_err(|e| decode_err("changed_by_id", e))?,
        changed_by_name: row
            .try_get::<Option<String>>("", "changed_by_name")
            .map_err(|e| decode_err("changed_by_name", e))?,
        old_value_json: row
            .try_get::<Option<String>>("", "old_value_json")
            .map_err(|e| decode_err("old_value_json", e))?,
        new_value_json: row
            .try_get::<Option<String>>("", "new_value_json")
            .map_err(|e| decode_err("new_value_json", e))?,
        changed_at: row
            .try_get::<String>("", "changed_at")
            .map_err(|e| decode_err("changed_at", e))?,
        required_step_up,
        apply_result: row
            .try_get::<String>("", "apply_result")
            .map_err(|e| decode_err("apply_result", e))?,
    })
}

/// Validation outcome for a policy draft rule. `code` + `params` let the screen say it in the
/// user's language (`POLICY_TEST_*`); `message` is the server's English text, kept as a fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTestResult {
    pub rule_name: String,
    pub severity: String,
    pub code: String,
    pub params: std::collections::BTreeMap<String, String>,
    pub message: String,
}

/// Stable codes of the policy draft test results (`PolicyTestResult::code`).
pub mod policy_test_codes {
    /// The draft is not a valid policy document. Params: `domain`, `detail`.
    pub const SCHEMA_INVALID: &str = "POLICY_TEST_SCHEMA_INVALID";
    /// A value is within its range. Params: `field`, `value`, `min`, `max`.
    pub const IN_RANGE: &str = "POLICY_TEST_IN_RANGE";
    /// A value is outside its range. Params: `field`, `value`, `min`, `max`.
    pub const OUT_OF_RANGE: &str = "POLICY_TEST_OUT_OF_RANGE";
    /// The idle timeout fits in the session length. Params: `idle`, `absolute`.
    pub const IDLE_WITHIN_ABSOLUTE: &str = "POLICY_TEST_IDLE_WITHIN_ABSOLUTE";
    /// The idle timeout exceeds the session length. Params: `idle`, `absolute`.
    pub const IDLE_EXCEEDS_ABSOLUTE: &str = "POLICY_TEST_IDLE_EXCEEDS_ABSOLUTE";
    /// The backup schedule is empty. No params.
    pub const SCHEDULE_EMPTY: &str = "POLICY_TEST_SCHEDULE_EMPTY";
    /// The backup schedule cannot be read. Params: `schedule`, `detail`.
    pub const SCHEDULE_INVALID: &str = "POLICY_TEST_SCHEDULE_INVALID";
    /// The backup schedule is valid. Params: `schedule`.
    pub const SCHEDULE_VALID: &str = "POLICY_TEST_SCHEDULE_VALID";
    /// Whether photos are included in backups (information). Params: `value` (`true` / `false`).
    pub const INCLUDE_PHOTOS: &str = "POLICY_TEST_INCLUDE_PHOTOS";
    /// The password policy is edited in its own panel. No params.
    pub const PASSWORD_ELSEWHERE: &str = "POLICY_TEST_PASSWORD_ELSEWHERE";
    /// The policy domain is not known. Params: `domain`.
    pub const DOMAIN_UNSUPPORTED: &str = "POLICY_TEST_DOMAIN_UNSUPPORTED";
}

fn policy_result(
    rule_name: &str,
    severity: &str,
    code: &str,
    params: &[(&str, String)],
    message: String,
) -> PolicyTestResult {
    PolicyTestResult {
        rule_name: rule_name.into(),
        severity: severity.into(),
        code: code.into(),
        params: params.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
        message,
    }
}

/// Read-only connection profile row for the Integrations tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: i64,
    pub integration_type: String,
    pub profile_name: String,
    pub status: String,
    pub last_tested_at: Option<String>,
    pub last_test_result: Option<String>,
    pub created_at: String,
}

/// Backup operational policy document stored in `policy_snapshots`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupPolicy {
    pub schedule_cron: String,
    pub retention_days: u32,
    pub include_photos: bool,
    pub compression_level: u32,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            schedule_cron: "0 2 * * *".to_string(),
            retention_days: 30,
            include_photos: true,
            compression_level: 6,
        }
    }
}

fn is_security_domain(domain: &str) -> bool {
    matches!(domain, "session" | "password")
}

/// List all policy snapshots for a domain, newest version first.
pub async fn list_policy_snapshots(db: &DatabaseConnection, domain: &str) -> AppResult<Vec<PolicySnapshot>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   policy_domain,
                   version_no,
                   snapshot_json,
                   is_active,
                   CASE
                       WHEN activated_at IS NULL THEN NULL
                       ELSE COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', activated_at), activated_at)
                   END AS activated_at,
                   activated_by_id
               FROM policy_snapshots
               WHERE policy_domain = ?
               ORDER BY version_no DESC",
            [domain.into()],
        ))
        .await?;

    rows.into_iter().map(map_policy_snapshot).collect()
}

async fn get_draft_policy(db: &DatabaseConnection, domain: &str) -> AppResult<Option<PolicySnapshot>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   policy_domain,
                   version_no,
                   snapshot_json,
                   is_active,
                   CASE
                       WHEN activated_at IS NULL THEN NULL
                       ELSE COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', activated_at), activated_at)
                   END AS activated_at,
                   activated_by_id
               FROM policy_snapshots
               WHERE policy_domain = ? AND is_active = 0 AND activated_at IS NULL
               ORDER BY version_no DESC
               LIMIT 1",
            [domain.into()],
        ))
        .await?;

    row.map(map_policy_snapshot).transpose()
}

async fn next_policy_version(db: &DatabaseConnection, domain: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(version_no), 0) AS max_ver FROM policy_snapshots WHERE policy_domain = ?",
            [domain.into()],
        ))
        .await?;

    let max = row.map(|r| r.try_get::<i64>("", "max_ver").unwrap_or(0)).unwrap_or(0);
    Ok(max + 1)
}

/// Create or update the single draft snapshot for a domain.
pub async fn save_policy_draft(
    db: &DatabaseConnection,
    domain: &str,
    snapshot_json: &str,
    actor_id: i32,
) -> AppResult<PolicySnapshot> {
    if domain.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_SET_POLICY_DOMAIN_REQUIRED: policy domain must not be empty".to_string(),
        ]));
    }
    let _: serde_json::Value = serde_json::from_str(snapshot_json).map_err(|_| {
        AppError::ValidationFailed(vec![
            "GATE_SET_POLICY_JSON_INVALID: snapshot_json is not valid JSON".to_string()
        ])
    })?;

    let now = chrono::Utc::now().to_rfc3339();

    if let Some(existing) = get_draft_policy(db, domain).await? {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"UPDATE policy_snapshots
               SET snapshot_json = ?
               WHERE id = ?",
            [snapshot_json.into(), existing.id.into()],
        ))
        .await?;

        insert_change_event(
            db,
            domain,
            &format!("Policy draft updated for domain '{domain}'"),
            Some(&existing.snapshot_json),
            Some(snapshot_json),
            Some(sha256_hex(&existing.snapshot_json)),
            Some(sha256_hex(snapshot_json)),
            Some(i64::from(actor_id)),
            &now,
            false,
        )
        .await?;

        return get_draft_policy(db, domain)
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("draft missing after update")));
    }

    let version = next_policy_version(db, domain).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO policy_snapshots
               (policy_domain, version_no, snapshot_json, is_active, activated_at, activated_by_id)
           VALUES (?, ?, ?, 0, NULL, NULL)",
        [domain.into(), version.into(), snapshot_json.into()],
    ))
    .await?;

    insert_change_event(
        db,
        domain,
        &format!("Policy draft created for domain '{domain}'"),
        None,
        Some(snapshot_json),
        None,
        Some(sha256_hex(snapshot_json)),
        Some(i64::from(actor_id)),
        &now,
        false,
    )
    .await?;

    get_draft_policy(db, domain)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("draft missing after insert")))
}

/// Validate the current draft for a domain.
pub async fn test_policy_draft(db: &DatabaseConnection, domain: &str) -> AppResult<Vec<PolicyTestResult>> {
    let draft = get_draft_policy(db, domain).await?.ok_or_else(|| {
        AppError::ValidationFailed(vec![format!(
            "GATE_SET_POLICY_NO_DRAFT:domain={domain}: no draft exists for domain '{domain}'"
        )])
    })?;

    Ok(validate_policy_json(domain, &draft.snapshot_json))
}

fn validate_policy_json(domain: &str, snapshot_json: &str) -> Vec<PolicyTestResult> {
    use policy_test_codes as c;
    let mut results = Vec::new();

    match domain {
        "session" => match serde_json::from_str::<SessionPolicy>(snapshot_json) {
            Err(e) => results.push(policy_result(
                "schema",
                "fail",
                c::SCHEMA_INVALID,
                &[("domain", domain.to_string()), ("detail", e.to_string())],
                format!("Session policy JSON is invalid: {e}"),
            )),
            Ok(p) => {
                results.push(range_rule("idle_timeout_minutes", p.idle_timeout_minutes, 1, 480));
                results.push(range_rule(
                    "absolute_session_minutes",
                    p.absolute_session_minutes,
                    15,
                    1440,
                ));
                results.push(range_rule("offline_grace_hours", p.offline_grace_hours, 0, 720));
                results.push(range_rule("step_up_window_minutes", p.step_up_window_minutes, 1, 30));
                results.push(range_rule("max_failed_attempts", p.max_failed_attempts, 1, 20));
                results.push(range_rule("lockout_minutes", p.lockout_minutes, 1, 1440));
                let params = [
                    ("idle", p.idle_timeout_minutes.to_string()),
                    ("absolute", p.absolute_session_minutes.to_string()),
                ];
                if p.idle_timeout_minutes > p.absolute_session_minutes {
                    results.push(policy_result(
                        "idle_vs_absolute",
                        "fail",
                        c::IDLE_EXCEEDS_ABSOLUTE,
                        &params,
                        "Idle timeout must not exceed absolute session length".into(),
                    ));
                } else {
                    results.push(policy_result(
                        "idle_vs_absolute",
                        "pass",
                        c::IDLE_WITHIN_ABSOLUTE,
                        &params,
                        "Idle timeout is within absolute session length".into(),
                    ));
                }
            }
        },
        "backup" => match serde_json::from_str::<BackupPolicy>(snapshot_json) {
            Err(e) => results.push(policy_result(
                "schema",
                "fail",
                c::SCHEMA_INVALID,
                &[("domain", domain.to_string()), ("detail", e.to_string())],
                format!("Backup policy JSON is invalid: {e}"),
            )),
            Ok(p) => {
                if p.schedule_cron.trim().is_empty() {
                    results.push(policy_result(
                        "schedule_cron",
                        "fail",
                        c::SCHEDULE_EMPTY,
                        &[],
                        "Backup schedule cron must not be empty".into(),
                    ));
                } else if let Err(e) = crate::backup::scheduler::parse_backup_schedule(&p.schedule_cron) {
                    results.push(policy_result(
                        "schedule_cron",
                        "fail",
                        c::SCHEDULE_INVALID,
                        &[("schedule", p.schedule_cron.clone()), ("detail", e.clone())],
                        e,
                    ));
                } else {
                    results.push(policy_result(
                        "schedule_cron",
                        "pass",
                        c::SCHEDULE_VALID,
                        &[("schedule", p.schedule_cron.clone())],
                        "Backup schedule cron is valid".into(),
                    ));
                }
                results.push(range_rule("retention_days", p.retention_days, 1, 3650));
                results.push(range_rule("compression_level", p.compression_level, 0, 9));
                results.push(policy_result(
                    "include_photos",
                    "pass",
                    c::INCLUDE_PHOTOS,
                    &[("value", p.include_photos.to_string())],
                    format!("include_photos={}", p.include_photos),
                ));
            }
        },
        "password" => results.push(policy_result(
            "password_ssot",
            "warn",
            c::PASSWORD_ELSEWHERE,
            &[],
            "Password policy is managed via RBAC settings; use PasswordPolicyPanel".into(),
        )),
        other => results.push(policy_result(
            "domain",
            "fail",
            c::DOMAIN_UNSUPPORTED,
            &[("domain", other.to_string())],
            format!("Unsupported policy domain '{other}'"),
        )),
    }

    results
}

fn range_rule(name: &str, value: u32, min: u32, max: u32) -> PolicyTestResult {
    let params = [
        ("field", name.to_string()),
        ("value", value.to_string()),
        ("min", min.to_string()),
        ("max", max.to_string()),
    ];
    if value < min || value > max {
        policy_result(
            name,
            "fail",
            policy_test_codes::OUT_OF_RANGE,
            &params,
            format!("{name}={value} is outside allowed range [{min}, {max}]"),
        )
    } else {
        policy_result(
            name,
            "pass",
            policy_test_codes::IN_RANGE,
            &params,
            format!("{name}={value} is within [{min}, {max}]"),
        )
    }
}

/// Promote a draft snapshot to active. Caller must enforce step-up for security domains.
pub async fn activate_policy(
    db: &DatabaseConnection,
    domain: &str,
    snapshot_id: i64,
    actor_id: i32,
) -> AppResult<PolicySnapshot> {
    let snapshots = list_policy_snapshots(db, domain).await?;
    let target = snapshots
        .into_iter()
        .find(|s| s.id == snapshot_id)
        .ok_or_else(|| AppError::NotFound {
            entity: "policy_snapshot".into(),
            id: snapshot_id.to_string(),
        })?;

    if target.is_active {
        return Ok(target);
    }

    let failures: Vec<_> = validate_policy_json(domain, &target.snapshot_json)
        .into_iter()
        .filter(|r| r.severity == "fail")
        .collect();
    if !failures.is_empty() {
        return Err(AppError::ValidationFailed(
            failures.into_iter().map(|f| f.message).collect(),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let required_step_up = is_security_domain(domain);
    let previous_active = get_active_policy(db, domain).await?.map(|p| p.snapshot_json);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE policy_snapshots SET is_active = 0 WHERE policy_domain = ? AND is_active = 1",
        [domain.into()],
    ))
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"UPDATE policy_snapshots
           SET is_active = 1, activated_at = ?, activated_by_id = ?
           WHERE id = ?",
        [now.clone().into(), i64::from(actor_id).into(), snapshot_id.into()],
    ))
    .await?;

    insert_change_event(
        db,
        domain,
        &format!("Policy activated for domain '{domain}' (snapshot {snapshot_id})"),
        previous_active.as_deref(),
        Some(&target.snapshot_json),
        previous_active.as_deref().map(sha256_hex),
        Some(sha256_hex(&target.snapshot_json)),
        Some(i64::from(actor_id)),
        &now,
        required_step_up,
    )
    .await?;

    get_active_policy(db, domain)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("active policy missing after activate")))
}

/// Delete the current draft for a domain (if any).
pub async fn discard_policy_draft(db: &DatabaseConnection, domain: &str, actor_id: i32) -> AppResult<()> {
    let Some(draft) = get_draft_policy(db, domain).await? else {
        return Ok(());
    };

    let now = chrono::Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM policy_snapshots WHERE id = ?",
        [draft.id.into()],
    ))
    .await?;

    insert_change_event(
        db,
        domain,
        &format!("Policy draft discarded for domain '{domain}'"),
        Some(&draft.snapshot_json),
        None,
        Some(sha256_hex(&draft.snapshot_json)),
        None,
        Some(i64::from(actor_id)),
        &now,
        false,
    )
    .await?;

    Ok(())
}

/// Ensure active session/backup policy snapshots exist (idempotent).
pub async fn ensure_default_policies(db: &DatabaseConnection) -> AppResult<()> {
    if get_active_policy(db, "session").await?.is_none() {
        let json = serde_json::to_string(&SessionPolicy::default())?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO policy_snapshots
                   (policy_domain, version_no, snapshot_json, is_active, activated_at, activated_by_id)
               SELECT 'session', 1, ?, 1, ?, NULL
               WHERE NOT EXISTS (
                   SELECT 1 FROM policy_snapshots WHERE policy_domain = 'session' AND is_active = 1
               )",
            [json.into(), chrono::Utc::now().to_rfc3339().into()],
        ))
        .await?;
    }

    if get_active_policy(db, "backup").await?.is_none() {
        let json = serde_json::to_string(&BackupPolicy::default())?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO policy_snapshots
                   (policy_domain, version_no, snapshot_json, is_active, activated_at, activated_by_id)
               SELECT 'backup', 1, ?, 1, ?, NULL
               WHERE NOT EXISTS (
                   SELECT 1 FROM policy_snapshots WHERE policy_domain = 'backup' AND is_active = 1
               )",
            [json.into(), chrono::Utc::now().to_rfc3339().into()],
        ))
        .await?;
    }

    Ok(())
}

/// List connection profiles for the Integrations settings tab (read-only).
pub async fn list_connection_profiles(db: &DatabaseConnection) -> AppResult<Vec<ConnectionProfile>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   integration_type,
                   profile_name,
                   status,
                   last_tested_at,
                   last_test_result,
                   COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', created_at), created_at) AS created_at
               FROM connection_profiles
               ORDER BY profile_name"
                .to_string(),
        ))
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ConnectionProfile {
                id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
                integration_type: row
                    .try_get::<String>("", "integration_type")
                    .map_err(|e| decode_err("integration_type", e))?,
                profile_name: row
                    .try_get::<String>("", "profile_name")
                    .map_err(|e| decode_err("profile_name", e))?,
                status: row
                    .try_get::<String>("", "status")
                    .map_err(|e| decode_err("status", e))?,
                last_tested_at: row
                    .try_get::<Option<String>>("", "last_tested_at")
                    .map_err(|e| decode_err("last_tested_at", e))?,
                last_test_result: row
                    .try_get::<Option<String>>("", "last_test_result")
                    .map_err(|e| decode_err("last_test_result", e))?,
                created_at: row
                    .try_get::<String>("", "created_at")
                    .map_err(|e| decode_err("created_at", e))?,
            })
        })
        .collect()
}

#[cfg(test)]
mod policy_test_code_tests {
    use super::{policy_test_codes as c, validate_policy_json};

    /// Every test result carries a code and the params its translation needs; the English
    /// message stays as the fallback.
    #[test]
    fn policy_test_results_are_coded() {
        let session = validate_policy_json(
            "session",
            &serde_json::to_string(&super::SessionPolicy {
                idle_timeout_minutes: 600,
                ..super::SessionPolicy::default()
            })
            .expect("json"),
        );
        let idle = session
            .iter()
            .find(|r| r.rule_name == "idle_timeout_minutes")
            .expect("idle");
        assert_eq!(idle.code, c::OUT_OF_RANGE);
        assert_eq!(idle.params["field"], "idle_timeout_minutes");
        assert_eq!(idle.params["value"], "600");
        assert_eq!((idle.params["min"].as_str(), idle.params["max"].as_str()), ("1", "480"));
        assert!(!idle.message.is_empty(), "the server text stays as a fallback");
        assert!(session.iter().all(|r| r.code.starts_with("POLICY_TEST_")));

        let bad = validate_policy_json("backup", "{");
        assert_eq!(bad[0].code, c::SCHEMA_INVALID);
        assert_eq!(bad[0].params["domain"], "backup");
        let other = validate_policy_json("nope", "{}");
        assert_eq!(other[0].code, c::DOMAIN_UNSUPPORTED);
        assert_eq!(other[0].params["domain"], "nope");
        assert_eq!(validate_policy_json("password", "{}")[0].code, c::PASSWORD_ELSEWHERE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, Statement};

    #[test]
    fn sha256_hex_is_stable() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn load_session_policy_uses_defaults_when_missing() {
        let db = crate::db::test_db::migrated().await;

        let policy = load_session_policy(&db).await;
        assert_eq!(policy, SessionPolicy::default());
    }

    #[tokio::test]
    async fn set_setting_writes_setting_and_audit_event() {
        let db = crate::db::test_db::migrated().await;

        set_setting(
            &db,
            "appearance.color_mode",
            "tenant",
            r#""dark""#,
            Some(1),
            "Changed color mode",
        )
        .await
        .expect("set_setting should succeed");

        let settings_count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM app_settings WHERE setting_key = 'appearance.color_mode' AND setting_scope = 'tenant';".to_string(),
            ))
            .await
            .expect("settings count query should succeed")
            .expect("settings count query should return a row")
            .try_get::<i64>("", "cnt")
            .expect("settings count should be readable");

        let audit_count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                // Migrations write their own history rows (the retired count tolerance, migration 259).
                "SELECT COUNT(*) AS cnt FROM settings_change_events WHERE setting_key_or_domain = 'appearance.color_mode';"
                    .to_string(),
            ))
            .await
            .expect("audit count query should succeed")
            .expect("audit count query should return a row")
            .try_get::<i64>("", "cnt")
            .expect("audit count should be readable");

        assert_eq!(settings_count, 1, "setting row should be upserted");
        assert_eq!(audit_count, 1, "audit row should be appended");
    }

    #[tokio::test]
    async fn change_history_keeps_values_and_filters_by_key() {
        let db = crate::db::test_db::seeded().await;
        set_setting(
            &db,
            "wo.reopen_window_days",
            "tenant",
            "45",
            Some(1),
            "Reopen window 45",
        )
        .await
        .expect("set");
        set_setting(
            &db,
            "backup.retention_daily",
            "tenant",
            "10",
            Some(1),
            "Keep 10 dailies",
        )
        .await
        .expect("set");

        let reopen = list_change_events(&db, 50, Some("wo.reopen_window_days"))
            .await
            .expect("history");
        assert_eq!(reopen.len(), 1, "only the requested key");
        assert_eq!(reopen[0].old_value_json.as_deref(), Some("30"), "shipped value before");
        assert_eq!(reopen[0].new_value_json.as_deref(), Some("45"));
        assert!(list_change_events(&db, 50, None).await.expect("all").len() >= 2);
    }

    #[tokio::test]
    async fn licence_state_changes_record_no_value() {
        let db = crate::db::test_db::seeded().await;
        set_setting(
            &db,
            "product.license_onboarding",
            "device",
            r#"{"activation_claim":{"activation_token":"tok-123"}}"#,
            Some(1),
            "licence",
        )
        .await
        .expect("set");
        let stored: Option<String> = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COALESCE(old_value_json, new_value_json) AS v FROM settings_change_events \
                 WHERE setting_key_or_domain = 'product.license_onboarding'"
                    .to_string(),
            ))
            .await
            .unwrap()
            .expect("history row")
            .try_get("", "v")
            .unwrap();
        assert_eq!(
            stored, None,
            "the activation token is not copied into the synced history"
        );

        // A row written by an earlier build is cleared at startup and blanked on read.
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO settings_change_events \
             (setting_key_or_domain, change_summary, changed_at, required_step_up, apply_result, new_value_json) \
             VALUES ('product.license_onboarding', 'old build', '2026-10-16T00:00:00Z', 0, 'applied', '{\"t\":1}')"
                .to_string(),
        ))
        .await
        .unwrap();
        let listed = list_change_events(&db, 50, Some("product.license_onboarding"))
            .await
            .unwrap();
        assert!(listed
            .iter()
            .all(|e| e.new_value_json.is_none() && e.old_value_json.is_none()));
        assert_eq!(scrub_hash_only_history(&db).await.unwrap(), 1);
        assert_eq!(scrub_hash_only_history(&db).await.unwrap(), 0, "idempotent");
    }

    #[tokio::test]
    async fn the_history_leaves_out_machine_written_rows_unless_asked_for_by_key() {
        let db = crate::db::test_db::seeded().await;
        set_setting_recorded(&db, "wo.reopen_window_days", "tenant", "40", Some(1), "x", false)
            .await
            .unwrap();
        for key in ["product.license_onboarding", "tenant.initial_bootstrap"] {
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "INSERT INTO settings_change_events                      (setting_key_or_domain, change_summary, changed_at, required_step_up, apply_result)                      VALUES ('{key}', 'machine', '2026-10-16T00:00:00Z', 0, 'applied')"
                ),
            ))
            .await
            .unwrap();
        }
        let all = list_change_events(&db, 50, None).await.unwrap();
        assert!(all.iter().any(|e| e.setting_key_or_domain == "wo.reopen_window_days"));
        assert!(
            all.iter().all(|e| !e.setting_key_or_domain.starts_with("product.")
                && e.setting_key_or_domain != "tenant.initial_bootstrap"),
            "licence checks and first-start set-up do not push people's changes out of the list"
        );
        let by_key = list_change_events(&db, 50, Some("product.license_onboarding"))
            .await
            .unwrap();
        assert!(!by_key.is_empty(), "a key asked for by name is still listed");
    }

    #[tokio::test]
    async fn step_up_is_recorded_when_it_was_required() {
        let db = crate::db::test_db::seeded().await;
        set_setting_recorded(&db, "wo.reopen_window_days", "tenant", "40", Some(1), "x", true)
            .await
            .unwrap();
        let events = list_change_events(&db, 1, Some("wo.reopen_window_days")).await.unwrap();
        assert!(events[0].required_step_up);
    }

    #[tokio::test]
    async fn tenant_currency_locks_once_amounts_are_stored() {
        let db = crate::db::test_db::seeded().await;
        set_setting(&db, "locale.currency_code", "tenant", r#""EUR""#, Some(1), "EUR")
            .await
            .expect("free to change before any valued data");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO inventory_valuation_periods \
             (entity_sync_id, period_code, period_start, period_end, currency_code, closed_at, created_at, updated_at) \
             VALUES ('lock-test', '2026-01', '2026-01-01', '2026-01-31', 'EUR', '2026-02-01', '2026-02-01', '2026-02-01')"
                .to_string(),
        ))
        .await
        .expect("closed valuation period");

        let err = set_setting(&db, "locale.currency_code", "tenant", r#""USD""#, Some(1), "USD")
            .await
            .expect_err("locked once a period is valued");
        assert!(err.to_string().contains("GATE_FIN_CURRENCY_LOCKED"));
        set_setting(&db, "locale.currency_code", "tenant", r#""EUR""#, Some(1), "same value")
            .await
            .expect("re-saving the current currency is not a change");
        assert!(catalog::lock_reason(&db, "locale.currency_code")
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn set_setting_rejects_invalid_json() {
        let db = crate::db::test_db::migrated().await;

        let err = set_setting(
            &db,
            "appearance.color_mode",
            "tenant",
            "not-json",
            Some(1),
            "Invalid update",
        )
        .await
        .expect_err("invalid JSON should be rejected");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn ensure_default_policies_seeds_active_session_and_backup() {
        let db = crate::db::test_db::migrated().await;

        ensure_default_policies(&db)
            .await
            .expect("ensure_default_policies should succeed");

        let session = get_active_policy(&db, "session")
            .await
            .expect("session lookup")
            .expect("active session policy should exist");
        assert!(session.is_active);
        assert_eq!(session.policy_domain, "session");
        let parsed: SessionPolicy = serde_json::from_str(&session.snapshot_json).expect("session JSON");
        assert_eq!(parsed, SessionPolicy::default());

        let backup = get_active_policy(&db, "backup")
            .await
            .expect("backup lookup")
            .expect("active backup policy should exist");
        assert!(backup.is_active);
        assert_eq!(backup.policy_domain, "backup");

        // Idempotent: second call must not duplicate actives.
        ensure_default_policies(&db)
            .await
            .expect("ensure_default_policies should be idempotent");
        let session_rows = list_policy_snapshots(&db, "session").await.expect("list session");
        assert_eq!(
            session_rows.iter().filter(|s| s.is_active).count(),
            1,
            "exactly one active session policy"
        );
    }

    #[tokio::test]
    async fn policy_draft_test_activate_discard_lifecycle() {
        let db = crate::db::test_db::migrated().await;

        ensure_default_policies(&db).await.expect("seed defaults");

        let mut draft_policy = SessionPolicy::default();
        draft_policy.idle_timeout_minutes = 45;
        let draft_json = serde_json::to_string(&draft_policy).expect("serialize draft");

        let draft = save_policy_draft(&db, "session", &draft_json, 1)
            .await
            .expect("save draft");
        assert!(!draft.is_active);
        assert!(draft.activated_at.is_none());

        let results = test_policy_draft(&db, "session").await.expect("test draft");
        assert!(
            results.iter().all(|r| r.severity != "fail"),
            "default-range draft should pass: {results:?}"
        );

        let activated = activate_policy(&db, "session", draft.id, 1)
            .await
            .expect("activate draft");
        assert!(activated.is_active);
        assert_eq!(activated.id, draft.id);

        let active = get_active_policy(&db, "session")
            .await
            .expect("get active")
            .expect("active exists");
        let loaded: SessionPolicy = serde_json::from_str(&active.snapshot_json).expect("parse active");
        assert_eq!(loaded.idle_timeout_minutes, 45);

        let second = SessionPolicy {
            idle_timeout_minutes: 20,
            ..SessionPolicy::default()
        };
        let second_json = serde_json::to_string(&second).expect("serialize second");
        let second_draft = save_policy_draft(&db, "session", &second_json, 1)
            .await
            .expect("save second draft");
        discard_policy_draft(&db, "session", 1).await.expect("discard draft");
        let after_discard = list_policy_snapshots(&db, "session").await.expect("list after discard");
        assert!(
            after_discard.iter().all(|s| s.id != second_draft.id),
            "discarded draft must be gone"
        );
        assert!(
            get_active_policy(&db, "session").await.expect("active still").is_some(),
            "active policy remains after discard"
        );
    }
}
