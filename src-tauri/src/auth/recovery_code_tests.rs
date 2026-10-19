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
    // The window itself must be one the vendor issues (checked before expiry).
    assert_eq!(
        verify(
            &db,
            &signed_variant(|p| p.expires_at = "2030-01-01T00:00:00.000Z".into()),
            primary_now()
        )
        .await
        .err(),
        Some(Refusal::InvalidWindow)
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
                 AND usa.scope_reference IS NULL AND usa.valid_to IS NULL AND usa.deleted_at IS NULL"
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
    assert_eq!(
        notices[0].redeemed_device_code,
        crate::sync::device_code::this_device_code(&db)
            .await
            .expect("device code")
    );
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
async fn a_recipient_demoted_since_still_sees_and_acknowledges_the_notice() {
    let db = administered_pc().await;
    let chef = user_id(&db, "chef").await;
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "nouveau"), primary_now())
        .await
        .expect("redeemed");
    assert_eq!(notices_for(&db, Some(chef)).await.expect("chef").len(), 1);
    // The new administrator removes chef's Administrator role: chef is no administrator any more...
    db.execute_unprepared(&format!(
        "DELETE FROM user_scope_assignments WHERE user_id = {chef};          UPDATE user_accounts SET is_admin = 0 WHERE id = {chef};"
    ))
    .await
    .expect("demote");
    assert!(
        !crate::auth::rbac::user_has_elevated_admin_access(&db, i32::try_from(chef).expect("id"))
            .await
            .expect("rights")
    );
    // ...and still sees the notice at sign-in, and acknowledges it.
    let notices = notices_for(&db, Some(chef)).await.expect("chef");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].created_username, "nouveau");
    acknowledge_notice_in(&db, chef, notices[0].id).await.expect("ack");
    assert!(notices_for(&db, Some(chef)).await.expect("chef").is_empty());
}

#[tokio::test]
async fn a_disabled_recipient_is_shown_nothing() {
    // A disabled account cannot sign in: nothing is read for it.
    let db = administered_pc().await;
    let chef = user_id(&db, "chef").await;
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "nouveau"), primary_now())
        .await
        .expect("redeemed");
    db.execute_unprepared(&format!("UPDATE user_accounts SET is_active = 0 WHERE id = {chef}"))
        .await
        .expect("disable");
    assert!(notices_for(&db, Some(chef)).await.expect("chef").is_empty());
}

// ── Review should-fix round (S1, S2, S4, N4, N7) ─────────────────────────────────────────────────

#[tokio::test]
async fn the_code_lifetime_is_at_most_24_hours() {
    let db = activated_pc().await;
    let now = primary_now();
    // issued 2026-10-19T08:30:00.000Z (the vector's primary code: exactly 24 h, accepted).
    assert!(verify(&db, &primary_code(), now).await.is_ok());
    let exactly = signed_variant(|p| p.expires_at = "2026-10-20T08:30:00.000Z".into());
    assert!(verify(&db, &exactly, now).await.is_ok());
    let shorter = signed_variant(|p| p.expires_at = "2026-10-19T09:30:00.000Z".into());
    assert!(verify(&db, &shorter, now).await.is_ok());
    for expires_at in [
        // 24 h + 1 ms.
        "2026-10-20T08:30:00.001Z",
        // Ends when it starts, or before.
        "2026-10-19T08:30:00.000Z",
        "2026-10-19T08:29:59.999Z",
        // Years ahead.
        "2036-10-19T08:30:00.000Z",
    ] {
        let code = signed_variant(|p| p.expires_at = expires_at.into());
        assert_eq!(
            verify(&db, &code, now).await.err(),
            Some(Refusal::InvalidWindow),
            "{expires_at}"
        );
    }
    assert_eq!(Refusal::InvalidWindow.as_str(), "invalid_window");
    assert_eq!(
        Refusal::InvalidWindow.error_code(),
        "ADMIN_RECOVERY_CODE_INVALID_WINDOW"
    );
    // A genuine code with such a window is refused when redeemed, audited, and creates nothing.
    let long = signed_variant(|p| p.expires_at = "2026-10-20T08:30:00.001Z".into());
    let err = redeem_admin_recovery_code_in(&db, input(&long, "nouveau"), now)
        .await
        .expect_err("long window");
    assert_eq!(code_of(&err), Some("ADMIN_RECOVERY_CODE_INVALID_WINDOW"));
    assert_eq!(
        scalar(
            &db,
            "SELECT COUNT(*) AS n FROM user_accounts WHERE username = 'nouveau'"
        )
        .await,
        0
    );
    assert_eq!(
        scalar(
            &db,
            &format!(
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_REFUSED}' \
                 AND json_extract(details_json, '$.reason') = 'invalid_window'"
            )
        )
        .await,
        1
    );
}

