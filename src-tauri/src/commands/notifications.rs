use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::rbac::{check_permission_cached, PermissionScope};
use crate::errors::{AppError, AppResult};
use crate::notifications::delivery;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[derive(Debug, Deserialize)]
pub struct NotificationFilterInput {
    pub delivery_state: Option<String>,
    pub category_code: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct NotificationSummary {
    pub id: i64,
    pub title: String,
    pub body: Option<String>,
    pub category_code: String,
    pub severity: String,
    pub delivery_state: String,
    pub created_at: String,
    pub read_at: Option<String>,
    pub acknowledged_at: Option<String>,
    pub action_url: Option<String>,
    pub escalation_level: i64,
    pub requires_ack: bool,
}

#[derive(Debug, Serialize)]
pub struct UserPreferenceRow {
    pub category_code: String,
    pub label: String,
    pub is_user_configurable: bool,
    pub in_app_enabled: bool,
    pub os_enabled: bool,
    pub email_enabled: bool,
    pub sms_enabled: bool,
    pub digest_mode: String,
    pub muted_until: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePreferenceInput {
    pub category_code: String,
    pub in_app_enabled: Option<bool>,
    pub os_enabled: Option<bool>,
    pub email_enabled: Option<bool>,
    pub sms_enabled: Option<bool>,
    pub digest_mode: Option<String>,
    pub muted_until: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NotificationRuleDetail {
    pub id: i64,
    pub category_code: String,
    pub category_label: String,
    pub routing_mode: String,
    pub requires_ack: bool,
    pub dedupe_window_minutes: i64,
    pub quiet_hours_policy_json: Option<String>,
    pub escalation_policy_id: Option<i64>,
    pub escalation_policy_name: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotificationRuleInput {
    pub rule_id: i64,
    pub routing_mode: Option<String>,
    pub requires_ack: Option<bool>,
    pub dedupe_window_minutes: Option<i64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct NotificationCategory {
    pub id: i64,
    pub code: String,
    pub label: String,
    pub default_severity: String,
    pub default_requires_ack: bool,
    pub is_user_configurable: bool,
}

#[tauri::command]
pub async fn list_notifications(
    filter: Option<NotificationFilterInput>,
    state: State<'_, AppState>,
) -> AppResult<Vec<NotificationSummary>> {
    let user = require_session!(state);

    let filter = filter.unwrap_or(NotificationFilterInput {
        delivery_state: None,
        category_code: None,
        limit: Some(50),
        offset: Some(0),
    });

    let limit = filter.limit.unwrap_or(50).clamp(1, 200);
    let offset = filter.offset.unwrap_or(0).max(0);

    let mut sql = String::from(
        "SELECT
            n.id,
            n.title,
            n.body,
            e.category_code,
            e.severity,
            n.delivery_state,
            n.created_at,
            n.read_at,
            n.acknowledged_at,
            n.action_url,
            n.escalation_level,
            COALESCE((
                SELECT MAX(r2.requires_ack)
                FROM notification_rules r2
                WHERE r2.category_code = e.category_code
                  AND r2.is_active = 1
            ), 0) AS requires_ack
         FROM notifications n
         JOIN notification_events e ON e.id = n.notification_event_id
         WHERE n.recipient_user_id = ?",
    );
    let mut params: Vec<sea_orm::Value> = vec![user.user_id.into()];

    if let Some(category_code) = filter.category_code {
        sql.push_str(" AND e.category_code = ?");
        params.push(category_code.into());
    }

    if let Some(delivery_state) = filter.delivery_state {
        if delivery_state == "unread" {
            sql.push_str(" AND n.delivery_state IN ('delivered', 'escalated')");
        } else {
            sql.push_str(" AND n.delivery_state = ?");
            params.push(delivery_state.into());
        }
    }

    sql.push_str(" ORDER BY n.created_at DESC LIMIT ? OFFSET ?");
    params.push(limit.into());
    params.push(offset.into());

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            params,
        ))
        .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(NotificationSummary {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            title: row.try_get::<String>("", "title").unwrap_or_default(),
            body: row.try_get::<Option<String>>("", "body").unwrap_or(None),
            category_code: row
                .try_get::<String>("", "category_code")
                .unwrap_or_default(),
            severity: row.try_get::<String>("", "severity").unwrap_or_default(),
            delivery_state: row
                .try_get::<String>("", "delivery_state")
                .unwrap_or_default(),
            created_at: row.try_get::<String>("", "created_at").unwrap_or_default(),
            read_at: row.try_get::<Option<String>>("", "read_at").unwrap_or(None),
            acknowledged_at: row
                .try_get::<Option<String>>("", "acknowledged_at")
                .unwrap_or(None),
            action_url: row.try_get::<Option<String>>("", "action_url").unwrap_or(None),
            escalation_level: row
                .try_get::<i64>("", "escalation_level")
                .unwrap_or_default(),
            requires_ack: row.try_get::<i32>("", "requires_ack").unwrap_or(0) == 1,
        });
    }

    Ok(items)
}

#[tauri::command]
pub async fn get_unread_count(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt
             FROM notifications
             WHERE recipient_user_id = ?
               AND delivery_state IN ('delivered', 'escalated')",
            [user.user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count query returned no row")))?;

    Ok(row.try_get::<i64>("", "cnt").unwrap_or(0))
}

#[tauri::command]
pub async fn mark_notification_read(
    notification_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    delivery::mark_as_read(&state.db, notification_id, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn acknowledge_notification(
    notification_id: i64,
    note: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    let user_id = i64::from(user.user_id);

    let recipient_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT recipient_user_id FROM notifications WHERE id = ?",
            [notification_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "notification".to_string(),
            id: notification_id.to_string(),
        })?;

    let recipient_user_id = recipient_row
        .try_get::<Option<i64>>("", "recipient_user_id")
        .unwrap_or(None);

    if recipient_user_id == Some(user_id) {
        return delivery::acknowledge(&state.db, notification_id, user_id, note).await;
    }

    let is_admin_users = check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        "adm.users",
        &PermissionScope::Global,
    )
    .await?;

    if !is_admin_users {
        return Err(AppError::PermissionDenied(
            "Permission requise : recipient owner or adm.users".to_string(),
        ));
    }

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notifications
             SET delivery_state = 'acknowledged',
                 acknowledged_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [notification_id.into()],
        ))
        .await?;

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_acknowledgements
                (notification_id, acknowledged_by_id, acknowledged_at, acknowledgement_note)
             VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)",
            [notification_id.into(), user_id.into(), note.into()],
        ))
        .await?;

    Ok(())
}

