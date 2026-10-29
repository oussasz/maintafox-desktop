//! The phone enrolment (`phone_enrolment`): the QR text and the typed code read strictly before any
//! call, the redemption against the in-process control API stub of the policy refresh tests, the
//! binding stored with the activation (no licence key) and erased with it.

use serde_json::{json, Value};

use super::super::policy_refresh_tests::{start_stub, SIGNED_OUT};
use super::super::{
    load_state_record_from_db, mobile_enrolment_binding, record_activation_claim_for_tests,
    refresh_product_activation_policy_in, reset_to_first_run_in, PolicyRefreshResult, ProductLicenseActivationStatus,
};
use super::*;
use crate::commands::mobile_enrolment::qr_payload;
use crate::errors::AppError;

const API_BASE: &str = "https://api.maintafox.example";
const TOKEN: &str = "Zx9_kq-3TfA0bVw7LmN4pR8sYc2dE6gH1jKoU5iXzQa";
const TENANT: &str = "tenant-phones";
const FINGERPRINT: &str = "fp-phone-1";
const DEVICE_TOKEN: &str = "device-token-phone-77ab";

fn phone() -> PhoneIdentity {
    PhoneIdentity {
        machine_fingerprint: FINGERPRINT.into(),
        machine_label: PHONE_LABEL.into(),
        app_version: "1.2.3".into(),
    }
}

fn qr(api_base: &str) -> RedeemMobileEnrolmentInput {
    RedeemMobileEnrolmentInput {
        qr_payload: Some(qr_payload(api_base, TOKEN)),
        code: None,
    }
}

fn typed(code: &str) -> RedeemMobileEnrolmentInput {
    RedeemMobileEnrolmentInput {
        qr_payload: None,
        code: Some(code.into()),
    }
}

fn code_of(err: &AppError) -> String {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()).unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

fn param_of(err: &AppError, name: &str) -> Option<String> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().and_then(|i| i.params.get(name).cloned()),
        _ => None,
    }
}

fn redeem_answer() -> Value {
    json!({
        "tenant_id": TENANT,
        "license_id": "lic-phones",
        "machine_fingerprint": FINGERPRINT,
        "activation_token": DEVICE_TOKEN,
        "edition": "professional",
        "company_name": "Usine Téléphones",
        "is_initialized": false,
        "enrolment": {
            "enrolment_id": "enr-41",
            "person_sync_id": "person-sync-7",
            "person_label": "Amina Benali",
            "site_sync_id": "site-sync-2",
            "site_label": "Usine Nord",
            "enrolled_at": "2026-10-29T08:00:00.000Z",
        },
    })
}

// ── QR text ─────────────────────────────────────────────────────────────────────────────────────

#[test]
fn the_desktop_qr_gives_its_token_for_this_server() {
    assert_eq!(
        token_of_qr(&qr_payload(API_BASE, TOKEN), API_BASE).expect("token"),
        TOKEN
    );
    // Trailing slashes on either side do not matter; the scanner's trailing newline neither.
    let payload = format!("{}\n", qr_payload(&format!("{API_BASE}/"), TOKEN));
    assert_eq!(token_of_qr(&payload, &format!("{API_BASE}/")).expect("token"), TOKEN);
}

#[test]
fn a_qr_for_another_server_is_refused() {
    for other in [
        "https://evil.example",
        "https://api.maintafox.example.evil.example",
        "http://api.maintafox.example",
        "https://api.maintafox.example:8443",
        "https://api.maintafox.example/other",
        "https://API.maintafox.example",
    ] {
        let err = token_of_qr(&qr_payload(other, TOKEN), API_BASE).expect_err(other);
        assert_eq!(code_of(&err), ENROLMENT_QR_OTHER_SERVER, "{other}");
    }
}

