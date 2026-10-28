//! The licence policy refresh sent from Rust (`refresh_product_activation_policy_in`) against an
//! in-process HTTP stub of the control API on 127.0.0.1 (one request per connection). The stored
//! activation token is sent as the bearer and never comes back in the result. The stub is shared
//! with the other activation calls' tests (`product_license_activation_calls_tests.rs`).

use std::sync::{Arc, Mutex};

use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::{
    bootstrap_initial_tenant_admin_in, get_activation_claim_record, load_state_record_from_db, policy_refresh_claim,
    record_activation_claim_for_tests, refresh_product_activation_policy_in, well_formed_app_version,
    BootstrapInitialAdminInput, LicenceCaller, PolicyRefreshResult, ProductLicenseActivationStatus,
};
use crate::errors::AppError;

pub(super) const SIGNED_OUT: LicenceCaller = LicenceCaller {
    user_id: None,
    may_administer: false,
    step_up: false,
};
/// An administrator signed in with a fresh identity check.
pub(super) const ADMIN: LicenceCaller = LicenceCaller {
    user_id: Some(1),
    may_administer: true,
    step_up: true,
};
const TENANT: &str = "tenant-refresh";
const FINGERPRINT: &str = "fp-refresh-1";
/// The activation token stored on the PC; must reach the server and nothing else.
const STORED_TOKEN: &str = "stored-activation-token-7f3a";
const PATH: &str = "/api/v1/activation/policy-refresh";

#[derive(Debug, Clone)]
pub(super) struct Recorded {
    pub(super) path: String,
    pub(super) authorization: Option<String>,
    pub(super) body: String,
}

/// A control API that answers every request with `status` and `body`, recording each request.
pub(super) struct Stub {
    pub(super) base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Stub {
    pub(super) fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("requests").clone()
    }
}

pub(super) async fn start_stub(status: u16, body: String) -> Stub {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = requests.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = Vec::new();
            let mut chunk = [0_u8; 8 * 1024];
            let header_end = loop {
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(p + 4);
                }
            };
            let Some(header_end) = header_end else { continue };
            let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let mut lines = head.split("\r\n");
            let path = lines
                .next()
                .and_then(|l| l.split(' ').nth(1))
                .unwrap_or_default()
                .to_string();
            let headers: Vec<(String, String)> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                .collect();
            let length: usize = headers
                .iter()
                .find(|(k, _)| k == "content-length")
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(0);
            while buf.len() < header_end + length {
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            let end = buf.len().min(header_end + length);
            recorded.lock().expect("requests").push(Recorded {
                path,
                authorization: headers
                    .iter()
                    .find(|(k, _)| k == "authorization")
                    .map(|(_, v)| v.clone()),
                body: String::from_utf8_lossy(&buf[header_end..end]).to_string(),
            });
            let response = format!(
                "HTTP/1.1 {status} STUB\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    Stub { base_url, requests }
}

/// A PC activated for `TENANT` with `STORED_TOKEN`, no administrator yet.
async fn activated_pc() -> DatabaseConnection {
    let db = crate::db::test_db::seeded().await;
    record_activation_claim_for_tests(&db, TENANT, FINGERPRINT, STORED_TOKEN)
        .await
        .expect("claim");
    db
}

/// `activated_pc` once its first administrator exists: licence changes then need one.
async fn administered_pc() -> DatabaseConnection {
    let db = activated_pc().await;
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

pub(super) fn refusal_code(err: &AppError) -> Option<String> {
    match err {
        AppError::OrgValidationFailed(issues) => issues.first().map(|i| i.code.clone()),
        _ => None,
    }
}

/// The refresh answer of the activation stored here, without an activation token.
fn renewed_claim() -> Value {
    json!({
        "tenant_id": TENANT,
        "license_id": "test-licence",
        "machine_fingerprint": FINGERPRINT,
    })
}

fn assert_no_token(result: &PolicyRefreshResult) {
    let text = serde_json::to_string(result).expect("result json");
    assert!(!text.contains(STORED_TOKEN), "the token leaked into the result: {text}");
}

#[tokio::test]
async fn without_a_stored_claim_nothing_is_sent() {
    let db = crate::db::test_db::seeded().await;
    let stub = start_stub(200, renewed_claim().to_string()).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, Some("1.2.3"))
        .await
        .expect("refresh");
    assert!(matches!(result, PolicyRefreshResult::NoToken), "{result:?}");
    assert_eq!(
        serde_json::to_value(&result).expect("json"),
        json!({ "kind": "no_token" })
    );
    assert!(stub.requests().is_empty(), "no request without a stored claim");
}

#[tokio::test]
async fn a_renewed_licence_is_recorded_with_the_stored_token_as_bearer() {
    let db = activated_pc().await;
    let stub = start_stub(200, renewed_claim().to_string()).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, Some("1.2.3"))
        .await
        .expect("refresh");
    let PolicyRefreshResult::Refreshed { state } = &result else {
        panic!("expected refreshed, got {result:?}");
    };
    assert_eq!(state.status, ProductLicenseActivationStatus::Active);
    assert_eq!(state.tenant_id.as_deref(), Some(TENANT));
    assert_no_token(&result);

    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].path, PATH);
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some(format!("Bearer {STORED_TOKEN}").as_str())
    );
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).expect("body"),
        json!({ "app_version": "1.2.3" })
    );

    // The answer had no activation token: the stored one is kept, and the success is recorded.
    let claim = get_activation_claim_record(&db)
        .await
        .expect("claim")
        .expect("stored claim");
    assert_eq!(claim.activation_token, STORED_TOKEN);
    let record = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record");
    assert!(record.reconciliation.last_success_at.is_some());
    assert_eq!(record.app_version.as_deref(), Some("1.2.3"));
}