#[tauri::command]
pub async fn snooze_notification(
    notification_id: i64,
    snooze_minutes: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    if !(1..=480).contains(&snooze_minutes) {
        return Err(AppError::ValidationFailed(vec![
            "snooze_minutes must be within 1..=480".to_string(),
        ]));
    }
    delivery::snooze(
        &state.db,
        notification_id,
        i64::from(user.user_id),
        snooze_minutes,
    )
    .await
}

#[tauri::command]
pub async fn get_notification_preferences(state: State<'_, AppState>) -> AppResult<Vec<UserPreferenceRow>> {
    let user = require_session!(state);
    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                c.code AS category_code,
                c.label,
                c.is_user_configurable,
                COALESCE(p.in_app_enabled, 1) AS in_app_enabled,
                COALESCE(p.os_enabled, 1) AS os_enabled,
                COALESCE(p.email_enabled, 0) AS email_enabled,
                COALESCE(p.sms_enabled, 0) AS sms_enabled,
                COALESCE(p.digest_mode, 'instant') AS digest_mode,
                p.muted_until
             FROM notification_categories c
             LEFT JOIN notification_preferences p
                ON p.category_code = c.code
               AND p.user_id = ?
             ORDER BY c.code",
            [user.user_id.into()],
        ))
        .await?;

    let mut prefs = Vec::with_capacity(rows.len());
    for row in rows {
        prefs.push(UserPreferenceRow {
            category_code: row
                .try_get::<String>("", "category_code")
                .unwrap_or_default(),
            label: row.try_get::<String>("", "label").unwrap_or_default(),
            is_user_configurable: row
                .try_get::<i32>("", "is_user_configurable")
                .unwrap_or(1)
                == 1,
            in_app_enabled: row.try_get::<i32>("", "in_app_enabled").unwrap_or(1) == 1,
            os_enabled: row.try_get::<i32>("", "os_enabled").unwrap_or(1) == 1,
            email_enabled: row.try_get::<i32>("", "email_enabled").unwrap_or(0) == 1,
            sms_enabled: row.try_get::<i32>("", "sms_enabled").unwrap_or(0) == 1,
            digest_mode: row.try_get::<String>("", "digest_mode").unwrap_or_default(),
            muted_until: row
                .try_get::<Option<String>>("", "muted_until")
                .unwrap_or(None),
        });
    }

    Ok(prefs)
}

