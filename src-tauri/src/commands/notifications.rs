//! Notification IPC: the signed-in person's inbox and preferences, the OS toast and email
//! channels, and the plant's notification settings (Données de référence → Notifications).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::rbac::{check_permission_cached, PermissionScope};
use crate::errors::{AppError, AppResult};
use crate::notifications::{config, delivery, inbox};
use crate::state::AppState;
use crate::{require_permission, require_session};

async fn visible_modules(state: &AppState, user_id: i32) -> AppResult<inbox::Visibility> {
    inbox::visible_to(&state.db, user_id).await
}

/// One page of the inbox (see `notifications::inbox` for the views and filters).
#[tauri::command]
pub async fn list_notifications(
    filter: Option<inbox::InboxFilter>,
    state: State<'_, AppState>,
) -> AppResult<Vec<inbox::InboxItem>> {
    let user = require_session!(state);
    let modules = visible_modules(&state, user.user_id).await?;
    inbox::list(
        &state.db,
        i64::from(user.user_id),
        &modules,
        &filter.unwrap_or_default(),
    )
    .await
}

/// Unread count of the bell (same condition as the inbox's unread view).
#[tauri::command]
pub async fn get_unread_count(state: State<'_, AppState>) -> AppResult<i64> {
    let user = require_session!(state);
    let modules = visible_modules(&state, user.user_id).await?;
    inbox::unread_count(&state.db, i64::from(user.user_id), &modules).await
}

#[tauri::command]
pub async fn get_notification_counts(state: State<'_, AppState>) -> AppResult<inbox::InboxCounts> {
    let user = require_session!(state);
    let modules = visible_modules(&state, user.user_id).await?;
    inbox::counts(&state.db, i64::from(user.user_id), &modules).await
}

async fn visible(state: &AppState, user_id: i32, notification_id: i64) -> AppResult<()> {
    let modules = visible_modules(state, user_id).await?;
    inbox::ensure_visible(&state.db, i64::from(user_id), &modules, notification_id).await
}

