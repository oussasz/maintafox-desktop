//! Per-site rules (`site_settings`, migration 227).
//!
//! A site is a planning site: an organisation node whose type can own work
//! (`planning::weekly_schedule::planning_site_of_node`). A rule holds for a site only once someone
//! set it there; without a row the rule's strict default applies (`SiteSettingDefinition::default`).
//! Every change is audited in `site_setting_events` — who, when, old → new and the reason — and is
//! made under step-up with a reason of at least [`super::catalog::MIN_REASON_CHARS`] characters.

use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::sync::identity::{new_entity_sync_id, optional_entity_sync_id, seeded_entity_sync_id};

/// A rule a site may set.
#[derive(Debug, Clone, Copy)]
pub struct SiteSettingDefinition {
    pub key: &'static str,
    /// Applies where the site has no row (the strict choice).
    pub default: bool,
}

/// Every per-site rule. Values are JSON booleans.
pub const SITE_SETTINGS: &[SiteSettingDefinition] = &[SiteSettingDefinition {
    key: crate::reliability::study::decisions::SELF_APPROVAL_SETTING,
    default: false,
}];

pub fn site_definition(key: &str) -> Option<&'static SiteSettingDefinition> {
    SITE_SETTINGS.iter().find(|d| d.key == key)
}

fn invalid(code: &str) -> AppError {
    AppError::ValidationFailed(vec![code.to_string()])
}

fn decode_err(field: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("site_settings.{field}: {e}"))
}

fn get<T: sea_orm::TryGetable>(r: &QueryResult, col: &str) -> AppResult<T> {
    r.try_get("", col).map_err(|e| decode_err(col, e))
}

/// The value of `key` at `site` (`None`: never set there).
pub async fn site_setting<C: ConnectionTrait>(db: &C, site_node_id: i64, key: &str) -> AppResult<Option<bool>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value_json FROM site_settings WHERE site_node_id = ? AND setting_key = ?",
            [site_node_id.into(), key.into()],
        ))
        .await?;
    row.map(|r| {
        let v: String = get(&r, "value_json")?;
        serde_json::from_str::<bool>(v.trim()).map_err(|e| decode_err("value_json", e))
    })
    .transpose()
}

/// A rule at one planning site, for the Settings screen.
#[derive(Debug, Clone, Serialize)]
pub struct SiteSettingView {
    pub site_node_id: i64,
    pub site_code: String,
    pub site_name: String,
    /// `None`: never set at this site (the default applies).
    pub value: Option<bool>,
    pub default_value: bool,
    pub updated_by_name: Option<String>,
    pub updated_at: Option<String>,
    pub row_version: Option<i64>,
}