#[tokio::test]
async fn a_new_token_in_the_answer_replaces_the_stored_one() {
    let db = activated_pc().await;
    let mut answer = renewed_claim();
    answer["activation_token"] = json!("rotated-token");
    let stub = start_stub(200, answer.to_string()).await;
    // A token not bound to this activation (`activation_token_bound`) needs an administrator.
    let result = refresh_product_activation_policy_in(&db, ADMIN, &stub.base_url, None)
        .await
        .expect("refresh");
    assert!(matches!(result, PolicyRefreshResult::Refreshed { .. }), "{result:?}");
    let text = serde_json::to_string(&result).expect("json");
    assert!(!text.contains("rotated-token"), "{text}");
    let claim = get_activation_claim_record(&db)
        .await
        .expect("claim")
        .expect("stored claim");
    assert_eq!(claim.activation_token, "rotated-token");
}

#[tokio::test]
async fn the_app_version_is_sent_only_when_well_formed() {
    for (version, sent) in [
        (Some("0.9.2-rc.1"), json!({ "app_version": "0.9.2-rc.1" })),
        (Some(" 1.0.0 "), json!({ "app_version": "1.0.0" })),
        (Some("1.2"), json!({})),
        (Some("dev build"), json!({})),
        (Some("1.2.3-"), json!({})),
        (Some("1.2.3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), json!({})),
        (None, json!({})),
    ] {
        let db = activated_pc().await;
        let stub = start_stub(200, renewed_claim().to_string()).await;
        refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, version)
            .await
            .expect("refresh");
        let requests = stub.requests();
        assert_eq!(requests.len(), 1, "{version:?}");
        assert_eq!(
            serde_json::from_str::<Value>(&requests[0].body).expect("body"),
            sent,
            "{version:?}"
        );
    }
}

#[tokio::test]
async fn an_error_answer_carries_the_server_code_and_message() {
    let db = activated_pc().await;
    let body = json!({ "error": { "code": "activation_revoked", "message": "Device revoked" } }).to_string();
    let stub = start_stub(403, body.clone()).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, Some("1.2.3"))
        .await
        .expect("refresh");
    assert_eq!(
        serde_json::to_value(&result).expect("json"),
        json!({
            "kind": "http_error",
            "status": 403,
            "code": "activation_revoked",
            "message": "Device revoked",
            "details": body,
        })
    );
    // Nothing recorded: the webview records the denial (it decides denial vs. re-claim).
    let record = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record");
    assert_eq!(record.status, ProductLicenseActivationStatus::Active);

    // Top-level fields, a token the server echoed back is never returned.
    let db = activated_pc().await;
    let body = json!({ "code": "invalid_token", "detail": format!("bad bearer {STORED_TOKEN}") }).to_string();
    let stub = start_stub(401, body).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect("refresh");
    let PolicyRefreshResult::HttpError {
        status, code, message, ..
    } = &result
    else {
        panic!("expected http_error, got {result:?}");
    };
    assert_eq!(*status, 401);
    assert_eq!(code.as_deref(), Some("invalid_token"));
    assert_eq!(message.as_deref(), Some("bad bearer [activation token]"));
    assert_no_token(&result);

    // An empty body: the status alone (the webview reads a 409 as a slot limit); a body that is
    // not JSON is the message.
    let db = activated_pc().await;
    let stub = start_stub(409, String::new()).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect("refresh");
    assert_eq!(
        serde_json::to_value(&result).expect("json"),
        json!({ "kind": "http_error", "status": 409, "code": null, "message": null, "details": null })
    );
    let db = activated_pc().await;
    let stub = start_stub(502, "  Bad gateway  ".to_string()).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect("refresh");
    let PolicyRefreshResult::HttpError { code, message, .. } = &result else {
        panic!("expected http_error, got {result:?}");
    };
    assert_eq!(code, &None);
    assert_eq!(message.as_deref(), Some("Bad gateway"));
}

