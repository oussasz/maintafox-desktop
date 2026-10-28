//! Daily digest: grouping per user and local day, idempotence, the timezone boundary, the
//! preference rules, and the channels a digest-mode category is held back from.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use sea_orm::DatabaseConnection;

use super::{build_due_digests, digest_text, run_digest_round, validate_digest_mode, DIGEST_CATEGORY};
use crate::notifications::delivery::schedule_deliveries;
use crate::notifications::os_toast::pending_os_toasts;
use crate::sync::capture::{begin_suppression, end_suppression};
use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_db, RowFactory};

struct Fixture {
    db: DatabaseConnection,
    alice: i64,
    bob: i64,
}

/// Alice gets `wo_assigned` in the daily digest; Bob keeps the defaults (instant).
async fn setup() -> Fixture {
    let db = started_db().await;
    let mut factory = RowFactory::for_device(1);
    let alice = factory.create(&db, "user_accounts").await.expect("alice").rowid;
    let bob = factory.create(&db, "user_accounts").await.expect("bob").rowid;
    // Administrators: every module is theirs to see, so the channels here test delivery only
    // (what a user may see is `inbox::visible_to`, tested on its own).
    exec(
        &db,
        "UPDATE user_accounts SET is_admin = 1 WHERE id IN (?, ?)",
        vec![alice.into(), bob.into()],
    )
    .await;
    set_mode(&db, alice, "wo_assigned", "daily_digest").await;
    Fixture { db, alice, bob }
}

async fn set_mode(db: &DatabaseConnection, user: i64, category: &str, mode: &str) {
    exec(
        db,
        "INSERT INTO notification_preferences (user_id, category_code, digest_mode) VALUES (?, ?, ?) \
         ON CONFLICT(user_id, category_code) DO UPDATE SET digest_mode = excluded.digest_mode",
        vec![user.into(), category.into(), mode.into()],
    )
    .await;
}

async fn enable_email(db: &DatabaseConnection, user: i64, category: &str) {
    exec(
        db,
        "INSERT INTO notification_preferences (user_id, category_code, email_enabled) VALUES (?, ?, 1) \
         ON CONFLICT(user_id, category_code) DO UPDATE SET email_enabled = 1",
        vec![user.into(), category.into()],
    )
    .await;
}

/// A notification of `category` for `user`, created at `at` (RFC 3339), deliveries scheduled.
async fn notify_at(db: &DatabaseConnection, user: i64, category: &str, at: &str) -> i64 {
    exec(
        db,
        "INSERT INTO notification_events (source_module, source_record_id, event_code, category_code, severity, \
         occurred_at) VALUES ('wo', 'OT-1', 'wo.assigned', ?, 'info', ?)",
        vec![category.into(), at.into()],
    )
    .await;
    let event = scalar_i64(db, "SELECT MAX(id) FROM notification_events", vec![]).await;
    exec(
        db,
        "INSERT INTO notifications (notification_event_id, recipient_user_id, delivery_state, title, created_at) \
         VALUES (?, ?, 'pending', 'OT assigné', ?)",
        vec![event.into(), user.into(), at.into()],
    )
    .await;
    let id = scalar_i64(db, "SELECT MAX(id) FROM notifications", vec![]).await;
    schedule_deliveries(db, id).await.expect("deliveries");
    id
}

/// One digest round with capture suppressed, like `run_digest_round`.
async fn round<Tz: TimeZone>(db: &DatabaseConnection, tz: &Tz, now: &str) -> i64 {
    let now = DateTime::parse_from_rfc3339(now).expect("now").with_timezone(&Utc);
    begin_suppression(db).await.expect("suppress");
    let created = build_due_digests(db, tz, now, "en").await.expect("digest round");
    end_suppression(db).await.expect("unsuppress");
    created
}

async fn digests_of(db: &DatabaseConnection, user: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT COUNT(*) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'daily_digest' AND n.recipient_user_id = ?",
        vec![user.into()],
    )
    .await
}