#[tauri::command]
pub async fn mark_notification_read(notification_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    visible(&state, user.user_id, notification_id).await?;
    delivery::mark_as_read(&state.db, notification_id, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn mark_notification_unread(notification_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    visible(&state, user.user_id, notification_id).await?;
    delivery::mark_as_unread(&state.db, notification_id, i64::from(user.user_id)).await
}

/// Marks read every unread notification matching the filter; returns how many.
#[tauri::command]
pub async fn mark_all_notifications_read(
    filter: Option<inbox::InboxFilter>,
    state: State<'_, AppState>,
) -> AppResult<u64> {
    let user = require_session!(state);
    let modules = visible_modules(&state, user.user_id).await?;
    inbox::mark_all_read(
        &state.db,
        i64::from(user.user_id),
        &modules,
        &filter.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub async fn acknowledge_notification(
    notification_id: i64,
    note: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    let is_admin_users = check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::ADM_USERS,
        &PermissionScope::Global,
    )
    .await?;
    acknowledge_notification_in(&state.db, user.user_id, notification_id, note, is_admin_users).await
}

/// [`acknowledge_notification`] once the session and the `adm.users` right are known. A safety rule
/// notice goes to its recipient-only, audited path (`settings::received_notice`), whatever the
/// caller's rights.
pub(crate) async fn acknowledge_notification_in(
    db: &sea_orm::DatabaseConnection,
    session_user_id: i32,
    notification_id: i64,
    note: Option<String>,
    is_admin_users: bool,
) -> AppResult<()> {
    let user_id = i64::from(session_user_id);
    let recipient_row = db
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

    // A safety rule notice: its recipient alone acknowledges it, audited.
    if crate::settings::received_notice::is_notice(db, notification_id).await? {
        return crate::settings::received_notice::acknowledge_notice(db, user_id, notification_id).await;
    }
    if recipient_user_id == Some(user_id) {
        let modules = inbox::visible_to(db, session_user_id).await?;
        inbox::ensure_visible(db, user_id, &modules, notification_id).await?;
        return delivery::acknowledge(db, notification_id, user_id, note).await;
    }

    if !is_admin_users {
        return Err(AppError::PermissionDenied(
            "Permission requise : recipient owner or adm.users".to_string(),
        ));
    }

    delivery::record_acknowledgement(db, notification_id, user_id, note).await
}

/// The signed-in administrator's safety rule notices still to acknowledge (shown after each
/// sign-in until acknowledged, `settings::received_notice`).
#[tauri::command]
pub async fn get_safety_rule_notices(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::settings::received_notice::SafetyRuleNotice>> {
    let user = require_session!(state);
    crate::settings::received_notice::notices_for(&state.db, i64::from(user.user_id)).await
}

/// Acknowledges one of the signed-in administrator's safety rule notices (audited).
#[tauri::command]
pub async fn acknowledge_safety_rule_notice(notification_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    crate::settings::received_notice::acknowledge_notice(&state.db, i64::from(user.user_id), notification_id).await
}

/// Snoozes until `snooze_until` (RFC 3339, within `delivery::MAX_SNOOZE_DAYS`).
#[tauri::command]
pub async fn snooze_notification(
    notification_id: i64,
    snooze_until: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    let until = chrono::DateTime::parse_from_rfc3339(snooze_until.trim())
        .map_err(|_| {
            AppError::ValidationFailed(vec![
                "GATE_NOTIFICATION_SNOOZE_INVALID: snooze_until must be an RFC 3339 time.".to_string(),
            ])
        })?
        .with_timezone(&chrono::Utc);
    visible(&state, user.user_id, notification_id).await?;
    delivery::snooze_until(&state.db, notification_id, i64::from(user.user_id), until).await
}

#[tauri::command]
pub async fn archive_notification(notification_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    visible(&state, user.user_id, notification_id).await?;
    delivery::set_archived(&state.db, notification_id, i64::from(user.user_id), true).await
}

#[tauri::command]
pub async fn unarchive_notification(notification_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    visible(&state, user.user_id, notification_id).await?;
    delivery::set_archived(&state.db, notification_id, i64::from(user.user_id), false).await
}

#[derive(Debug, Serialize)]
pub struct UserPreferenceRow {
    pub category_code: String,
    pub module_code: String,
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

// ─── Plant settings (Données de référence → Notifications) ─────────────────────

/// Rules, escalation steps and retention.
#[tauri::command]
pub async fn get_notification_config(state: State<'_, AppState>) -> AppResult<config::NotificationConfig> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_VIEW,
        PermissionScope::Global
    );
    config::load(&state.db).await
}

#[tauri::command]
pub async fn update_notification_rule(
    payload: config::UpdateRuleInput,
    state: State<'_, AppState>,
) -> AppResult<config::NotificationConfig> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    config::update_rule(&state.db, &payload).await?;
    config::load(&state.db).await
}

#[tauri::command]
pub async fn update_notification_escalation_step(
    step_id: i64,
    payload: config::EscalationStepInput,
    state: State<'_, AppState>,
) -> AppResult<config::NotificationConfig> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    config::update_step(&state.db, step_id, &payload).await?;
    config::load(&state.db).await
}

#[tauri::command]
pub async fn add_notification_escalation_step(
    policy_id: i64,
    payload: config::EscalationStepInput,
    state: State<'_, AppState>,
) -> AppResult<config::NotificationConfig> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    config::add_step(&state.db, policy_id, &payload).await?;
    config::load(&state.db).await
}

#[tauri::command]
pub async fn remove_notification_escalation_step(
    step_id: i64,
    state: State<'_, AppState>,
) -> AppResult<config::NotificationConfig> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    config::remove_step(&state.db, step_id).await?;
    config::load(&state.db).await
}