#[test]
fn a_qr_with_another_shape_is_refused() {
    let s = "https%3A%2F%2Fapi.maintafox.example";
    for payload in [
        format!("maintafox-enrol:2?s={s}&t={TOKEN}"),
        format!("maintafox-enrol:1?t={TOKEN}"),
        format!("maintafox-enrol:1?s={s}"),
        format!("maintafox-enrol:1?s={s}&t={TOKEN}&x=1"),
        format!("maintafox-enrol:1?s={s}&t={TOKEN}&t={TOKEN}"),
        format!("maintafox-enrol:1?s={s}&s={s}&t={TOKEN}"),
        format!("maintafox-enrol:1?s={s}&t="),
        format!("maintafox-enrol:1?s={s}&t"),
        format!("maintafox-enrol:1?s={s}&&t={TOKEN}"),
        format!("MAINTAFOX-ENROL:1?s={s}&t={TOKEN}"),
        format!("https://api.maintafox.example/?s={s}&t={TOKEN}"),
        // Raw reserved characters, `+`, a bad escape.
        format!("maintafox-enrol:1?s=https://api.maintafox.example&t={TOKEN}"),
        format!("maintafox-enrol:1?s={s}&t={TOKEN}+"),
        format!("maintafox-enrol:1?s=%ZZ&t={TOKEN}"),
        format!("maintafox-enrol:1?s=%C3&t={TOKEN}"),
        String::new(),
    ] {
        let err = token_of_qr(&payload, API_BASE).expect_err(&payload);
        assert_eq!(code_of(&err), ENROLMENT_QR_INVALID, "{payload}");
    }
}

#[test]
fn a_qr_token_must_be_base64url_of_the_accepted_length() {
    for bad in [
        "short".to_string(),
        "a".repeat(15),
        "a".repeat(129),
        format!("{}=", &TOKEN[..42]),
        format!("{}/", &TOKEN[..42]),
        "é".repeat(20),
    ] {
        let err = token_of_qr(&qr_payload(API_BASE, &bad), API_BASE).expect_err(&bad);
        assert_eq!(code_of(&err), ENROLMENT_QR_INVALID, "{bad}");
    }
    assert!(token_of_qr(&qr_payload(API_BASE, &"a".repeat(16)), API_BASE).is_ok());
    assert!(token_of_qr(&qr_payload(API_BASE, &"a".repeat(128)), API_BASE).is_ok());
}

// ── Typed code ──────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_typed_code_is_read_as_the_server_reads_it() {
    assert_eq!(normalise_enrolment_code("7K3M-9P2Q").as_deref(), Some("7K3M9P2Q"));
    assert_eq!(normalise_enrolment_code(" 7k3m 9p2q ").as_deref(), Some("7K3M9P2Q"));
    assert_eq!(normalise_enrolment_code("il0o-ab12").as_deref(), Some("1100AB12"));
    assert_eq!(normalise_enrolment_code("--7K3M\t9P2Q--").as_deref(), Some("7K3M9P2Q"));
}

#[test]
fn a_typed_code_that_cannot_be_one_is_refused() {
    for bad in [
        "",
        "7K3M-9P2",
        "7K3M-9P2QX",
        "7K3M-9P2U",
        "7K3M_9P2Q",
        "7K3M-9P2Ü",
        "７K3M-9P2Q",
        "7K3M.9P2Q",
    ] {
        assert_eq!(normalise_enrolment_code(bad), None, "{bad}");
    }
    let err = secret_of_input(&typed("ABCU-1234"), API_BASE).expect_err("refused");
    assert_eq!(code_of(&err), ENROLMENT_CODE_INVALID);
}

#[test]
fn exactly_one_of_the_qr_and_the_code() {
    let both = RedeemMobileEnrolmentInput {
        qr_payload: Some(qr_payload(API_BASE, TOKEN)),
        code: Some("7K3M-9P2Q".into()),
    };
    let neither = RedeemMobileEnrolmentInput {
        qr_payload: Some("  ".into()),
        code: None,
    };
    for input in [both, neither] {
        let err = secret_of_input(&input, API_BASE).expect_err("refused");
        assert_eq!(code_of(&err), ENROLMENT_INPUT_REQUIRED);
    }
}

#[test]
fn debug_never_shows_the_token_or_the_code() {
    let input = RedeemMobileEnrolmentInput {
        qr_payload: Some(qr_payload(API_BASE, TOKEN)),
        code: Some("7K3M-9P2Q".into()),
    };
    let text = format!(
        "{input:?} {:?} {:?}",
        EnrolmentSecret::Token(TOKEN.into()),
        EnrolmentSecret::Code("7K3M9P2Q".into())
    );
    assert!(
        !text.contains(TOKEN) && !text.contains("7K3M") && !text.contains("9P2Q"),
        "{text}"
    );
}

