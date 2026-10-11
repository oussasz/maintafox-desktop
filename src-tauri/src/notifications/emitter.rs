use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

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

pub async fn emit_event(pool: &impl ConnectionTrait, input: NotificationEventInput) -> Result<()> {
    // Keep "disconnected pool" behavior visible to callers.
    pool.query_one(Statement::from_string(DbBackend::Sqlite, "SELECT 1".to_string()))
        .await?;

    if let Err(err) = emit_event_inner(pool, input, None).await {
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
    let event_sync_id = identity.map(|basis| seeded_entity_sync_id("notification_events", basis));
    if let Err(err) = emit_event_inner(conn, input, event_sync_id).await {
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

/// Creates the notification of `user_id` for an event at an escalation level; `None` when it
/// already exists (received from another device of the recipient, or created earlier here).
pub(crate) async fn insert_notification(
    conn: &impl ConnectionTrait,
    event_id: i64,
    user_id: i64,
    title: &str,
    body: Option<&str>,
    action_url: Option<&str>,
    level: i64,
) -> Result<Option<i64>> {
    let sync_id = notification_sync_id(conn, event_id, user_id, level).await?;
    let inserted = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notifications
                (entity_sync_id, notification_event_id, recipient_user_id, delivery_state, title, body, action_url, created_at, escalation_level)
             VALUES (?, ?, ?, 'pending', ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)
             ON CONFLICT(entity_sync_id) DO NOTHING",
            [
                sync_id.clone().into(),
                event_id.into(),
                user_id.into(),
                title.into(),
                body.map(str::to_string).into(),
                action_url.map(str::to_string).into(),
                level.into(),
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

async fn emit_event_inner(
    pool: &impl ConnectionTrait,
    input: NotificationEventInput,
    event_sync_id: Option<String>,
) -> Result<()> {
    if let Some(dedupe_key) = input.dedupe_key.as_deref() {
        let existing = pool
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT n.id, n.delivery_state
                 FROM notifications n
                 JOIN notification_events e ON e.id = n.notification_event_id
                 WHERE e.dedupe_key = ?
                   AND n.delivery_state NOT IN ('closed', 'expired', 'acknowledged')
                 ORDER BY n.id DESC
                 LIMIT 1",
                [dedupe_key.into()],
            ))
            .await?;

        if let Some(row) = existing {
            let notification_id = row.try_get::<i64>("", "id")?;
            let state = row.try_get::<String>("", "delivery_state")?;

            pool.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE notifications
                 SET title = ?,
                     body = ?,
                     action_url = ?,
                     delivery_state = CASE WHEN delivery_state = 'read' THEN 'delivered' ELSE delivery_state END,
                     read_at = CASE WHEN delivery_state = 'read' THEN NULL ELSE read_at END
                 WHERE id = ?",
                [
                    input.title.clone().into(),
                    input.body.clone().into(),
                    input.action_url.clone().into(),
                    notification_id.into(),
                ],
            ))
            .await?;

            if state == "read" {
                let _ = delivery::schedule_in_app_delivery(pool, notification_id).await;
            }
            return Ok(());
        }

        // Event-level dedupe: freeze-breach (and similar) may emit with no recipients
        // when team scope resolves empty — still must not insert duplicate events.
        let existing_event = pool
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM notification_events WHERE dedupe_key = ? LIMIT 1",
                [dedupe_key.into()],
            ))
            .await?;
        if existing_event.is_some() {
            return Ok(());
        }
    }

    let event_sync_id = event_sync_id.unwrap_or_else(new_entity_sync_id);
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_events
            (entity_sync_id, source_module, source_record_id, event_code, category_code, severity, dedupe_key, payload_json)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(entity_sync_id) DO NOTHING",
        [
            event_sync_id.clone().into(),
            input.source_module.clone().into(),
            input.source_record_id.clone().into(),
            input.event_code.clone().into(),
            input.category_code.clone().into(),
            input.severity.clone().into(),
            input.dedupe_key.clone().into(),
            input.payload_json.clone().into(),
        ],
    ))
    .await?;
    let event_id = row_id_of(pool, "notification_events", &event_sync_id).await?;

    let rule = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT routing_mode
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
        return Ok(());
    };

    let routing_mode = rule_row
        .try_get::<String>("", "routing_mode")
        .unwrap_or_else(|_| "manual".to_string());
    let payload = compose_payload(&input);
    let routing = router::resolve_recipients(pool, &routing_mode, &payload).await?;

    let mut recipient_users = BTreeSet::new();
    for user_id in routing.recipient_user_ids {
        recipient_users.insert(user_id);
    }
    for role_id in routing.recipient_role_ids {
        let rows = pool
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT DISTINCT usa.user_id
                 FROM user_scope_assignments usa
                 JOIN user_accounts ua ON ua.id = usa.user_id
                 WHERE usa.role_id = ?
                   AND usa.deleted_at IS NULL
                   AND ua.is_active = 1
                   AND (usa.valid_to IS NULL OR usa.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                [role_id.into()],
            ))
            .await?;
        for row in rows {
            if let Ok(user_id) = row.try_get::<i64>("", "user_id") {
                recipient_users.insert(user_id);
            }
        }
    }

    for user_id in recipient_users {
        let Some(notification_id) = insert_notification(
            pool,
            event_id,
            user_id,
            &input.title,
            input.body.as_deref(),
            input.action_url.as_deref(),
            0,
        )
        .await?
        else {
            continue;
        };

        if let Err(err) = delivery::schedule_in_app_delivery(pool, notification_id).await {
            tracing::error!(
                notification_id,
                error = %err,
                "notifications::emit_event failed to schedule in-app delivery"
            );
        }
    }

    Ok(())
}

fn compose_payload(input: &NotificationEventInput) -> serde_json::Value {
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
    use sea_orm::{Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn emit_event_dedupe_updates_existing_notification() {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory db");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("apply migrations");

        let now = chrono::Utc::now().to_rfc3339();

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
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect in-memory db");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("apply migrations");

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
