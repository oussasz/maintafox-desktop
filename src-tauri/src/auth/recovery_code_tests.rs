//! Vendor recovery codes: the cross-language vector of the vendor console, each refusal, and the
//! redemption (one new lasting administrator, code spent, audit, notices) with its guards.

use base64::Engine as _;
use chrono::{DateTime, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;
use crate::commands::product_license::{bootstrap_initial_tenant_admin_in, BootstrapInitialAdminInput};

const VECTOR: &str = include_str!("testdata/admin_recovery_vector.json");
const TENANT: &str = "ten_vector_0001";
const LICENSE: &str = "lic_vector_0001";
const FINGERPRINT: &str = "fp-vector-9c41e7b2a05d";
const PASSWORD: &str = "Chef-Usine-2026!Sûr";

fn vector() -> Value {
    serde_json::from_str(VECTOR).expect("vector json")
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("instant")
        .with_timezone(&Utc)
}

/// Within the primary code's validity window.
fn primary_now() -> DateTime<Utc> {
    instant("2026-10-19T09:00:00.000Z")
}

fn primary_code() -> String {
    vector()["code"].as_str().expect("code").to_string()
}

/// TEST-ONLY key: the public seed of `entitlements::test_support` and of the vendor console's
/// `api/scripts/lib/test-signing-key.mjs` (checked against the vector's public key below).
fn test_key_pair() -> Ed25519KeyPair {
    let seed = Sha256::digest(b"maintafox-entitlement-TEST-ONLY-seed-v1");
    Ed25519KeyPair::from_seed_unchecked(&seed).expect("test seed")
}

/// A code for `payload_json`, signed with the test key over the recovery domain.
fn sign(payload_json: &str) -> String {
    let signature = test_key_pair().sign(&signed_message(payload_json));
    format!(
        "{CODE_PREFIX}.{}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json.as_bytes()),
        hex::encode(signature.as_ref())
    )
}

/// The primary payload with `edit` applied, re-serialised canonically and signed.
fn signed_variant(edit: impl FnOnce(&mut RecoveryPayload)) -> String {
    let mut payload = decode_code(&primary_code()).expect("primary decodes").payload;
    edit(&mut payload);
    sign(&canonical_payload(&payload))
}

async fn set_claim(db: &DatabaseConnection, tenant: &str, fingerprint: &str, license: &str) {
    // Same record as `product_license::record_activation_claim_for_tests`, with this licence id.
    let record = serde_json::json!({
        "schema_version": 1,
        "key_sha256": "test",
        "submitted_at": Utc::now().to_rfc3339(),
        "status": "active",
        "pending_online_validation": false,
        "activation_claim": {
            "tenant_id": tenant,
            "license_id": license,
            "machine_fingerprint": fingerprint,
            "activation_token": "token-1",
        },
        "reconciliation": { "retry_attempt": 0 },
        "diagnostics": [],
    });
    crate::settings::set_setting(
        db,
        "product.license_onboarding",
        "device",
        &record.to_string(),
        None,
        "test",
    )
    .await
    .expect("claim");
    let stored = crate::commands::product_license::get_activation_claim_record(db)
        .await
        .expect("read claim")
        .expect("claim stored");
    assert_eq!(stored.machine_fingerprint, fingerprint);
}

/// Seeded PC with the pinned keys registered, no claim.
async fn bare_pc() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    crate::license::security::ensure_default_licensing_trust_keys(&db)
        .await
        .expect("trust keys");
    db
}

/// Seeded PC activated for the vector's organisation, machine and licence.
async fn activated_pc() -> DatabaseConnection {
    let db = bare_pc().await;
    set_claim(&db, TENANT, FINGERPRINT, LICENSE).await;
    db
}

/// Activated PC whose first administrator "chef" was created at first start.
async fn administered_pc() -> DatabaseConnection {
    let db = activated_pc().await;
    bootstrap_initial_tenant_admin_in(
        &db,
        BootstrapInitialAdminInput {
            username: "chef".into(),
            email: "chef@plant.example".into(),
            password: PASSWORD.into(),
            display_name: Some("Chef".into()),
        },
    )
    .await
    .expect("first administrator");
    db
}

