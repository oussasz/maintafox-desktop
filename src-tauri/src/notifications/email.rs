//! Email channel of notifications (Phase 6.7): the desktop asks the Maintafox server to send the
//! email; it never talks SMTP and never handles email addresses.
//!
//! ## Server contract — `POST {control API}/api/v1/notifications/email`
//!
//! - Auth: `Authorization: Bearer <device activation token>` (the token sync uses).
//! - Body (`NotificationEmailRequest`, JSON): `request_id`, `notification_sync_id`,
//!   `recipient_user_sync_id`, `category_code`, `severity` (`info|warning|critical`), `title`,
//!   `body` (nullable), `action_url` (nullable, an in-app route), `occurred_at` (RFC 3339),
//!   `source_module`, `source_record_id` (nullable).
//! - The server resolves the recipient's address from its own copy of `user_accounts`
//!   (`recipient_user_sync_id`), checks the recipient belongs to the device's tenant, renders and
//!   sends the email.
//! - Idempotent on `request_id`: the same notification held by several devices is sent once.
//!   Answer `202 {"request_id": …, "status": "accepted" | "duplicate"}`.
//! - Errors `{ "error": code, "message": text }`: `400`/`422` invalid body, `404` recipient
//!   unknown or without an address, `409` conflicting reuse of a `request_id` — not retried;
//!   `401`/`403` (token or licence), `408`, `429` and `5xx` — retried with backoff
//!   (2^attempt minutes, at most [`MAX_EMAIL_ATTEMPTS`] attempts).
//!
//! ## Availability
//!
//! The channel is available only when the signed entitlement lists the capability
//! [`EMAIL_CAPABILITY`] explicitly as `true` (the vendor console turns it on once the endpoint is
//! deployed; an absent capability means "not available", unlike other capabilities), the
//! entitlement state allows it, and the device is activated. Otherwise the settings show the
//! channel as not available and waiting emails are closed as `not_available` (they are not sent
//! late when the channel appears).
//!
//! ## Queue
//!
//! `notification_deliveries` rows with `channel = 'email'` (local to each device): `queued` when a
//! notification is created for a recipient who enabled email for its category, then `sent`,
//! `retry`, `rejected`, `abandoned` or `not_available`, with the reason in `failure_reason` and
//! the number of attempts in `attempt_no`.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};
use crate::sync::transport::client::{ControlPlaneClient, TransportError};
use crate::sync::transport::wire::{NotificationEmailAccepted, NotificationEmailRequest};

/// Entitlement capability that turns the email channel on.
pub const EMAIL_CAPABILITY: &str = "notifications.email";

/// Attempts per email before it is abandoned.
pub const MAX_EMAIL_ATTEMPTS: i64 = 5;

/// Pause between two dispatch rounds.
pub const DISPATCH_INTERVAL: Duration = Duration::from_secs(60);

/// Emails sent per round.
const DISPATCH_BATCH: i64 = 20;

/// Why the email channel is not available.
pub const REASON_NOT_ENABLED: &str = "server_endpoint_not_enabled";
pub const REASON_LICENCE_BLOCKED: &str = "licence_blocked";
pub const REASON_NOT_ACTIVATED: &str = "device_not_activated";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChannelAvailability {
    pub available: bool,
    /// One of the `REASON_*` codes when not available.
    pub reason: Option<String>,
}

impl ChannelAvailability {
    fn unavailable(reason: &str) -> Self {
        Self {
            available: false,
            reason: Some(reason.to_string()),
        }
    }
}

/// Availability from the entitlement's capability map, the entitlement decision for the
/// capability, and whether the device holds an activation token.
pub fn availability_from(capability_map_json: &str, capability_allowed: bool, activated: bool) -> ChannelAvailability {
    let explicitly_enabled = serde_json::from_str::<std::collections::HashMap<String, bool>>(capability_map_json)
        .ok()
        .and_then(|m| m.get(EMAIL_CAPABILITY).copied())
        == Some(true);
    if !explicitly_enabled {
        ChannelAvailability::unavailable(REASON_NOT_ENABLED)
    } else if !capability_allowed {
        ChannelAvailability::unavailable(REASON_LICENCE_BLOCKED)
    } else if !activated {
        ChannelAvailability::unavailable(REASON_NOT_ACTIVATED)
    } else {
        ChannelAvailability {
            available: true,
            reason: None,
        }
    }
}

/// Availability and, when available, the activation token to send with.
async fn channel(db: &DatabaseConnection) -> AppResult<(ChannelAvailability, Option<String>)> {
    let summary = crate::entitlements::queries::get_entitlement_summary(db).await?;
    let check = crate::entitlements::queries::check_entitlement_capability(db, EMAIL_CAPABILITY.to_string()).await?;
    let token = crate::commands::product_license::sync_activation_bearer_token(db)
        .await?
        .filter(|t| !t.trim().is_empty());
    let availability = availability_from(&summary.capability_map_json, check.allowed, token.is_some());
    Ok((availability, token))
}

pub async fn email_channel_status(db: &DatabaseConnection) -> AppResult<ChannelAvailability> {
    Ok(channel(db).await?.0)
}