async fn refusal_rows(db: &DatabaseConnection, reason: &str) -> i64 {
    scalar(
        db,
        &format!(
            "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_REFUSED}' \
             AND json_extract(details_json, '$.reason') = '{reason}'"
        ),
    )
    .await
}

#[tokio::test]
async fn repeated_refusals_of_one_genuine_code_are_audited_once_per_reason() {
    let db = administered_pc().await;
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "sauveur"), primary_now())
        .await
        .expect("redeemed");
    // The spent code replayed again and again from the login screen: one audit row.
    for attempt in 0..5 {
        let err = redeem_admin_recovery_code_in(&db, input(&primary_code(), &format!("rejeu{attempt}")), primary_now())
            .await
            .expect_err("replay");
        assert_eq!(code_of(&err), Some(Refusal::AlreadyUsed.error_code()));
    }
    assert_eq!(refusal_rows(&db, "already_used").await, 1);

    // Another genuine code, for another PC: one row for that code and reason too.
    let other = signed_variant(|p| p.code_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".into());
    set_claim(&db, TENANT, "fp-other-pc", LICENSE).await;
    for _ in 0..3 {
        let err = redeem_admin_recovery_code_in(&db, input(&other, "autre"), primary_now())
            .await
            .expect_err("other PC");
        assert_eq!(code_of(&err), Some(Refusal::WrongMachine.error_code()));
    }
    assert_eq!(refusal_rows(&db, "wrong_machine").await, 1);
    // The same code refused for another reason is a new row (one per code and reason).
    set_claim(&db, TENANT, FINGERPRINT, "lic_other").await;
    for _ in 0..2 {
        redeem_admin_recovery_code_in(&db, input(&other, "autre"), primary_now())
            .await
            .expect_err("other licence");
    }
    assert_eq!(refusal_rows(&db, "wrong_license").await, 1);
    assert_eq!(
        scalar(
            &db,
            &format!("SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_REFUSED}'")
        )
        .await,
        3
    );
}

#[test]
fn format_and_control_characters_are_recognised() {
    for c in [
        '\u{202E}',
        '\u{202A}',
        '\u{2066}',
        '\u{2069}',
        '\u{200B}',
        '\u{200F}',
        '\u{200D}',
        '\u{00AD}',
        '\u{FEFF}',
        '\u{061C}',
        '\u{E0041}',
        // Default_Ignorable_Code_Point outside Cf: Hangul fillers, combining grapheme joiner,
        // variation selectors, Mongolian free variation selectors, Khmer inherent vowel.
        '\u{115F}',
        '\u{1160}',
        '\u{3164}',
        '\u{FFA0}',
        '\u{034F}',
        '\u{FE00}',
        '\u{FE0F}',
        '\u{E0100}',
        '\u{E01EF}',
        '\u{180B}',
        '\u{17B4}',
    ] {
        assert!(is_invisible_code_point(c), "U+{:04X}", u32::from(c));
        assert!(has_invisible_character(&format!("Jean{c}Dupont")));
    }
    assert!(has_invisible_character("Jean\u{0007}Dupont"));
    for name in ["Jean Dupont", "Hélène Çelik", "محمد", "Ørsted-Nuñez", "李雷", "김민준"] {
        assert!(!has_invisible_character(name), "{name}");
    }
    // The table is sorted and its ranges are well formed.
    for pair in INVISIBLE_CODE_POINTS.windows(2) {
        assert!(pair[0].1 < pair[1].0, "{pair:?}");
    }
    assert!(INVISIBLE_CODE_POINTS.iter().all(|(first, last)| first <= last));
}

#[tokio::test]
async fn invisible_characters_are_refused_in_the_name_and_username() {
    let db = administered_pc().await;
    for display_name in [
        "Jean\u{202E}tnopuD",
        "Chef\u{200B}",
        "\u{2066}Chef\u{2069}",
        "Jean\u{00AD}Dupont",
        "Jean\nDupont",
        // A variation selector, a combining grapheme joiner, a Hangul filler as a "blank" name.
        "Jean\u{FE0F}Dupont",
        "Jean\u{034F} Dupont",
        "\u{3164}",
    ] {
        let mut sneaky = input(&primary_code(), "nouveau");
        sneaky.display_name = display_name.into();
        let err = redeem_admin_recovery_code_in(&db, sneaky, primary_now())
            .await
            .expect_err("display name");
        assert_eq!(code_of(&err), Some(NAME_INVISIBLE_CHARACTERS), "{display_name:?}");
    }
    // The Hangul fillers are letters (category Lo) that show nothing: "chef" + U+3164 would pass
    // the "never had this username" check and read as "chef".
    for username in [
        "chef\u{200D}2",
        "\u{202E}fehc",
        "adm\u{FEFF}in",
        "chef\u{3164}",
        "chef\u{115F}",
        "chef\u{FFA0}",
        "chef\u{FE0F}",
    ] {
        let err = redeem_admin_recovery_code_in(&db, input(&primary_code(), username), primary_now())
            .await
            .expect_err("username");
        assert_eq!(code_of(&err), Some(NAME_INVISIBLE_CHARACTERS), "{username:?}");
    }
    assert_eq!(
        scalar(&db, "SELECT COUNT(*) AS n FROM admin_recovery_codes_used").await,
        0
    );
    // Letters of any script stay allowed.
    let mut accented = input(&primary_code(), "hélène");
    accented.display_name = "Hélène Çelik".into();
    redeem_admin_recovery_code_in(&db, accented, primary_now())
        .await
        .expect("accented names");
}