async fn verify(db: &DatabaseConnection, code: &str, now: DateTime<Utc>) -> Result<RecoveryPayload, Refusal> {
    verify_code_in(db, code, now).await.expect("database")
}

fn reason(result: &Result<RecoveryPayload, Refusal>) -> &'static str {
    match result {
        Ok(_) => "valid",
        Err(refusal) => refusal.as_str(),
    }
}

fn code_of(err: &AppError) -> Option<&str> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.as_str()),
        _ => None,
    }
}

fn input(code: &str, username: &str) -> RedeemAdminRecoveryCodeInput {
    RedeemAdminRecoveryCodeInput {
        code: code.into(),
        username: username.into(),
        display_name: "Responsable Maintenance".into(),
        password: PASSWORD.into(),
        password_confirmation: PASSWORD.into(),
    }
}

async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await
        .expect("query")
        .and_then(|r| r.try_get::<i64>("", "n").ok())
        .expect("row")
}

async fn user_id(db: &DatabaseConnection, username: &str) -> i64 {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT id FROM user_accounts WHERE username = ?",
        [username.into()],
    ))
    .await
    .expect("query")
    .and_then(|r| r.try_get::<i64>("", "id").ok())
    .expect("user")
}

/// Every account row as text, to prove none changed.
async fn accounts_snapshot(db: &DatabaseConnection) -> Vec<String> {
    db.query_all(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT id || '|' || username || '|' || COALESCE(password_hash, '') || '|' || is_active || '|' \
         || COALESCE(deleted_at, '') || '|' || force_password_change || '|' || row_version AS line \
         FROM user_accounts ORDER BY id"
            .to_string(),
    ))
    .await
    .expect("accounts")
    .iter()
    .map(|r| r.try_get::<String>("", "line").expect("line"))
    .collect()
}

// ── The vendor console's vector ──────────────────────────────────────────────────────────────────

#[test]
fn vector_key_is_the_test_key_and_bytes_match() {
    let v = vector();
    assert_eq!(
        hex::encode(test_key_pair().public_key().as_ref()),
        v["public_key_hex"].as_str().expect("public key")
    );
    assert_eq!(
        crate::entitlements::trusted_keys::TEST_ENTITLEMENT_KEY_ID,
        v["key_id"].as_str().expect("key id")
    );
    assert_eq!(SIGNING_DOMAIN, v["signing_domain"].as_str().expect("domain"));
    let decoded = decode_code(&primary_code()).expect("primary decodes");
    assert_eq!(decoded.payload_json, v["payload_json"].as_str().expect("payload json"));
    assert_eq!(canonical_payload(&decoded.payload), decoded.payload_json);
    assert_eq!(
        hex::encode(signed_message(&decoded.payload_json)),
        v["signed_message_hex"].as_str().expect("message hex")
    );
    assert_eq!(decoded.signature_hex, v["signature_hex"].as_str().expect("signature"));
    // Our signer reproduces the vendor's code byte for byte (Ed25519 is deterministic).
    assert_eq!(sign(&decoded.payload_json), primary_code());
}

#[tokio::test]
async fn vector_primary_code_is_valid_and_tampered_is_refused() {
    let db = activated_pc().await;
    let v = vector();
    let payload = verify(&db, &primary_code(), primary_now()).await.expect("valid");
    assert_eq!(payload.code_id, "3f6c1d2e-9a4b-4c7d-8e1f-2a3b4c5d6e7f");

    let tampered = v["tampered"]["code"].as_str().expect("tampered");
    assert_eq!(
        reason(&verify(&db, tampered, primary_now()).await),
        v["tampered"]["expected"].as_str().expect("expected")
    );
}

#[tokio::test]
async fn vector_extra_cases_each_give_their_result() {
    let v = vector();
    let cases = v["extra_cases"].as_array().expect("extra cases");
    assert_eq!(cases.len(), 6);
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let payload: Value = serde_json::from_str(case["payload_json"].as_str().expect("payload")).expect("payload");
        // This PC's stored claim names what the case's payload names.
        let db = bare_pc().await;
        set_claim(
            &db,
            payload["tenant_id"].as_str().expect("tenant"),
            payload["machine_fingerprint"].as_str().expect("fingerprint"),
            payload["license_id"].as_str().expect("licence"),
        )
        .await;
        let now = instant(case["now"].as_str().expect("now"));
        let result = verify(&db, case["code"].as_str().expect("code"), now).await;
        assert_eq!(
            reason(&result),
            case["expected"].as_str().expect("expected"),
            "case {name}"
        );
    }
}