#[tokio::test]
async fn no_server_is_a_network_error() {
    let db = activated_pc().await;
    let port = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr").port()
    };
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &format!("http://127.0.0.1:{port}"), None)
        .await
        .expect("refresh");
    assert!(matches!(result, PolicyRefreshResult::NetworkError { .. }), "{result:?}");
    assert_no_token(&result);
}

#[tokio::test]
async fn a_success_answer_that_is_not_a_claim_records_nothing() {
    let db = activated_pc().await;
    let before = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record")
        .reconciliation
        .last_attempt_at;
    let stub = start_stub(200, json!({ "license_id": "test-licence" }).to_string()).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect("refresh");
    assert!(
        matches!(result, PolicyRefreshResult::InvalidResponse { .. }),
        "{result:?}"
    );
    assert_no_token(&result);
    let after = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record")
        .reconciliation
        .last_attempt_at;
    assert_eq!(before, after, "nothing recorded");
}

#[tokio::test]
async fn a_refused_licence_change_is_returned_as_the_refusal() {
    let db = administered_pc().await;
    let mut answer = renewed_claim();
    answer["tenant_id"] = json!("another-tenant");
    let stub = start_stub(200, answer.to_string()).await;
    let err = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect_err("another organisation needs an administrator");
    assert_eq!(
        refusal_code(&err).as_deref(),
        Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
        "{err:?}"
    );
    assert!(!format!("{err:?}").contains(STORED_TOKEN));
}

#[tokio::test]
async fn a_rotated_token_not_bound_here_is_refused_signed_out_and_nothing_recorded() {
    let db = administered_pc().await;
    let before = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record")
        .reconciliation
        .last_attempt_at;
    let mut answer = renewed_claim();
    answer["activation_token"] = json!("rotated-token-from-elsewhere");
    let stub = start_stub(200, answer.to_string()).await;
    let err = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect_err("a token not bound to this activation needs an administrator");
    assert_eq!(
        refusal_code(&err).as_deref(),
        Some("LICENSE_RECONCILE_ADMIN_REQUIRED"),
        "{err:?}"
    );
    let text = format!("{err:?}");
    assert!(
        !text.contains(STORED_TOKEN) && !text.contains("rotated-token-from-elsewhere"),
        "{text}"
    );
    let claim = get_activation_claim_record(&db)
        .await
        .expect("claim")
        .expect("stored claim");
    assert_eq!(claim.activation_token, STORED_TOKEN, "the stored token is kept");
    let after = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record")
        .reconciliation
        .last_attempt_at;
    assert_eq!(before, after, "nothing recorded");
}

