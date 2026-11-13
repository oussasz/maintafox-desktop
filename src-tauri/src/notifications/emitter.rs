//! Raising a notification event and creating the notifications of its recipients.
//!
//! An event names the record it is about (`source_module`, `source_record_id`) and its category;
//! the category's rule lists who receives it (`notifications::router`). Each recipient gets one
//! notification, with the audience that matched (why they got it). A recipient who turned the
//! category off (preference `in_app_enabled = 0` on a category users may configure) gets none.
//!
//! Text: an event may carry a message key and display parameters (`NotificationMessage`); the
//! inbox renders them in the reader's language. `title` / `body` are kept as written for channels
//! without a language (email) and for events without a key.
//!
//! De-duplication: an event with a `dedupe_key` that is still open refreshes its notifications
//! instead of creating others; once the condition cleared (`resolved_at`, set by
//! `notifications::conditions`), the same key raises a new event. Device-generated events use a
//! seeded identity, so the same event and notifications are created once per device and share
//! their ids everywhere.

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::notifications::catalog::{
    always_delivered_audiences, always_delivered_fallback, normalize_severity, rule_audiences,
};
use crate::notifications::{delivery, router, Result};
use crate::sync::identity::{new_entity_sync_id, optional_entity_sync_id, seeded_entity_sync_id};

#[derive(Debug, Clone)]
pub struct NotificationEventInput {
    pub source_module: String,
    pub source_record_id: Option<String>,
    pub event_code: String,
    pub category_code: String,
    pub severity: String,
    pub dedupe_key: Option<String>,
    pub payload_json: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub action_url: Option<String>,
}

/// Text of a notification in the reader's language: message key (`notifications:messages.<key>`)
/// and display parameters (codes, titles, ISO dates — never local ids), plus the level of the
/// shared urgency scale when the notification is about work.
#[derive(Debug, Clone, Default)]
pub struct NotificationMessage {
    pub key: String,
    pub params: serde_json::Value,
    pub urgency_code: Option<String>,
}

impl NotificationMessage {
    pub fn new(key: &str, params: serde_json::Value) -> Self {
        Self {
            key: key.to_string(),
            params,
            urgency_code: None,
        }
    }

    #[must_use]
    pub fn with_urgency(mut self, urgency: Option<String>) -> Self {
        self.urgency_code = urgency;
        self
    }
}

pub async fn emit_event(pool: &impl ConnectionTrait, input: NotificationEventInput) -> Result<()> {
    emit_event_with(pool, input, None).await
}

/// [`emit_event`] with a localisable message.
pub async fn emit_event_with(
    pool: &impl ConnectionTrait,
    input: NotificationEventInput,
    message: Option<&NotificationMessage>,
) -> Result<()> {
    // Keep "disconnected pool" behavior visible to callers.
    pool.query_one(Statement::from_string(DbBackend::Sqlite, "SELECT 1".to_string()))
        .await?;

    if let Err(err) = emit(pool, &input, None, message, Arrival::New).await {
        tracing::error!(error = %err, "notifications::emit_event fire-and-log failure");
    }

    Ok(())
}

/// An event that every device generates for itself from synchronised data (the PM scheduler's due
/// occurrences, the DI SLA poller). The caller runs it with sync capture suppressed, so it is not
/// sent; `identity` (built from sync ids) gives the event, and therefore its notifications, the
/// same id on every device, so what a recipient does with a notification on one device (read,
/// acknowledge, snooze) applies to the same notification on the others.
pub async fn emit_device_event(
    conn: &impl ConnectionTrait,
    input: NotificationEventInput,
    identity: Option<&str>,
) -> Result<()> {
    emit_device_event_with(conn, input, identity, None).await
}

/// [`emit_device_event`] with a localisable message.
pub async fn emit_device_event_with(
    conn: &impl ConnectionTrait,
    input: NotificationEventInput,
    identity: Option<&str>,
    message: Option<&NotificationMessage>,
) -> Result<()> {
    let event_sync_id = identity.map(|basis| seeded_entity_sync_id("notification_events", basis));
    if let Err(err) = emit(conn, &input, event_sync_id, message, Arrival::New).await {
        tracing::error!(error = %err, "notifications::emit_device_event fire-and-log failure");
    }
    Ok(())
}