/// Queues the email of a new notification when its recipient enabled email for the category
/// (and has not muted it, and receives the category instantly rather than in the daily digest or
/// in-app only, `notifications::digest`). Same transaction as the notification. Never for the
/// safety rule notice (`catalog::ALWAYS_DELIVERED`), whatever the preference says.
pub async fn enqueue_email(conn: &impl ConnectionTrait, notification_id: i64) -> AppResult<bool> {
    let res = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "INSERT INTO notification_deliveries (notification_id, channel, attempt_no, delivery_status) \
                 SELECT n.id, 'email', 0, 'queued' FROM notifications n \
                 JOIN notification_events e ON e.id = n.notification_event_id \
                 LEFT JOIN notification_preferences p \
                        ON p.user_id = n.recipient_user_id AND p.category_code = e.category_code \
                 WHERE n.id = ? AND n.recipient_user_id IS NOT NULL \
                   AND e.category_code <> 'safety_rule_received' \
                   AND COALESCE(p.email_enabled, 0) = 1 \
                   AND {} \
                   AND (p.muted_until IS NULL OR p.muted_until <= strftime('%Y-%m-%dT%H:%M:%SZ','now')) \
                   AND NOT EXISTS (SELECT 1 FROM notification_deliveries d \
                                   WHERE d.notification_id = n.id AND d.channel = 'email')",
                crate::notifications::digest::INSTANT_DELIVERY_SQL
            ),
            [notification_id.into()],
        ))
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Sends one email request (the control API client in production).
pub trait EmailSender: Send + Sync {
    fn send<'a>(
        &'a self,
        token: &'a str,
        request: &'a NotificationEmailRequest,
    ) -> Pin<Box<dyn Future<Output = Result<NotificationEmailAccepted, TransportError>> + Send + 'a>>;
}

impl EmailSender for ControlPlaneClient {
    fn send<'a>(
        &'a self,
        token: &'a str,
        request: &'a NotificationEmailRequest,
    ) -> Pin<Box<dyn Future<Output = Result<NotificationEmailAccepted, TransportError>> + Send + 'a>> {
        Box::pin(self.send_notification_email(token, request))
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct DispatchReport {
    pub sent: i64,
    pub retry: i64,
    pub rejected: i64,
    pub abandoned: i64,
    /// The server could not be reached: the round stopped, nothing was counted as an attempt.
    pub offline: bool,
}

/// What to do after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureAction {
    /// Not retried (the request itself is wrong for the server).
    Reject,
    /// Counted as an attempt and retried later (or abandoned after the last attempt).
    Retry,
    /// Nothing reached the server: stop the round, no attempt counted.
    Offline,
}

pub fn classify_failure(err: &TransportError) -> FailureAction {
    match err {
        TransportError::Offline(_) => FailureAction::Offline,
        TransportError::Http { status, .. }
            if (400..500).contains(status) && !matches!(status, 401 | 403 | 408 | 429) =>
        {
            FailureAction::Reject
        }
        _ => FailureAction::Retry,
    }
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("email dispatch decode '{field}': {err}"))
}

