//! Email and OS toast channels: who gets queued, what the server receives, how failures are
//! classified, and which notifications a window toasts.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use sea_orm::DatabaseConnection;

use crate::notifications::delivery::schedule_deliveries;
use crate::notifications::email::{
    availability_from, classify_failure, close_unavailable, dispatch_email_batch, EmailSender, FailureAction,
    MAX_EMAIL_ATTEMPTS, REASON_LICENCE_BLOCKED, REASON_NOT_ACTIVATED, REASON_NOT_ENABLED,
};
use crate::notifications::os_toast::{ensure_owned, pending_os_toasts, record_os_toasts};
use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_db, RowFactory};
use crate::sync::transport::client::TransportError;
use crate::sync::transport::wire::{NotificationEmailAccepted, NotificationEmailRequest};

/// Scripted control API: answers in order and records the requests.
struct ScriptedSender {
    answers: Mutex<Vec<Result<&'static str, TransportError>>>,
    requests: Mutex<Vec<NotificationEmailRequest>>,
}

impl ScriptedSender {
    fn new(answers: Vec<Result<&'static str, TransportError>>) -> Self {
        Self {
            answers: Mutex::new(answers.into_iter().rev().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl EmailSender for ScriptedSender {
    fn send<'a>(
        &'a self,
        _token: &'a str,
        request: &'a NotificationEmailRequest,
    ) -> Pin<Box<dyn Future<Output = Result<NotificationEmailAccepted, TransportError>> + Send + 'a>> {
        self.requests.lock().expect("lock").push(request.clone());
        let answer = self.answers.lock().expect("lock").pop().expect("scripted answer");
        Box::pin(async move {
            answer.map(|status| NotificationEmailAccepted {
                request_id: request.request_id.clone(),
                status: status.to_string(),
            })
        })
    }
}

fn http(status: u16) -> TransportError {
    TransportError::Http {
        status,
        code: Some("x".into()),
        message: format!("status {status}"),
        retry_after: None,
        wipe_without_upload: None,
    }
}

struct Fixture {
    db: DatabaseConnection,
    alice: i64,
    alice_sync: String,
    bob: i64,
}

async fn setup() -> Fixture {
    let db = started_db().await;
    let mut factory = RowFactory::for_device(1);
    let alice = factory.create(&db, "user_accounts").await.expect("alice");
    let bob = factory.create(&db, "user_accounts").await.expect("bob");
    // Administrators: every module is theirs to see, so the channels here test delivery only
    // (what a user may see is `inbox::visible_to`, tested on its own).
    exec(
        &db,
        "UPDATE user_accounts SET is_admin = 1 WHERE id IN (?, ?)",
        vec![alice.rowid.into(), bob.rowid.into()],
    )
    .await;
    // Alice wants emails for assigned work orders; Bob keeps the defaults (no email).
    exec(
        &db,
        "INSERT INTO notification_preferences (user_id, category_code, email_enabled) VALUES (?, 'wo_assigned', 1)",
        vec![alice.rowid.into()],
    )
    .await;
    Fixture {
        db,
        alice: alice.rowid,
        alice_sync: alice.entity_sync_id,
        bob: bob.rowid,
    }
}

/// A notification of `category` for `user`, with its deliveries scheduled.
async fn notify(db: &DatabaseConnection, user: i64, category: &str, title: &str) -> i64 {
    exec(
        db,
        "INSERT INTO notification_events (source_module, source_record_id, event_code, category_code, severity, \
         occurred_at) VALUES ('wo', 'OT-1', 'wo.assigned', ?, 'info', '2026-10-11T08:00:00Z')",
        vec![category.into()],
    )
    .await;
    let event = scalar_i64(db, "SELECT MAX(id) FROM notification_events", vec![]).await;
    exec(
        db,
        "INSERT INTO notifications (notification_event_id, recipient_user_id, delivery_state, title, body) \
         VALUES (?, ?, 'pending', ?, 'Pompe P-12 — révision')",
        vec![event.into(), user.into(), title.into()],
    )
    .await;
    let id = scalar_i64(db, "SELECT MAX(id) FROM notifications", vec![]).await;
    schedule_deliveries(db, id).await.expect("deliveries");
    id
}

async fn email_status(db: &DatabaseConnection, notification_id: i64) -> Option<String> {
    scalar_opt_text(
        db,
        "SELECT delivery_status FROM notification_deliveries WHERE notification_id = ? AND channel = 'email'",
        vec![notification_id.into()],
    )
    .await
}

#[tokio::test]
async fn email_is_queued_only_for_recipients_who_enabled_it() {
    let f = setup().await;
    let for_alice = notify(&f.db, f.alice, "wo_assigned", "OT assigné").await;
    let for_bob = notify(&f.db, f.bob, "wo_assigned", "OT assigné").await;
    let other_category = notify(&f.db, f.alice, "pm_due", "PM à venir").await;
    assert_eq!(email_status(&f.db, for_alice).await.as_deref(), Some("queued"));
    assert_eq!(email_status(&f.db, for_bob).await, None);
    assert_eq!(email_status(&f.db, other_category).await, None);
    // Scheduling again (dedupe re-notification) does not queue a second email.
    schedule_deliveries(&f.db, for_alice).await.expect("again");
    let n = scalar_i64(
        &f.db,
        "SELECT COUNT(*) FROM notification_deliveries WHERE notification_id = ? AND channel = 'email'",
        vec![for_alice.into()],
    )
    .await;
    assert_eq!(n, 1);
}

#[tokio::test]
async fn muted_category_is_not_emailed() {
    let f = setup().await;
    exec(
        &f.db,
        "UPDATE notification_preferences SET muted_until = '2999-01-01T00:00:00Z' WHERE user_id = ?",
        vec![f.alice.into()],
    )
    .await;
    let id = notify(&f.db, f.alice, "wo_assigned", "OT assigné").await;
    assert_eq!(email_status(&f.db, id).await, None);
}

#[tokio::test]
async fn dispatch_sends_the_contract_and_classifies_answers() {
    let f = setup().await;
    let sent = notify(&f.db, f.alice, "wo_assigned", "OT-1 assigné").await;
    let rejected = notify(&f.db, f.alice, "wo_assigned", "OT-2 assigné").await;
    let retried = notify(&f.db, f.alice, "wo_assigned", "OT-3 assigné").await;
    let sender = ScriptedSender::new(vec![Ok("accepted"), Err(http(422)), Err(http(503))]);
    let report = dispatch_email_batch(&f.db, &sender, "token").await.expect("dispatch");
    assert_eq!(
        (report.sent, report.rejected, report.retry, report.offline),
        (1, 1, 1, false)
    );
    assert_eq!(email_status(&f.db, sent).await.as_deref(), Some("sent"));
    assert_eq!(email_status(&f.db, rejected).await.as_deref(), Some("rejected"));
    assert_eq!(email_status(&f.db, retried).await.as_deref(), Some("retry"));

    let requests = sender.requests.lock().expect("lock").clone();
    let first = &requests[0];
    let n_sync = scalar_opt_text(
        &f.db,
        "SELECT entity_sync_id FROM notifications WHERE id = ?",
        vec![sent.into()],
    )
    .await
    .expect("sync id");
    assert_eq!(first.notification_sync_id, n_sync);
    assert_eq!(
        first.request_id,
        crate::sync::identity::seeded_entity_sync_id("notification_email", &n_sync),
        "the idempotency key is the same on every device"
    );
    assert_eq!(first.recipient_user_sync_id, f.alice_sync);
    assert_eq!(first.category_code, "wo_assigned");
    assert_eq!(first.title, "OT-1 assigné");
    assert_eq!(first.body.as_deref(), Some("Pompe P-12 — révision"));

    // The retry is not due before its backoff.
    let again = dispatch_email_batch(&f.db, &ScriptedSender::new(vec![]), "token")
        .await
        .expect("dispatch");
    assert_eq!(again.retry + again.sent, 0);
}

#[tokio::test]
async fn last_failed_attempt_abandons_the_email() {
    let f = setup().await;
    let id = notify(&f.db, f.alice, "wo_assigned", "OT assigné").await;
    exec(
        &f.db,
        "UPDATE notification_deliveries SET delivery_status = 'retry', attempt_no = ?, \
         attempted_at = '2000-01-01T00:00:00Z' WHERE notification_id = ? AND channel = 'email'",
        vec![(MAX_EMAIL_ATTEMPTS - 1).into(), id.into()],
    )
    .await;
    let report = dispatch_email_batch(&f.db, &ScriptedSender::new(vec![Err(http(500))]), "token")
        .await
        .expect("dispatch");
    assert_eq!(report.abandoned, 1);
    assert_eq!(email_status(&f.db, id).await.as_deref(), Some("abandoned"));
}

#[tokio::test]
async fn offline_stops_the_round_without_counting_an_attempt() {
    let f = setup().await;
    let a = notify(&f.db, f.alice, "wo_assigned", "OT-1").await;
    notify(&f.db, f.alice, "wo_assigned", "OT-2").await;
    let sender = ScriptedSender::new(vec![Err(TransportError::Offline("no route".into()))]);
    let report = dispatch_email_batch(&f.db, &sender, "token").await.expect("dispatch");
    assert!(report.offline);
    assert_eq!(sender.requests.lock().expect("lock").len(), 1, "the round stopped");
    assert_eq!(email_status(&f.db, a).await.as_deref(), Some("queued"));
    let attempts = scalar_i64(
        &f.db,
        "SELECT attempt_no FROM notification_deliveries WHERE notification_id = ? AND channel = 'email'",
        vec![a.into()],
    )
    .await;
    assert_eq!(attempts, 0);
}

#[tokio::test]
async fn unavailable_channel_closes_waiting_emails_only() {
    let f = setup().await;
    let waiting = notify(&f.db, f.alice, "wo_assigned", "OT-1").await;
    let sent = notify(&f.db, f.alice, "wo_assigned", "OT-2").await;
    exec(
        &f.db,
        "UPDATE notification_deliveries SET delivery_status = 'sent' WHERE notification_id = ? AND channel = 'email'",
        vec![sent.into()],
    )
    .await;
    assert_eq!(close_unavailable(&f.db, REASON_NOT_ENABLED).await.expect("close"), 1);
    assert_eq!(email_status(&f.db, waiting).await.as_deref(), Some("not_available"));
    assert_eq!(email_status(&f.db, sent).await.as_deref(), Some("sent"));
}

#[test]
fn email_channel_needs_explicit_capability_licence_and_activation() {
    assert_eq!(
        availability_from("{}", true, true).reason.as_deref(),
        Some(REASON_NOT_ENABLED),
        "an absent capability is not available (the endpoint may not exist)"
    );
    assert_eq!(
        availability_from(r#"{"notifications.email": false}"#, true, true)
            .reason
            .as_deref(),
        Some(REASON_NOT_ENABLED)
    );
    assert_eq!(
        availability_from(r#"{"notifications.email": true}"#, false, true)
            .reason
            .as_deref(),
        Some(REASON_LICENCE_BLOCKED)
    );
    assert_eq!(
        availability_from(r#"{"notifications.email": true}"#, true, false)
            .reason
            .as_deref(),
        Some(REASON_NOT_ACTIVATED)
    );
    assert!(availability_from(r#"{"notifications.email": true}"#, true, true).available);
    assert_eq!(
        availability_from("not json", true, true).reason.as_deref(),
        Some(REASON_NOT_ENABLED)
    );
}

#[test]
fn failures_are_classified() {
    assert_eq!(classify_failure(&http(400)), FailureAction::Reject);
    assert_eq!(classify_failure(&http(404)), FailureAction::Reject);
    assert_eq!(classify_failure(&http(401)), FailureAction::Retry);
    assert_eq!(classify_failure(&http(429)), FailureAction::Retry);
    assert_eq!(classify_failure(&http(502)), FailureAction::Retry);
    assert_eq!(
        classify_failure(&TransportError::Interrupted("reset".into())),
        FailureAction::Retry
    );
    assert_eq!(
        classify_failure(&TransportError::Offline("dns".into())),
        FailureAction::Offline
    );
}

#[tokio::test]
async fn os_toasts_cover_new_notifications_once() {
    let f = setup().await;
    let before = notify(&f.db, f.alice, "wo_assigned", "avant l'ouverture").await;
    let start = pending_os_toasts(&f.db, f.alice, None).await.expect("start");
    assert!(start.items.is_empty());
    assert_eq!(start.watermark, before, "the inbox at sign-in is not replayed");

    let new_one = notify(&f.db, f.alice, "wo_assigned", "OT assigné").await;
    notify(&f.db, f.bob, "wo_assigned", "pour Bob").await;
    exec(
        &f.db,
        "INSERT INTO notification_preferences (user_id, category_code, os_enabled) VALUES (?, 'pm_due', 0)",
        vec![f.alice.into()],
    )
    .await;
    notify(&f.db, f.alice, "pm_due", "toast désactivé").await;

    let batch = pending_os_toasts(&f.db, f.alice, Some(start.watermark))
        .await
        .expect("pending");
    let ids: Vec<i64> = batch.items.iter().map(|i| i.notification_id).collect();
    assert_eq!(ids, vec![new_one]);
    assert_eq!(batch.items[0].title, "OT assigné");

    record_os_toasts(&f.db, &ids, &Ok(())).await.expect("record");
    let again = pending_os_toasts(&f.db, f.alice, Some(start.watermark))
        .await
        .expect("again");
    assert!(again.items.is_empty(), "a toasted notification is not shown twice");
}

#[tokio::test]
async fn failed_toast_is_recorded_with_its_reason() {
    let f = setup().await;
    let id = notify(&f.db, f.alice, "wo_assigned", "OT assigné").await;
    record_os_toasts(&f.db, &[id], &Err("toast service unavailable".into()))
        .await
        .expect("record");
    let reason = scalar_opt_text(
        &f.db,
        "SELECT delivery_status || ':' || failure_reason FROM notification_deliveries \
         WHERE notification_id = ? AND channel = 'os'",
        vec![id.into()],
    )
    .await;
    assert_eq!(reason.as_deref(), Some("failed:toast service unavailable"));
}

#[tokio::test]
async fn toasts_only_cover_own_notifications() {
    let f = setup().await;
    let bobs = notify(&f.db, f.bob, "wo_assigned", "pour Bob").await;
    assert!(ensure_owned(&f.db, f.alice, &[bobs]).await.is_err());
    assert!(ensure_owned(&f.db, f.bob, &[bobs]).await.is_ok());
    assert!(ensure_owned(&f.db, f.bob, &[]).await.is_err());
}

/// AU-112 review: no toast and no email about a record the reader may not see (the inbox
/// condition): a user without the work-order view right gets neither.
#[tokio::test]
async fn no_toast_or_email_about_what_the_reader_may_not_see() {
    let f = setup().await;
    let mut factory = RowFactory::for_device(2);
    let carol = factory.create(&f.db, "user_accounts").await.expect("carol").rowid;
    exec(
        &f.db,
        "UPDATE user_accounts SET is_admin = 0 WHERE id = ?",
        vec![carol.into()],
    )
    .await;
    exec(
        &f.db,
        "INSERT INTO notification_preferences (user_id, category_code, email_enabled) VALUES (?, 'wo_assigned', 1)",
        vec![carol.into()],
    )
    .await;
    let start = pending_os_toasts(&f.db, carol, None).await.expect("start");
    let hidden = notify(&f.db, carol, "wo_assigned", "OT assigné").await;
    assert_eq!(email_status(&f.db, hidden).await, None, "not emailed");
    let batch = pending_os_toasts(&f.db, carol, Some(start.watermark))
        .await
        .expect("pending");
    assert!(batch.items.is_empty(), "not toasted");
}