#[tauri::command]
pub async fn update_notification_preference(
    payload: UpdatePreferenceInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);

    let category = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_user_configurable FROM notification_categories WHERE code = ?",
            [payload.category_code.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "notification_category".to_string(),
            id: payload.category_code.clone(),
        })?;

    let is_user_configurable = category
        .try_get::<i32>("", "is_user_configurable")
        .unwrap_or(0)
        == 1;
    if !is_user_configurable {
        return Err(AppError::PermissionDenied(
            "System-managed category cannot be customized".to_string(),
        ));
    }

    if let Some(mode) = payload.digest_mode.as_deref() {
        if !matches!(mode, "instant" | "daily_digest" | "off") {
            return Err(AppError::ValidationFailed(vec![
                "digest_mode must be one of: instant, daily_digest, off".to_string(),
            ]));
        }
    }

    let existing = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT in_app_enabled, os_enabled, email_enabled, sms_enabled, digest_mode, muted_until
             FROM notification_preferences
             WHERE user_id = ? AND category_code = ?",
            [user.user_id.into(), payload.category_code.clone().into()],
        ))
        .await?;

    let current_in_app = existing
        .as_ref()
        .and_then(|r| r.try_get::<i32>("", "in_app_enabled").ok())
        .unwrap_or(1);
    let current_os = existing
        .as_ref()
        .and_then(|r| r.try_get::<i32>("", "os_enabled").ok())
        .unwrap_or(1);
    let current_email = existing
        .as_ref()
        .and_then(|r| r.try_get::<i32>("", "email_enabled").ok())
        .unwrap_or(0);
    let current_sms = existing
        .as_ref()
        .and_then(|r| r.try_get::<i32>("", "sms_enabled").ok())
        .unwrap_or(0);
    let current_digest = existing
        .as_ref()
        .and_then(|r| r.try_get::<String>("", "digest_mode").ok())
        .unwrap_or_else(|| "instant".to_string());
    let current_muted_until = existing
        .as_ref()
        .and_then(|r| r.try_get::<Option<String>>("", "muted_until").ok())
        .unwrap_or(None);

    let in_app_enabled = payload
        .in_app_enabled
        .map(|v| if v { 1 } else { 0 })
        .unwrap_or(current_in_app);
    let os_enabled = payload
        .os_enabled
        .map(|v| if v { 1 } else { 0 })
        .unwrap_or(current_os);
    let email_enabled = payload
        .email_enabled
        .map(|v| if v { 1 } else { 0 })
        .unwrap_or(current_email);
    let sms_enabled = payload
        .sms_enabled
        .map(|v| if v { 1 } else { 0 })
        .unwrap_or(current_sms);
    let digest_mode = payload.digest_mode.unwrap_or(current_digest);
    let muted_until = if let Some(v) = payload.muted_until {
        if v.trim().is_empty() {
            None
        } else {
            Some(v)
        }
    } else {
        current_muted_until
    };

    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_preferences
                (user_id, category_code, in_app_enabled, os_enabled, email_enabled, sms_enabled, digest_mode, muted_until)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, category_code)
             DO UPDATE SET
                in_app_enabled = excluded.in_app_enabled,
                os_enabled = excluded.os_enabled,
                email_enabled = excluded.email_enabled,
                sms_enabled = excluded.sms_enabled,
                digest_mode = excluded.digest_mode,
                muted_until = excluded.muted_until",
            [
                user.user_id.into(),
                payload.category_code.into(),
                in_app_enabled.into(),
                os_enabled.into(),
                email_enabled.into(),
                sms_enabled.into(),
                digest_mode.into(),
                muted_until.into(),
            ],
        ))
        .await?;

    Ok(())
}