// ── Redemption ──────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_redeemed_qr_activates_the_phone_bound_to_its_person_without_a_licence_key() {
    let db = crate::db::test_db::seeded().await;
    let stub = start_stub(200, redeem_answer().to_string()).await;
    let redeemed = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &qr(&stub.base_url), &phone())
        .await
        .expect("redeemed");
    assert_eq!(redeemed.person_label, "Amina Benali");
    assert_eq!(redeemed.site_sync_id, "site-sync-2");
    assert_eq!(redeemed.tenant_id, TENANT);
    let shown = serde_json::to_string(&redeemed).expect("json");
    assert!(!shown.contains(DEVICE_TOKEN) && !shown.contains(TOKEN), "{shown}");

    // The request: the token, this phone as the engine knows it, nothing from the webview.
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, REDEEM_PATH);
    assert_eq!(requests[0].authorization, None);
    let body: Value = serde_json::from_str(&requests[0].body).expect("body");
    assert_eq!(
        body,
        json!({
            "token": TOKEN,
            "machine_fingerprint": FINGERPRINT,
            "machine_label": PHONE_LABEL,
            "app_version": "1.2.3",
            "device_class": "mobile",
        })
    );

    let record = load_state_record_from_db(&db).await.expect("load").expect("record");
    assert_eq!(record.status, ProductLicenseActivationStatus::Active);
    assert_eq!(record.license_key_plaintext, None, "no licence key on a phone");
    assert_eq!(
        record.key_sha256,
        crate::settings::sha256_hex("mobile-enrolment:enr-41")
    );
    let claim = record.activation_claim.as_ref().expect("claim");
    assert_eq!(claim.activation_token, DEVICE_TOKEN);
    assert_eq!(claim.machine_fingerprint, FINGERPRINT);
    let binding = mobile_enrolment_binding(&db).await.expect("binding").expect("bound");
    assert_eq!(binding.enrolment_id, "enr-41");
    assert_eq!(binding.person_sync_id, "person-sync-7");
    assert_eq!(binding.enrolled_at, "2026-10-29T08:00:00.000Z");
}

#[tokio::test]
async fn a_typed_code_is_sent_in_its_canonical_form() {
    let db = crate::db::test_db::seeded().await;
    let stub = start_stub(200, redeem_answer().to_string()).await;
    redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &typed("7k3m-9plq"), &phone())
        .await
        .expect("redeemed");
    let body: Value = serde_json::from_str(&stub.requests()[0].body).expect("body");
    assert_eq!(body["code"], "7K3M9P1Q");
    assert!(body.get("token").is_none());
}

#[tokio::test]
async fn nothing_is_sent_for_another_server_a_bad_code_or_an_activated_phone() {
    let db = crate::db::test_db::seeded().await;
    let stub = start_stub(200, redeem_answer().to_string()).await;
    let err = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &qr(API_BASE), &phone())
        .await
        .expect_err("other server");
    assert_eq!(code_of(&err), ENROLMENT_QR_OTHER_SERVER);
    let err = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &typed("7K3M-9P2"), &phone())
        .await
        .expect_err("bad code");
    assert_eq!(code_of(&err), ENROLMENT_CODE_INVALID);

    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, "stored-token-1")
        .await
        .expect("activated");
    let err = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &qr(&stub.base_url), &phone())
        .await
        .expect_err("activated");
    assert_eq!(code_of(&err), ENROLMENT_ALREADY_ACTIVATED);
    assert!(stub.requests().is_empty(), "nothing may reach a server");
}

#[tokio::test]
async fn nothing_is_sent_while_the_phone_erases_itself() {
    let db = crate::db::test_db::seeded().await;
    crate::sync::transport::removal::record(&db, crate::sync::transport::removal::RemovalKind::Removed)
        .await
        .expect("removal");
    let stub = start_stub(200, redeem_answer().to_string()).await;
    let err = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &qr(&stub.base_url), &phone())
        .await
        .expect_err("removal in progress");
    assert_eq!(code_of(&err), ENROLMENT_REMOVAL_IN_PROGRESS);
    assert!(stub.requests().is_empty());
}

