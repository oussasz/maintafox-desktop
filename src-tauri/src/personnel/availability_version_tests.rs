//! AU-63: an availability block is updated, ended and deleted only at the version the screen read;
//! a change made since, here or received by sync, is refused (`GATE_RECORD_CHANGED_ELSEWHERE`).

use crate::errors::AppError;
use crate::personnel::absence_access::AbsenceAccess;
use crate::personnel::availability::{self, AvailabilityBlockUpdateInput};
use crate::sync::test_support::{exec, scalar_i64, started_db, RowFactory, TestServer};

fn assert_changed_elsewhere(result: Result<impl std::fmt::Debug, AppError>) {
    match result {
        Err(AppError::ValidationFailed(messages)) => assert!(
            messages[0].starts_with("GATE_RECORD_CHANGED_ELSEWHERE:"),
            "{messages:?}"
        ),
        other => panic!("expected a stale-version refusal, got {other:?}"),
    }
}

fn edit(id: i64, note: &str, expected_row_version: i64) -> AvailabilityBlockUpdateInput {
    AvailabilityBlockUpdateInput {
        id,
        block_type: None,
        start_at: "2030-03-02T08:00:00Z".into(),
        end_at: "2030-03-02T16:00:00Z".into(),
        reason_note: Some(note.into()),
        keep_reason_note: false,
        is_critical: None,
        confirm_overlap: Some(true),
        expected_row_version,
    }
}

async fn block_on(db: &sea_orm::DatabaseConnection, rows: &mut RowFactory) -> (i64, String) {
    let created = rows.create(db, "personnel_availability_blocks").await.expect("absence");
    (created.rowid, created.entity_sync_id)
}

async fn version(db: &sea_orm::DatabaseConnection, id: i64) -> i64 {
    scalar_i64(
        db,
        "SELECT row_version FROM personnel_availability_blocks WHERE id = ?",
        vec![id.into()],
    )
    .await
}

#[tokio::test]
async fn an_edit_from_a_stale_screen_is_refused_and_changes_nothing() {
    let db = started_db().await;
    let (id, _) = block_on(&db, &mut RowFactory::new()).await;
    let read = version(&db, id).await;

    let saved =
        availability::update_availability_block(&db, edit(id, "first", read), 1, &AbsenceAccess::full_for_tests())
            .await
            .expect("edit at the version read");
    assert_eq!(saved.row_version, read + 1, "a saved edit raises the version");
    assert_eq!(saved.reason_note.as_deref(), Some("first"));

    // A second screen opened before that edit.
    assert_changed_elsewhere(
        availability::update_availability_block(&db, edit(id, "second", read), 1, &AbsenceAccess::full_for_tests())
            .await,
    );
    assert_eq!(version(&db, id).await, read + 1);
    assert_eq!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM personnel_availability_blocks WHERE id = ? AND reason_note = 'first'",
            vec![id.into()]
        )
        .await,
        1,
        "the first edit is kept"
    );
}

#[tokio::test]
async fn ending_or_deleting_from_a_stale_screen_is_refused() {
    let db = started_db().await;
    let (id, _) = block_on(&db, &mut RowFactory::new()).await;
    // Active now, so it can be ended.
    exec(
        &db,
        "UPDATE personnel_availability_blocks SET start_at = '2000-01-01T00:00:00Z', end_at = '2100-01-01T00:00:00Z' \
         WHERE id = ?",
        vec![id.into()],
    )
    .await;
    let read = version(&db, id).await;

    assert_changed_elsewhere(availability::end_availability_block(&db, id, read - 1).await);
    assert_changed_elsewhere(availability::delete_availability_block(&db, id, read + 1).await);
    assert_eq!(version(&db, id).await, read, "nothing changed");

    let ended = availability::end_availability_block(&db, id, read)
        .await
        .expect("end at the version read");
    assert_eq!(ended.row_version, read + 1);
    assert_ne!(ended.end_at, "2100-01-01T00:00:00Z");

    // The screen that ended it still holds the old version: the deletion is refused.
    assert_changed_elsewhere(availability::delete_availability_block(&db, id, read).await);
    availability::delete_availability_block(&db, id, read + 1)
        .await
        .expect("delete at the current version");
    match availability::delete_availability_block(&db, id, read + 1).await {
        Err(AppError::NotFound { .. }) => {}
        other => panic!("a block deleted elsewhere is not found, got {other:?}"),
    }
}

/// A change received by sync raises the version on the receiving PC: a screen opened there before
/// it arrived is refused.
#[tokio::test]
async fn a_change_received_by_sync_makes_an_open_screen_stale() {
    let a = started_db().await;
    let b = started_db().await;
    let mut server = TestServer::new();
    let (id_a, sync_id) = block_on(&a, &mut RowFactory::for_device(0)).await;
    server.exchange("a", &a).await;
    server.exchange("b", &b).await;
    let id_b = scalar_i64(
        &b,
        "SELECT id FROM personnel_availability_blocks WHERE entity_sync_id = ?",
        vec![sync_id.into()],
    )
    .await;
    let read_on_b = version(&b, id_b).await;

    let read_on_a = version(&a, id_a).await;
    availability::update_availability_block(
        &a,
        edit(id_a, "changed on A", read_on_a),
        1,
        &AbsenceAccess::full_for_tests(),
    )
    .await
    .expect("edit on A");
    server.exchange("a", &a).await;
    server.exchange("b", &b).await;
    assert!(
        version(&b, id_b).await > read_on_b,
        "the received change raised the version on B"
    );

    assert_changed_elsewhere(
        availability::update_availability_block(
            &b,
            edit(id_b, "stale on B", read_on_b),
            1,
            &AbsenceAccess::full_for_tests(),
        )
        .await,
    );
    assert_eq!(
        scalar_i64(
            &b,
            "SELECT COUNT(*) FROM personnel_availability_blocks WHERE id = ? AND reason_note = 'changed on A'",
            vec![id_b.into()]
        )
        .await,
        1,
        "the change received from A is kept"
    );
}