/// Identity of a notification: the same event, recipient and escalation level give the same id on
/// every device. Rows without a sync identity (capture not installed) get a random one.
async fn notification_sync_id(conn: &impl ConnectionTrait, event_id: i64, user_id: i64, level: i64) -> Result<String> {
    let event = optional_entity_sync_id(conn, "notification_events", event_id).await?;
    let user = optional_entity_sync_id(conn, "user_accounts", user_id).await?;
    Ok(match (event, user) {
        (Some(event), Some(user)) => seeded_entity_sync_id("notifications", &format!("{event}|{user}|{level}")),
        _ => new_entity_sync_id(),
    })
}

/// Why a recipient received a notification (`notifications.reason_code` / `reason_ref`).
#[derive(Debug, Clone, Copy)]
pub struct Reason<'a> {
    pub code: &'a str,
    pub reference: Option<&'a str>,
}

/// Creates the notification of `user_id` for an event at an escalation level, with why the person
/// receives it; `None` when it already exists (received from another device of the recipient, or
/// created earlier here).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_notification_for(
    conn: &impl ConnectionTrait,
    event_id: i64,
    user_id: i64,
    title: &str,
    body: Option<&str>,
    action_url: Option<&str>,
    level: i64,
    reason: Option<Reason<'_>>,
) -> Result<Option<i64>> {
    let sync_id = notification_sync_id(conn, event_id, user_id, level).await?;
    let inserted = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notifications
                (entity_sync_id, notification_event_id, recipient_user_id, delivery_state, title, body, action_url,
                 created_at, escalation_level, reason_code, reason_ref)
             VALUES (?, ?, ?, 'pending', ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?, ?, ?)
             ON CONFLICT(entity_sync_id) DO NOTHING",
            [
                sync_id.clone().into(),
                event_id.into(),
                user_id.into(),
                title.into(),
                body.map(str::to_string).into(),
                action_url.map(str::to_string).into(),
                level.into(),
                reason.map(|r| r.code.to_string()).into(),
                reason.and_then(|r| r.reference.map(str::to_string)).into(),
            ],
        ))
        .await?;
    if inserted.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(Some(row_id_of(conn, "notifications", &sync_id).await?))
}

async fn row_id_of(conn: &impl ConnectionTrait, table: &str, sync_id: &str) -> Result<i64> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("SELECT id FROM {table} WHERE entity_sync_id = ?"),
        [sync_id.into()],
    ))
    .await?
    .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("{table} row {sync_id} missing after insert")))?
    .try_get::<i64>("", "id")
    .map_err(Into::into)
}

/// The recipient turned the category off (only categories users may configure).
pub(crate) async fn unsubscribed(conn: &impl ConnectionTrait, user_id: i64, category: &str) -> Result<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM notification_preferences p \
             JOIN notification_categories c ON c.code = p.category_code \
             WHERE p.user_id = ? AND p.category_code = ? AND p.in_app_enabled = 0 AND c.is_user_configurable = 1",
            [user_id.into(), category.into()],
        ))
        .await?
        .is_some())
}

/// How the notifications of a new event reach their recipients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrival {
    /// Delivered: unread, toasted and emailed per the recipient's preferences.
    New,
    /// The condition was already true when this device started watching it: the notification is
    /// listed as read (it does not ring the bell, it is not toasted or emailed) and says so; one
    /// that must be acknowledged still waits for its acknowledgement.
    AlreadyTrue,
}

