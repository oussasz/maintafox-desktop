//! The activation calls sent from Rust (preview, claim, key entry of the pending claim, tenant
//! initialisation, re-claim with the stored key) against the in-process control API stub of the
//! policy refresh tests. The activation token and the stored licence key never come back to the
//! webview; the key entry records only the claim held in Rust.

use serde_json::{json, Value};

use sea_orm::DatabaseConnection;

use super::policy_refresh_tests::{refusal_code, start_stub, ADMIN, SIGNED_OUT};
use super::{
    bootstrap_initial_tenant_admin_in, claim_product_activation_in, get_activation_claim_record,
    load_state_record_from_db, mark_activation_tenant_initialized_in, preview_product_activation_in,
    reclaim_product_activation_in, record_activation_claim_for_tests, refuse_success_from_webview,
    submit_pending_product_license_key_in, ActivationCallResult, BootstrapInitialAdminInput,
    PendingActivationClaimSlot, ProductLicenseActivationStatus,
};
use crate::errors::AppError;

const KEY: &str = "MFX-KEY-CALLS-0001";
const TENANT: &str = "tenant-calls";
const FINGERPRINT: &str = "fp-calls-1";
/// The token the claim answer issues; must stay in Rust.
const CLAIM_TOKEN: &str = "claimed-activation-token-91c2";
const STORED_TOKEN: &str = "stored-activation-token-44d0";

fn claim_answer(token: &str) -> Value {
    claim_answer_for(TENANT, token)
}

fn claim_answer_for(tenant: &str, token: &str) -> Value {
    json!({
        "tenant_id": tenant,
        "license_id": "lic-calls",
        "machine_fingerprint": FINGERPRINT,
        "activation_token": token,
        "edition": "professional",
        "company_name": "Usine Calls",
        "is_initialized": false,
        "nested": { "activation_token": token, "note": format!("issued {token}") },
    })
}

fn gate_code(err: &AppError) -> Option<String> {
    match err {
        AppError::ValidationFailed(messages) => messages.first().and_then(|m| m.split(':').next()).map(str::to_string),
        _ => None,
    }
}

fn assert_no_secret<T: serde::Serialize>(result: &T, secrets: &[&str]) {
    let text = serde_json::to_string(result).expect("result json");
    for secret in secrets {
        assert!(!text.contains(secret), "{secret} leaked into the result: {text}");
    }
}

async fn claimed(db_slot: &PendingActivationClaimSlot) {
    claimed_with(db_slot, KEY, claim_answer(CLAIM_TOKEN)).await;
}

/// A claim taken with `key`, the server answering `answer`.
async fn claimed_with(db_slot: &PendingActivationClaimSlot, key: &str, answer: Value) {
    let stub = start_stub(200, answer.to_string()).await;
    let result = claim_product_activation_in(db_slot, &stub.base_url, key, FINGERPRINT, Some("Bench"), Some("1.2.3"))
        .await
        .expect("claim");
    assert!(matches!(result, ActivationCallResult::Answered { .. }), "{result:?}");
}

/// A PC whose key `KEY` was entered for `TENANT` (claim token `CLAIM_TOKEN`), then given its first
/// administrator: licence changes then need one.
async fn administered_plant() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    let slot = PendingActivationClaimSlot::default();
    claimed(&slot).await;
    submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect("key entry");
    crate::sync::plant_join::confirm_empty_plant_for_tests(&db, TENANT).await;
    bootstrap_initial_tenant_admin_in(
        &db,
        BootstrapInitialAdminInput {
            username: "chef".into(),
            email: "chef@plant.example".into(),
            password: "Chef-Usine-2026!Sûr".into(),
            display_name: None,
        },
    )
    .await
    .expect("first administrator");
    db
}

/// The whole licence record stored here, to compare before and after a refused change.
async fn stored_record_json(db: &DatabaseConnection) -> String {
    let record = load_state_record_from_db(db)
        .await
        .expect("record")
        .expect("stored record");
    serde_json::to_string(&record).expect("record json")
}