/// `key` at every planning site (active nodes whose type can own work), by site name.
pub async fn list_site_settings(db: &DatabaseConnection, key: &str) -> AppResult<Vec<SiteSettingView>> {
    let def = site_definition(key).ok_or_else(|| invalid("SITE_SETTING_UNKNOWN"))?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.id AS site_node_id, n.code AS site_code, n.name AS site_name, s.value_json, \
                    COALESCE(u.display_name, u.username) AS updated_by_name, s.updated_at, s.row_version \
             FROM org_nodes n \
             JOIN org_node_types t ON t.id = n.node_type_id AND t.can_own_work = 1 \
             LEFT JOIN site_settings s ON s.site_node_id = n.id AND s.setting_key = ? \
             LEFT JOIN user_accounts u ON u.id = s.updated_by_id \
             WHERE n.deleted_at IS NULL \
             ORDER BY n.name COLLATE NOCASE, n.id",
            [key.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            let value_json: Option<String> = get(r, "value_json")?;
            Ok(SiteSettingView {
                site_node_id: get(r, "site_node_id")?,
                site_code: get(r, "site_code")?,
                site_name: get(r, "site_name")?,
                value: value_json
                    .map(|v| serde_json::from_str::<bool>(v.trim()).map_err(|e| decode_err("value_json", e)))
                    .transpose()?,
                default_value: def.default,
                updated_by_name: get(r, "updated_by_name")?,
                updated_at: get(r, "updated_at")?,
                row_version: get(r, "row_version")?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetSiteSettingInput {
    pub site_node_id: i64,
    pub key: String,
    pub value: bool,
    /// Why (audited); at least `MIN_REASON_CHARS` characters.
    pub reason: String,
    /// The row version read (`None`: the site had no row).
    pub expected_row_version: Option<i64>,
}

/// Sets `key` at a planning site and appends its audit event, in one transaction.
pub async fn set_site_setting(
    db: &DatabaseConnection,
    input: &SetSiteSettingInput,
    actor_id: i32,
    required_step_up: bool,
) -> AppResult<()> {
    site_definition(&input.key).ok_or_else(|| invalid("SITE_SETTING_UNKNOWN"))?;
    let reason = input.reason.trim();
    if reason.chars().count() < super::catalog::MIN_REASON_CHARS {
        return Err(invalid("SETTING_REASON_REQUIRED"));
    }
    let tx = db.begin().await?;
    let is_site = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM org_nodes n JOIN org_node_types t ON t.id = n.node_type_id \
              WHERE n.id = ? AND n.deleted_at IS NULL AND t.can_own_work = 1",
            [input.site_node_id.into()],
        ))
        .await?
        .is_some();
    if !is_site {
        return Err(invalid("SITE_SETTING_NOT_A_SITE"));
    }
    let current = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT value_json, row_version FROM site_settings WHERE site_node_id = ? AND setting_key = ?",
            [input.site_node_id.into(), input.key.clone().into()],
        ))
        .await?;
    let (old_value, current_version) = match &current {
        Some(r) => (
            Some(get::<String>(r, "value_json")?),
            Some(get::<i64>(r, "row_version")?),
        ),
        None => (None, None),
    };
    if current_version != input.expected_row_version {
        return Err(invalid("SITE_SETTING_STALE"));
    }
    let new_value = input.value.to_string();
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    if current.is_some() {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE site_settings SET value_json = ?, updated_by_id = ?, updated_at = ?, row_version = row_version + 1 \
             WHERE site_node_id = ? AND setting_key = ?",
            [
                new_value.clone().into(),
                actor_id.into(),
                now.clone().into(),
                input.site_node_id.into(),
                input.key.clone().into(),
            ],
        ))
        .await?;
    } else {
        // One identity per site and rule on every device (two devices setting it meet on one row).
        let sync_id = match optional_entity_sync_id(&tx, "org_nodes", input.site_node_id).await? {
            Some(site) => seeded_entity_sync_id("site_settings", &format!("{site}|{}", input.key)),
            None => new_entity_sync_id(),
        };
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO site_settings (entity_sync_id, site_node_id, setting_key, value_json, updated_by_id, \
             updated_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                sync_id.into(),
                input.site_node_id.into(),
                input.key.clone().into(),
                new_value.clone().into(),
                actor_id.into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO site_setting_events (entity_sync_id, site_node_id, setting_key, old_value_json, \
         new_value_json, reason, changed_by_id, required_step_up, changed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            new_entity_sync_id().into(),
            input.site_node_id.into(),
            input.key.clone().into(),
            old_value.into(),
            new_value.into(),
            reason.to_string().into(),
            actor_id.into(),
            i32::from(required_step_up).into(),
            now.into(),
        ],
    ))
    .await?;
    tx.commit().await?;
    Ok(())
}

/// One audited change of a site rule.
#[derive(Debug, Clone, Serialize)]
pub struct SiteSettingEvent {
    pub id: i64,
    pub old_value: Option<bool>,
    pub new_value: bool,
    pub reason: String,
    pub changed_by_name: Option<String>,
    pub required_step_up: bool,
    pub changed_at: String,
}

/// The changes of `key` at a site, newest first.
pub async fn list_site_setting_events(
    db: &DatabaseConnection,
    site_node_id: i64,
    key: &str,
    limit: u64,
) -> AppResult<Vec<SiteSettingEvent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, e.old_value_json, e.new_value_json, e.reason, e.required_step_up, e.changed_at, \
                    COALESCE(u.display_name, u.username) AS changed_by_name \
             FROM site_setting_events e LEFT JOIN user_accounts u ON u.id = e.changed_by_id \
             WHERE e.site_node_id = ? AND e.setting_key = ? ORDER BY e.changed_at DESC, e.id DESC LIMIT ?",
            [site_node_id.into(), key.into(), limit.min(200).into()],
        ))
        .await?;
    let flag = |v: Option<String>| -> AppResult<Option<bool>> {
        v.map(|s| serde_json::from_str::<bool>(s.trim()).map_err(|e| decode_err("value_json", e)))
            .transpose()
    };
    rows.iter()
        .map(|r| {
            Ok(SiteSettingEvent {
                id: get(r, "id")?,
                old_value: flag(get(r, "old_value_json")?)?,
                new_value: flag(Some(get(r, "new_value_json")?))?.unwrap_or(false),
                reason: get(r, "reason")?,
                changed_by_name: get(r, "changed_by_name")?,
                required_step_up: get::<i64>(r, "required_step_up")? != 0,
                changed_at: get(r, "changed_at")?,
            })
        })
        .collect()
}