#[tokio::test]
async fn a_token_echoed_across_the_details_limit_is_never_returned() {
    let db = activated_pc().await;
    // The token starts a few characters before the 4000-character limit of the details.
    let body = format!("{}{STORED_TOKEN} and more", "x".repeat(3990));
    let stub = start_stub(400, body).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect("refresh");
    let PolicyRefreshResult::HttpError { details, message, .. } = &result else {
        panic!("expected http_error, got {result:?}");
    };
    let details = details.as_deref().expect("details");
    assert_eq!(details.chars().count(), 4000);
    assert!(
        !details.contains("stored-act"),
        "a prefix of the token leaked: {details}"
    );
    assert!(message.as_deref().is_some_and(|m| m.contains("[activation token]")));
    assert_no_token(&result);
}

#[tokio::test]
async fn an_oversized_answer_is_cut_off_and_records_nothing() {
    let db = activated_pc().await;
    let mut answer = renewed_claim();
    answer["padding"] = json!("p".repeat(crate::sync::transport::client::RAW_ANSWER_MAX_BYTES));
    let stub = start_stub(200, answer.to_string()).await;
    let result = refresh_product_activation_policy_in(&db, SIGNED_OUT, &stub.base_url, None)
        .await
        .expect("refresh");
    let PolicyRefreshResult::InvalidResponse { message } = &result else {
        panic!("expected invalid_response, got {result:?}");
    };
    assert!(message.contains("larger than"), "{message}");
    assert_no_token(&result);
    let record = load_state_record_from_db(&db)
        .await
        .expect("record")
        .expect("stored record");
    assert!(record.reconciliation.last_success_at.is_none(), "nothing recorded");
}

#[test]
fn app_versions_follow_the_server_rule() {
    for ok in ["0.9.2", "10.20.30", "1.0.0-dev", "1.0.0-rc.1-x"] {
        assert!(well_formed_app_version(ok), "{ok}");
    }
    for bad in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "v1.2.3",
        "1.2.x",
        "1.2.3-",
        "1.2.3+build",
        "1.2.3-é",
    ] {
        assert!(!well_formed_app_version(bad), "{bad}");
    }
    assert!(!well_formed_app_version("1.2.3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
}

#[test]
fn the_answer_is_read_as_the_webview_read_it() {
    let raw = json!({
        "tenant_id": "t",
        "license_id": "l",
        "machine_fingerprint": "f",
        "activation_token": "  ",
        "tier": "core",
        "license_plan": "professional",
        "company_name": "Acme",
        "entitlement": { "slot_limit": "7" },
        "limits": { "activated": 2 },
        "is_demo": false,
        "tenant": { "slug": "acme", "is_initialized": true },
        "force_update_mode": "off",
    });
    let claim = policy_refresh_claim(raw.as_object().expect("object"), "stored");
    assert_eq!(claim["activation_token"], json!("stored"));
    // license_plan comes before tier, as in normalizeActivationPayload.
    assert_eq!(claim["license_tier"], json!("professional"));
    assert_eq!(claim["license_plan"], json!("professional"));
    assert_eq!(claim["tenant_display_name"], json!("Acme"));
    assert_eq!(claim["device_limit"], json!(7));
    assert_eq!(claim["activated_device_count"], json!(2));
    assert_eq!(claim["has_demo_data"], json!(false));
    assert_eq!(claim["tenant_slug"], json!("acme"));
    assert_eq!(claim["is_initialized"], json!(true));
    assert_eq!(claim["license_status"], json!("active"));
    assert!(!claim.contains_key("force_update_mode"));
    assert!(!claim.contains_key("tier"));

    // `edition` wins over the tier and plan labels.
    let raw = json!({ "edition": "enterprise", "license_tier": "core" });
    let claim = policy_refresh_claim(raw.as_object().expect("object"), "stored");
    assert_eq!(claim["license_tier"], json!("enterprise"));
}