#[tokio::test]
async fn the_preview_is_sent_without_a_token_and_returned_as_answered() {
    let mut answer = claim_answer("should-not-be-there");
    answer["device_limit"] = json!(5);
    let stub = start_stub(200, answer.to_string()).await;
    let result =
        preview_product_activation_in(&stub.base_url, &format!(" {KEY} "), FINGERPRINT, Some(" "), Some("bad"))
            .await
            .expect("preview");
    let ActivationCallResult::Answered { answer } = &result else {
        panic!("expected answered, got {result:?}");
    };
    assert_eq!(answer["tenant_id"], json!(TENANT));
    assert_eq!(answer["device_limit"], json!(5));
    assert!(answer.get("activation_token").is_none());
    assert!(answer["nested"].get("activation_token").is_none());
    assert_no_secret(&result, &["should-not-be-there"]);

    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, "/api/v1/activation/preview");
    assert_eq!(requests[0].authorization, None);
    // The key trimmed; no label when blank; no malformed version.
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).expect("body"),
        json!({ "license_key": KEY, "machine_fingerprint": FINGERPRINT })
    );
}

#[tokio::test]
async fn a_preview_error_carries_the_server_code_without_the_key() {
    let body = json!({ "error": "license_not_found", "message": format!("no licence {KEY}") }).to_string();
    let stub = start_stub(404, body).await;
    let result = preview_product_activation_in(&stub.base_url, KEY, FINGERPRINT, None, None)
        .await
        .expect("preview");
    let ActivationCallResult::HttpError {
        status, code, message, ..
    } = &result
    else {
        panic!("expected http_error, got {result:?}");
    };
    assert_eq!(*status, 404);
    assert_eq!(code.as_deref(), Some("license_not_found"));
    assert_eq!(message.as_deref(), Some("no licence [licence key]"));
    assert_no_secret(&result, &[KEY]);

    let stub = start_stub(200, "[1, 2]".to_string()).await;
    let result = preview_product_activation_in(&stub.base_url, KEY, FINGERPRINT, None, None)
        .await
        .expect("preview");
    assert!(
        matches!(result, ActivationCallResult::InvalidResponse { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_short_key_or_blank_fingerprint_is_refused_before_sending() {
    let stub = start_stub(200, claim_answer(CLAIM_TOKEN).to_string()).await;
    let err = preview_product_activation_in(&stub.base_url, " short ", FINGERPRINT, None, None)
        .await
        .expect_err("short key");
    assert_eq!(gate_code(&err).as_deref(), Some("GATE_LICENSE_KEY_SHORT"));
    let slot = PendingActivationClaimSlot::default();
    let err = claim_product_activation_in(&slot, &stub.base_url, KEY, "  ", None, None)
        .await
        .expect_err("blank fingerprint");
    assert_eq!(refusal_code(&err).as_deref(), Some("LICENSE_CLAIM_INVALID"));
    assert!(stub.requests().is_empty());
}

#[tokio::test]
async fn the_claim_keeps_its_token_in_rust_and_returns_the_rest() {
    let slot = PendingActivationClaimSlot::default();
    let stub = start_stub(200, claim_answer(CLAIM_TOKEN).to_string()).await;
    let result = claim_product_activation_in(&slot, &stub.base_url, KEY, FINGERPRINT, Some("Bench"), Some("1.2.3"))
        .await
        .expect("claim");
    let ActivationCallResult::Answered { answer } = &result else {
        panic!("expected answered, got {result:?}");
    };
    assert_eq!(answer["tenant_id"], json!(TENANT));
    assert_eq!(answer["is_initialized"], json!(false));
    assert_eq!(answer["nested"]["note"], json!("issued [activation token]"));
    assert_no_secret(&result, &[CLAIM_TOKEN]);

    let requests = stub.requests();
    assert_eq!(requests[0].path, "/api/v1/activation/claim");
    assert_eq!(requests[0].authorization, None);
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).expect("body"),
        json!({
            "license_key": KEY,
            "machine_fingerprint": FINGERPRINT,
            "machine_label": "Bench",
            "app_version": "1.2.3",
        })
    );

    let pending = slot.lock().await.clone().expect("pending claim");
    assert_eq!(pending.claim.activation_token, CLAIM_TOKEN);
    assert_eq!(pending.claim.license_tier.as_deref(), Some("professional"));
    assert_eq!(pending.machine_fingerprint, FINGERPRINT);
    assert!(
        !format!("{pending:?}").contains(CLAIM_TOKEN),
        "Debug never shows the token"
    );
}

#[tokio::test]
async fn a_failed_or_incomplete_claim_leaves_no_pending_claim() {
    let slot = PendingActivationClaimSlot::default();
    claimed(&slot).await;

    // An answer without a token is not a claim: nothing pending, the earlier claim dropped.
    let mut answer = claim_answer(CLAIM_TOKEN);
    answer.as_object_mut().expect("object").remove("activation_token");
    let stub = start_stub(200, answer.to_string()).await;
    let result = claim_product_activation_in(&slot, &stub.base_url, KEY, FINGERPRINT, None, None)
        .await
        .expect("claim");
    assert!(
        matches!(result, ActivationCallResult::InvalidResponse { .. }),
        "{result:?}"
    );
    assert!(slot.lock().await.is_none());

    // An identifier the key entry would refuse (surrounding spaces): refused here, before the
    // webview wipes local data for it; the token never comes back in the message.
    let mut answer = claim_answer(CLAIM_TOKEN);
    answer["tenant_id"] = json!(format!(" {TENANT}"));
    let stub = start_stub(200, answer.to_string()).await;
    let result = claim_product_activation_in(&slot, &stub.base_url, KEY, FINGERPRINT, None, None)
        .await
        .expect("claim");
    assert!(
        matches!(result, ActivationCallResult::InvalidResponse { .. }),
        "{result:?}"
    );
    assert_no_secret(&result, &[CLAIM_TOKEN, KEY]);
    assert!(slot.lock().await.is_none());

    claimed(&slot).await;
    let stub = start_stub(409, String::new()).await;
    let result = claim_product_activation_in(&slot, &stub.base_url, KEY, FINGERPRINT, None, None)
        .await
        .expect("claim");
    assert_eq!(
        serde_json::to_value(&result).expect("json"),
        json!({ "kind": "http_error", "status": 409, "code": null, "message": null, "details": null })
    );
    assert!(slot.lock().await.is_none());
}

#[tokio::test]
async fn the_key_entry_records_only_the_pending_claim_of_that_key_and_organisation() {
    let db = crate::db::test_db::seeded().await;
    let slot = PendingActivationClaimSlot::default();

    let err = submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect_err("nothing pending");
    assert_eq!(gate_code(&err).as_deref(), Some("GATE_LICENSE_CLAIM_NOT_PENDING"));

    claimed(&slot).await;
    for (key, tenant) in [("MFX-KEY-OTHER-0001", TENANT), (KEY, "another-tenant")] {
        let err = submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, key, tenant, None)
            .await
            .expect_err("mismatch");
        assert_eq!(
            gate_code(&err).as_deref(),
            Some("GATE_LICENSE_CLAIM_MISMATCH"),
            "{key} {tenant}"
        );
    }
    assert!(load_state_record_from_db(&db).await.expect("record").is_none());
    assert!(slot.lock().await.is_some(), "a refused entry keeps the pending claim");

    submit_pending_product_license_key_in(
        &db,
        SIGNED_OUT,
        &slot,
        &format!(" {KEY} "),
        TENANT,
        Some("1.2.3".into()),
    )
    .await
    .expect("key entry");
    assert!(slot.lock().await.is_none(), "a recorded entry clears the pending claim");
    let record = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record");
    assert_eq!(record.status, ProductLicenseActivationStatus::Active);
    assert_eq!(record.license_key_plaintext.as_deref(), Some(KEY));
    assert_eq!(record.machine_fingerprint.as_deref(), Some(FINGERPRINT));
    let claim = record.activation_claim.expect("claim");
    assert_eq!(claim.tenant_id, TENANT);
    assert_eq!(claim.activation_token, CLAIM_TOKEN);

    let err = submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect_err("consumed");
    assert_eq!(gate_code(&err).as_deref(), Some("GATE_LICENSE_CLAIM_NOT_PENDING"));
}

#[tokio::test]
async fn tenant_initialisation_uses_the_stored_token_never_a_pending_one() {
    let db = crate::db::test_db::seeded().await;
    let slot = PendingActivationClaimSlot::default();
    let stub = start_stub(200, json!({ "tenant_id": TENANT, "is_initialized": true }).to_string()).await;

    // A claim taken and not recorded (its key entry abandoned): nothing stored, nothing sent.
    claimed(&slot).await;
    let result = mark_activation_tenant_initialized_in(&db, &stub.base_url)
        .await
        .expect("initialise");
    assert_eq!(
        serde_json::to_value(&result).expect("json"),
        json!({ "kind": "not_activated" })
    );
    assert!(stub.requests().is_empty(), "nothing sent with a pending token");

    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("stored claim");
    let result = mark_activation_tenant_initialized_in(&db, &stub.base_url)
        .await
        .expect("initialise");
    assert_eq!(
        serde_json::to_value(&result).expect("json"),
        json!({ "kind": "answered", "answer": { "tenant_id": TENANT, "is_initialized": true } })
    );
    // A newer claim for another organisation pending: still the stored activation's token.
    claimed_with(
        &slot,
        KEY,
        claim_answer_for("tenant-other", "other-activation-token-0b3e"),
    )
    .await;
    mark_activation_tenant_initialized_in(&db, &stub.base_url)
        .await
        .expect("initialise");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].path, "/api/v1/activation/tenant-status/initialize");
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).expect("body"),
        json!({})
    );
    for request in &requests {
        assert_eq!(
            request.authorization.as_deref(),
            Some(format!("Bearer {STORED_TOKEN}").as_str())
        );
    }
}

