//! Settings service.
//!
//! Provides typed access to `app_settings`, `policy_snapshots`, and
//! `settings_change_events`. All writes through this module emit an audit event.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod branding;

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
    pub changed_at: String,
    pub required_step_up: bool,
    pub apply_result: String,
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

pub async fn get_setting(db: &DatabaseConnection, key: &str, scope: &str) -> AppResult<Option<AppSetting>> {
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
    if value_json.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "setting_value_json for '{}' must not be empty",
            key
        )]));
    }

    let _: serde_json::Value = serde_json::from_str(value_json)
        .map_err(|_| AppError::ValidationFailed(vec![format!("setting_value_json for '{}' is not valid JSON", key)]))?;

    let old_hash = get_setting(db, key, scope)
        .await?
        .map(|s| sha256_hex(&s.setting_value_json));

    let new_hash = sha256_hex(value_json);
    let now = chrono::Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO app_settings
               (setting_key, setting_scope, setting_value_json, category,
                setting_risk, validation_status, last_modified_by_id, last_modified_at)
           VALUES (?, ?, ?, 'general', 'low', 'valid', ?, ?)
           ON CONFLICT(setting_key, setting_scope)
           DO UPDATE SET
               setting_value_json = excluded.setting_value_json,
               last_modified_by_id = excluded.last_modified_by_id,
               last_modified_at = excluded.last_modified_at",
        [
            key.into(),
            scope.into(),
            value_json.into(),
            changed_by_id.map(i64::from).into(),
            now.clone().into(),
        ],
    ))
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO settings_change_events
               (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                changed_by_id, changed_at, required_step_up, apply_result)
           VALUES (?, ?, ?, ?, ?, ?, 0, 'applied')",
        [
            key.into(),
            change_summary.into(),
            old_hash.into(),
            new_hash.into(),
            changed_by_id.map(i64::from).into(),
            now.into(),
        ],
    ))
    .await?;

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

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO settings_change_events
               (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                changed_by_id, changed_at, required_step_up, apply_result)
           VALUES (?, ?, ?, NULL, ?, ?, 0, 'applied')",
        [
            key.into(),
            change_summary.into(),
            old_hash.into(),
            changed_by_id.map(i64::from).into(),
            now.into(),
        ],
    ))
    .await?;

    tracing::info!(
        setting_key = key,
        scope = scope,
        actor = ?changed_by_id,
        "setting deleted"
    );

    Ok(true)
}

pub async fn list_change_events(db: &DatabaseConnection, limit: i64) -> AppResult<Vec<SettingsChangeEvent>> {
    let safe_limit = limit.clamp(1, 500);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT
                   id,
                   setting_key_or_domain,
                   change_summary,
                   old_value_hash,
                   new_value_hash,
                   changed_by_id,
                   COALESCE(strftime('%Y-%m-%dT%H:%M:%SZ', changed_at), changed_at) AS changed_at,
                   required_step_up,
                   apply_result
               FROM settings_change_events
               ORDER BY changed_at DESC
               LIMIT ?",
            [safe_limit.into()],
        ))
        .await?;

    rows.into_iter().map(map_settings_change_event).collect()
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
        changed_at: row
            .try_get::<String>("", "changed_at")
            .map_err(|e| decode_err("changed_at", e))?,
        required_step_up,
        apply_result: row
            .try_get::<String>("", "apply_result")
            .map_err(|e| decode_err("apply_result", e))?,
    })
}

/// Validation outcome for a policy draft rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTestResult {
    pub rule_name: String,
    pub severity: String,
    pub message: String,
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
            "policy domain must not be empty".to_string()
        ]));
    }
    let _: serde_json::Value = serde_json::from_str(snapshot_json)
        .map_err(|_| AppError::ValidationFailed(vec!["snapshot_json is not valid JSON".to_string()]))?;

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

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"INSERT INTO settings_change_events
                   (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                    changed_by_id, changed_at, required_step_up, apply_result)
               VALUES (?, ?, ?, ?, ?, ?, 0, 'applied')",
            [
                domain.into(),
                format!("Policy draft updated for domain '{domain}'").into(),
                sha256_hex(&existing.snapshot_json).into(),
                sha256_hex(snapshot_json).into(),
                i64::from(actor_id).into(),
                now.into(),
            ],
        ))
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

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO settings_change_events
               (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                changed_by_id, changed_at, required_step_up, apply_result)
           VALUES (?, ?, NULL, ?, ?, ?, 0, 'applied')",
        [
            domain.into(),
            format!("Policy draft created for domain '{domain}'").into(),
            sha256_hex(snapshot_json).into(),
            i64::from(actor_id).into(),
            now.into(),
        ],
    ))
    .await?;

    get_draft_policy(db, domain)
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("draft missing after insert")))
}

/// Validate the current draft for a domain.
pub async fn test_policy_draft(db: &DatabaseConnection, domain: &str) -> AppResult<Vec<PolicyTestResult>> {
    let draft = get_draft_policy(db, domain)
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("no draft exists for domain '{domain}'")]))?;

    Ok(validate_policy_json(domain, &draft.snapshot_json))
}