#[test]
fn debug_of_the_input_never_shows_the_code_or_the_passwords() {
    let shown = format!("{:?}", input(&primary_code(), "sauveur"));
    assert!(!shown.contains(PASSWORD), "{shown}");
    assert!(!shown.contains("MFXREC1"), "{shown}");
    assert!(shown.contains("sauveur"), "{shown}");
    assert_eq!(shown.matches("<redacted>").count(), 3, "{shown}");
}

async fn first_admin_marker(db: &DatabaseConnection) -> Option<String> {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT value FROM system_config WHERE key = ?",
        [crate::auth::builtin_admin::FIRST_ADMIN_MARKER_KEY.into()],
    ))
    .await
    .expect("marker")
    .and_then(|r| r.try_get::<String>("", "value").ok())
}

async fn sync_id_of(db: &DatabaseConnection, username: &str) -> String {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT entity_sync_id FROM user_accounts WHERE username = ?",
        [username.into()],
    ))
    .await
    .expect("query")
    .and_then(|r| r.try_get::<String>("", "entity_sync_id").ok())
    .expect("sync id")
}

#[tokio::test]
async fn the_recovered_administrator_is_recorded_as_first_administrator_like_first_start() {
    // No administrator yet: the recovered one becomes the PC's first-administrator marker, so the
    // first-start screen stays closed even if this account is disabled before a restart.
    let db = activated_pc().await;
    assert!(!crate::auth::builtin_admin::first_admin_marker_closes(&db)
        .await
        .expect("marker"));
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "sauveur"), primary_now())
        .await
        .expect("redeemed");
    assert_eq!(first_admin_marker(&db).await, Some(sync_id_of(&db, "sauveur").await));
    assert!(crate::auth::builtin_admin::first_admin_marker_closes(&db)
        .await
        .expect("marker"));

    // A PC whose first administrator exists keeps its marker (the call is idempotent).
    let db = administered_pc().await;
    let chef_marker = first_admin_marker(&db).await;
    assert_eq!(chef_marker, Some(sync_id_of(&db, "chef").await));
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "sauveur"), primary_now())
        .await
        .expect("redeemed");
    assert_eq!(first_admin_marker(&db).await, chef_marker);
}

// ── S3: notices synchronised across PCs ──────────────────────────────────────────────────────────

use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
use crate::sync::test_support::{current_payload, started_device_db, TestServer};

type Payload = serde_json::Map<String, Value>;

/// A named administrator whose role travels by synchronisation (plant-wide, no scope reference).
async fn synced_admin(db: &DatabaseConnection, username: &str) -> i64 {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_accounts
            (entity_sync_id, username, display_name, identity_mode, password_hash, is_active, is_admin,
             force_password_change, password_changed_at, failed_login_attempts, created_at, updated_at, row_version)
         VALUES (?, ?, ?, 'local', 'x', 1, 0, 0, ?, 0, ?, ?, 1)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            username.into(),
            username.into(),
            now.clone().into(),
            now.clone().into(),
            now.clone().into(),
        ],
    ))
    .await
    .expect("account");
    let id = user_id(db, username).await;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO user_scope_assignments
            (entity_sync_id, user_id, role_id, scope_type, scope_reference, valid_to, created_at, updated_at,
             row_version)
         VALUES (?, ?, (SELECT id FROM roles WHERE name = 'Administrator' AND deleted_at IS NULL), 'tenant',
                 NULL, NULL, ?, ?, 1)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            id.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await
    .expect("role");
    id
}

