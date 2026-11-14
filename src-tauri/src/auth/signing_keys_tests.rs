//! Tests of `auth::signing_keys`: the cross-language vector shared with the sync server, key
//! wrapping, and the one-key-per-password rule.

use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

use super::*;

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(s).expect("base64")
}

fn vector() -> Value {
    let raw = include_str!("testdata/sync-signing-vector.json");
    serde_json::from_str(raw).expect("vector json")
}

fn change_input(v: &Value) -> ChangeSigningInput<'_> {
    ChangeSigningInput {
        tenant_id: v["tenant_id"].as_str().expect("tenant_id"),
        entity_type: v["entity_type"].as_str().expect("entity_type"),
        entity_sync_id: v["entity_sync_id"].as_str().expect("entity_sync_id"),
        operation: v["operation"].as_str().expect("operation"),
        row_version: v["row_version"].as_i64().expect("row_version"),
        base_row_version: v["base_row_version"].as_i64(),
        payload_json: v["payload_json"].as_str().expect("payload_json"),
        signer_user_sync_id: v["signer_user_sync_id"].as_str().expect("signer"),
        key_id: v["key_id"].as_str().expect("key_id"),
    }
}

/// The desktop builds byte-for-byte the messages the sync server verifies, and its Ed25519
/// signatures equal the server-side ones (vector generated independently with Node's crypto).
#[test]
fn shared_vector_messages_and_signatures_match() {
    let v = vector();
    let seed = hex::decode(v["seed_hex"].as_str().expect("seed")).expect("seed hex");
    let pair = Ed25519KeyPair::from_seed_unchecked(&seed).expect("seed key");
    let public_key = pair.public_key().as_ref().to_vec();
    assert_eq!(public_key, b64(v["public_key_b64"].as_str().expect("pub")));
    assert_eq!(key_id_of(&public_key), v["key_id"].as_str().expect("key_id"));

    for case in v["changes"].as_array().expect("changes") {
        let input = change_input(&case["input"]);
        let message = change_signing_message(&input);
        assert_eq!(
            hex::encode(&message),
            case["message_hex"].as_str().expect("message_hex")
        );
        let expected = b64(case["signature_b64"].as_str().expect("signature"));
        assert_eq!(pair.sign(&message).as_ref(), expected.as_slice());
        assert!(verify_signature(&public_key, &message, &expected));

        // Any other entity, version or payload byte breaks the signature (replay on another item).
        let other_version = ChangeSigningInput {
            row_version: input.row_version + 1,
            ..input
        };
        assert!(!verify_signature(
            &public_key,
            &change_signing_message(&other_version),
            &expected
        ));
        let other_tenant = ChangeSigningInput {
            tenant_id: "tenant-0002",
            ..input
        };
        assert!(!verify_signature(
            &public_key,
            &change_signing_message(&other_tenant),
            &expected
        ));
        let tampered = format!("{} ", input.payload_json);
        let other_payload = ChangeSigningInput {
            payload_json: &tampered,
            ..input
        };
        assert!(!verify_signature(
            &public_key,
            &change_signing_message(&other_payload),
            &expected
        ));
    }

    let enrol = &v["enrol"];
    let i = &enrol["input"];
    let message = enrol_proof_message(
        i["tenant_id"].as_str().expect("tenant"),
        i["user_sync_id"].as_str().expect("user"),
        i["key_id"].as_str().expect("key"),
        i["machine_fingerprint"].as_str().expect("fp"),
    );
    assert_eq!(
        hex::encode(&message),
        enrol["message_hex"].as_str().expect("message_hex")
    );
    assert_eq!(
        pair.sign(&message).as_ref(),
        b64(enrol["signature_b64"].as_str().expect("signature")).as_slice()
    );
}

/// A length prefix keeps one field from bleeding into the next.
#[test]
fn encoding_is_length_prefixed() {
    assert_ne!(encode_signed("t", &["ab", "c"]), encode_signed("t", &["a", "bc"]));
    assert_eq!(encode_signed("t", &[""]), vec![0, 0, 0, 1, b't', 0, 0, 0, 0]);
}