async fn digest_for_day(db: &DatabaseConnection, user: i64, day: &str) -> Option<(String, String)> {
    let id = scalar_opt_text(
        db,
        "SELECT CAST(n.id AS TEXT) FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'daily_digest' AND n.recipient_user_id = ? AND e.source_record_id = ?",
        vec![user.into(), day.into()],
    )
    .await?;
    let title = scalar_opt_text(
        db,
        "SELECT title FROM notifications WHERE id = ?",
        vec![id.clone().into()],
    )
    .await?;
    let body = scalar_opt_text(db, "SELECT body FROM notifications WHERE id = ?", vec![id.into()]).await?;
    Some((title, body))
}

#[tokio::test]
async fn unread_digest_notifications_are_grouped_per_user_and_finished_day() {
    let f = setup().await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T09:00:00Z").await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T15:00:00Z").await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-11T10:00:00Z").await;
    // Today: not finished yet. Instant category: not in a digest. Bob: no digest preference.
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-12T08:00:00Z").await;
    notify_at(&f.db, f.alice, "pm_due", "2026-10-10T09:30:00Z").await;
    notify_at(&f.db, f.bob, "wo_assigned", "2026-10-10T09:00:00Z").await;

    assert_eq!(round(&f.db, &Utc, "2026-10-12T12:00:00Z").await, 2);
    assert_eq!(digests_of(&f.db, f.alice).await, 2);
    assert_eq!(digests_of(&f.db, f.bob).await, 0);
    let (title, body) = digest_for_day(&f.db, f.alice, "2026-10-10")
        .await
        .expect("digest of the 10th");
    assert_eq!(title, "Daily digest for 2026-10-10: 2 unread notification(s)");
    assert_eq!(body, "WO Assigned: 2");
    let (title, _) = digest_for_day(&f.db, f.alice, "2026-10-11")
        .await
        .expect("digest of the 11th");
    assert!(title.contains(": 1 unread"), "{title}");
    // The digest is delivered in-app like any notification.
    let state = scalar_opt_text(
        &f.db,
        "SELECT n.delivery_state FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'daily_digest' AND e.source_record_id = '2026-10-10'",
        vec![],
    )
    .await;
    assert_eq!(state.as_deref(), Some("delivered"));
}

#[tokio::test]
async fn a_second_round_does_not_make_a_second_digest() {
    let f = setup().await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T09:00:00Z").await;
    assert_eq!(round(&f.db, &Utc, "2026-10-11T06:00:00Z").await, 1);
    assert_eq!(round(&f.db, &Utc, "2026-10-11T06:15:00Z").await, 0);
    // A notification that arrives for that day afterwards does not reopen it either.
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T20:00:00Z").await;
    assert_eq!(round(&f.db, &Utc, "2026-10-12T06:00:00Z").await, 0);
    assert_eq!(digests_of(&f.db, f.alice).await, 1);
}

#[tokio::test]
async fn read_notifications_are_left_out() {
    let f = setup().await;
    let read = notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T09:00:00Z").await;
    exec(
        &f.db,
        "UPDATE notifications SET delivery_state = 'read' WHERE id = ?",
        vec![read.into()],
    )
    .await;
    assert_eq!(round(&f.db, &Utc, "2026-10-12T06:00:00Z").await, 0);
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T10:00:00Z").await;
    assert_eq!(round(&f.db, &Utc, "2026-10-12T06:00:00Z").await, 1);
    let (_, body) = digest_for_day(&f.db, f.alice, "2026-10-10").await.expect("digest");
    assert_eq!(body, "WO Assigned: 1");
}

#[tokio::test]
async fn days_follow_the_timezone_of_the_device() {
    // 23:30 UTC on the 11th is already the 12th at UTC+1.
    let f = setup().await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-11T23:30:00Z").await;
    let plus_one = FixedOffset::east_opt(3600).expect("offset");
    assert_eq!(
        round(&f.db, &plus_one, "2026-10-12T10:00:00Z").await,
        0,
        "still today at UTC+1"
    );
    assert_eq!(round(&f.db, &Utc, "2026-10-12T10:00:00Z").await, 1, "yesterday in UTC");
    assert!(digest_for_day(&f.db, f.alice, "2026-10-11").await.is_some());
}