#[tauri::command]
pub async fn list_notification_rules(state: State<'_, AppState>) -> AppResult<Vec<NotificationRuleDetail>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);

    let rows = state
        .db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                r.id,
                r.category_code,
                c.label AS category_label,
                r.routing_mode,
                r.requires_ack,
                r.dedupe_window_minutes,
                r.quiet_hours_policy_json,
                r.escalation_policy_id,
                p.name AS escalation_policy_name,
                r.is_active
             FROM notification_rules r
             JOIN notification_categories c ON c.code = r.category_code
             LEFT JOIN notification_escalation_policies p ON p.id = r.escalation_policy_id
             ORDER BY r.category_code"
                .to_string(),
        ))
        .await?;

    let mut rules = Vec::with_capacity(rows.len());
    for row in rows {
        rules.push(NotificationRuleDetail {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            category_code: row
                .try_get::<String>("", "category_code")
                .unwrap_or_default(),
            category_label: row
                .try_get::<String>("", "category_label")
                .unwrap_or_default(),
            routing_mode: row
                .try_get::<String>("", "routing_mode")
                .unwrap_or_default(),
            requires_ack: row.try_get::<i32>("", "requires_ack").unwrap_or(0) == 1,
            dedupe_window_minutes: row
                .try_get::<i64>("", "dedupe_window_minutes")
                .unwrap_or(60),
            quiet_hours_policy_json: row
                .try_get::<Option<String>>("", "quiet_hours_policy_json")
                .unwrap_or(None),
            escalation_policy_id: row
                .try_get::<Option<i64>>("", "escalation_policy_id")
                .unwrap_or(None),
            escalation_policy_name: row
                .try_get::<Option<String>>("", "escalation_policy_name")
                .unwrap_or(None),
            is_active: row.try_get::<i32>("", "is_active").unwrap_or(1) == 1,
        });
    }

    Ok(rules)
}

#[tauri::command]
pub async fn update_notification_rule(
    payload: UpdateNotificationRuleInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);

    if let Some(ref routing_mode) = payload.routing_mode {
        if !matches!(
            routing_mode.as_str(),
            "assignee" | "reviewer" | "role" | "team" | "entity_manager" | "watcher" | "manual"
        ) {
            return Err(AppError::ValidationFailed(vec![
                "routing_mode must be one of: assignee/reviewer/role/team/entity_manager/watcher/manual"
                    .to_string(),
            ]));
        }
    }

    if let Some(v) = payload.dedupe_window_minutes {
        if !(1..=10080).contains(&v) {
            return Err(AppError::ValidationFailed(vec![
                "dedupe_window_minutes must be within 1..=10080".to_string(),
            ]));
        }
    }

    let mut set_parts: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(routing_mode) = payload.routing_mode {
        set_parts.push("routing_mode = ?".to_string());
        values.push(routing_mode.into());
    }
    if let Some(requires_ack) = payload.requires_ack {
        set_parts.push("requires_ack = ?".to_string());
        values.push((if requires_ack { 1 } else { 0 }).into());
    }
    if let Some(dedupe) = payload.dedupe_window_minutes {
        set_parts.push("dedupe_window_minutes = ?".to_string());
        values.push(dedupe.into());
    }
    if let Some(is_active) = payload.is_active {
        set_parts.push("is_active = ?".to_string());
        values.push((if is_active { 1 } else { 0 }).into());
    }

    if set_parts.is_empty() {
        return Ok(());
    }

    values.push(payload.rule_id.into());
    let sql = format!(
        "UPDATE notification_rules SET {} WHERE id = ?",
        set_parts.join(", ")
    );
    let update_res = state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            values,
        ))
        .await?;

    if update_res.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "notification_rule".to_string(),
            id: payload.rule_id.to_string(),
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn list_notification_categories(
    state: State<'_, AppState>,
) -> AppResult<Vec<NotificationCategory>> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);

    let rows = state
        .db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, label, default_severity, default_requires_ack, is_user_configurable
             FROM notification_categories
             ORDER BY code"
                .to_string(),
        ))
        .await?;

    let mut categories = Vec::with_capacity(rows.len());
    for row in rows {
        categories.push(NotificationCategory {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            code: row.try_get::<String>("", "code").unwrap_or_default(),
            label: row.try_get::<String>("", "label").unwrap_or_default(),
            default_severity: row
                .try_get::<String>("", "default_severity")
                .unwrap_or_default(),
            default_requires_ack: row
                .try_get::<i32>("", "default_requires_ack")
                .unwrap_or(0)
                == 1,
            is_user_configurable: row
                .try_get::<i32>("", "is_user_configurable")
                .unwrap_or(1)
                == 1,
        });
    }

    Ok(categories)
}
