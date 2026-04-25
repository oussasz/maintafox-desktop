use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::AppError;
use crate::notifications::{Result, SqlitePool};

pub async fn schedule_in_app_delivery(pool: &SqlitePool, notification_id: i64) -> Result<()> {
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_deliveries
            (notification_id, channel, attempt_no, delivery_status, attempted_at, delivered_at)
         VALUES (?, 'in_app', 1, 'queued', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [notification_id.into()],
    ))
    .await?;

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = 'delivered'
         WHERE id = ?",
        [notification_id.into()],
    ))
    .await?;

    Ok(())
}

pub async fn mark_as_read(pool: &SqlitePool, notification_id: i64, user_id: i64) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = 'read',
             read_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [notification_id.into()],
    ))
    .await?;

    Ok(())
}

pub async fn acknowledge(
    pool: &SqlitePool,
    notification_id: i64,
    user_id: i64,
    note: Option<String>,
) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = 'acknowledged',
             acknowledged_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [notification_id.into()],
    ))
    .await?;

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_acknowledgements
            (notification_id, acknowledged_by_id, acknowledged_at, acknowledgement_note)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)",
        [notification_id.into(), user_id.into(), note.into()],
    ))
    .await?;

    Ok(())
}

pub async fn snooze(
    pool: &SqlitePool,
    notification_id: i64,
    user_id: i64,
    snooze_minutes: i64,
) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;

    if !(1..=480).contains(&snooze_minutes) {
        return Err(AppError::ValidationFailed(vec![
            "snooze_minutes must be within 1..=480".to_string(),
        ]));
    }

    let modifier = format!("+{snooze_minutes} minutes");
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = 'snoozed',
             snoozed_until = strftime('%Y-%m-%dT%H:%M:%SZ', datetime('now', ?))
         WHERE id = ?",
        [modifier.into(), notification_id.into()],
    ))
    .await?;

    Ok(())
}

async fn ensure_recipient(pool: &SqlitePool, notification_id: i64, user_id: i64) -> Result<()> {
    let row = pool
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT recipient_user_id FROM notifications WHERE id = ?",
            [notification_id.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Err(AppError::NotFound {
            entity: "notification".to_string(),
            id: notification_id.to_string(),
        });
    };

    let recipient_user_id = row
        .try_get::<Option<i64>>("", "recipient_user_id")
        .ok()
        .flatten();

    if recipient_user_id != Some(user_id) {
        return Err(AppError::PermissionDenied(
            "Notification does not belong to the current user".to_string(),
        ));
    }

    Ok(())
}