#[test]
fn wrapped_key_opens_only_with_its_password_and_owner() {
    let (key, wrapped) = generate_wrapped("Correct-horse-1", "user-a").expect("generate");
    let opened = unwrap_key("Correct-horse-1", "user-a", key.key_id(), key.public_key(), &wrapped).expect("unwrap");
    assert_eq!(opened.key_id(), key.key_id());
    assert!(unwrap_key("wrong", "user-a", key.key_id(), key.public_key(), &wrapped).is_err());
    // The wrap is bound to its user and key id: a row copied to another user does not open.
    assert!(unwrap_key("Correct-horse-1", "user-b", key.key_id(), key.public_key(), &wrapped).is_err());
    let mut altered = wrapped.clone();
    altered.ciphertext[0] ^= 1;
    assert!(unwrap_key("Correct-horse-1", "user-a", key.key_id(), key.public_key(), &altered).is_err());
    assert!(!format!("{key:?}").contains("pkcs8"));
}

#[test]
fn a_key_signs_only_as_its_own_user() {
    let (key, _) = generate_wrapped("pw-123456789", "user-a").expect("generate");
    let payload = r#"{"a":1}"#;
    let mine = ChangeSigningInput {
        tenant_id: "t",
        entity_type: "work_permits",
        entity_sync_id: "e",
        operation: "update",
        row_version: 2,
        base_row_version: Some(1),
        payload_json: payload,
        signer_user_sync_id: "user-a",
        key_id: key.key_id(),
    };
    let sig = key.sign_change(&mine).expect("sign");
    assert!(verify_signature(
        key.public_key(),
        &change_signing_message(&mine),
        &b64(&sig)
    ));
    let forged = ChangeSigningInput {
        signer_user_sync_id: "user-b",
        ..mine
    };
    assert!(key.sign_change(&forged).is_err());
}

async fn insert_user(db: &impl ConnectionTrait, sync_id: &str) -> i64 {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO user_accounts \
         (entity_sync_id, username, identity_mode, password_hash, is_active, is_admin, \
          force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version) \
         VALUES (?, ?, 'local', 'x', 1, 0, 0, '2026-01-01T00:00:00Z', 0, '2026-01-01T00:00:00Z', \
                 '2026-01-01T00:00:00Z', 1)",
        vec![sync_id.into(), format!("u-{sync_id}").into()],
    ))
    .await
    .expect("insert user");
    db.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
        vec![sync_id.into()],
    ))
    .await
    .expect("select")
    .expect("user row")
    .try_get("", "id")
    .expect("id")
}

/// Same password ⇒ same key; a password change anywhere (new hash) ⇒ a new key, the old one retired.
#[tokio::test]
async fn one_key_per_password() {
    let db = crate::db::test_db::migrated().await;
    let user_id = insert_user(&db, "user-k").await;
    let first = unlock_or_create(&db, user_id, "user-k", "pw-one-123", "$argon2id$hash-1")
        .await
        .expect("create");
    let again = unlock_or_create(&db, user_id, "user-k", "pw-one-123", "$argon2id$hash-1")
        .await
        .expect("unlock");
    assert_eq!(first.key_id(), again.key_id());

    let rotated = unlock_or_create(&db, user_id, "user-k", "pw-two-456", "$argon2id$hash-2")
        .await
        .expect("rotate");
    assert_ne!(rotated.key_id(), first.key_id());
    let current = current_local_key(&db, user_id).await.expect("read").expect("current");
    assert_eq!(current.key_id, rotated.key_id());
    let retired: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM user_signing_keys_local WHERE user_id = ? AND retired_at IS NOT NULL",
            vec![user_id.into()],
        ))
        .await
        .expect("count")
        .expect("row")
        .try_get("", "n")
        .expect("n");
    assert_eq!(retired, 1);

    mark_enrolled(&db, rotated.key_id()).await.expect("enrol");
    let enrolled = current_local_key(&db, user_id).await.expect("read").expect("current");
    assert!(enrolled.enrolled_at.is_some());
}
