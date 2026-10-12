//! Daily digest of notifications (Phase 6 leftovers 3).
//!
//! A user sets a category's delivery to `daily_digest` (notification preferences). From then on,
//! notifications of that category stay in the in-app inbox but are not toasted or emailed one by
//! one (`os_toast`, `email`); instead, once a day is over, the user gets one "daily digest"
//! notification (category [`DIGEST_CATEGORY`]) listing that day's still-unread notifications per
//! category. The digest itself is delivered like any notification of its category: in-app, OS
//! toast, and email through the control API when that channel is available (otherwise its email
//! is closed as `not_available` by the email dispatcher).
//!
//! - Day boundaries follow the timezone of the device the job runs on (the user's computer); the
//!   desktop has no other notion of a user timezone.
//! - Idempotent per user per day: the digest event and notification get identities seeded from
//!   (user, local date), so a second run, or another device of the same user that computes the
//!   same date, does not create a second digest. Notifications that arrive for a day after its
//!   digest was made are not added to it.
//! - Like the other notifications each device generates for itself (PM due, SLA), the digest is
//!   written with sync capture suppressed: every device makes the same row, nothing is sent.
//! - `digest_mode = 'off'`: the category stays in the in-app inbox only (no toast, no email, no
//!   digest). `instant` (default): immediate delivery.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::errors::{AppError, AppResult};
use crate::notifications::{delivery, emitter};
use crate::sync::capture::{begin_suppression, end_suppression};
use crate::sync::identity::seeded_entity_sync_id;

/// Category of the digest notification itself (seeded by migration 176).
pub const DIGEST_CATEGORY: &str = "daily_digest";

/// Pause between two digest rounds of the background job.
pub const DIGEST_TICK: Duration = Duration::from_secs(15 * 60);

/// SQL condition (preferences `p`, events `e`): the notification is delivered right away. Holds
/// for the digest category itself, whatever its preference says.
pub const INSTANT_DELIVERY_SQL: &str =
    "(e.category_code = 'daily_digest' OR COALESCE(p.digest_mode, 'instant') = 'instant')";

/// Checks a delivery mode for a category: `instant`, `daily_digest` or `off`, and only `instant`
/// for the digest category itself (a digest of digests would never be delivered).
pub fn validate_digest_mode(category_code: &str, mode: &str) -> AppResult<()> {
    if !matches!(mode, "instant" | "daily_digest" | "off") {
        return Err(AppError::ValidationFailed(vec![
            "digest_mode must be one of: instant, daily_digest, off".to_string(),
        ]));
    }
    if category_code == DIGEST_CATEGORY && mode != "instant" {
        return Err(AppError::ValidationFailed(vec![
            "The daily digest category is always delivered instantly.".to_string(),
        ]));
    }
    Ok(())
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("notification digest decode '{field}': {err}"))
}

struct Pending {
    category_label: String,
    created_at: DateTime<Utc>,
}

/// Title and body of a digest, in the device language (`fr`, otherwise English).
pub fn digest_text(lang: &str, date: NaiveDate, counts: &BTreeMap<String, usize>) -> (String, String) {
    let total: usize = counts.values().sum();
    let day = date.format("%Y-%m-%d");
    let title = if lang.starts_with("fr") {
        format!("Résumé du {day} : {total} notification(s) non lue(s)")
    } else {
        format!("Daily digest for {day}: {total} unread notification(s)")
    };
    let body = counts
        .iter()
        .map(|(label, n)| format!("{label}: {n}"))
        .collect::<Vec<_>>()
        .join(" · ");
    (title, body)
}