async fn update_delivery(
    conn: &impl ConnectionTrait,
    delivery_id: i64,
    status: &str,
    attempts: i64,
    reason: Option<String>,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE notification_deliveries SET delivery_status = ?, attempt_no = ?, failure_reason = ?, \
         attempted_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'), \
         delivered_at = CASE WHEN ? = 'sent' THEN strftime('%Y-%m-%dT%H:%M:%SZ','now') ELSE delivered_at END \
         WHERE id = ?",
        [
            status.into(),
            attempts.into(),
            reason.into(),
            status.into(),
            delivery_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Sends the emails that are due (queued, or retry after 2^attempts minutes).
pub async fn dispatch_email_batch(
    conn: &impl ConnectionTrait,
    sender: &dyn EmailSender,
    token: &str,
) -> AppResult<DispatchReport> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT d.id AS delivery_id, d.attempt_no, n.entity_sync_id AS notification_sync_id, n.title, n.body, \
                    n.action_url, e.category_code, e.severity, e.occurred_at, e.source_module, e.source_record_id, \
                    u.entity_sync_id AS user_sync_id \
             FROM notification_deliveries d \
             JOIN notifications n ON n.id = d.notification_id \
             JOIN notification_events e ON e.id = n.notification_event_id \
             JOIN user_accounts u ON u.id = n.recipient_user_id \
             WHERE d.channel = 'email' AND (d.delivery_status = 'queued' OR (d.delivery_status = 'retry' \
                   AND d.attempted_at <= strftime('%Y-%m-%dT%H:%M:%SZ', \
                       datetime('now', '-' || (1 << d.attempt_no) || ' minutes')))) \
             ORDER BY d.id LIMIT ?",
            [DISPATCH_BATCH.into()],
        ))
        .await?;
    let mut report = DispatchReport::default();
    for r in &rows {
        let delivery_id: i64 = r.try_get("", "delivery_id").map_err(|e| decode_err("delivery_id", e))?;
        let attempts: i64 = r.try_get("", "attempt_no").map_err(|e| decode_err("attempt_no", e))?;
        let notification_sync_id: Option<String> = r
            .try_get("", "notification_sync_id")
            .map_err(|e| decode_err("notification_sync_id", e))?;
        let user_sync_id: Option<String> = r
            .try_get("", "user_sync_id")
            .map_err(|e| decode_err("user_sync_id", e))?;
        let (Some(notification_sync_id), Some(user_sync_id)) = (notification_sync_id, user_sync_id) else {
            update_delivery(
                conn,
                delivery_id,
                "rejected",
                attempts,
                Some("notification or recipient has no sync identity".into()),
            )
            .await?;
            report.rejected += 1;
            continue;
        };
        let request = NotificationEmailRequest {
            request_id: crate::sync::identity::seeded_entity_sync_id("notification_email", &notification_sync_id),
            notification_sync_id,
            recipient_user_sync_id: user_sync_id,
            category_code: r
                .try_get("", "category_code")
                .map_err(|e| decode_err("category_code", e))?,
            severity: r.try_get("", "severity").map_err(|e| decode_err("severity", e))?,
            title: r.try_get("", "title").map_err(|e| decode_err("title", e))?,
            body: r.try_get("", "body").map_err(|e| decode_err("body", e))?,
            action_url: r.try_get("", "action_url").map_err(|e| decode_err("action_url", e))?,
            occurred_at: r.try_get("", "occurred_at").map_err(|e| decode_err("occurred_at", e))?,
            source_module: r
                .try_get("", "source_module")
                .map_err(|e| decode_err("source_module", e))?,
            source_record_id: r
                .try_get("", "source_record_id")
                .map_err(|e| decode_err("source_record_id", e))?,
        };
        match sender.send(token, &request).await {
            Ok(accepted) if accepted.request_id == request.request_id => {
                update_delivery(conn, delivery_id, "sent", attempts + 1, None).await?;
                report.sent += 1;
            }
            Ok(accepted) => {
                let reason = format!(
                    "server acknowledged request {} instead of {}",
                    accepted.request_id, request.request_id
                );
                retry_or_abandon(conn, delivery_id, attempts + 1, reason, &mut report).await?;
            }
            Err(err) => match classify_failure(&err) {
                FailureAction::Offline => {
                    report.offline = true;
                    break;
                }
                FailureAction::Reject => {
                    update_delivery(conn, delivery_id, "rejected", attempts + 1, Some(err.message())).await?;
                    report.rejected += 1;
                }
                FailureAction::Retry => {
                    retry_or_abandon(conn, delivery_id, attempts + 1, err.message(), &mut report).await?;
                }
            },
        }
    }
    Ok(report)
}

async fn retry_or_abandon(
    conn: &impl ConnectionTrait,
    delivery_id: i64,
    attempts: i64,
    reason: String,
    report: &mut DispatchReport,
) -> AppResult<()> {
    if attempts >= MAX_EMAIL_ATTEMPTS {
        update_delivery(conn, delivery_id, "abandoned", attempts, Some(reason)).await?;
        report.abandoned += 1;
    } else {
        update_delivery(conn, delivery_id, "retry", attempts, Some(reason)).await?;
        report.retry += 1;
    }
    Ok(())
}

/// Closes the waiting emails while the channel is not available (they are not sent late).
pub async fn close_unavailable(conn: &impl ConnectionTrait, reason: &str) -> AppResult<u64> {
    let res = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE notification_deliveries SET delivery_status = 'not_available', failure_reason = ?, \
             attempted_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
             WHERE channel = 'email' AND delivery_status IN ('queued', 'retry')",
            [format!("email channel not available: {reason}").into()],
        ))
        .await?;
    Ok(res.rows_affected())
}

async fn run_round(db: &DatabaseConnection, sender: &dyn EmailSender) -> AppResult<()> {
    let (availability, token) = channel(db).await?;
    match (availability.available, token) {
        (true, Some(token)) => {
            let report = dispatch_email_batch(db, sender, &token).await?;
            if report != DispatchReport::default() {
                tracing::info!(?report, "notifications::email dispatch round");
            }
        }
        _ => {
            let reason = availability.reason.unwrap_or_else(|| REASON_NOT_ACTIVATED.to_string());
            close_unavailable(db, &reason).await?;
        }
    }
    Ok(())
}

/// Background loop: one dispatch round every [`DISPATCH_INTERVAL`].
pub async fn start_email_dispatcher(db: DatabaseConnection, api_base_url: String) {
    let client = match ControlPlaneClient::new(&api_base_url, crate::sync::transport::SyncTiming::production().timeouts)
    {
        Ok(client) => client,
        Err(message) => {
            tracing::error!("notifications::email dispatcher not started: {message}");
            return;
        }
    };
    let mut ticker = tokio::time::interval(DISPATCH_INTERVAL);
    loop {
        ticker.tick().await;
        if let Err(err) = run_round(&db, &client).await {
            tracing::error!(error = %err, "notifications::email dispatch round failed");
        }
    }
}