#[test]
fn refusal_names_cover_the_vectors_refusal_codes() {
    let v = vector();
    let known = [
        Refusal::InvalidSignature,
        Refusal::NonCanonical,
        Refusal::WrongPurpose,
        Refusal::Expired,
        Refusal::IssuedInFuture,
    ];
    for name in v["refusal_codes"].as_object().expect("codes").keys() {
        assert!(
            name == "valid" || known.iter().any(|r| r.as_str() == name),
            "unknown refusal {name}"
        );
    }
}

// ── Each refusal ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn malformed_codes_are_refused_before_anything_else() {
    let good = primary_code();
    let (_, rest) = good.split_once('.').expect("prefix");
    let (payload_b64, signature) = rest.split_once('.').expect("signature");
    let cases = [
        String::new(),
        format!("MFXREC2.{rest}"),
        format!("mfxrec1.{rest}"),
        format!("{CODE_PREFIX}.{payload_b64}"),
        format!("{CODE_PREFIX}.{payload_b64}.{signature}.x"),
        format!("{CODE_PREFIX}.{payload_b64}=.{signature}"),
        format!("{CODE_PREFIX}.{}.{signature}", format!("+{}", &payload_b64[1..])),
        format!("{CODE_PREFIX}.{payload_b64}.{}", signature.to_uppercase()),
        format!("{CODE_PREFIX}.{payload_b64}.{}", &signature[..126]),
        format!("{CODE_PREFIX}.{payload_b64}.{}zz", &signature[..126]),
        format!("{CODE_PREFIX}.!!!.{signature}"),
        "x".repeat(5000),
    ];
    for case in cases {
        assert_eq!(decode_code(&case).err(), Some(Refusal::Malformed), "{case}");
    }
    // Standard base64 alphabet instead of base64url (the escaped vector's payload has a '_').
    let escaped = vector()["extra_cases"][0]["code"].as_str().expect("code").to_string();
    assert!(escaped.contains('_'));
    assert!(decode_code(&escaped).is_ok());
    assert_eq!(decode_code(&escaped.replace('_', "/")).err(), Some(Refusal::Malformed));
    // Whitespace around a pasted code is not part of it.
    assert!(decode_code(&format!("  {good}\n")).is_ok());
}

#[test]
fn json_of_another_shape_is_malformed() {
    let primary = vector()["payload_json"].as_str().expect("payload").to_string();
    let cases = [
        // Unknown key.
        primary.replace("}", ",\"extra\":1}"),
        // Missing key.
        primary.replace(",\"license_id\":\"lic_vector_0001\"", ""),
        // Wrong type.
        primary.replace("\"v\":1", "\"v\":\"1\""),
        primary.replace("\"v\":1", "\"v\":-1"),
        // Duplicate key.
        primary.replace("}", ",\"v\":1}"),
        // Not JSON.
        "not json".to_string(),
    ];
    for payload in cases {
        assert_eq!(
            decode_code(&sign(&payload)).err(),
            Some(Refusal::Malformed),
            "{payload}"
        );
    }
    let not_utf8 = format!(
        "{CODE_PREFIX}.{}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xff, 0xfe, 0x7b]),
        "0".repeat(128)
    );
    assert_eq!(decode_code(&not_utf8).err(), Some(Refusal::Malformed));
}

