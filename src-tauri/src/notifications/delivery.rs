//! Delivery of a new notification and what its recipient does with it (read, acknowledge,
//! snooze, archive). Every action checks the notification belongs to the person acting; the
//! inbox additionally checks the person may still view its module (`notifications::inbox`).

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::AppError;
use crate::notifications::Result;

/// Longest snooze: a week. A snoozed notification comes back unread in the inbox at its time.
pub const MAX_SNOOZE_DAYS: i64 = 7;

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

pub async fn schedule_in_app_delivery(pool: &impl ConnectionTrait, notification_id: i64) -> Result<()> {
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

/// In-app delivery of a new notification, plus its email when the recipient enabled email for
/// the category, never for the safety rule notice (`notifications::email`,
/// `catalog::ALWAYS_DELIVERED`; the OS toast is claimed by the signed-in recipient's window,
/// `notifications::os_toast`).
pub async fn schedule_deliveries(pool: &impl ConnectionTrait, notification_id: i64) -> Result<()> {
    schedule_in_app_delivery(pool, notification_id).await?;
    crate::notifications::email::enqueue_email(pool, notification_id).await?;
    Ok(())
}

pub async fn mark_as_read(pool: &impl ConnectionTrait, notification_id: i64, user_id: i64) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;

    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "UPDATE notifications
             SET delivery_state = CASE WHEN delivery_state IN ('pending', 'delivered', 'escalated', 'snoozed')
                                       THEN 'read' ELSE delivery_state END,
                 read_at = COALESCE(read_at, {NOW})
             WHERE id = ?"
        ),
        [notification_id.into()],
    ))
    .await?;

    Ok(())
}

/// Back to unread (the person wants to come back to it).
pub async fn mark_as_unread(pool: &impl ConnectionTrait, notification_id: i64, user_id: i64) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = CASE WHEN delivery_state = 'read' THEN 'delivered' ELSE delivery_state END,
             read_at = NULL
         WHERE id = ? AND acknowledged_at IS NULL",
        [notification_id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn acknowledge(
    pool: &impl ConnectionTrait,
    notification_id: i64,
    user_id: i64,
    note: Option<String>,
) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;
    record_acknowledgement(pool, notification_id, user_id, note).await
}

/// Acknowledges a notification on behalf of `acted_by` (its recipient, or an administrator).
pub(crate) async fn record_acknowledgement(
    pool: &impl ConnectionTrait,
    notification_id: i64,
    acted_by: i64,
    note: Option<String>,
) -> Result<()> {
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "UPDATE notifications
             SET delivery_state = 'acknowledged',
                 acknowledged_at = {NOW},
                 read_at = COALESCE(read_at, {NOW}),
                 snoozed_until = NULL
             WHERE id = ?"
        ),
        [notification_id.into()],
    ))
    .await?;

    let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO notification_acknowledgements
            (notification_id, acknowledged_by_id, acknowledged_at, acknowledgement_note)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?)",
        [notification_id.into(), acted_by.into(), note.into()],
    ))
    .await?;

    Ok(())
}

/// Snoozes for `snooze_minutes` from now (see [`snooze_until`]).
pub async fn snooze(
    pool: &impl ConnectionTrait,
    notification_id: i64,
    user_id: i64,
    snooze_minutes: i64,
) -> Result<()> {
    if snooze_minutes < 1 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_NOTIFICATION_SNOOZE_INVALID: the snooze must last at least one minute.".to_string(),
        ]));
    }
    snooze_until(
        pool,
        notification_id,
        user_id,
        crate::clock::now() + Duration::minutes(snooze_minutes),
    )
    .await
}

/// Hides the notification until `until` (in the future, at most [`MAX_SNOOZE_DAYS`] ahead); it then
/// comes back unread (`notifications::scheduler`).
pub async fn snooze_until(
    pool: &impl ConnectionTrait,
    notification_id: i64,
    user_id: i64,
    until: DateTime<Utc>,
) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;
    refuse_hiding_unacknowledged_notice(pool, notification_id).await?;
    let now = crate::clock::now();
    if until <= now || until > now + Duration::days(MAX_SNOOZE_DAYS) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_NOTIFICATION_SNOOZE_INVALID: a notification can be snoozed until a time within the next {MAX_SNOOZE_DAYS} days."
        )]));
    }
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notifications
         SET delivery_state = 'snoozed',
             snoozed_until = ?
         WHERE id = ? AND acknowledged_at IS NULL",
        [
            until.format("%Y-%m-%dT%H:%M:%SZ").to_string().into(),
            notification_id.into(),
        ],
    ))
    .await?;

    Ok(())
}

/// Archives (out of the inbox, kept in "Archived"), or restores to the inbox.
pub async fn set_archived(
    pool: &impl ConnectionTrait,
    notification_id: i64,
    user_id: i64,
    archived: bool,
) -> Result<()> {
    ensure_recipient(pool, notification_id, user_id).await?;
    if archived {
        refuse_hiding_unacknowledged_notice(pool, notification_id).await?;
    }
    let sql = if archived {
        format!(
            "UPDATE notifications SET archived_at = {NOW}, read_at = COALESCE(read_at, {NOW}), \
             delivery_state = CASE WHEN delivery_state IN ('pending', 'delivered', 'escalated', 'snoozed') \
                                   THEN 'read' ELSE delivery_state END, snoozed_until = NULL \
             WHERE id = ?"
        )
    } else {
        "UPDATE notifications SET archived_at = NULL WHERE id = ?".to_string()
    };
    pool.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [notification_id.into()],
    ))
    .await?;
    Ok(())
}

/// A safety rule notice is acknowledged before it is archived or snoozed away
/// (`settings::received_notice`).
async fn refuse_hiding_unacknowledged_notice(pool: &impl ConnectionTrait, notification_id: i64) -> Result<()> {
    if crate::settings::received_notice::is_unacknowledged_notice(pool, notification_id).await? {
        return Err(AppError::ValidationFailed(vec![
            "GATE_NOTIFICATION_ACK_REQUIRED: acknowledge this safety notice before hiding it.".to_string(),
        ]));
    }
    Ok(())
}

pub(crate) async fn ensure_recipient(pool: &impl ConnectionTrait, notification_id: i64, user_id: i64) -> Result<()> {
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

    let recipient_user_id = row.try_get::<Option<i64>>("", "recipient_user_id").ok().flatten();

    if recipient_user_id != Some(user_id) {
        return Err(AppError::PermissionDenied(
            "Notification does not belong to the current user".to_string(),
        ));
    }

    Ok(())
}
