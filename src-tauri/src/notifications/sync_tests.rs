//! Two devices: the notifications each one generates from synchronised data are the same rows,
//! and what the recipient does on one device (acknowledge, archive) reaches the other.

use sea_orm::DatabaseConnection;

use crate::notifications::{delivery, scheduler};
use crate::sync::test_support::{exec, scalar_i64, scalar_opt_text, started_device_db, RowFactory, TestServer};

/// The seeded administrator: same identity on every device.
const ADMIN: &str = "SELECT id FROM user_accounts ORDER BY id LIMIT 1";

async fn trust_admin(db: &DatabaseConnection, device: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO trusted_devices (id, device_fingerprint, user_id, trusted_at, is_revoked)
             VALUES (?, ?, ({ADMIN}), '2026-10-11T00:00:00Z', 0)"
        ),
        vec![format!("nt-trust-{device}").into(), format!("nt-fp-{device}").into()],
    )
    .await;
}

/// The admin's `wo_assigned` notification for the order with sync id `wo_sid`, if any.
async fn assignment_of(db: &DatabaseConnection, wo_sid: &str) -> Option<String> {
    scalar_opt_text(
        db,
        &format!(
            "SELECT n.entity_sync_id FROM notifications n
             JOIN notification_events e ON e.id = n.notification_event_id
             JOIN work_orders wo ON wo.id = CAST(e.source_record_id AS INTEGER)
             WHERE e.category_code = 'wo_assigned' AND wo.entity_sync_id = ? AND n.recipient_user_id = ({ADMIN})"
        ),
        vec![wo_sid.into()],
    )
    .await
}

#[tokio::test]
async fn an_acknowledgement_made_on_one_device_reaches_the_same_notification_on_the_other() {
    let a = started_device_db().await;
    let b = started_device_db().await;
    let mut server = TestServer::new();
    trust_admin(&a, "a").await;
    trust_admin(&b, "b").await;
    // Both devices watch from now on.
    scheduler::run_scheduler_tick_for_test(&a).await.expect("tick a");
    scheduler::run_scheduler_tick_for_test(&b).await.expect("tick b");
    for db in [&a, &b] {
        exec(
            db,
            "UPDATE notification_watch_state SET watching_since = '2000-01-01T00:00:00Z'",
            vec![],
        )
        .await;
    }

    // A work order assigned to the administrator on device A, synchronised to B.
    let mut factory = RowFactory::for_device(1);
    let wo = factory.create(&a, "work_orders").await.expect("work order");
    exec(
        &a,
        &format!(
            "UPDATE work_orders SET primary_responsible_id = ({ADMIN}), planned_end = NULL,
                status_id = (SELECT id FROM work_order_statuses WHERE code = 'ready'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE rowid = ?"
        ),
        vec![wo.rowid.into()],
    )
    .await;
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM work_orders WHERE entity_sync_id = ?",
            vec![wo.entity_sync_id.clone().into()]
        )
        .await,
        1,
        "B has the order"
    );

    // Each device generates the notification itself: same identity, nothing sent.
    scheduler::run_scheduler_tick_for_test(&a).await.expect("tick a");
    scheduler::run_scheduler_tick_for_test(&b).await.expect("tick b");
    let on_a = assignment_of(&a, &wo.entity_sync_id).await.expect("notification on A");
    let on_b = assignment_of(&b, &wo.entity_sync_id).await.expect("notification on B");
    assert_eq!(on_a, on_b, "same notification on both devices");

    // The administrator acknowledges and archives it on A.
    let admin_a = scalar_i64(&a, ADMIN, vec![]).await;
    let id_a = scalar_i64(
        &a,
        "SELECT id FROM notifications WHERE entity_sync_id = ?",
        vec![on_a.clone().into()],
    )
    .await;
    delivery::acknowledge(&a, id_a, admin_a, Some("on it".into()))
        .await
        .expect("acknowledge");
    delivery::set_archived(&a, id_a, admin_a, true).await.expect("archive");
    server.exchange("A", &a).await;
    let received = server.exchange("B", &b).await;
    assert!(received.received > 0, "B received the change");

    let acknowledged = scalar_opt_text(
        &b,
        "SELECT acknowledged_at FROM notifications WHERE entity_sync_id = ?",
        vec![on_b.clone().into()],
    )
    .await;
    assert!(acknowledged.is_some(), "acknowledgement reached B");
    let archived = scalar_opt_text(
        &b,
        "SELECT archived_at FROM notifications WHERE entity_sync_id = ?",
        vec![on_b.clone().into()],
    )
    .await;
    assert!(archived.is_some(), "archive reached B");
    let acks = scalar_i64(
        &b,
        "SELECT COUNT(*) FROM notification_acknowledgements k JOIN notifications n ON n.id = k.notification_id
         WHERE n.entity_sync_id = ?",
        vec![on_b.into()],
    )
    .await;
    assert_eq!(acks, 1, "the acknowledgement record is there too");
}