/// Sets how long notifications are kept (tenant setting `notifications.expiry_days`).
#[tauri::command]
pub async fn set_notification_retention(
    expiry_days: i64,
    state: State<'_, AppState>,
) -> AppResult<config::NotificationConfig> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    config::validate_expiry_days(expiry_days)?;
    crate::settings::set_setting(
        &state.db,
        crate::notifications::scheduler::EXPIRY_SETTING_KEY,
        "tenant",
        &expiry_days.to_string(),
        Some(user.user_id),
        "Notification retention changed",
    )
    .await?;
    config::load(&state.db).await
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
                COALESCE(c.module_code, '') AS module_code,
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
             WHERE c.is_offered = 1
             ORDER BY c.module_code, c.code",
            [user.user_id.into()],
        ))
        .await?;

    let mut prefs = Vec::with_capacity(rows.len());
    for row in rows {
        prefs.push(UserPreferenceRow {
            category_code: row.try_get::<String>("", "category_code").unwrap_or_default(),
            module_code: row.try_get::<String>("", "module_code").unwrap_or_default(),
            label: row.try_get::<String>("", "label").unwrap_or_default(),
            is_user_configurable: row.try_get::<i32>("", "is_user_configurable").unwrap_or(1) == 1,
            in_app_enabled: row.try_get::<i32>("", "in_app_enabled").unwrap_or(1) == 1,
            os_enabled: row.try_get::<i32>("", "os_enabled").unwrap_or(1) == 1,
            email_enabled: row.try_get::<i32>("", "email_enabled").unwrap_or(0) == 1,
            sms_enabled: row.try_get::<i32>("", "sms_enabled").unwrap_or(0) == 1,
            digest_mode: row.try_get::<String>("", "digest_mode").unwrap_or_default(),
            muted_until: row.try_get::<Option<String>>("", "muted_until").unwrap_or(None),
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
            "SELECT is_user_configurable FROM notification_categories WHERE code = ? AND is_offered = 1",
            [payload.category_code.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "notification_category".to_string(),
            id: payload.category_code.clone(),
        })?;

    let is_user_configurable = category.try_get::<i32>("", "is_user_configurable").unwrap_or(0) == 1;
    if !is_user_configurable {
        return Err(AppError::PermissionDenied(
            "System-managed category cannot be customized".to_string(),
        ));
    }

    if let Some(mode) = payload.digest_mode.as_deref() {
        crate::notifications::digest::validate_digest_mode(&payload.category_code, mode)?;
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
    let os_enabled = payload.os_enabled.map(|v| if v { 1 } else { 0 }).unwrap_or(current_os);
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

    let sync_id = crate::sync::identity::user_keyed_entity_sync_id(
        &state.db,
        "notification_preferences",
        i64::from(user.user_id),
        &payload.category_code,
    )
    .await?;
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_preferences
                (entity_sync_id, user_id, category_code, in_app_enabled, os_enabled, email_enabled, sms_enabled, digest_mode, muted_until)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id, category_code)
             DO UPDATE SET
                in_app_enabled = excluded.in_app_enabled,
                os_enabled = excluded.os_enabled,
                email_enabled = excluded.email_enabled,
                sms_enabled = excluded.sms_enabled,
                digest_mode = excluded.digest_mode,
                muted_until = excluded.muted_until",
            [
                sync_id.into(),
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

/// Notifications to toast for the signed-in user (see `notifications::os_toast`). Without
/// `since_id` (window just opened): the watermark to pass on the next calls, nothing to show.
#[tauri::command]
pub async fn list_pending_os_toasts(
    since_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<crate::notifications::os_toast::OsToastBatch> {
    let user = require_session!(state);
    crate::notifications::os_toast::pending_os_toasts(&state.db, i64::from(user.user_id), since_id).await
}

#[derive(Debug, Deserialize)]
pub struct ShowOsToastInput {
    pub notification_ids: Vec<i64>,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct ShowOsToastResult {
    pub shown: bool,
    pub error: Option<String>,
}

/// Shows one OS toast covering `notification_ids` (worded by the window) and records the outcome
/// for each of them.
#[tauri::command]
pub async fn show_os_toast(
    input: ShowOsToastInput,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ShowOsToastResult> {
    use tauri_plugin_notification::NotificationExt;

    let user = require_session!(state);
    crate::notifications::os_toast::ensure_owned(&state.db, i64::from(user.user_id), &input.notification_ids).await?;
    if input.title.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_CMD_NOTIF_TITLE_REQUIRED: title is required.".into(),
        ]));
    }
    let outcome = app
        .notification()
        .builder()
        .title(input.title.trim())
        .body(input.body.trim())
        .show()
        .map_err(|e| e.to_string());
    crate::notifications::os_toast::record_os_toasts(&state.db, &input.notification_ids, &outcome).await?;
    Ok(ShowOsToastResult {
        shown: outcome.is_ok(),
        error: outcome.err(),
    })
}

#[derive(Debug, Serialize)]
pub struct NotificationChannelStatus {
    pub email: crate::notifications::email::ChannelAvailability,
}

/// Whether the email channel can be used on this device (`notifications::email`).
#[tauri::command]
pub async fn get_notification_channel_status(state: State<'_, AppState>) -> AppResult<NotificationChannelStatus> {
    let _user = require_session!(state);
    Ok(NotificationChannelStatus {
        email: crate::notifications::email::email_channel_status(&state.db).await?,
    })
}