/// Makes the digests that are due: for every user with a category in `daily_digest`, one digest
/// per finished local day that still has unread notifications of those categories. Returns how
/// many digests were created.
pub async fn build_due_digests<Tz: TimeZone>(
    conn: &impl ConnectionTrait,
    tz: &Tz,
    now: DateTime<Utc>,
    lang: &str,
) -> AppResult<i64> {
    let today = now.with_timezone(tz).date_naive();
    let users = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT p.user_id, u.entity_sync_id AS user_sync_id FROM notification_preferences p \
             JOIN user_accounts u ON u.id = p.user_id \
             WHERE p.digest_mode = 'daily_digest' AND p.category_code <> ?",
            [DIGEST_CATEGORY.into()],
        ))
        .await?;
    let mut created = 0;
    for u in &users {
        let user_id: i64 = u.try_get("", "user_id").map_err(|e| decode_err("user_id", e))?;
        let user_sync: Option<String> = u
            .try_get("", "user_sync_id")
            .map_err(|e| decode_err("user_sync_id", e))?;
        let Some(user_sync) = user_sync else {
            continue;
        };
        let rows = conn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT n.created_at, COALESCE(c.label, e.category_code) AS category_label \
                 FROM notifications n \
                 JOIN notification_events e ON e.id = n.notification_event_id \
                 JOIN notification_preferences p \
                      ON p.user_id = n.recipient_user_id AND p.category_code = e.category_code \
                 LEFT JOIN notification_categories c ON c.code = e.category_code \
                 WHERE n.recipient_user_id = ? AND p.digest_mode = 'daily_digest' \
                   AND e.category_code <> ? AND n.delivery_state IN ('pending', 'delivered')",
                [user_id.into(), DIGEST_CATEGORY.into()],
            ))
            .await?;
        let mut by_day: BTreeMap<NaiveDate, Vec<Pending>> = BTreeMap::new();
        for r in &rows {
            let raw: String = r.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?;
            let created_at = DateTime::parse_from_rfc3339(&raw)
                .map_err(|e| decode_err("created_at", e))?
                .with_timezone(&Utc);
            let day = created_at.with_timezone(tz).date_naive();
            if day < today {
                by_day.entry(day).or_default().push(Pending {
                    category_label: r
                        .try_get("", "category_label")
                        .map_err(|e| decode_err("category_label", e))?,
                    created_at,
                });
            }
        }
        for (day, items) in by_day {
            if make_digest(conn, user_id, &user_sync, day, &items, lang).await? {
                created += 1;
            }
        }
    }
    Ok(created)
}

/// Creates the digest of one user for one day; `false` when it already exists.
async fn make_digest(
    conn: &impl ConnectionTrait,
    user_id: i64,
    user_sync: &str,
    day: NaiveDate,
    items: &[Pending],
    lang: &str,
) -> AppResult<bool> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for i in items {
        *counts.entry(i.category_label.clone()).or_default() += 1;
    }
    let (title, body) = digest_text(lang, day, &counts);
    let event_sync = seeded_entity_sync_id("notification_events", &format!("{DIGEST_CATEGORY}|{user_sync}|{day}"));
    let last = items.iter().map(|i| i.created_at).max().unwrap_or_else(Utc::now);
    let inserted = conn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO notification_events (entity_sync_id, source_module, source_record_id, event_code, \
             category_code, severity, occurred_at, payload_json) \
             VALUES (?, 'notifications', ?, 'notification.daily_digest', ?, 'info', ?, ?) \
             ON CONFLICT(entity_sync_id) DO NOTHING",
            [
                event_sync.clone().into(),
                day.to_string().into(),
                DIGEST_CATEGORY.into(),
                last.format("%Y-%m-%dT%H:%M:%SZ").to_string().into(),
                serde_json::json!({ "date": day.to_string(), "counts": counts })
                    .to_string()
                    .into(),
            ],
        ))
        .await?;
    if inserted.rows_affected() == 0 {
        return Ok(false);
    }
    let event_id: i64 = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM notification_events WHERE entity_sync_id = ?",
            [event_sync.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("digest event missing after insert")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;
    let Some(notification_id) =
        emitter::insert_notification(conn, event_id, user_id, &title, Some(&body), None, 0).await?
    else {
        return Ok(false);
    };
    delivery::schedule_deliveries(conn, notification_id).await?;
    Ok(true)
}

/// One round on this device: digests due now, in the device timezone and language.
pub async fn run_digest_round(db: &DatabaseConnection) -> AppResult<i64> {
    let lang = crate::locale::resolve_locale_preference(db).await?.active_locale;
    let txn = db.begin().await?;
    begin_suppression(&txn).await?;
    let created = build_due_digests(&txn, &chrono::Local, Utc::now(), &lang).await?;
    end_suppression(&txn).await?;
    txn.commit().await?;
    Ok(created)
}

/// Background job (`BackgroundTaskSupervisor` task `notification_digest`): a round at start,
/// then every [`DIGEST_TICK`], until cancelled.
pub async fn run_digest_job(db: DatabaseConnection, cancel: tokio_util::sync::CancellationToken) {
    loop {
        match run_digest_round(&db).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(created = n, "notifications::digest round"),
            Err(err) => tracing::error!(error = %err, "notifications::digest round failed"),
        }
        tokio::select! {
            () = cancel.cancelled() => break,
            () = tokio::time::sleep(DIGEST_TICK) => {}
        }
    }
}

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;
