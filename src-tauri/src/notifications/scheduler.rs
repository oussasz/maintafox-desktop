use sea_orm::{ConnectionTrait, DbBackend, Statement};
use tokio::time::{interval, Duration};

use crate::notifications::{delivery, router, Result, SqlitePool};

pub async fn start_notification_scheduler(pool: SqlitePool) {
    let mut ticker = interval(Duration::from_secs(60));

    loop {
        ticker.tick().await;
        if let Err(err) = run_scheduler_tick(&pool).await {
            tracing::error!(error = %err, "notifications::scheduler tick failed");
        }
    }
}

async fn run_scheduler_tick(pool: &SqlitePool) -> Result<()> {
    pool.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = 'delivered'
         WHERE delivery_state = 'snoozed'
           AND snoozed_until IS NOT NULL
           AND snoozed_until <= strftime('%Y-%m-%dT%H:%M:%SZ','now')"
            .to_string(),
    ))
    .await?;

    run_escalation_sweep(pool).await?;

    let expiry_days = fetch_expiry_days(pool).await.unwrap_or(180);
    let expiry_modifier = format!("-{expiry_days} days");
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = 'expired'
         WHERE delivery_state IN ('pending', 'delivered', 'read')
           AND created_at < strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', ?))",
        [expiry_modifier.into()],
    ))
    .await?;

    Ok(())
}

async fn run_escalation_sweep(pool: &SqlitePool) -> Result<()> {
    let rows = pool
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                n.id AS notification_id,
                n.escalation_level,
                n.title,
                n.body,
                n.action_url,
                e.id AS event_id,
                e.source_module,
                e.source_record_id,
                e.payload_json,
                s.level_no,
                s.route_to_type,
                s.route_to_reference
             FROM notifications n
             JOIN notification_events e ON e.id = n.notification_event_id
             JOIN notification_rules r ON r.category_code = e.category_code
             JOIN notification_escalation_steps s ON s.escalation_policy_id = r.escalation_policy_id
             WHERE r.is_active = 1
               AND r.requires_ack = 1
               AND n.delivery_state NOT IN ('acknowledged', 'closed', 'expired')
               AND s.level_no = (n.escalation_level + 1)
               AND CAST((julianday('now') - julianday(n.created_at)) * 24 * 60 AS INTEGER) >= s.wait_minutes"
                .to_string(),
        ))
        .await?;

    for row in rows {
        let notification_id = row.try_get::<i64>("", "notification_id")?;
        let event_id = row.try_get::<i64>("", "event_id")?;
        let level_no = row.try_get::<i64>("", "level_no")?;
        let title = row.try_get::<String>("", "title")?;
        let body = row.try_get::<Option<String>>("", "body")?;
        let action_url = row.try_get::<Option<String>>("", "action_url")?;
        let source_module = row.try_get::<String>("", "source_module")?;
        let source_record_id = row.try_get::<Option<String>>("", "source_record_id")?;
        let route_to_type = row.try_get::<String>("", "route_to_type")?;
        let route_to_reference = row.try_get::<Option<String>>("", "route_to_reference")?;
        let payload_json = row.try_get::<Option<String>>("", "payload_json")?;

        let mut payload = payload_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if !payload.is_object() {
            payload = serde_json::json!({});
        }
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("source_module".to_string(), serde_json::Value::String(source_module));
            obj.insert(
                "source_record_id".to_string(),
                source_record_id
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null),
            );

            if route_to_type == "role" {
                if let Some(reference) = route_to_reference.clone() {
                    obj.insert("routing_role_name".to_string(), serde_json::Value::String(reference));
                }
            } else if route_to_type == "team" {
                if let Some(reference) = route_to_reference.clone() {
                    obj.insert("team_id".to_string(), serde_json::Value::String(reference));
                }
            }
        }

        let mut recipients = Vec::new();
        if route_to_type == "user" {
            if let Some(reference) = route_to_reference.as_deref() {
                if let Ok(user_id) = reference.parse::<i64>() {
                    recipients.push(user_id);
                }
            }
        } else {
            let routing_mode = match route_to_type.as_str() {
                "role" => "role",
                "team" => "team",
                "entity_manager" => "entity_manager",
                _ => "manual",
            };
            let resolved = router::resolve_recipients(pool, routing_mode, &payload).await?;
            recipients.extend(resolved.recipient_user_ids);
            for role_id in resolved.recipient_role_ids {
                let role_users = pool
                    .query_all(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT DISTINCT user_id FROM user_scope_assignments WHERE role_id = ? AND deleted_at IS NULL",
                        [role_id.into()],
                    ))
                    .await?;
                for role_user in role_users {
                    if let Ok(user_id) = role_user.try_get::<i64>("", "user_id") {
                        recipients.push(user_id);
                    }
                }
            }
        }

        recipients.sort_unstable();
        recipients.dedup();

        for user_id in recipients {
            let insert_res = pool
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO notifications
                        (notification_event_id, recipient_user_id, delivery_state, title, body, action_url, created_at, escalation_level)
                     VALUES (?, ?, 'pending', ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)",
                    [
                        event_id.into(),
                        user_id.into(),
                        title.clone().into(),
                        body.clone().into(),
                        action_url.clone().into(),
                        level_no.into(),
                    ],
                ))
                .await?;
            let new_notification_id = insert_res.last_insert_id() as i64;
            if let Err(err) = delivery::schedule_in_app_delivery(pool, new_notification_id).await {
                tracing::error!(
                    notification_id = new_notification_id,
                    error = %err,
                    "notifications::scheduler escalation delivery scheduling failed"
                );
            }
        }

        pool.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notifications
             SET escalation_level = ?, delivery_state = 'escalated'
             WHERE id = ?",
            [level_no.into(), notification_id.into()],
        ))
        .await?;
    }

    Ok(())
}

async fn fetch_expiry_days(pool: &SqlitePool) -> Result<i64> {
    let row = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT setting_value_json
             FROM app_settings
             WHERE setting_key = 'notifications.expiry_days'
               AND setting_scope = 'tenant'
             LIMIT 1",
            [],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(180);
    };

    let raw_value = row.try_get::<String>("", "setting_value_json")?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw_value).unwrap_or_else(|_| serde_json::json!(180));

    let days = parsed
        .as_i64()
        .or_else(|| parsed.get("days").and_then(serde_json::Value::as_i64))
        .unwrap_or(180);

    Ok(days.clamp(1, 3650))
}