/// Raises the event and notifies its recipients. Returns the event id, or `None` when an open
/// event with the same de-duplication key was refreshed instead. An event with a seeded identity
/// that exists already but was resolved (the same occurrence came back) is reopened.
pub(crate) async fn emit(
    pool: &impl ConnectionTrait,
    input: &NotificationEventInput,
    event_sync_id: Option<String>,
    message: Option<&NotificationMessage>,
    arrival: Arrival,
) -> Result<Option<i64>> {
    let message_params = message.map(|m| m.params.to_string());
    if let Some(dedupe_key) = input.dedupe_key.as_deref() {
        let existing = pool
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM notification_events WHERE dedupe_key = ? AND resolved_at IS NULL \
                 ORDER BY id DESC LIMIT 1",
                [dedupe_key.into()],
            ))
            .await?;
        if let Some(row) = existing {
            let event_id = row.try_get::<i64>("", "id")?;
            refresh_event(pool, event_id, input, message, message_params.as_deref()).await?;
            return Ok(None);
        }
    }

    // The record's routing context also gives the event its org node (read-time visibility).
    let payload = compose_payload(input);
    let ctx = router::load_context(pool, &payload).await?;

    let event_sync_id = event_sync_id.unwrap_or_else(new_entity_sync_id);
    let inserted = pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_events
            (entity_sync_id, source_module, source_record_id, event_code, category_code, severity, dedupe_key,
             payload_json, message_key, message_params_json, urgency_code, org_node_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(entity_sync_id) DO NOTHING",
            [
                event_sync_id.clone().into(),
                input.source_module.clone().into(),
                input.source_record_id.clone().into(),
                input.event_code.clone().into(),
                input.category_code.clone().into(),
                normalize_severity(&input.severity).into(),
                input.dedupe_key.clone().into(),
                input.payload_json.clone().into(),
                message.map(|m| m.key.clone()).into(),
                message_params.clone().into(),
                message.and_then(|m| m.urgency_code.clone()).into(),
                ctx.org_node_id.into(),
            ],
        ))
        .await?;
    let event_id = row_id_of(pool, "notification_events", &event_sync_id).await?;
    if inserted.rows_affected() == 0 {
        reopen_event(pool, event_id, input, message, message_params.as_deref(), arrival).await?;
    }

    // A category that cannot be muted or re-routed goes to its fixed audience; its rule row and the
    // recipients' preferences are not read (`catalog::ALWAYS_DELIVERED`).
    let fixed_audiences = always_delivered_audiences(&input.category_code);
    let fixed = !fixed_audiences.is_empty();
    let audiences = if fixed {
        fixed_audiences
    } else {
        let rule = pool
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT routing_mode, audiences_json
             FROM notification_rules
             WHERE category_code = ?
               AND is_active = 1
             LIMIT 1",
                [input.category_code.clone().into()],
            ))
            .await?;

        let Some(rule_row) = rule else {
            tracing::warn!(
                category = input.category_code,
                "notifications::emit_event no active routing rule found"
            );
            return Ok(Some(event_id));
        };

        let routing_mode = rule_row
            .try_get::<Option<String>>("", "routing_mode")?
            .unwrap_or_else(|| "manual".to_string());
        let audiences_json = rule_row.try_get::<Option<String>>("", "audiences_json")?;
        rule_audiences(audiences_json.as_deref(), &routing_mode)
    };
    let mut recipients = router::resolve_audiences(pool, &audiences, &ctx).await?;
    // A safety notice whose fixed audiences reach nobody here goes to its fallback
    // (`catalog::ALWAYS_DELIVERED_FALLBACK`).
    if fixed && recipients.is_empty() {
        let fallback = always_delivered_fallback(&input.category_code);
        if !fallback.is_empty() {
            recipients = router::resolve_audiences(pool, &fallback, &ctx).await?;
        }
    }

    for recipient in recipients {
        if !fixed && unsubscribed(pool, recipient.user_id, &input.category_code).await? {
            continue;
        }
        let Some(notification_id) = insert_notification_for(
            pool,
            event_id,
            recipient.user_id,
            &input.title,
            input.body.as_deref(),
            input.action_url.as_deref(),
            0,
            Some(Reason {
                code: &recipient.reason,
                reference: recipient.reason_ref.as_deref(),
            }),
        )
        .await?
        else {
            continue;
        };

        match arrival {
            Arrival::New => {
                if let Err(err) = delivery::schedule_deliveries(pool, notification_id).await {
                    tracing::error!(
                        notification_id,
                        error = %err,
                        "notifications::emit_event failed to schedule in-app delivery"
                    );
                }
            }
            Arrival::AlreadyTrue => {
                pool.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE notifications SET delivery_state = 'read', \
                     read_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), already_true = 1 WHERE id = ?",
                    [notification_id.into()],
                ))
                .await?;
            }
        }
    }

    Ok(Some(event_id))
}