#[test]
fn non_canonical_bytes_are_refused_even_when_signed() {
    let primary = vector()["payload_json"].as_str().expect("payload").to_string();
    let cases = [
        primary.replace("\"v\":1,", "\"v\": 1,"),
        format!("{primary}\n"),
        primary.replace("\"v\":1", "\"v\":1.0"),
        primary.replace("maintafox-vps", "maintafox\\u002dvps"),
    ];
    for payload in cases {
        let decoded = decode_code(&sign(&payload)).err();
        // `1.0` is not an integer for the payload's shape; everything else decodes but differs.
        assert!(
            matches!(decoded, Some(Refusal::NonCanonical | Refusal::Malformed)),
            "{payload}: {decoded:?}"
        );
    }
    let reordered = primary.replace(
        "{\"v\":1,\"purpose\":\"admin_recovery\",",
        "{\"purpose\":\"admin_recovery\",\"v\":1,",
    );
    assert_eq!(decode_code(&sign(&reordered)).err(), Some(Refusal::NonCanonical));
}

#[tokio::test]
async fn fixed_fields_issuer_and_keys_are_checked() {
    let db = activated_pc().await;
    let now = primary_now();
    let cases: Vec<(String, Refusal)> = vec![
        (signed_variant(|p| p.v = 2), Refusal::WrongVersion),
        (
            signed_variant(|p| p.purpose = "entitlement".into()),
            Refusal::WrongPurpose,
        ),
        (
            signed_variant(|p| p.signature_alg = "ed25519".into()),
            Refusal::WrongAlgorithm,
        ),
        (
            signed_variant(|p| p.issuer = "someone-else".into()),
            Refusal::WrongIssuer,
        ),
        (
            signed_variant(|p| p.key_id = "no-such-key".into()),
            Refusal::UntrustedKey,
        ),
        (
            signed_variant(|p| p.key_id = "mfx-ed25519-2026-10".into()),
            Refusal::InvalidSignature,
        ),
    ];
    for (code, expected) in cases {
        assert_eq!(verify(&db, &code, now).await.err(), Some(expected), "{code}");
    }
}

#[tokio::test]
async fn a_compromised_or_inactive_key_is_refused() {
    let db = activated_pc().await;
    for (sql, expected) in [
        (
            "UPDATE licensing_trust_keys SET is_active = 0 WHERE key_id = ?",
            Refusal::UntrustedKey,
        ),
        (
            "UPDATE licensing_trust_keys SET is_active = 1, is_compromised = 1 WHERE key_id = ?",
            Refusal::CompromisedKey,
        ),
        (
            "DELETE FROM licensing_trust_keys WHERE key_id = ?",
            Refusal::UntrustedKey,
        ),
    ] {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [crate::entitlements::trusted_keys::TEST_ENTITLEMENT_KEY_ID.into()],
        ))
        .await
        .expect("key state");
        assert_eq!(verify(&db, &primary_code(), primary_now()).await.err(), Some(expected));
    }
}

#[tokio::test]
async fn a_code_for_another_pc_is_refused() {
    let now = primary_now();
    let db = bare_pc().await;
    assert_eq!(
        verify(&db, &primary_code(), now).await.err(),
        Some(Refusal::NotActivated)
    );
    for (tenant, fingerprint, license, expected) in [
        ("ten_other", FINGERPRINT, LICENSE, Refusal::WrongTenant),
        (TENANT, "fp-other-pc", LICENSE, Refusal::WrongMachine),
        (TENANT, FINGERPRINT, "lic_other", Refusal::WrongLicense),
        // Exact bytes: no case folding or trimming.
        (TENANT, "FP-VECTOR-9C41E7B2A05D", LICENSE, Refusal::WrongMachine),
    ] {
        set_claim(&db, tenant, fingerprint, license).await;
        assert_eq!(verify(&db, &primary_code(), now).await.err(), Some(expected));
    }
}

#[tokio::test]
async fn the_validity_window_is_enforced_at_its_edges() {
    let db = activated_pc().await;
    let code = primary_code();
    // issued 2026-10-19T08:30Z, expires 2026-10-20T08:30Z.
    assert!(verify(&db, &code, instant("2026-10-20T08:29:59.999Z")).await.is_ok());
    assert_eq!(
        verify(&db, &code, instant("2026-10-20T08:30:00.000Z")).await.err(),
        Some(Refusal::Expired)
    );
    assert!(verify(&db, &code, instant("2026-10-19T08:25:00.000Z")).await.is_ok());
    assert_eq!(
        verify(&db, &code, instant("2026-10-19T08:24:59.999Z")).await.err(),
        Some(Refusal::IssuedInFuture)
    );
    let unreadable = signed_variant(|p| p.expires_at = "tomorrow".into());
    assert_eq!(
        verify(&db, &unreadable, primary_now()).await.err(),
        Some(Refusal::Malformed)
    );
}

