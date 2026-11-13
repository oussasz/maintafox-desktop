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

async fn user_with_key(
    db: &DatabaseConnection,
    sync_id: &str,
    enrolled: bool,
) -> crate::auth::signing_keys::UnlockedSigningKey {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    exec(
        db,
        "INSERT INTO user_accounts (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin,
             force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version)
         VALUES (?, ?, 'local', 'x', 1, 0, 0, '2026-01-01T00:00:00Z', 0, '2026-01-01T00:00:00Z',
                 '2026-01-01T00:00:00Z', 1)",
        vec![sync_id.into(), format!("login-{sync_id}").into()],
    )
    .await;
    let user_id: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
            [sync_id.into()],
        ))
        .await
        .expect("select")
        .expect("user")
        .try_get("", "id")
        .expect("id");
    let key = crate::auth::signing_keys::unlock_or_create(db, user_id, sync_id, "pw-123456789", "$argon2id$h")
        .await
        .expect("key");
    if enrolled {
        crate::auth::signing_keys::mark_enrolled(db, key.key_id())
            .await
            .expect("enrolled");
    }
    key
}

async fn signatures(db: &DatabaseConnection, entity_sync_id: &str) -> Vec<Vec<super::transport::wire::WireSignature>> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.query_all(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT signatures_json FROM sync_outbox WHERE entity_sync_id = ? ORDER BY id",
        [entity_sync_id.into()],
    ))
    .await
    .expect("outbox")
    .iter()
    .map(|r| {
        r.try_get::<Option<String>>("", "signatures_json")
            .expect("signatures")
            .map(|s| serde_json::from_str(&s).expect("json"))
            .unwrap_or_default()
    })
    .collect()
}

async fn awaiting(db: &DatabaseConnection, entity_sync_id: &str) -> Vec<bool> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.query_all(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        format!(
            "SELECT {} AS held FROM sync_outbox o WHERE o.entity_sync_id = ? ORDER BY o.id",
            super::signing::AWAITING_MAKER_SIGNATURE_SQL
        ),
        [entity_sync_id.into()],
    ))
    .await
    .expect("awaiting")
    .iter()
    .map(|r| r.try_get::<bool>("", "held").expect("held"))
    .collect()
}

/// Only the maker's own enrolled key signs; another user's held key never signs it; an item
/// waiting for its maker holds back the later changes of the same record.
#[tokio::test]
async fn only_the_makers_key_signs_and_unsigned_items_wait() {
    use crate::auth::signing_keys::{change_signing_message, hold_session_key, release_session_keys, verify_signature};
    let db = started_device_db().await;
    let v_key = user_with_key(&db, "user-v", true).await;
    let u_key = user_with_key(&db, "user-u", true).await;
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    set_capture_actor(&db, Some("user-v")).await.expect("V");
    set_reference(&db, row.rowid, "x").await;
    set_capture_actor(&db, Some("user-u")).await.expect("U");
    set_reference(&db, row.rowid, "y").await;
    set_capture_actor(&db, None).await.expect("none");

    // U is signed in (V is not): U's key does not sign V's item, so V's and U's items wait.
    release_session_keys();
    hold_session_key(u_key);
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    let sigs = signatures(&db, &row.entity_sync_id).await;
    assert!(sigs[1].is_empty(), "V's change is not signed by U");
    assert_eq!(sigs[2][0].user_sync_id, "user-u");
    assert_eq!(awaiting(&db, &row.entity_sync_id).await, vec![false, true, true]);

    // V signs in: V's item is signed by V's key, and nothing waits any more.
    hold_session_key(v_key);
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    let sigs = signatures(&db, &row.entity_sync_id).await;
    assert_eq!(sigs[1].len(), 1);
    assert_eq!(sigs[1][0].user_sync_id, "user-v");
    assert_eq!(awaiting(&db, &row.entity_sync_id).await, vec![false, false, false]);

    // The signature verifies over the exact item (server rule §7).
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let item = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_type, operation, row_version, base_row_version, payload_json FROM sync_outbox
             WHERE entity_sync_id = ? ORDER BY id LIMIT 1 OFFSET 1",
            [row.entity_sync_id.clone().into()],
        ))
        .await
        .expect("item")
        .expect("row");
    let held = crate::auth::signing_keys::held_key("user-v").expect("held");
    let payload: String = item.try_get("", "payload_json").expect("payload");
    let message = change_signing_message(&crate::auth::signing_keys::ChangeSigningInput {
        tenant_id: "tenant-1",
        entity_type: &item.try_get::<String>("", "entity_type").expect("type"),
        entity_sync_id: &row.entity_sync_id,
        operation: &item.try_get::<String>("", "operation").expect("op"),
        row_version: item.try_get("", "row_version").expect("rv"),
        base_row_version: item.try_get("", "base_row_version").expect("base"),
        payload_json: &payload,
        signer_user_sync_id: "user-v",
        key_id: held.key_id(),
    });
    let sig = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &sigs[1][0].signature).expect("b64");
    assert!(verify_signature(held.public_key(), &message, &sig));
    release_session_keys();
}

/// A key not enrolled with the server yet never signs (the server would refuse the item).
#[tokio::test]
async fn an_unenrolled_key_does_not_sign() {
    let db = started_device_db().await;
    let key = user_with_key(&db, "user-n", false).await;
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    set_capture_actor(&db, Some("user-n")).await.expect("N");
    set_reference(&db, row.rowid, "x").await;
    set_capture_actor(&db, None).await.expect("none");

    crate::auth::signing_keys::hold_session_key(key);
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    assert!(signatures(&db, &row.entity_sync_id).await[1].is_empty());
    assert_eq!(awaiting(&db, &row.entity_sync_id).await[1], true);
}