#[tokio::test]
async fn tenant_initialisation_is_still_sent_for_an_organisation_stored_as_initialised() {
    // The first initialisation's answer was lost and a refresh has since stored
    // `is_initialized: true`: the gate's retry must still finish (the server answers idempotently).
    let db = crate::db::test_db::seeded().await;
    let slot = PendingActivationClaimSlot::default();
    let mut answer = claim_answer(CLAIM_TOKEN);
    answer["is_initialized"] = json!(true);
    claimed_with(&slot, KEY, answer).await;
    submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect("key entry");
    let stub = start_stub(200, json!({ "tenant_id": TENANT, "is_initialized": true }).to_string()).await;
    let result = mark_activation_tenant_initialized_in(&db, &stub.base_url)
        .await
        .expect("initialise");
    assert!(matches!(result, ActivationCallResult::Answered { .. }), "{result:?}");
    assert_eq!(stub.requests().len(), 1);
    let stub = start_stub(200, json!({ "tenant_id": TENANT, "is_initialized": true }).to_string()).await;

    // Stored as not initialised: the retry after a failed initialisation still sends.
    let db = crate::db::test_db::seeded().await;
    claimed(&slot).await;
    submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect("key entry");
    let failing = start_stub(503, String::new()).await;
    let result = mark_activation_tenant_initialized_in(&db, &failing.base_url)
        .await
        .expect("initialise");
    assert!(
        matches!(result, ActivationCallResult::HttpError { status: 503, .. }),
        "{result:?}"
    );
    let result = mark_activation_tenant_initialized_in(&db, &stub.base_url)
        .await
        .expect("retry");
    assert!(matches!(result, ActivationCallResult::Answered { .. }), "{result:?}");
    assert_eq!(
        stub.requests()[0].authorization.as_deref(),
        Some(format!("Bearer {CLAIM_TOKEN}").as_str())
    );
}

