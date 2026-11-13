//! Per-user signing, capture side (design v9 §9): every staged change records whose session made
//! it, the outbox never merges two makers, and a change made while nobody is signed in has none.

use chrono::{TimeDelta, Utc};
use sea_orm::DatabaseConnection;

use super::capture::{current_capture_actor, drain_capture_journal, set_capture_actor, CaptureActor};
use super::test_support::{exec, started_device_db, RowFactory};

/// Local account id used for a test user sync id (the stored session row names it).
fn local_id(user_sync_id: &str) -> i64 {
    match user_sync_id {
        "user-v" => 101,
        "user-u" => 102,
        "user-n" => 103,
        "user-b" => 104,
        _ => 199,
    }
}

/// `user_sync_id` signs in on this PC: an open stored session, and capture for that user.
async fn sign_in(db: &DatabaseConnection, user_sync_id: &str) {
    sign_in_until(db, user_sync_id, Utc::now() + TimeDelta::hours(1)).await;
}

async fn sign_in_until(db: &DatabaseConnection, user_sync_id: &str, expires_at: chrono::DateTime<Utc>) {
    let user_id = local_id(user_sync_id);
    exec(
        db,
        "INSERT OR REPLACE INTO app_sessions (id, user_id, created_at, expires_at, last_activity_at, is_revoked)
         VALUES (?, ?, '2026-01-01T00:00:00Z', ?, NULL, 0)",
        vec![
            format!("session-{user_id}").into(),
            user_id.to_string().into(),
            expires_at.to_rfc3339().into(),
        ],
    )
    .await;
    set_capture_actor(
        db,
        Some(&CaptureActor {
            user_sync_id,
            user_id,
            session_id: &format!("session-{user_id}"),
            expires_at,
        }),
    )
    .await
    .expect("sign in");
}

fn session_end() -> chrono::DateTime<Utc> {
    Utc::now() + TimeDelta::hours(1)
}

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

    sign_in(&db, "user-v").await;
    assert_eq!(
        current_capture_actor(&db).await.expect("actor").as_deref(),
        Some("user-v")
    );
    set_reference(&db, row.rowid, "x").await;
    // V signs out and U signs in before any push: V's change is staged as V's first.
    sign_in(&db, "user-u").await;
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
    sign_in(&db, "user-u").await;
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
    sign_in(&db, "user-v").await;
    set_reference(&db, row.rowid, "x").await;
    sign_in(&db, "user-u").await;
    set_reference(&db, row.rowid, "y").await;
    set_capture_actor(&db, None).await.expect("none");

    // U is signed in (V is not): U's key does not sign V's item, so V's and U's items wait.
    release_session_keys();
    hold_session_key(u_key, session_end());
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    let sigs = signatures(&db, &row.entity_sync_id).await;
    assert!(sigs[1].is_empty(), "V's change is not signed by U");
    assert_eq!(sigs[2][0].user_sync_id, "user-u");
    assert_eq!(awaiting(&db, &row.entity_sync_id).await, vec![false, true, true]);

    // V signs in: V's item is signed by V's key, and nothing waits any more.
    hold_session_key(v_key, session_end());
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
    sign_in(&db, "user-n").await;
    set_reference(&db, row.rowid, "x").await;
    set_capture_actor(&db, None).await.expect("none");

    crate::auth::signing_keys::hold_session_key(key, session_end());
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    assert!(signatures(&db, &row.entity_sync_id).await[1].is_empty());
    assert_eq!(awaiting(&db, &row.entity_sync_id).await[1], true);
}

/// A session that expired or was closed without a sign-out (expiry, deactivation, password reset
/// elsewhere, a clear that failed) makes nobody the maker of later changes (review F1).
#[tokio::test]
async fn an_expired_or_closed_session_captures_nobody() {
    let db = started_device_db().await;
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");

    sign_in(&db, "user-u").await;
    exec(&db, "UPDATE app_sessions SET is_revoked = 1", vec![]).await;
    assert_eq!(current_capture_actor(&db).await.expect("actor"), None);
    set_reference(&db, row.rowid, "closed").await;

    sign_in_until(&db, "user-u", Utc::now() - TimeDelta::minutes(1)).await;
    assert_eq!(current_capture_actor(&db).await.expect("actor"), None);
    set_reference(&db, row.rowid, "expired").await;
    drain_capture_journal(&db).await.expect("drain");

    assert!(makers(&db, &row.entity_sync_id)
        .await
        .iter()
        .all(|(_, maker)| maker.is_none()));
}