#[tokio::test]
async fn digest_and_in_app_only_categories_are_not_toasted_or_emailed_one_by_one() {
    let f = setup().await;
    enable_email(&f.db, f.alice, "wo_assigned").await;
    set_mode(&f.db, f.alice, "pm_due", "off").await;
    enable_email(&f.db, f.alice, "pm_due").await;
    let in_digest = notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T09:00:00Z").await;
    let in_app_only = notify_at(&f.db, f.alice, "pm_due", "2026-10-10T09:00:00Z").await;
    let instant = notify_at(&f.db, f.alice, "di_pending_review", "2026-10-10T09:00:00Z").await;
    let emails = scalar_i64(
        &f.db,
        "SELECT COUNT(*) FROM notification_deliveries WHERE channel = 'email' AND notification_id IN (?, ?)",
        vec![in_digest.into(), in_app_only.into()],
    )
    .await;
    assert_eq!(emails, 0);
    let toasts = pending_os_toasts(&f.db, f.alice, Some(0)).await.expect("toasts");
    let ids: Vec<i64> = toasts.items.iter().map(|t| t.notification_id).collect();
    assert_eq!(ids, vec![instant]);
    // 'off' keeps the category out of the digest too.
    assert_eq!(round(&f.db, &Utc, "2026-10-12T06:00:00Z").await, 1);
    let (_, body) = digest_for_day(&f.db, f.alice, "2026-10-10").await.expect("digest");
    assert_eq!(body, "WO Assigned: 1");
}

#[tokio::test]
async fn the_digest_itself_is_toasted_and_emailed_when_the_user_wants_it() {
    let f = setup().await;
    enable_email(&f.db, f.alice, DIGEST_CATEGORY).await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T09:00:00Z").await;
    assert_eq!(round(&f.db, &Utc, "2026-10-12T06:00:00Z").await, 1);
    let digest = scalar_i64(
        &f.db,
        "SELECT n.id FROM notifications n JOIN notification_events e ON e.id = n.notification_event_id \
         WHERE e.category_code = 'daily_digest'",
        vec![],
    )
    .await;
    let email = scalar_opt_text(
        &f.db,
        "SELECT delivery_status FROM notification_deliveries WHERE notification_id = ? AND channel = 'email'",
        vec![digest.into()],
    )
    .await;
    assert_eq!(email.as_deref(), Some("queued"));
    let toasts = pending_os_toasts(&f.db, f.alice, Some(0)).await.expect("toasts");
    assert_eq!(toasts.items.len(), 1);
    assert_eq!(toasts.items[0].notification_id, digest);
}

#[tokio::test]
async fn the_digest_is_made_by_each_device_and_not_sent_through_sync() {
    let f = setup().await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T09:00:00Z").await;
    assert_eq!(round(&f.db, &Utc, "2026-10-12T06:00:00Z").await, 1);
    let queued = scalar_i64(
        &f.db,
        "SELECT COUNT(*) FROM sync_outbox o WHERE o.entity_sync_id IN ( \
             SELECT e.entity_sync_id FROM notification_events e WHERE e.category_code = 'daily_digest' \
             UNION SELECT n.entity_sync_id FROM notifications n \
                   JOIN notification_events e ON e.id = n.notification_event_id \
                   WHERE e.category_code = 'daily_digest')",
        vec![],
    )
    .await;
    assert_eq!(queued, 0);
    // Same (user, day) gives the same identity on every device.
    let user_sync = scalar_opt_text(
        &f.db,
        "SELECT entity_sync_id FROM user_accounts WHERE id = ?",
        vec![f.alice.into()],
    )
    .await
    .expect("user sync id");
    let expected = crate::sync::identity::seeded_entity_sync_id(
        "notification_events",
        &format!("daily_digest|{user_sync}|2026-10-10"),
    );
    let actual = scalar_opt_text(
        &f.db,
        "SELECT entity_sync_id FROM notification_events WHERE category_code = 'daily_digest'",
        vec![],
    )
    .await;
    assert_eq!(actual, Some(expected));
}