/// PC A (activated for the vector, administrators "chef" and "adjoint") and PC B, in sync, with a
/// recovery code redeemed on A ("sauveur") and everything exchanged once more.
async fn two_pcs_after_recovery() -> (DatabaseConnection, DatabaseConnection, TestServer) {
    let a = started_device_db().await;
    let b = started_device_db().await;
    crate::license::security::ensure_default_licensing_trust_keys(&a)
        .await
        .expect("trust keys");
    set_claim(&a, TENANT, FINGERPRINT, LICENSE).await;
    synced_admin(&a, "chef").await;
    synced_admin(&a, "adjoint").await;
    let mut server = TestServer::new();
    server.exchange("A", &a).await;
    server.exchange("B", &b).await;
    assert!(
        crate::auth::rbac::user_has_elevated_admin_access(&b, i32::try_from(user_id(&b, "chef").await).expect("id"))
            .await
            .expect("rights"),
        "chef is an administrator on B too"
    );

    redeem_admin_recovery_code_in(&a, input(&primary_code(), "sauveur"), primary_now())
        .await
        .expect("redeemed on A");
    // A replay, refused and audited (its audit row names the vendor's code id as target).
    redeem_admin_recovery_code_in(&a, input(&primary_code(), "rejeu"), primary_now())
        .await
        .expect_err("replay");
    server.exchange("A", &a).await;
    let received = server.exchange("B", &b).await;
    assert!(
        received.materialized.errors.is_empty(),
        "{:?}",
        received.materialized.errors
    );
    // The refusal's audit row reached B with the code id as target.
    assert_eq!(
        scalar(
            &b,
            &format!(
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_REFUSED}' \
                 AND target_type = 'admin_recovery_code' AND target_id = '3f6c1d2e-9a4b-4c7d-8e1f-2a3b4c5d6e7f'"
            )
        )
        .await,
        1
    );
    // Everything staged on A, the new administrator's role and the refusal's audit row included.
    assert_eq!(
        scalar(
            &a,
            "SELECT COUNT(*) AS n FROM sync_conflicts WHERE conflict_type = 'STAGE_ERROR'"
        )
        .await,
        0
    );
    (a, b, server)
}

async fn notice_sync_id(db: &DatabaseConnection, recipient: &str) -> String {
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT n.entity_sync_id FROM admin_recovery_notices n JOIN user_accounts u ON u.id = n.recipient_user_id
          WHERE u.username = ?",
        [recipient.into()],
    ))
    .await
    .expect("query")
    .and_then(|r| r.try_get::<String>("", "entity_sync_id").ok())
    .expect("notice")
}

async fn wire_of(db: &DatabaseConnection, entity_type: &str, sync_id: &str) -> Payload {
    current_payload(db, entity_type, sync_id, &Default::default())
        .await
        .expect("payload")
        .expect("row")
}

async fn receive(
    db: &DatabaseConnection,
    entity_type: &str,
    sync_id: &str,
    operation: &str,
    row: &Payload,
) -> ApplyOutcome {
    apply_inbound_payload(
        db,
        entity_type,
        sync_id,
        operation,
        2,
        &serde_json::to_string(row).expect("json"),
    )
    .await
    .expect("apply")
}

fn conflict_code(outcome: &ApplyOutcome) -> Option<&str> {
    match outcome {
        ApplyOutcome::Conflict { code, .. } => Some(code.as_str()),
        _ => None,
    }
}