/// A held key past its session's expiry never signs (review F1).
#[tokio::test]
async fn a_key_past_its_session_expiry_does_not_sign() {
    let db = started_device_db().await;
    let key = user_with_key(&db, "user-u", true).await;
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    sign_in(&db, "user-u").await;
    set_reference(&db, row.rowid, "x").await;
    set_capture_actor(&db, None).await.expect("none");

    crate::auth::signing_keys::hold_session_key(key, Utc::now() - TimeDelta::seconds(1));
    assert!(crate::auth::signing_keys::held_key("user-u").is_none());
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    assert!(signatures(&db, &row.entity_sync_id).await[1].is_empty());
    assert!(awaiting(&db, &row.entity_sync_id).await[1]);
}

/// When the action commits, the second signer's key signs right away exactly the items of that
/// record by that maker staged after their password was checked and naming them; the item is
/// then sent once the maker signed and the second signer's key is enrolled (review F2, review of
/// 5feb40ade: the co-signature no longer depends on the second signer staying here).
#[tokio::test]
async fn a_committed_action_is_cosigned_at_once_and_only_its_item() {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let db = started_device_db().await;
    let v_key = user_with_key(&db, "user-v", true).await;
    let b_key = user_with_key(&db, "user-b", true).await;
    let b_local: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE entity_sync_id = 'user-b'",
            [],
        ))
        .await
        .expect("select")
        .expect("user")
        .try_get("", "id")
        .expect("id");
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    sign_in(&db, "user-v").await;
    // An earlier change of V, staged before B typed the password: never co-signed by B.
    set_reference(&db, row.rowid, "before").await;
    drain_capture_journal(&db).await.expect("drain");
    let after: i64 = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT MAX(id) AS m FROM sync_outbox".to_string(),
        ))
        .await
        .expect("select")
        .expect("row")
        .try_get("", "m")
        .expect("max");
    // The action: V records B as the approver.
    exec(
        &db,
        "UPDATE user_scope_assignments SET assigned_by_id = ? WHERE id = ?",
        vec![b_local.into(), row.rowid.into()],
    )
    .await;
    let grant = super::signing::CosignGrant {
        entity_type: "user_scope_assignments".to_string(),
        entity_sync_id: row.entity_sync_id.clone(),
        maker_user_sync_id: "user-v".to_string(),
    };
    let n = super::signing::cosign_committed(&db, &b_key, &grant, after, "tenant-1")
        .await
        .expect("cosign");
    assert_eq!(n, 1);
    let sigs = signatures(&db, &row.entity_sync_id).await;
    assert!(
        sigs[1].iter().all(|s| s.user_sync_id != "user-b"),
        "earlier item not co-signed"
    );
    assert_eq!(sigs.last().expect("item")[0].user_sync_id, "user-b");

    // The maker signs at the push; B's key is enrolled: nothing waits.
    crate::auth::signing_keys::hold_session_key(v_key, session_end());
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    assert!(!*awaiting(&db, &row.entity_sync_id).await.last().expect("item"));
    crate::auth::signing_keys::release_session_keys();
}

/// A staged second signer's key is taken once, and never after the session ends.
#[tokio::test]
async fn a_staged_cosigner_key_is_taken_once() {
    use crate::auth::signing_keys::{hold_cosigner_key, release_session_keys, take_cosigner_key};
    let db = started_device_db().await;
    let key = user_with_key(&db, "user-b", true).await;
    hold_cosigner_key(key, 7);
    let (_, after) = take_cosigner_key("user-b").expect("staged");
    assert_eq!(after, 7);
    assert!(take_cosigner_key("user-b").is_none(), "taken once");
    let again = user_with_key(&db, "user-n", true).await;
    hold_cosigner_key(again, 0);
    release_session_keys();
    assert!(take_cosigner_key("user-n").is_none(), "released with the session");
}