#[tokio::test]
async fn the_background_round_uses_the_clock_and_the_device_language() {
    let f = setup().await;
    let three_days_ago = (Utc::now() - Duration::days(3))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    notify_at(&f.db, f.alice, "wo_assigned", &three_days_ago).await;
    assert_eq!(run_digest_round(&f.db).await.expect("round"), 1);
    assert_eq!(run_digest_round(&f.db).await.expect("round again"), 0);
}

#[tokio::test]
async fn the_digest_category_ships_with_the_seeded_identity() {
    let f = setup().await;
    let actual = scalar_opt_text(
        &f.db,
        "SELECT entity_sync_id FROM notification_categories WHERE code = 'daily_digest'",
        vec![],
    )
    .await;
    assert_eq!(
        actual,
        Some(crate::sync::identity::seeded_entity_sync_id(
            "notification_categories",
            "daily_digest"
        ))
    );
}

#[test]
fn digest_text_is_localized() {
    let day = NaiveDate::from_ymd_opt(2026, 10, 10).expect("date");
    let mut counts = BTreeMap::new();
    counts.insert("WO Assigned".to_string(), 2);
    counts.insert("PM Due Soon".to_string(), 1);
    let (title, body) = digest_text("fr-DZ", day, &counts);
    assert_eq!(title, "Résumé du 2026-10-10 : 3 notification(s) non lue(s)");
    assert_eq!(body, "PM Due Soon: 1 · WO Assigned: 2");
    let (title, _) = digest_text("en", day, &counts);
    assert_eq!(title, "Daily digest for 2026-10-10: 3 unread notification(s)");
}

#[test]
fn delivery_modes_are_validated() {
    for mode in ["instant", "daily_digest", "off"] {
        assert!(validate_digest_mode("wo_assigned", mode).is_ok(), "{mode}");
    }
    assert!(validate_digest_mode("wo_assigned", "weekly").is_err());
    assert!(validate_digest_mode(DIGEST_CATEGORY, "instant").is_ok());
    assert!(validate_digest_mode(DIGEST_CATEGORY, "daily_digest").is_err());
    assert!(validate_digest_mode(DIGEST_CATEGORY, "off").is_err());
}

#[tokio::test]
async fn the_digest_catches_up_a_week_at_most_and_is_rendered_from_category_codes() {
    let f = setup().await;
    // Ten days ago: older than the catch-up window, no digest for that day.
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-02T09:00:00Z").await;
    notify_at(&f.db, f.alice, "wo_assigned", "2026-10-10T09:00:00Z").await;
    assert_eq!(round(&f.db, &Utc, "2026-10-12T06:00:00Z").await, 1);
    assert!(digest_for_day(&f.db, f.alice, "2026-10-02").await.is_none());
    let params = scalar_opt_text(
        &f.db,
        "SELECT message_params_json FROM notification_events WHERE category_code = 'daily_digest'",
        vec![],
    )
    .await
    .expect("params");
    let params: serde_json::Value = serde_json::from_str(&params).expect("json");
    assert_eq!(params["counts"]["wo_assigned"], 1);
    assert_eq!(params["date"], "2026-10-10");
    // Archived or resolved notifications are not counted.
    let late = notify_at(&f.db, f.alice, "wo_assigned", "2026-10-11T09:00:00Z").await;
    exec(
        &f.db,
        "UPDATE notifications SET archived_at = '2026-10-11T10:00:00Z' WHERE id = ?",
        vec![late.into()],
    )
    .await;
    assert_eq!(round(&f.db, &Utc, "2026-10-12T07:00:00Z").await, 0);
}