#[tokio::test]
async fn a_notice_reaches_every_pc_and_its_acknowledgement_travels_back() {
    let (a, b, mut server) = two_pcs_after_recovery().await;
    let device_a = crate::sync::device_code::this_device_code(&a).await.expect("code A");
    let device_b = crate::sync::device_code::this_device_code(&b).await.expect("code B");

    // One notice per administrator active at redemption, the new one excluded, on both PCs.
    for db in [&a, &b] {
        assert_eq!(scalar(db, "SELECT COUNT(*) AS n FROM admin_recovery_notices").await, 2);
        assert_eq!(
            scalar(
                db,
                "SELECT COUNT(*) AS n FROM admin_recovery_notices n JOIN user_accounts u \
                 ON u.id = n.recipient_user_id WHERE u.username = 'sauveur'"
            )
            .await,
            0
        );
    }
    // PC B received the account AND its Administrator role: a lasting, plant-wide administrator there.
    let sauveur_b = user_id(&b, "sauveur").await;
    assert_eq!(
        scalar(
            &b,
            &format!(
                "SELECT COUNT(*) AS n FROM user_scope_assignments usa JOIN roles r ON r.id = usa.role_id                  WHERE usa.user_id = {sauveur_b} AND r.name = 'Administrator' AND usa.scope_type = 'tenant'                  AND usa.scope_reference IS NULL AND usa.valid_to IS NULL AND usa.deleted_at IS NULL"
            )
        )
        .await,
        1
    );
    assert!(
        crate::auth::rbac::user_has_elevated_admin_access(&b, i32::try_from(sauveur_b).expect("id"))
            .await
            .expect("rights")
    );
    assert!(crate::auth::last_admin::lasting_administrator_ids(&b)
        .await
        .expect("lasting")
        .contains(&sauveur_b));
    assert!(crate::auth::last_admin::active_administrator_ids(&b)
        .await
        .expect("active")
        .contains(&sauveur_b));
    let chef_b = user_id(&b, "chef").await;
    let on_b = notices_for(&b, Some(chef_b)).await.expect("chef on B");
    assert_eq!(on_b.len(), 1, "the notice is visible on the second PC");
    assert_eq!(on_b[0].created_username, "sauveur");
    assert_eq!(on_b[0].created_display_name, "Responsable Maintenance");
    assert_eq!(on_b[0].redeemed_device_code, device_a);
    assert_eq!(
        notices_for(&b, Some(user_id(&b, "adjoint").await))
            .await
            .expect("adjoint on B")
            .len(),
        1
    );

    // Chef acknowledges on B: the acknowledgement (who, when, which PC) reaches A and clears it there.
    let chef_a = user_id(&a, "chef").await;
    assert_eq!(notices_for(&a, Some(chef_a)).await.expect("chef on A").len(), 1);
    acknowledge_notice_in(&b, chef_b, on_b[0].id).await.expect("ack on B");
    assert!(notices_for(&b, Some(chef_b)).await.expect("chef on B").is_empty());
    server.exchange("B", &b).await;
    server.exchange("A", &a).await;
    assert_eq!(
        scalar(
            &a,
            "SELECT COUNT(*) AS n FROM sync_conflicts WHERE entity_type LIKE 'admin_recovery%'"
        )
        .await,
        0
    );
    assert!(notices_for(&a, Some(chef_a)).await.expect("chef on A").is_empty());
    assert_eq!(
        scalar(
            &a,
            &format!(
                "SELECT COUNT(*) AS n FROM admin_recovery_acknowledgements \
                 WHERE acknowledged_by_id = {chef_a} AND device_code = '{device_b}'"
            )
        )
        .await,
        1
    );
    // Adjoint's own notice is still waiting on both PCs.
    assert_eq!(
        notices_for(&a, Some(user_id(&a, "adjoint").await))
            .await
            .expect("adjoint on A")
            .len(),
        1
    );
    // The acknowledgement was audited where it was made.
    assert_eq!(
        scalar(
            &b,
            &format!(
                "SELECT COUNT(*) AS n FROM audit_events WHERE action_code = '{AUDIT_NOTICE_ACKNOWLEDGED}' \
                 AND actor_id = {chef_b} AND json_extract(details_json, '$.device_code') = '{device_b}'"
            )
        )
        .await,
        1
    );
}

#[tokio::test]
async fn an_acknowledgement_made_by_anyone_but_the_recipient_is_refused() {
    let (a, b, _server) = two_pcs_after_recovery().await;
    // In the backend command: adjoint cannot acknowledge chef's notice, nor can the new administrator.
    let chef_notice_b = notice_sync_id(&b, "chef").await;
    let chef_notice_b_id = scalar(
        &b,
        &format!("SELECT id AS n FROM admin_recovery_notices WHERE entity_sync_id = '{chef_notice_b}'"),
    )
    .await;
    for intruder in ["adjoint", "sauveur"] {
        let err = acknowledge_notice_in(&b, user_id(&b, intruder).await, chef_notice_b_id)
            .await
            .expect_err("not theirs");
        assert_eq!(code_of(&err), Some(NOTICE_NOT_FOUND), "{intruder}");
    }

    // A genuine acknowledgement by chef on B, as B sends it.
    let chef_b = user_id(&b, "chef").await;
    acknowledge_notice_in(&b, chef_b, chef_notice_b_id).await.expect("ack");
    let ack_sync: String = b
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT entity_sync_id AS v FROM admin_recovery_acknowledgements".to_string(),
        ))
        .await
        .expect("query")
        .and_then(|r| r.try_get::<String>("", "v").ok())
        .expect("ack");
    let genuine = wire_of(&b, ACK_ENTITY, &ack_sync).await;
    assert_eq!(
        genuine.get("acknowledged_by_sync_id").and_then(Value::as_str),
        Some(sync_id_of(&b, "chef").await.as_str())
    );

    // The same acknowledgement rewritten on a PC to name someone else: refused on arrival.
    for intruder in ["adjoint", "sauveur"] {
        let mut forged = genuine.clone();
        forged.insert(
            "acknowledged_by_sync_id".into(),
            Value::String(sync_id_of(&a, intruder).await),
        );
        let forged_sync = uuid::Uuid::new_v4().to_string();
        let outcome = receive(&a, ACK_ENTITY, &forged_sync, "upsert", &forged).await;
        assert_eq!(
            conflict_code(&outcome),
            Some(CONFLICT_ACK_NOT_RECIPIENT),
            "{intruder}: {outcome:?}"
        );
    }
    assert_eq!(
        scalar(&a, "SELECT COUNT(*) AS n FROM admin_recovery_acknowledgements").await,
        0
    );
    let chef_a = user_id(&a, "chef").await;
    assert_eq!(notices_for(&a, Some(chef_a)).await.expect("chef on A").len(), 1);

    // The genuine one is applied; changed afterwards (another time, another PC), it is refused.
    let outcome = receive(&a, ACK_ENTITY, &ack_sync, "upsert", &genuine).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(notices_for(&a, Some(chef_a)).await.expect("chef on A").is_empty());
    let mut changed = genuine.clone();
    changed.insert("acknowledged_at".into(), Value::String("2027-01-01T00:00:00Z".into()));
    assert_eq!(
        conflict_code(&receive(&a, ACK_ENTITY, &ack_sync, "upsert", &changed).await),
        Some(CONFLICT_ACK_CHANGED)
    );
    // Nor can an acknowledgement be removed by synchronisation (append-only).
    assert_eq!(
        conflict_code(&receive(&a, ACK_ENTITY, &ack_sync, "delete", &genuine).await),
        Some("APPEND_ONLY_DELETE")
    );
}