/// An item that sets or changes an author field naming someone other than its maker is held until
/// that person signed it; an unchanged author field named in the payload is not (review F3).
#[test]
fn required_cosigners_follow_set_or_changed_author_fields() {
    use super::signing::required_cosigners;
    let before = r#"{"assigned_by_sync_id":"user-x","scope_reference":"a"}"#;
    // Unchanged author (made by someone earlier): nobody else must sign.
    let edit = r#"{"assigned_by_sync_id":"user-x","scope_reference":"b"}"#;
    assert!(required_cosigners("user_scope_assignments", edit, Some(before), "user-v").is_empty());
    // Changed to another person: that person must sign.
    let other = r#"{"assigned_by_sync_id":"user-b","scope_reference":"a"}"#;
    assert_eq!(
        required_cosigners("user_scope_assignments", other, Some(before), "user-v"),
        vec!["user-b".to_string()]
    );
    // Changed to the maker: the maker's own signature is enough.
    let own = r#"{"assigned_by_sync_id":"user-v","scope_reference":"a"}"#;
    assert!(required_cosigners("user_scope_assignments", own, Some(before), "user-v").is_empty());
    // New to the server: every author it names is set.
    assert_eq!(
        required_cosigners("user_scope_assignments", before, None, "user-v"),
        vec!["user-x".to_string()]
    );
}

/// The push holds back an item naming a second signer who has not signed it, even when its maker
/// did, and the later changes of the same record with it (review F3).
#[tokio::test]
async fn an_item_naming_an_unsigned_second_person_is_held() {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let db = started_device_db().await;
    let v_key = user_with_key(&db, "user-v", true).await;
    let _b_key = user_with_key(&db, "user-b", true).await;
    let b_local: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE entity_sync_id = 'user-b'",
            [],
        ))
        .await
        .expect("select")
        .expect("user")
        .try_get("", "id")
        .expect("id");
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    sign_in(&db, "user-v").await;
    exec(
        &db,
        "UPDATE user_scope_assignments SET assigned_by_id = ? WHERE id = ?",
        vec![b_local.into(), row.rowid.into()],
    )
    .await;
    set_reference(&db, row.rowid, "later").await;
    drain_capture_journal(&db).await.expect("drain");

    crate::auth::signing_keys::hold_session_key(v_key, session_end());
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    let sigs = signatures(&db, &row.entity_sync_id).await;
    assert_eq!(sigs.last().expect("item")[0].user_sync_id, "user-v");
    assert!(
        *awaiting(&db, &row.entity_sync_id).await.last().expect("item"),
        "B named, not signed: held"
    );
    crate::auth::signing_keys::release_session_keys();
}

/// A rewritten unsent item loses its signatures and is judged again (review F7).
#[tokio::test]
async fn a_rewritten_item_loses_its_signatures() {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let db = started_device_db().await;
    let key = user_with_key(&db, "user-u", true).await;
    let mut factory = RowFactory::new();
    let row = factory.create(&db, "user_scope_assignments").await.expect("row");
    sign_in(&db, "user-u").await;
    set_reference(&db, row.rowid, "x").await;
    drain_capture_journal(&db).await.expect("drain");
    crate::auth::signing_keys::hold_session_key(key, session_end());
    super::signing::sign_pending_items(&db, "tenant-1").await.expect("sign");
    assert!(!signatures(&db, &row.entity_sync_id).await[1].is_empty());

    exec(
        &db,
        "UPDATE sync_outbox SET payload_json = '{}', payload_hash = 'rewritten' WHERE entity_sync_id = ? AND maker_user_sync_id = 'user-u'",
        vec![row.entity_sync_id.clone().into()],
    )
    .await;
    let cleared = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT signatures_json IS NULL AND awaiting_signers_json IS NULL AS cleared FROM sync_outbox
             WHERE entity_sync_id = ? AND maker_user_sync_id = 'user-u'",
            [row.entity_sync_id.clone().into()],
        ))
        .await
        .expect("select")
        .expect("row")
        .try_get::<bool>("", "cleared")
        .expect("cleared");
    assert!(cleared);
    crate::auth::signing_keys::release_session_keys();
}