// ── Redeeming ────────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn redeeming_creates_one_lasting_administrator_and_spends_the_code() {
    let db = administered_pc().await;
    let chef = user_id(&db, "chef").await;
    let before = accounts_snapshot(&db).await;

    let created = redeem_admin_recovery_code_in(&db, input(&primary_code(), "  recovery.admin "), primary_now())
        .await
        .expect("redeemed");
    assert_eq!(created.username, "recovery.admin");
    let new_id = user_id(&db, "recovery.admin").await;

    // Existing accounts untouched; exactly one new one.
    let after = accounts_snapshot(&db).await;
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(&after[..before.len()], &before[..]);

    // Normal Administrator role, plant-wide, no end date: a lasting administrator.
    let lasting = crate::auth::last_admin::lasting_administrator_ids(&db)
        .await
        .expect("lasting");
    assert!(lasting.contains(&new_id), "{lasting:?}");
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM user_scope_assignments usa JOIN roles r ON r.id = usa.role_id \
                 WHERE usa.user_id = {new_id} AND r.name = 'Administrator' AND usa.scope_type = 'tenant' \
                 AND usa.scope_reference = '{TENANT}' AND usa.valid_to IS NULL AND usa.deleted_at IS NULL"
            )
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM user_accounts WHERE id = {new_id} AND is_admin = 0 \
                 AND is_active = 1 AND force_password_change = 0 AND display_name = 'Responsable Maintenance'"
            )
        )
        .await,
        1
    );
    let found = crate::auth::builtin_admin::find_sign_in_account(&db, "recovery.admin")
        .await
        .expect("sign-in lookup")
        .expect("the new account can sign in");
    assert_eq!(i64::from(found.0), new_id);
    let stored_hash = found.5.expect("password hash");
    assert!(crate::auth::password::verify_password(PASSWORD, &stored_hash).expect("verify"));

    // Spent: who and when, never the code.
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM admin_recovery_codes_used WHERE code_id = \
                 '3f6c1d2e-9a4b-4c7d-8e1f-2a3b4c5d6e7f' AND created_user_id = {new_id} \
                 AND created_username = 'recovery.admin'"
            )
        )
        .await,
        1
    );
    let columns = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM pragma_table_info('admin_recovery_codes_used')".to_string(),
        ))
        .await
        .expect("columns")
        .iter()
        .map(|r| r.try_get::<String>("", "name").expect("name"))
        .collect::<Vec<_>>();
    assert!(
        !columns.iter().any(|c| c.contains("code") && c != "code_id"),
        "{columns:?}"
    );

    // Audited.
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_USED}' \
                 AND target_id = '{new_id}' AND result = 'success'"
            )
        )
        .await,
        1
    );

    // The notice goes to the other active administrator, not to the new one.
    let notices = notices_for(&db, Some(chef)).await.expect("notices");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].created_username, "recovery.admin");
    assert_eq!(notices[0].created_display_name, "Responsable Maintenance");
    assert!(notices_for(&db, Some(new_id)).await.expect("notices").is_empty());
    assert!(notices_for(&db, None).await.expect("signed out").is_empty());

    // Replay: refused, nothing more created.
    let again = redeem_admin_recovery_code_in(&db, input(&primary_code(), "second.admin"), primary_now())
        .await
        .expect_err("replay");
    assert_eq!(code_of(&again), Some(Refusal::AlreadyUsed.error_code()), "{again:?}");
    assert_eq!(accounts_snapshot(&db).await.len(), after.len());
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_REFUSED}'")
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_plant_without_any_administrator_is_recovered() {
    // No precondition on the administrators: here there are none at all.
    let db = activated_pc().await;
    assert!(crate::auth::last_admin::lasting_administrator_ids(&db)
        .await
        .expect("lasting")
        .is_empty());
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "sauveur"), primary_now())
        .await
        .expect("redeemed");
    let new_id = user_id(&db, "sauveur").await;
    assert_eq!(
        crate::auth::last_admin::lasting_administrator_ids(&db)
            .await
            .expect("lasting"),
        vec![new_id]
    );
    assert_eq!(scalar(&db, "SELECT COUNT(*) AS n FROM admin_recovery_notices").await, 0);
}