#[tokio::test]
async fn nobody_edits_or_deletes_a_notice() {
    let (a, b, _server) = two_pcs_after_recovery().await;
    let notice = notice_sync_id(&a, "chef").await;
    let original = wire_of(&a, NOTICE_ENTITY, &notice).await;

    // Received edits of any field are refused, whoever made them; the notice stays as it was.
    for (key, value) in [
        ("created_username", Value::String("quelquun".into())),
        ("created_display_name", Value::String("Jean Dupont".into())),
        ("created_at", Value::String("2020-01-01T00:00:00Z".into())),
        ("redeemed_device_code", Value::String("ZZZZ".into())),
        ("recipient_user_sync_id", Value::String(sync_id_of(&a, "sauveur").await)),
    ] {
        let mut edited = original.clone();
        edited.insert(key.into(), value);
        let outcome = receive(&a, NOTICE_ENTITY, &notice, "upsert", &edited).await;
        assert_eq!(
            conflict_code(&outcome),
            Some(CONFLICT_NOTICE_CHANGED),
            "{key}: {outcome:?}"
        );
    }
    assert_eq!(wire_of(&a, NOTICE_ENTITY, &notice).await, original);
    // A received delete is refused.
    let outcome = receive(&a, NOTICE_ENTITY, &notice, "delete", &original).await;
    assert_eq!(conflict_code(&outcome), Some("APPEND_ONLY_DELETE"), "{outcome:?}");
    assert_eq!(scalar(&a, "SELECT COUNT(*) AS n FROM admin_recovery_notices").await, 2);
    // The same notice received again unchanged is a no-op.
    let outcome = receive(&a, NOTICE_ENTITY, &notice, "upsert", &original).await;
    assert!(matches!(outcome, ApplyOutcome::Unchanged), "{outcome:?}");

    // A new notice addressed to the created administrator himself is refused.
    let mut to_himself = original.clone();
    to_himself.insert(
        "recipient_user_sync_id".into(),
        original.get("created_user_sync_id").cloned().expect("created"),
    );
    to_himself.insert("code_id".into(), Value::String("other-code".into()));
    let outcome = receive(
        &b,
        NOTICE_ENTITY,
        &uuid::Uuid::new_v4().to_string(),
        "upsert",
        &to_himself,
    )
    .await;
    assert_eq!(conflict_code(&outcome), Some(CONFLICT_NOTICE_INVALID), "{outcome:?}");
    // Nor one whose names carry invisible characters.
    let mut disguised = original.clone();
    disguised.insert(
        "created_display_name".into(),
        Value::String("Jean\u{202E}tnopuD".into()),
    );
    disguised.insert("code_id".into(), Value::String("other-code-2".into()));
    let outcome = receive(
        &b,
        NOTICE_ENTITY,
        &uuid::Uuid::new_v4().to_string(),
        "upsert",
        &disguised,
    )
    .await;
    assert_eq!(conflict_code(&outcome), Some(CONFLICT_NOTICE_INVALID), "{outcome:?}");
    // Nor one carrying a Hangul filler or a variation selector (default-ignorable, not Cf), in any
    // field it shows.
    for (key, value) in [
        ("created_username", "sauveur\u{3164}"),
        ("created_display_name", "Responsable\u{FE0F} Maintenance"),
        ("created_display_name", "\u{115F}"),
        ("redeemed_device_code", "7K\u{034F}Q"),
    ] {
        let mut ignorable = original.clone();
        ignorable.insert(key.into(), Value::String(value.into()));
        ignorable.insert("code_id".into(), Value::String("other-code-3".into()));
        let outcome = receive(
            &b,
            NOTICE_ENTITY,
            &uuid::Uuid::new_v4().to_string(),
            "upsert",
            &ignorable,
        )
        .await;
        assert_eq!(
            conflict_code(&outcome),
            Some(CONFLICT_NOTICE_INVALID),
            "{key} {value:?}: {outcome:?}"
        );
    }

    // On the PC itself, the database refuses any change of a notice or an acknowledgement, the
    // new administrator's or anyone's.
    let sauveur = user_id(&a, "sauveur").await;
    assert!(a
        .execute_unprepared(&format!(
            "UPDATE admin_recovery_notices SET created_display_name = 'x', recipient_user_id = {sauveur}"
        ))
        .await
        .is_err());
    let chef_a = user_id(&a, "chef").await;
    let id = notices_for(&a, Some(chef_a)).await.expect("chef")[0].id;
    acknowledge_notice_in(&a, chef_a, id).await.expect("ack");
    assert!(a
        .execute_unprepared("UPDATE admin_recovery_acknowledgements SET acknowledged_at = 'x'")
        .await
        .is_err());
}

