//! Per-user signing, capture side (design v9 §9): every staged change records whose session made
//! it, the outbox never merges two makers, and a change made while nobody is signed in has none.

use sea_orm::DatabaseConnection;

use super::capture::{current_capture_actor, drain_capture_journal, set_capture_actor};
use super::test_support::{exec, started_device_db, RowFactory};

async fn makers(db: &DatabaseConnection, entity_sync_id: &str) -> Vec<(i64, Option<String>)> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.query_all(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT row_version, maker_user_sync_id FROM sync_outbox WHERE entity_sync_id = ? ORDER BY id",
        [entity_sync_id.into()],
    ))
    .await
    .expect("outbox")
    .iter()
    .map(|r| {
        (
            r.try_get::<i64>("", "row_version").expect("row_version"),
            r.try_get::<Option<String>>("", "maker_user_sync_id").expect("maker"),
        )
    })
    .collect()
}

async fn set_reference(db: &DatabaseConnection, rowid: i64, reference: &str) {
    exec(
        db,
        "UPDATE user_scope_assignments SET scope_type = 'tenant', scope_reference = ? WHERE id = ?",
        vec![reference.into(), rowid.into()],
    )
    .await;
}

#[tokio::test]
async fn only_the_session_user_is_the_maker_and_makers_never_merge() {
    let db = started_device_db().await;
    set_capture_actor(&db, None).await.expect("no actor");
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    drain_capture_journal(&db).await.expect("drain");
    // Made while nobody is signed in (startup seeders, background jobs): no maker, nobody signs.
    assert_eq!(makers(&db, &row.entity_sync_id).await, vec![(1, None)]);

    set_capture_actor(&db, Some("user-v")).await.expect("V signs in");
    assert_eq!(
        current_capture_actor(&db).await.expect("actor").as_deref(),
        Some("user-v")
    );
    set_reference(&db, row.rowid, "x").await;
    // V signs out and U signs in before any push: V's change is staged as V's first.
    set_capture_actor(&db, Some("user-u")).await.expect("U signs in");
    set_reference(&db, row.rowid, "y").await;
    set_capture_actor(&db, None).await.expect("U signs out");

    assert_eq!(
        makers(&db, &row.entity_sync_id).await,
        vec![
            (1, None),
            (2, Some("user-v".to_string())),
            (3, Some("user-u".to_string())),
        ]
    );
    assert_eq!(current_capture_actor(&db).await.expect("actor"), None);
}

/// Inbound rows are applied with capture suppressed: nothing is journaled, so no session user can
/// ever sign a row received from another PC.
#[tokio::test]
async fn suppressed_writes_are_not_captured_for_the_session_user() {
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    set_capture_actor(&db, Some("user-u")).await.expect("U signs in");
    exec(&db, "INSERT INTO sync_capture_suppression (id) VALUES (1)", vec![]).await;
    set_reference(&db, row.rowid, "z").await;
    exec(&db, "DELETE FROM sync_capture_suppression", vec![]).await;
    set_capture_actor(&db, None).await.expect("sign out");
    assert!(makers(&db, &row.entity_sync_id)
        .await
        .iter()
        .all(|(_, maker)| maker.as_deref() != Some("user-u")));
}