#[tokio::test]
async fn a_tenant_initialisation_error_never_returns_the_token() {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("stored claim");
    let stub = start_stub(404, format!("no route for {STORED_TOKEN}")).await;
    let result = mark_activation_tenant_initialized_in(&db, &stub.base_url)
        .await
        .expect("initialise");
    let ActivationCallResult::HttpError { status, details, .. } = &result else {
        panic!("expected http_error, got {result:?}");
    };
    assert_eq!(*status, 404);
    assert_eq!(details.as_deref(), Some("no route for [activation token]"));
    assert_no_secret(&result, &[STORED_TOKEN]);

    let stub = start_stub(200, json!({ "tenant_id": TENANT }).to_string()).await;
    let result = mark_activation_tenant_initialized_in(&db, &stub.base_url)
        .await
        .expect("initialise");
    assert!(
        matches!(result, ActivationCallResult::InvalidResponse { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn the_reclaim_uses_the_stored_key_and_records_the_new_claim() {
    let db = crate::db::test_db::seeded().await;
    let stub = start_stub(200, claim_answer("unused").to_string()).await;
    let result = reclaim_product_activation_in(&db, SIGNED_OUT, &stub.base_url, None, None)
        .await
        .expect("reclaim");
    assert!(matches!(result, ActivationCallResult::NotActivated), "{result:?}");

    // Stored without a key (as `record_activation_claim_for_tests`): nothing to claim with.
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("stored claim");
    let result = reclaim_product_activation_in(&db, SIGNED_OUT, &stub.base_url, None, None)
        .await
        .expect("reclaim");
    assert!(matches!(result, ActivationCallResult::NotActivated), "{result:?}");
    assert!(stub.requests().is_empty());

    // A key entered here: the reclaim sends it with the stored fingerprint.
    let db = crate::db::test_db::seeded().await;
    let slot = PendingActivationClaimSlot::default();
    claimed(&slot).await;
    submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect("key entry");
    let renewed = "renewed-activation-token-5e77";
    let stub = start_stub(200, claim_answer(renewed).to_string()).await;
    let result = reclaim_product_activation_in(&db, ADMIN, &stub.base_url, Some("Bench"), Some("1.2.4"))
        .await
        .expect("reclaim");
    let ActivationCallResult::Answered { answer } = &result else {
        panic!("expected answered, got {result:?}");
    };
    assert_eq!(answer.status, ProductLicenseActivationStatus::Active);
    assert_no_secret(&result, &[KEY, renewed, CLAIM_TOKEN]);

    let requests = stub.requests();
    assert_eq!(requests[0].path, "/api/v1/activation/claim");
    assert_eq!(requests[0].authorization, None);
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).expect("body"),
        json!({
            "license_key": KEY,
            "machine_fingerprint": FINGERPRINT,
            "machine_label": "Bench",
            "app_version": "1.2.4",
        })
    );
    let claim = get_activation_claim_record(&db)
        .await
        .expect("claim")
        .expect("stored claim");
    assert_eq!(claim.activation_token, renewed);
    let record = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record");
    assert_eq!(record.app_version.as_deref(), Some("1.2.4"));
}

#[tokio::test]
async fn a_reclaim_error_never_returns_the_stored_key() {
    let db = crate::db::test_db::seeded().await;
    let slot = PendingActivationClaimSlot::default();
    claimed(&slot).await;
    submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect("key entry");
    let stub = start_stub(
        403,
        json!({ "code": "license_revoked", "detail": format!("key {KEY}") }).to_string(),
    )
    .await;
    let result = reclaim_product_activation_in(&db, SIGNED_OUT, &stub.base_url, None, None)
        .await
        .expect("reclaim");
    let ActivationCallResult::HttpError { code, message, .. } = &result else {
        panic!("expected http_error, got {result:?}");
    };
    assert_eq!(code.as_deref(), Some("license_revoked"));
    assert_eq!(message.as_deref(), Some("key [licence key]"));
    assert_no_secret(&result, &[KEY]);
    // Nothing recorded: the webview records the denial, as after a policy refresh.
    let record = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record");
    assert_eq!(record.status, ProductLicenseActivationStatus::Active);
}

#[tokio::test]
async fn a_claim_answer_for_another_machine_is_not_held_nor_recorded() {
    let slot = PendingActivationClaimSlot::default();
    let mut answer = claim_answer(CLAIM_TOKEN);
    answer["machine_fingerprint"] = json!("fp-another-machine");
    let stub = start_stub(200, answer.to_string()).await;
    let result = claim_product_activation_in(&slot, &stub.base_url, KEY, FINGERPRINT, None, None)
        .await
        .expect("claim");
    assert!(
        matches!(result, ActivationCallResult::InvalidResponse { .. }),
        "{result:?}"
    );
    assert_no_secret(&result, &[CLAIM_TOKEN, KEY]);
    assert!(slot.lock().await.is_none());

    // The re-claim of an administered plant, signed in as an administrator: refused all the same,
    // nothing written.
    let db = administered_plant().await;
    let before = stored_record_json(&db).await;
    let stub = start_stub(200, answer.to_string()).await;
    let result = reclaim_product_activation_in(&db, ADMIN, &stub.base_url, None, None)
        .await
        .expect("reclaim");
    assert!(
        matches!(result, ActivationCallResult::InvalidResponse { .. }),
        "{result:?}"
    );
    assert_no_secret(&result, &[CLAIM_TOKEN, KEY]);
    assert_eq!(before, stored_record_json(&db).await, "nothing written");
}

#[tokio::test]
async fn a_signed_out_reclaim_of_another_activation_or_token_is_refused_and_writes_nothing() {
    let db = administered_plant().await;
    let before = stored_record_json(&db).await;
    let mut other_licence = claim_answer(CLAIM_TOKEN);
    other_licence["license_id"] = json!("lic-other");
    for (case, answer) in [
        ("another organisation", claim_answer_for("tenant-other", CLAIM_TOKEN)),
        ("another licence", other_licence),
        ("a token not bound here", claim_answer("unbound-activation-token-6d21")),
    ] {
        let stub = start_stub(200, answer.to_string()).await;
        let err = reclaim_product_activation_in(&db, SIGNED_OUT, &stub.base_url, None, None)
            .await
            .expect_err(case);
        assert_eq!(
            refusal_code(&err).as_deref(),
            Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
            "{case}: {err:?}"
        );
        let text = format!("{err:?}");
        assert!(
            !text.contains(KEY) && !text.contains("unbound-activation-token-6d21"),
            "{case}: {text}"
        );
        assert_eq!(before, stored_record_json(&db).await, "{case}: nothing written");
    }
    let claim = get_activation_claim_record(&db)
        .await
        .expect("claim")
        .expect("stored claim");
    assert_eq!(claim.activation_token, CLAIM_TOKEN, "the stored token is kept");
}

#[tokio::test]
async fn a_signed_out_key_entry_while_the_licence_is_usable_is_refused_and_keeps_the_claim() {
    let db = administered_plant().await;
    let before = stored_record_json(&db).await;
    let slot = PendingActivationClaimSlot::default();
    claimed_with(&slot, KEY, claim_answer("second-activation-token-2c48")).await;
    let err = submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect_err("a key entry on a usable licence needs an administrator");
    assert_eq!(
        refusal_code(&err).as_deref(),
        Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
        "{err:?}"
    );
    assert!(slot.lock().await.is_some(), "the pending claim is kept");
    assert_eq!(before, stored_record_json(&db).await, "nothing written");
}

#[tokio::test]
async fn a_key_entry_after_a_newer_claim_with_another_key_is_a_mismatch() {
    let db = crate::db::test_db::seeded().await;
    let slot = PendingActivationClaimSlot::default();
    let key_b = "MFX-KEY-CALLS-0002";
    claimed(&slot).await;
    claimed_with(&slot, key_b, claim_answer("key-b-activation-token-8e15")).await;
    let err = submit_pending_product_license_key_in(&db, SIGNED_OUT, &slot, KEY, TENANT, None)
        .await
        .expect_err("the claim waiting is key B's");
    assert_eq!(gate_code(&err).as_deref(), Some("GATE_LICENSE_CLAIM_MISMATCH"));
    assert!(slot.lock().await.is_some(), "key B's claim is kept");
    assert!(load_state_record_from_db(&db).await.expect("record").is_none());
}

#[test]
fn the_licence_records_never_show_their_secrets_in_debug() {
    let record: super::ProductLicenseStateRecord = serde_json::from_value(json!({
        "schema_version": 1,
        "key_sha256": "hash",
        "license_key_plaintext": KEY,
        "submitted_at": "2026-10-25T00:00:00Z",
        "status": "active",
        "pending_online_validation": false,
        "activation_claim": {
            "tenant_id": TENANT,
            "license_id": "lic-calls",
            "machine_fingerprint": FINGERPRINT,
            "activation_token": CLAIM_TOKEN,
        },
        "reconciliation": { "retry_attempt": 0 },
        "diagnostics": [],
    }))
    .expect("record");
    let text = format!("{record:?}");
    assert!(!text.contains(KEY) && !text.contains(CLAIM_TOKEN), "{text}");
    assert!(text.contains(TENANT), "the rest is shown: {text}");
}

#[test]
fn a_success_outcome_from_the_webview_is_refused() {
    let success = json!({ "kind": "success", "claim": { "tenant_id": TENANT } }).to_string();
    let err = refuse_success_from_webview(&success).expect_err("success refused");
    assert_eq!(
        gate_code(&err).as_deref(),
        Some("GATE_LICENSE_SUCCESS_FROM_SERVER_ONLY")
    );
    for allowed in [
        r#"{"kind":"denied","error_code":"license_revoked","error_message":"revoked"}"#,
        r#"{"kind":"network_error","error_message":"offline"}"#,
        r#"{"kind":"http_error","error_message":"502"}"#,
        "not json",
    ] {
        refuse_success_from_webview(allowed).expect(allowed);
    }
}