#[tokio::test]
async fn the_recovered_administrator_counts_everywhere_as_a_lasting_plant_wide_administrator() {
    // A plant with no administrator at all: every check reads the recovered one alone.
    let db = activated_pc().await;
    assert!(
        !crate::commands::product_license::tenant_has_administrator_for_id(&db, TENANT)
            .await
            .expect("tenant")
    );
    redeem_admin_recovery_code_in(&db, input(&primary_code(), "sauveur"), primary_now())
        .await
        .expect("redeemed");
    let id = user_id(&db, "sauveur").await;
    // auth::last_admin
    assert_eq!(
        crate::auth::last_admin::lasting_administrator_ids(&db)
            .await
            .expect("lasting"),
        vec![id]
    );
    assert_eq!(
        crate::auth::last_admin::active_administrator_ids(&db)
            .await
            .expect("active"),
        vec![id]
    );
    // auth::rbac: elevated access and the settings right plant-wide.
    let id32 = i32::try_from(id).expect("id");
    assert!(crate::auth::rbac::user_has_elevated_admin_access(&db, id32)
        .await
        .expect("elevated"));
    assert!(crate::auth::rbac::check_permission(
        &db,
        id32,
        crate::rbac::permissions::ADM_SETTINGS,
        &crate::auth::rbac::PermissionScope::Global,
    )
    .await
    .expect("settings permission"));
    // commands::product_license (AdministratorScope::Tenant and Any) and the first-admin marker.
    assert!(
        crate::commands::product_license::tenant_has_administrator_for_id(&db, TENANT)
            .await
            .expect("tenant")
    );
    assert!(crate::commands::product_license::plant_has_administrator(&db)
        .await
        .expect("plant"));
    assert!(crate::auth::builtin_admin::first_admin_marker_closes(&db)
        .await
        .expect("marker"));
    // notices_for: a later code tells this administrator.
    let second = signed_variant(|p| p.code_id = "11111111-2222-4333-8444-555555555555".into());
    redeem_admin_recovery_code_in(&db, input(&second, "deuxieme"), primary_now())
        .await
        .expect("second");
    let notices = notices_for(&db, Some(id)).await.expect("notices");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].created_username, "deuxieme");
}

// ── Round 3 (review of 7d8220e9): N2 forged notices, N3 acknowledgement before its notice ────────

fn deferred(outcome: &ApplyOutcome) -> bool {
    matches!(outcome, ApplyOutcome::Deferred { .. })
}

