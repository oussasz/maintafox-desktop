use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::notifications::{delivery, router, Result, SqlitePool};

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

pub async fn emit_event(pool: &SqlitePool, input: NotificationEventInput) -> Result<()> {
    // Keep "disconnected pool" behavior visible to callers.
    pool.query_one(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT 1".to_string(),
    ))
    .await?;

    if let Err(err) = emit_event_inner(pool, input).await {
        tracing::error!(error = %err, "notifications::emit_event fire-and-log failure");
    }

    Ok(())
}

async fn emit_event_inner(pool: &SqlitePool, input: NotificationEventInput) -> Result<()> {
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
    }

    let event_insert = pool
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_events
                (source_module, source_record_id, event_code, category_code, severity, dedupe_key, payload_json)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
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
    let event_id = event_insert.last_insert_id() as i64;

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
        let notif_insert = pool
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO notifications
                    (notification_event_id, recipient_user_id, delivery_state, title, body, action_url, created_at, escalation_level)
                 VALUES (?, ?, 'pending', ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), 0)",
                [
                    event_id.into(),
                    user_id.into(),
                    input.title.clone().into(),
                    input.body.clone().into(),
                    input.action_url.clone().into(),
                ],
            ))
            .await?;
        let notification_id = notif_insert.last_insert_id() as i64;

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
                (sync_id, username, display_name, is_active, is_admin, force_password_change, created_at, updated_at)
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
                (code, type_id, status_id, primary_responsible_id, title, created_at, updated_at)
             VALUES (?, 1, 1, ?, ?, ?, ?)",
            [
                "WO-NOTIF-001".into(),
                user_id.into(),
                "Original WO".into(),
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