/// The same occurrence (same seeded identity) is true again after it was resolved: the event and
/// its notifications are open again, unread when it is new to this device's watch, and its message
/// says what is true now (for example when the location went below zero this time).
async fn reopen_event(
    pool: &impl ConnectionTrait,
    event_id: i64,
    input: &NotificationEventInput,
    message: Option<&NotificationMessage>,
    message_params: Option<&str>,
    arrival: Arrival,
) -> Result<()> {
    let reopened = pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notification_events SET resolved_at = NULL, dedupe_key = COALESCE(?, dedupe_key) \
             WHERE id = ? AND resolved_at IS NOT NULL",
            [input.dedupe_key.clone().into(), event_id.into()],
        ))
        .await?;
    if reopened.rows_affected() == 0 {
        return Ok(());
    }
    if let Some(m) = message {
        pool.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notification_events SET message_key = ?, message_params_json = ?, urgency_code = ? WHERE id = ?",
            [
                m.key.clone().into(),
                message_params.map(str::to_string).into(),
                m.urgency_code.clone().into(),
                event_id.into(),
            ],
        ))
        .await?;
    }
    let unread = arrival == Arrival::New;
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications SET resolved_at = NULL, \
            delivery_state = CASE WHEN ? = 1 AND delivery_state = 'read' AND archived_at IS NULL \
                                  THEN 'delivered' ELSE delivery_state END, \
            read_at = CASE WHEN ? = 1 AND delivery_state = 'read' AND archived_at IS NULL \
                           THEN NULL ELSE read_at END \
         WHERE notification_event_id = ?",
        [i64::from(unread).into(), i64::from(unread).into(), event_id.into()],
    ))
    .await?;
    Ok(())
}

/// The condition of an open event happened again: its texts are refreshed and the notifications
/// already read become unread (and are delivered again).
async fn refresh_event(
    pool: &impl ConnectionTrait,
    event_id: i64,
    input: &NotificationEventInput,
    message: Option<&NotificationMessage>,
    message_params: Option<&str>,
) -> Result<()> {
    if let Some(m) = message {
        pool.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notification_events SET message_key = ?, message_params_json = ?, urgency_code = ?, \
             severity = ? WHERE id = ?",
            [
                m.key.clone().into(),
                message_params.map(str::to_string).into(),
                m.urgency_code.clone().into(),
                normalize_severity(&input.severity).into(),
                event_id.into(),
            ],
        ))
        .await?;
    }
    let read_ids: Vec<i64> = pool
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM notifications WHERE notification_event_id = ? AND delivery_state = 'read' \
             AND archived_at IS NULL AND resolved_at IS NULL",
            [event_id.into()],
        ))
        .await?
        .iter()
        .filter_map(|r| r.try_get::<i64>("", "id").ok())
        .collect();
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET title = ?,
             body = ?,
             action_url = ?,
             delivery_state = CASE WHEN delivery_state = 'read' AND archived_at IS NULL THEN 'delivered'
                                   ELSE delivery_state END,
             read_at = CASE WHEN delivery_state = 'read' AND archived_at IS NULL THEN NULL ELSE read_at END
         WHERE notification_event_id = ?
           AND resolved_at IS NULL
           AND delivery_state NOT IN ('closed', 'expired', 'acknowledged')",
        [
            input.title.clone().into(),
            input.body.clone().into(),
            input.action_url.clone().into(),
            event_id.into(),
        ],
    ))
    .await?;
    for id in read_ids {
        if let Err(err) = delivery::schedule_deliveries(pool, id).await {
            tracing::error!(notification_id = id, error = %err, "notifications: redelivery failed");
        }
    }
    Ok(())
}