#[tokio::test]
async fn a_forged_notice_is_refused() {
    let (a, _b, _server) = two_pcs_after_recovery().await;
    let genuine = wire_of(&a, NOTICE_ENTITY, &notice_sync_id(&a, "chef").await).await;
    let adjoint = sync_id_of(&a, "adjoint").await;
    let chef = sync_id_of(&a, "chef").await;
    // Each forged notice is new (its own sync id) and addressed to adjoint; it is judged before it
    // could be written.
    let forge = |edits: Vec<(&str, &str)>| {
        let mut forged = genuine.clone();
        forged.insert("recipient_user_sync_id".into(), Value::String(adjoint.clone()));
        for (key, value) in edits {
            forged.insert(key.into(), Value::String(value.into()));
        }
        forged
    };
    for (case, forged) in [
        (
            "a display name posing as someone else",
            forge(vec![("created_display_name", "Jean Dupont")]),
        ),
        ("another username", forge(vec![("created_username", "chef2")])),
        (
            "another created account than the one redeemed",
            forge(vec![
                ("created_user_sync_id", chef.as_str()),
                ("created_username", "chef"),
                ("created_display_name", "chef"),
            ]),
        ),
        (
            "a device code of another shape",
            forge(vec![("redeemed_device_code", "pc-1")]),
        ),
        ("a lowercase device code", forge(vec![("redeemed_device_code", "7kq2")])),
        ("a date that is not RFC 3339", forge(vec![("created_at", "hier")])),
    ] {
        let outcome = receive(&a, NOTICE_ENTITY, &uuid::Uuid::new_v4().to_string(), "upsert", &forged).await;
        assert_eq!(
            conflict_code(&outcome),
            Some(CONFLICT_NOTICE_INVALID),
            "{case}: {outcome:?}"
        );
    }
    // Nothing was written: chef's and adjoint's genuine notices only.
    assert_eq!(scalar(&a, "SELECT COUNT(*) AS n FROM admin_recovery_notices").await, 2);

    // A notice for an account or a redemption this PC has not received yet waits for it.
    let unknown_account = forge(vec![("created_user_sync_id", "00000000-0000-4000-8000-000000000000")]);
    let outcome = receive(
        &a,
        NOTICE_ENTITY,
        &uuid::Uuid::new_v4().to_string(),
        "upsert",
        &unknown_account,
    )
    .await;
    assert!(deferred(&outcome), "{outcome:?}");
    // Without the redemption's audit record here, a display name other than the account's waits
    // for that record...
    let unknown_code = forge(vec![
        ("code_id", "no-such-code"),
        ("created_display_name", "Jean Dupont"),
    ]);
    let outcome = receive(
        &a,
        NOTICE_ENTITY,
        &uuid::Uuid::new_v4().to_string(),
        "upsert",
        &unknown_code,
    )
    .await;
    assert!(deferred(&outcome), "{outcome:?}");
    // ...while the account's own names are accepted (the notice then describes the account truly).
    let truthful = forge(vec![("code_id", "no-such-code")]);
    let outcome = receive(
        &a,
        NOTICE_ENTITY,
        &uuid::Uuid::new_v4().to_string(),
        "upsert",
        &truthful,
    )
    .await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
}

#[tokio::test]
async fn an_acknowledgement_before_its_notice_waits_and_a_renamed_account_keeps_its_notices() {
    let (a, b, mut server) = two_pcs_after_recovery().await;
    // The new administrator edits his own display name after the redemption: the notices must
    // still be accepted everywhere (they are checked against the redemption's audit record).
    let sauveur_a = user_id(&a, "sauveur").await;
    a.execute_unprepared(&format!(
        "UPDATE user_accounts SET display_name = 'Jean Dupont' WHERE id = {sauveur_a}"
    ))
    .await
    .expect("rename");
    server.exchange("A", &a).await;

    // Chef acknowledges on B; that acknowledgement is not sent yet.
    let chef_b = user_id(&b, "chef").await;
    let notice_b = notices_for(&b, Some(chef_b)).await.expect("chef on B")[0].id;
    acknowledge_notice_in(&b, chef_b, notice_b).await.expect("ack on B");
    let ack_sync: String = b
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT entity_sync_id AS v FROM admin_recovery_acknowledgements".to_string(),
        ))
        .await
        .expect("query")
        .and_then(|r| r.try_get::<String>("", "v").ok())
        .expect("ack");
    let ack = wire_of(&b, ACK_ENTITY, &ack_sync).await;
    let notice_sync = notice_sync_id(&b, "chef").await;
    let notice = wire_of(&b, NOTICE_ENTITY, &notice_sync).await;

    // A third PC, still empty: the acknowledgement arrives before its notice and waits; so does
    // the notice before its account.
    let c = started_device_db().await;
    let outcome = receive(&c, ACK_ENTITY, &ack_sync, "upsert", &ack).await;
    assert!(deferred(&outcome), "acknowledgement before its notice: {outcome:?}");
    let outcome = receive(&c, NOTICE_ENTITY, &notice_sync, "upsert", &notice).await;
    assert!(deferred(&outcome), "notice before its account: {outcome:?}");
    assert_eq!(
        scalar(&c, "SELECT COUNT(*) AS n FROM admin_recovery_acknowledgements").await,
        0
    );

    // C then receives everything A sent: the renamed account, the audit record and the notices.
    let received = server.exchange("C", &c).await;
    assert!(
        received.materialized.errors.is_empty(),
        "{:?}",
        received.materialized.errors
    );
    assert_eq!(
        scalar(
            &c,
            "SELECT COUNT(*) AS n FROM sync_conflicts WHERE entity_type LIKE 'admin_recovery%'"
        )
        .await,
        0
    );
    let chef_c = user_id(&c, "chef").await;
    let on_c = notices_for(&c, Some(chef_c)).await.expect("chef on C");
    assert_eq!(on_c.len(), 1, "the notice is shown on C despite the later rename");
    assert_eq!(on_c[0].created_display_name, "Responsable Maintenance");
    // The waiting acknowledgement now applies, and clears the notice on C.
    let outcome = receive(&c, ACK_ENTITY, &ack_sync, "upsert", &ack).await;
    assert!(matches!(outcome, ApplyOutcome::Applied), "{outcome:?}");
    assert!(notices_for(&c, Some(chef_c)).await.expect("chef on C").is_empty());
}