#[tokio::test]
async fn server_refusals_have_stable_codes_and_never_echo_the_secret() {
    let cases = [
        (
            404,
            json!({ "error": "enrolment_invalid", "message": format!("no {TOKEN}") }),
            ENROLMENT_INVALID,
        ),
        (409, json!({ "error": "already_enrolled" }), ENROLMENT_ALREADY_ENROLLED),
        (409, json!({ "error": "wipe_pending" }), ENROLMENT_WIPE_PENDING),
        (403, json!({ "error": "license_revoked" }), ENROLMENT_LICENCE_REVOKED),
        (403, json!({ "error": "license_expired" }), ENROLMENT_LICENCE_EXPIRED),
        (404, json!({ "error": "license_not_found" }), ENROLMENT_LICENCE_INVALID),
        (403, json!({ "error": "mobile_not_licensed" }), ENROLMENT_NOT_LICENSED),
        (409, json!({ "error": "slot_limit_reached" }), ENROLMENT_NO_FREE_SLOT),
        (
            409,
            json!({ "error": "device_class_mismatch" }),
            ENROLMENT_DEVICE_CLASS_MISMATCH,
        ),
        (429, json!({ "error": "rate_limited" }), ENROLMENT_RATE_LIMITED),
        (503, json!({ "error": "unavailable" }), ENROLMENT_SERVER_UNAVAILABLE),
        (400, json!({ "error": "invalid_body" }), ENROLMENT_SERVER_REFUSED),
    ];
    for (status, body, expected) in cases {
        let db = crate::db::test_db::seeded().await;
        let stub = start_stub(status, body.to_string()).await;
        let err = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &qr(&stub.base_url), &phone())
            .await
            .expect_err("refused");
        assert_eq!(code_of(&err), expected, "{status} {body}");
        assert_eq!(param_of(&err, "status"), Some(status.to_string()));
        let text = format!("{err:?}");
        assert!(!text.contains(TOKEN), "{text}");
        assert!(
            load_state_record_from_db(&db).await.expect("load").is_none(),
            "nothing recorded"
        );
    }
}

#[tokio::test]
async fn an_unreachable_server_is_offline() {
    let db = crate::db::test_db::seeded().await;
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
    let base = format!("http://{}", probe.local_addr().expect("addr"));
    drop(probe);
    let err = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &base, &qr(&base), &phone())
        .await
        .expect_err("offline");
    assert_eq!(code_of(&err), ENROLMENT_OFFLINE);
}

#[tokio::test]
async fn an_answer_without_a_complete_binding_or_for_another_phone_records_nothing() {
    let mut no_binding = redeem_answer();
    no_binding.as_object_mut().expect("object").remove("enrolment");
    let mut blank_person = redeem_answer();
    blank_person["enrolment"]["person_sync_id"] = json!(" ");
    let mut bad_date = redeem_answer();
    bad_date["enrolment"]["enrolled_at"] = json!("yesterday");
    let mut other_phone = redeem_answer();
    other_phone["machine_fingerprint"] = json!("fp-other");
    for answer in [no_binding, blank_person, bad_date, other_phone] {
        let db = crate::db::test_db::seeded().await;
        let stub = start_stub(200, answer.to_string()).await;
        let err = redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &qr(&stub.base_url), &phone())
            .await
            .expect_err("refused");
        assert_eq!(code_of(&err), ENROLMENT_INVALID_ANSWER, "{answer}");
        assert!(!format!("{err:?}").contains(DEVICE_TOKEN));
        assert!(load_state_record_from_db(&db).await.expect("load").is_none());
    }
}

#[tokio::test]
async fn the_binding_survives_a_licence_refresh_and_goes_with_the_reset() {
    let db = crate::db::test_db::seeded().await;
    let stub = start_stub(200, redeem_answer().to_string()).await;
    redeem_mobile_enrolment_in(&db, SIGNED_OUT, &stub.base_url, &qr(&stub.base_url), &phone())
        .await
        .expect("redeemed");

    let refresh = start_stub(
        200,
        json!({ "tenant_id": TENANT, "license_id": "lic-phones", "machine_fingerprint": FINGERPRINT, "edition": "professional" })
            .to_string(),
    )
    .await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &refresh.base_url, Some("1.2.3"))
        .await
        .expect("refresh");
    assert!(matches!(result, PolicyRefreshResult::Refreshed { .. }), "{result:?}");
    let binding = mobile_enrolment_binding(&db).await.expect("binding");
    assert_eq!(binding.map(|b| b.person_sync_id).as_deref(), Some("person-sync-7"));

    let session = tokio::sync::RwLock::new(crate::auth::session_manager::SessionManager::new());
    reset_to_first_run_in(&db, &session, "test").await.expect("reset");
    assert_eq!(mobile_enrolment_binding(&db).await.expect("binding"), None);
    assert!(load_state_record_from_db(&db).await.expect("load").is_none());
}