pub(crate) fn compose_payload(input: &NotificationEventInput) -> serde_json::Value {
    let mut payload = input
        .payload_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if !payload.is_object() {
        payload = serde_json::json!({});
    }

    if let Some(obj) = payload.as_object_mut() {
        obj.entry("source_module".to_string())
            .or_insert_with(|| serde_json::Value::String(input.source_module.clone()));
        obj.entry("source_record_id".to_string()).or_insert_with(|| {
            input
                .source_record_id
                .clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null)
        });
    }

    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{DbBackend, Statement};

    #[tokio::test]
    async fn emit_event_dedupe_updates_existing_notification() {
        let db = crate::db::test_db::migrated().await;

        let now = crate::clock::now().to_rfc3339();

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_accounts
                (entity_sync_id, username, display_name, is_active, is_admin, force_password_change, created_at, updated_at)
             VALUES (?, ?, ?, 1, 0, 0, ?, ?)",
            [
                uuid::Uuid::new_v4().to_string().into(),
                "notif-user".into(),
                "Notif User".into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await
        .expect("insert user");

        let user_id = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts WHERE username = 'notif-user'".to_string(),
            ))
            .await
            .expect("query user")
            .expect("user row")
            .try_get::<i64>("", "id")
            .expect("user id");

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO work_orders
                (code, type_id, status_id, primary_responsible_id, title, entity_sync_id, created_at, updated_at)
             VALUES (?, 1, 1, ?, ?, ?, ?, ?)",
            [
                "WO-NOTIF-001".into(),
                user_id.into(),
                "Original WO".into(),
                crate::sync::identity::new_entity_sync_id().into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await
        .expect("insert work order");

        let wo_id = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM work_orders WHERE code = 'WO-NOTIF-001'".to_string(),
            ))
            .await
            .expect("query work order")
            .expect("work order row")
            .try_get::<i64>("", "id")
            .expect("wo id");

        emit_event(
            &db,
            NotificationEventInput {
                source_module: "work_orders".to_string(),
                source_record_id: Some(wo_id.to_string()),
                event_code: "wo_assigned".to_string(),
                category_code: "wo_assigned".to_string(),
                severity: "info".to_string(),
                dedupe_key: Some("wo-assigned-001".to_string()),
                payload_json: None,
                title: "First title".to_string(),
                body: Some("First body".to_string()),
                action_url: Some("/wo/1".to_string()),
            },
        )
        .await
        .expect("first emit");

        emit_event(
            &db,
            NotificationEventInput {
                source_module: "work_orders".to_string(),
                source_record_id: Some(wo_id.to_string()),
                event_code: "wo_assigned".to_string(),
                category_code: "wo_assigned".to_string(),
                severity: "info".to_string(),
                dedupe_key: Some("wo-assigned-001".to_string()),
                payload_json: None,
                title: "Updated title".to_string(),
                body: Some("Updated body".to_string()),
                action_url: Some("/wo/1".to_string()),
            },
        )
        .await
        .expect("second emit");

        let count = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM notifications".to_string(),
            ))
            .await
            .expect("count query")
            .expect("count row")
            .try_get::<i64>("", "cnt")
            .expect("count");
        assert_eq!(count, 1, "dedupe must keep a single active notification");

        let title = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT title FROM notifications LIMIT 1".to_string(),
            ))
            .await
            .expect("title query")
            .expect("title row")
            .try_get::<String>("", "title")
            .expect("title");
        assert_eq!(title, "Updated title");
    }

    #[tokio::test]
    async fn emit_event_fire_and_log_returns_ok_on_routing_failure() {
        let db = crate::db::test_db::migrated().await;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "DROP TABLE work_orders".to_string(),
        ))
        .await
        .expect("drop work_orders");

        let result = emit_event(
            &db,
            NotificationEventInput {
                source_module: "work_orders".to_string(),
                source_record_id: Some("1".to_string()),
                event_code: "wo_assigned".to_string(),
                category_code: "wo_assigned".to_string(),
                severity: "info".to_string(),
                dedupe_key: None,
                payload_json: None,
                title: "Routing failure test".to_string(),
                body: Some("Should not bubble up".to_string()),
                action_url: None,
            },
        )
        .await;

        assert!(
            result.is_ok(),
            "emit_event must swallow routing failures and return Ok(())"
        );
    }
}