#[tokio::test]
async fn an_existing_username_is_never_taken_over() {
    let db = administered_pc().await;
    let before = accounts_snapshot(&db).await;
    for name in ["chef", "CHEF", " Chef "] {
        let err = redeem_admin_recovery_code_in(&db, input(&primary_code(), name), primary_now())
            .await
            .expect_err("taken");
        assert_eq!(code_of(&err), Some(USERNAME_TAKEN), "{err:?}");
    }
    // A deleted account's name is not reused either.
    db.execute_unprepared("UPDATE user_accounts SET deleted_at = '2026-01-01T00:00:00Z' WHERE username = 'chef'")
        .await
        .expect("delete");
    let snapshot = accounts_snapshot(&db).await;
    let err = redeem_admin_recovery_code_in(&db, input(&primary_code(), "chef"), primary_now())
        .await
        .expect_err("taken");
    assert_eq!(code_of(&err), Some(USERNAME_TAKEN));
    assert_eq!(accounts_snapshot(&db).await, snapshot);
    assert_ne!(snapshot, before, "only the test's own delete changed a row");
    // The code was not spent by the refused attempts.
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) AS n FROM admin_recovery_codes_used").await,
        0
    );
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "nouveau"), primary_now())
        .await
        .expect("still usable");
}

#[tokio::test]
async fn the_form_is_examined_only_for_a_genuine_code() {
    let db = administered_pc().await;
    let tampered = vector()["tampered"]["code"].as_str().expect("tampered").to_string();
    // An existing username with a forged code: the answer is about the code only.
    let err = redeem_admin_recovery_code_in(&db, input(&tampered, "chef"), primary_now())
        .await
        .expect_err("forged");
    assert_eq!(code_of(&err), Some(Refusal::InvalidSignature.error_code()));
    // A forged code is not audited as a vendor code; a genuine one for another PC is.
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_REFUSED}'")
        )
        .await,
        0
    );
    set_claim(&db, TENANT, "fp-other-pc", LICENSE).await;
    let err = redeem_admin_recovery_code_in(&db, input(&primary_code(), "chef"), primary_now())
        .await
        .expect_err("other PC");
    assert_eq!(code_of(&err), Some(Refusal::WrongMachine.error_code()));
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_REFUSED}'")
        )
        .await,
        1
    );
}

#[tokio::test]
async fn the_password_policy_and_form_rules_apply() {
    let db = activated_pc().await;
    let mut weak = input(&primary_code(), "nouveau");
    weak.password = "short".into();
    weak.password_confirmation = "short".into();
    let err = redeem_admin_recovery_code_in(&db, weak, primary_now())
        .await
        .expect_err("weak");
    assert!(matches!(err, AppError::ValidationFailed(_)), "{err:?}");

    let mut mismatch = input(&primary_code(), "nouveau");
    mismatch.password_confirmation = format!("{PASSWORD}x");
    let err = redeem_admin_recovery_code_in(&db, mismatch, primary_now())
        .await
        .expect_err("mismatch");
    assert_eq!(code_of(&err), Some(PASSWORD_MISMATCH));

    let err = redeem_admin_recovery_code_in(&db, input(&primary_code(), "a b"), primary_now())
        .await
        .expect_err("username");
    assert_eq!(code_of(&err), Some(USERNAME_INVALID));

    let mut nameless = input(&primary_code(), "nouveau");
    nameless.display_name = "   ".into();
    let err = redeem_admin_recovery_code_in(&db, nameless, primary_now())
        .await
        .expect_err("display name");
    assert_eq!(code_of(&err), Some(DISPLAY_NAME_REQUIRED));

    assert_eq!(
        scalar(&db, "SELECT COUNT(*) AS n FROM admin_recovery_codes_used").await,
        0
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) AS n FROM user_accounts WHERE username = 'nouveau'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn two_submissions_of_one_code_make_one_administrator() {
    let db = administered_pc().await;
    let (a, b) = tokio::join!(
        redeem_admin_recovery_code_in(&db, input(&primary_code(), "premier"), primary_now()),
        redeem_admin_recovery_code_in(&db, input(&primary_code(), "second"), primary_now()),
    );
    let outcomes = [a, b];
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1, "{outcomes:?}");
    let loser = outcomes.iter().find_map(|r| r.as_ref().err()).expect("one refused");
    assert!(
        matches!(code_of(loser), Some(c) if c == Refusal::AlreadyUsed.error_code() || c == RETRY),
        "{loser:?}"
    );
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) AS n FROM user_accounts WHERE username IN ('premier', 'second')"
        )
        .await,
        1
    );
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) AS n FROM admin_recovery_codes_used").await,
        1
    );
}

