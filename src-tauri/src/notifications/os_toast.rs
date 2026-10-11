//! OS toast channel of notifications (Phase 6.7).
//!
//! The signed-in user's window asks for the notifications that arrived on this device since it
//! opened (`since_id`: the highest notification id when the session started, so a sign-in does not
//! replay the whole inbox) and are not toasted yet, for categories where the user left the OS
//! channel on and did not mute. The window words the toast (one notification, or a localized
//! summary), the backend shows it through the OS (`tauri-plugin-notification`, no webview
//! permission) and records one `notification_deliveries` row per notification (`channel = 'os'`,
//! `delivered` or `failed` with the reason). Each device toasts its own copy once.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};

/// Notifications offered per call (the window shows a summary for several).
pub const MAX_TOASTS_PER_CALL: i64 = 20;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OsToastCandidate {
    pub notification_id: i64,
    pub title: String,
    pub body: Option<String>,
    pub severity: String,
    pub category_code: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OsToastBatch {
    /// Pass back as `since_id` on the next call of this session.
    pub watermark: i64,
    pub items: Vec<OsToastCandidate>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("os toast decode '{field}': {err}"))
}

/// Without `since_id` (session start): the current watermark and nothing to show.
pub async fn pending_os_toasts(
    conn: &impl ConnectionTrait,
    user_id: i64,
    since_id: Option<i64>,
) -> AppResult<OsToastBatch> {
    let Some(since_id) = since_id else {
        let watermark = conn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COALESCE(MAX(id), 0) AS m FROM notifications".to_string(),
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "m"))
            .transpose()
            .map_err(|e| decode_err("m", e))?
            .unwrap_or(0);
        return Ok(OsToastBatch {
            watermark,
            items: Vec::new(),
        });
    };
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT n.id, n.title, n.body, e.severity, e.category_code FROM notifications n \
             JOIN notification_events e ON e.id = n.notification_event_id \
             LEFT JOIN notification_preferences p \
                    ON p.user_id = n.recipient_user_id AND p.category_code = e.category_code \
             WHERE n.recipient_user_id = ? AND n.id > ? \
               AND n.delivery_state IN ('pending', 'delivered') \
               AND COALESCE(p.os_enabled, 1) = 1 \
               AND (p.muted_until IS NULL OR p.muted_until <= strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
               AND NOT EXISTS (SELECT 1 FROM notification_deliveries d \
                               WHERE d.notification_id = n.id AND d.channel = 'os') \
             ORDER BY n.id LIMIT ?",
            [user_id.into(), since_id.into(), MAX_TOASTS_PER_CALL.into()],
        ))
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        items.push(OsToastCandidate {
            notification_id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            title: r.try_get("", "title").map_err(|e| decode_err("title", e))?,
            body: r.try_get("", "body").map_err(|e| decode_err("body", e))?,
            severity: r.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
            category_code: r
                .try_get("", "category_code")
                .map_err(|e| decode_err("category_code", e))?,
        });
    }
    Ok(OsToastBatch {
        watermark: since_id,
        items,
    })
}

/// Fails unless every notification belongs to `user_id`.
pub async fn ensure_owned(conn: &impl ConnectionTrait, user_id: i64, notification_ids: &[i64]) -> AppResult<()> {
    if notification_ids.is_empty() {
        return Err(AppError::ValidationFailed(vec!["notification_ids is empty.".into()]));
    }
    for id in notification_ids {
        let owner = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT recipient_user_id FROM notifications WHERE id = ?",
                [(*id).into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "notification".into(),
                id: id.to_string(),
            })?
            .try_get::<Option<i64>>("", "recipient_user_id")
            .map_err(|e| decode_err("recipient_user_id", e))?;
        if owner != Some(user_id) {
            return Err(AppError::PermissionDenied(
                "Notification does not belong to the current user".into(),
            ));
        }
    }
    Ok(())
}

/// Records the outcome of a toast for each notification it covered.
pub async fn record_os_toasts(
    conn: &impl ConnectionTrait,
    notification_ids: &[i64],
    outcome: &Result<(), String>,
) -> AppResult<()> {
    let (status, reason) = match outcome {
        Ok(()) => ("delivered", None),
        Err(e) => ("failed", Some(e.clone())),
    };
    for id in notification_ids {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_deliveries \
                (notification_id, channel, attempt_no, delivery_status, attempted_at, delivered_at, failure_reason) \
             VALUES (?, 'os', 1, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
                     CASE WHEN ? = 'delivered' THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') END, ?)",
            [(*id).into(), status.into(), status.into(), reason.clone().into()],
        ))
        .await?;
    }
    Ok(())
}