fn validate_policy_json(domain: &str, snapshot_json: &str) -> Vec<PolicyTestResult> {
    let mut results = Vec::new();

    match domain {
        "session" => match serde_json::from_str::<SessionPolicy>(snapshot_json) {
            Err(e) => results.push(PolicyTestResult {
                rule_name: "schema".into(),
                severity: "fail".into(),
                message: format!("Session policy JSON is invalid: {e}"),
            }),
            Ok(p) => {
                results.push(range_rule("idle_timeout_minutes", p.idle_timeout_minutes, 1, 480));
                results.push(range_rule(
                    "absolute_session_minutes",
                    p.absolute_session_minutes,
                    15,
                    1440,
                ));
                results.push(range_rule("offline_grace_hours", p.offline_grace_hours, 0, 720));
                results.push(range_rule("step_up_window_minutes", p.step_up_window_minutes, 1, 120));
                results.push(range_rule("max_failed_attempts", p.max_failed_attempts, 1, 20));
                results.push(range_rule("lockout_minutes", p.lockout_minutes, 1, 1440));
                if p.idle_timeout_minutes > p.absolute_session_minutes {
                    results.push(PolicyTestResult {
                        rule_name: "idle_vs_absolute".into(),
                        severity: "fail".into(),
                        message: "Idle timeout must not exceed absolute session length".into(),
                    });
                } else {
                    results.push(PolicyTestResult {
                        rule_name: "idle_vs_absolute".into(),
                        severity: "pass".into(),
                        message: "Idle timeout is within absolute session length".into(),
                    });
                }
            }
        },
        "backup" => match serde_json::from_str::<BackupPolicy>(snapshot_json) {
            Err(e) => results.push(PolicyTestResult {
                rule_name: "schema".into(),
                severity: "fail".into(),
                message: format!("Backup policy JSON is invalid: {e}"),
            }),
            Ok(p) => {
                if p.schedule_cron.trim().is_empty() {
                    results.push(PolicyTestResult {
                        rule_name: "schedule_cron".into(),
                        severity: "fail".into(),
                        message: "Backup schedule cron must not be empty".into(),
                    });
                } else if let Err(e) = crate::backup::scheduler::parse_backup_schedule(&p.schedule_cron) {
                    results.push(PolicyTestResult {
                        rule_name: "schedule_cron".into(),
                        severity: "fail".into(),
                        message: e,
                    });
                } else {
                    results.push(PolicyTestResult {
                        rule_name: "schedule_cron".into(),
                        severity: "pass".into(),
                        message: "Backup schedule cron is valid".into(),
                    });
                }
                results.push(range_rule("retention_days", p.retention_days, 1, 3650));
                results.push(range_rule("compression_level", p.compression_level, 0, 9));
                results.push(PolicyTestResult {
                    rule_name: "include_photos".into(),
                    severity: "pass".into(),
                    message: format!("include_photos={}", p.include_photos),
                });
            }
        },
        "password" => results.push(PolicyTestResult {
            rule_name: "password_ssot".into(),
            severity: "warn".into(),
            message: "Password policy is managed via RBAC settings; use PasswordPolicyPanel".into(),
        }),
        other => results.push(PolicyTestResult {
            rule_name: "domain".into(),
            severity: "fail".into(),
            message: format!("Unsupported policy domain '{other}'"),
        }),
    }

    results
}

fn range_rule(name: &str, value: u32, min: u32, max: u32) -> PolicyTestResult {
    if value < min || value > max {
        PolicyTestResult {
            rule_name: name.into(),
            severity: "fail".into(),
            message: format!("{name}={value} is outside allowed range [{min}, {max}]"),
        }
    } else {
        PolicyTestResult {
            rule_name: name.into(),
            severity: "pass".into(),
            message: format!("{name}={value} is within [{min}, {max}]"),
        }
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
    let required_step_up = if is_security_domain(domain) { 1 } else { 0 };

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

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO settings_change_events
               (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                changed_by_id, changed_at, required_step_up, apply_result)
           VALUES (?, ?, NULL, ?, ?, ?, ?, 'applied')",
        [
            domain.into(),
            format!("Policy activated for domain '{domain}' (snapshot {snapshot_id})").into(),
            sha256_hex(&target.snapshot_json).into(),
            i64::from(actor_id).into(),
            now.into(),
            required_step_up.into(),
        ],
    ))
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

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO settings_change_events
               (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                changed_by_id, changed_at, required_step_up, apply_result)
           VALUES (?, ?, ?, NULL, ?, ?, 0, 'applied')",
        [
            domain.into(),
            format!("Policy draft discarded for domain '{domain}'").into(),
            sha256_hex(&draft.snapshot_json).into(),
            i64::from(actor_id).into(),
            now.into(),
        ],
    ))
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
mod tests {
    use super::*;
    use sea_orm::{DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    #[test]
    fn sha256_hex_is_stable() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[tokio::test]
    async fn load_session_policy_uses_defaults_when_missing() {
        let db = crate::db::test_db::connect_in_memory().await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        let policy = load_session_policy(&db).await;
        assert_eq!(policy, SessionPolicy::default());
    }

    #[tokio::test]
    async fn set_setting_writes_setting_and_audit_event() {
        let db = crate::db::test_db::connect_in_memory().await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

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
                "SELECT COUNT(*) AS cnt FROM settings_change_events;".to_string(),
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
    async fn set_setting_rejects_invalid_json() {
        let db = crate::db::test_db::connect_in_memory().await.expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

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
        let db = crate::db::test_db::connect_in_memory().await.expect("In-memory DB");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

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
        let db = crate::db::test_db::connect_in_memory().await.expect("In-memory DB");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

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