#[tokio::test]
async fn the_spent_register_refuses_a_second_row_for_one_code() {
    let db = migrated_register().await;
    let insert = "INSERT INTO admin_recovery_codes_used
        (code_id, tenant_id, license_id, key_id, issued_at, expires_at, created_user_id, created_username, consumed_at)
        VALUES ('c1', 't', 'l', 'k', 'i', 'e', 1, 'u', 'n')";
    db.execute_unprepared(insert).await.expect("first");
    assert!(db.execute_unprepared(insert).await.is_err(), "primary key");
}

async fn migrated_register() -> DatabaseConnection {
    crate::db::test_db::migrated().await
}

// ── Notice ───────────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn each_administrator_acknowledges_their_own_notice() {
    let db = administered_pc().await;
    // A second administrator, "adjoint", comes from a first recovery code.
    let chef = user_id(&db, "chef").await;
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "adjoint"), primary_now())
        .await
        .expect("first recovery");
    let adjoint = user_id(&db, "adjoint").await;

    // A second code (another code_id) now tells both chef and adjoint.
    let second = signed_variant(|p| p.code_id = "11111111-2222-4333-8444-555555555555".into());
    redeem_admin_recovery_code_in(&db, input(&second, "troisieme"), primary_now())
        .await
        .expect("second recovery");
    let chef_notices = notices_for(&db, Some(chef)).await.expect("chef");
    let adjoint_notices = notices_for(&db, Some(adjoint)).await.expect("adjoint");
    assert_eq!(chef_notices.len(), 2);
    assert_eq!(adjoint_notices.len(), 1);
    assert_eq!(adjoint_notices[0].created_username, "troisieme");

    // Someone else's notice cannot be acknowledged.
    let err = acknowledge_notice_in(&db, adjoint, chef_notices[0].id)
        .await
        .expect_err("not theirs");
    assert_eq!(code_of(&err), Some(NOTICE_NOT_FOUND));
    assert_eq!(notices_for(&db, Some(chef)).await.expect("chef").len(), 2);

    acknowledge_notice_in(&db, chef, chef_notices[0].id).await.expect("ack");
    let left = notices_for(&db, Some(chef)).await.expect("chef");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, chef_notices[1].id);
    // Adjoint's own notice is still there.
    assert_eq!(notices_for(&db, Some(adjoint)).await.expect("adjoint").len(), 1);
    // Twice: refused.
    let err = acknowledge_notice_in(&db, chef, chef_notices[0].id)
        .await
        .expect_err("already");
    assert_eq!(code_of(&err), Some(NOTICE_NOT_FOUND));
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_NOTICE_ACKNOWLEDGED}' \
                 AND actor_id = {chef}"
            )
        )
        .await,
        1
    );
}

#[tokio::test]
async fn a_former_administrator_is_not_shown_the_notice() {
    let db = administered_pc().await;
    let chef = user_id(&db, "chef").await;
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "nouveau"), primary_now())
        .await
        .expect("redeemed");
    assert_eq!(notices_for(&db, Some(chef)).await.expect("chef").len(), 1);
    db.execute_unprepared(&format!("UPDATE user_accounts SET is_active = 0 WHERE id = {chef}"))
        .await
        .expect("disable");
    assert!(notices_for(&db, Some(chef)).await.expect("chef").is_empty());
}
