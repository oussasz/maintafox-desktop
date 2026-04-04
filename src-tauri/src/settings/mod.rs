//! Settings service.
//!
//! Provides typed access to `app_settings`, `policy_snapshots`, and
//! `settings_change_events`. All writes through this module emit an audit event.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

pub async fn get_setting(
    db: &DatabaseConnection,
    key: &str,
    scope: &str,
) -> AppResult<Option<AppSetting>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT
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
               WHERE setting_key = ? AND setting_scope = ?"#,
            [key.into(), scope.into()],
        ))
        .await?;

    row.map(map_app_setting).transpose()
}

pub async fn list_settings_by_category(
    db: &DatabaseConnection,
    category: &str,
) -> AppResult<Vec<AppSetting>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT
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
               ORDER BY setting_key"#,
            [category.into()],
        ))
        .await?;

    rows.into_iter().map(map_app_setting).collect()
}

pub async fn get_active_policy(
    db: &DatabaseConnection,
    domain: &str,
) -> AppResult<Option<PolicySnapshot>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT
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
               LIMIT 1"#,
            [domain.into()],
        ))
        .await?;

    row.map(map_policy_snapshot).transpose()
}

/// Load the session policy at startup.
/// Falls back to safe defaults whenever the DB policy is missing or malformed.
pub async fn load_session_policy(db: &DatabaseConnection) -> SessionPolicy {
    match get_active_policy(db, "session").await {
        Ok(Some(snap)) => serde_json::from_str::<SessionPolicy>(&snap.snapshot_json)
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "session policy snapshot is malformed, using defaults: {}",
                    e
                );
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
    changed_by_id: i32,
    change_summary: &str,
) -> AppResult<()> {
    if value_json.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "setting_value_json for '{}' must not be empty",
            key
        )]));
    }

    let _: serde_json::Value = serde_json::from_str(value_json).map_err(|_| {
        AppError::ValidationFailed(vec![format!(
            "setting_value_json for '{}' is not valid JSON",
            key
        )])
    })?;

    let old_hash = get_setting(db, key, scope)
        .await?
        .map(|s| sha256_hex(&s.setting_value_json));

    let new_hash = sha256_hex(value_json);
    let now = chrono::Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT INTO app_settings
               (setting_key, setting_scope, setting_value_json, category,
                setting_risk, validation_status, last_modified_by_id, last_modified_at)
           VALUES (?, ?, ?, 'general', 'low', 'valid', ?, ?)
           ON CONFLICT(setting_key, setting_scope)
           DO UPDATE SET
               setting_value_json = excluded.setting_value_json,
               last_modified_by_id = excluded.last_modified_by_id,
               last_modified_at = excluded.last_modified_at"#,
        [
            key.into(),
            scope.into(),
            value_json.into(),
            i64::from(changed_by_id).into(),
            now.clone().into(),
        ],
    ))
    .await?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r#"INSERT INTO settings_change_events
               (setting_key_or_domain, change_summary, old_value_hash, new_value_hash,
                changed_by_id, changed_at, required_step_up, apply_result)
           VALUES (?, ?, ?, ?, ?, ?, 0, 'applied')"#,
        [
            key.into(),
            change_summary.into(),
            old_hash.into(),
            new_hash.into(),
            i64::from(changed_by_id).into(),
            now.into(),
        ],
    ))
    .await?;

    tracing::info!(
        setting_key = key,
        scope = scope,
        actor = changed_by_id,
        "setting updated"
    );

    Ok(())
}

pub async fn list_change_events(
    db: &DatabaseConnection,
    limit: i64,
) -> AppResult<Vec<SettingsChangeEvent>> {
    let safe_limit = limit.clamp(1, 500);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"SELECT
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
               LIMIT ?"#,
            [safe_limit.into()],
        ))
        .await?;

    rows.into_iter().map(map_settings_change_event).collect()
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "settings row decode failed for column '{}': {}",
        column,
        e
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DbBackend, Statement};
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
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        let policy = load_session_policy(&db).await;
        assert_eq!(policy, SessionPolicy::default());
    }

    #[tokio::test]
    async fn set_setting_writes_setting_and_audit_event() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        set_setting(
            &db,
            "appearance.color_mode",
            "tenant",
            r#""dark""#,
            1,
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
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("In-memory DB");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("Migrations should apply");

        let err = set_setting(
            &db,
            "appearance.color_mode",
            "tenant",
            "not-json",
            1,
            "Invalid update",
        )
        .await
        .expect_err("invalid JSON should be rejected");

        assert!(matches!(err, AppError::ValidationFailed(_)));
    }
}
