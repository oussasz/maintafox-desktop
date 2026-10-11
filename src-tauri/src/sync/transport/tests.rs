//! Sync worker against an in-process HTTP stub of the control API (protocol v2).
//!
//! The stub is a minimal HTTP/1.1 server on 127.0.0.1 (one request per connection). Nothing here
//! talks to a real server.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use super::client::ClientTimeouts;
use super::store;
use super::wire::ExchangeOutboxItem;
use super::worker::{
    pause_sync, resume_sync, CycleOutcome, SyncAccess, SyncAccessProvider, SyncCommand, SyncTiming, SyncWorker,
    SyncWorkerConfig, SyncWorkerHandle, SyncWorkerState, BLOCKER_ACTIVATION_REFUSED,
};
use crate::errors::AppResult;
use crate::sync::domain::{StageOutboxItemInput, SyncOutboxItem};
use crate::sync::queries::{self, hash_payload, OUTBOX_MAX_DELIVERY_ATTEMPTS};

// ── Stub control API ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Recorded {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("request body is JSON")
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

enum Reply {
    Json {
        status: u16,
        body: Value,
        headers: Vec<(&'static str, String)>,
    },
    /// Raw bytes (object storage answer to a presigned GET).
    Bytes { status: u16, body: Vec<u8> },
    /// Close the connection without answering (the server may have processed the request).
    Drop,
}

fn ok(body: Value) -> Reply {
    Reply::Json {
        status: 200,
        body,
        headers: vec![],
    }
}

fn status(status: u16, body: Value) -> Reply {
    Reply::Json {
        status,
        body,
        headers: vec![],
    }
}

type Handler = Box<dyn FnMut(&Recorded) -> Reply + Send>;

struct StubState {
    handler: Mutex<Handler>,
    requests: Mutex<Vec<Recorded>>,
}

struct Stub {
    base_url: String,
    state: Arc<StubState>,
}

impl Stub {
    fn requests(&self) -> Vec<Recorded> {
        self.state.requests.lock().expect("requests").clone()
    }
    fn requests_to(&self, path: &str) -> Vec<Recorded> {
        self.requests().into_iter().filter(|r| r.path == path).collect()
    }
    fn set_handler(&self, handler: impl FnMut(&Recorded) -> Reply + Send + 'static) {
        *self.state.handler.lock().expect("handler") = Box::new(handler);
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

async fn serve_connection(mut socket: TcpStream, state: Arc<StubState>) {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    let header_end = loop {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if let Some(end) = find_header_end(&buf) {
            break end;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let method = request_line.next().unwrap_or_default().to_string();
    let path = request_line.next().unwrap_or_default().to_string();
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
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let recorded = Recorded {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&buf[header_end..header_end + length]).to_string(),
    };
    let reply = {
        let mut handler = state.handler.lock().expect("handler");
        (handler)(&recorded)
    };
    state.requests.lock().expect("requests").push(recorded);
    match reply {
        Reply::Drop => {}
        Reply::Bytes { status, body } => {
            let head = format!(
                "HTTP/1.1 {status} STUB\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
            let _ = socket.shutdown().await;
        }
        Reply::Json { status, body, headers } => {
            let body = body.to_string();
            let mut response = format!(
                "HTTP/1.1 {status} STUB\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (k, v) in headers {
                response.push_str(&format!("{k}: {v}\r\n"));
            }
            response.push_str("\r\n");
            response.push_str(&body);
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    }
}

async fn start_stub_on(listener: TcpListener, handler: impl FnMut(&Recorded) -> Reply + Send + 'static) -> Stub {
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let state = Arc::new(StubState {
        handler: Mutex::new(Box::new(handler)),
        requests: Mutex::new(Vec::new()),
    });
    let accept_state = state.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve_connection(socket, accept_state.clone()));
        }
    });
    Stub { base_url, state }
}

async fn start_stub(handler: impl FnMut(&Recorded) -> Reply + Send + 'static) -> Stub {
    start_stub_on(TcpListener::bind("127.0.0.1:0").await.expect("bind"), handler).await
}

// ── Answers shaped like the server's ──────────────────────────────────────────────────────

fn outbox_of(request: &Recorded) -> Vec<Value> {
    request.json()["outbox_batch"].as_array().cloned().unwrap_or_default()
}

fn acks_for(request: &Recorded, first_seq: i64) -> Vec<Value> {
    outbox_of(request)
        .iter()
        .enumerate()
        .map(|(i, item)| {
            json!({
                "idempotency_key": item["idempotency_key"],
                "entity_type": item["entity_type"],
                "entity_sync_id": item["entity_sync_id"],
                "operation": item["operation"],
                "row_version": item["row_version"],
                "server_seq": first_seq + i64::try_from(i).expect("index"),
                "duplicate": false,
            })
        })
        .collect()
}

fn inbound(entity_sync_id: &str, seq: i64) -> Value {
    let payload = json!({ "entity_sync_id": entity_sync_id, "label": "from server" }).to_string();
    json!({
        "server_seq": seq,
        "entity_type": "future_widgets",
        "entity_sync_id": entity_sync_id,
        "operation": "update",
        "row_version": 2,
        "payload_json": payload,
        "payload_hash": hash_payload(&payload),
        "owner_user_sync_id": null,
        "deleted": false,
    })
}

fn exchange_answer(checkpoint: &str, acks: Vec<Value>, inbound_items: Vec<Value>, has_more: bool) -> Value {
    json!({
        "protocol_version": "v2",
        "server_batch_id": format!("batch_{checkpoint}"),
        "checkpoint_token": checkpoint,
        "acknowledged_items": acks,
        "rejected_items": [],
        "conflict_items": [],
        "inbound_items": inbound_items,
        "has_more": has_more,
        "user_snapshot_required": [],
        "policy_metadata_json": null,
    })
}

fn snapshot_page(items: Vec<Value>, next_page_token: Option<&str>, checkpoint: Option<&str>) -> Value {
    json!({
        "protocol_version": "v2",
        "items": items,
        "has_more": next_page_token.is_some(),
        "next_page_token": next_page_token,
        "checkpoint_token": checkpoint,
    })
}

// ── Local fixtures ──────────────────────────────────────────────────────────────────────────

const TOKEN: &str = "activation-token-a";

struct FixedAccess(Mutex<String>);

impl FixedAccess {
    fn new(token: &str) -> Arc<Self> {
        Arc::new(Self(Mutex::new(token.to_string())))
    }
    fn set(&self, token: &str) {
        *self.0.lock().expect("token") = token.to_string();
    }
}

impl SyncAccessProvider for FixedAccess {
    fn access<'a>(
        &'a self,
        _db: &'a DatabaseConnection,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<SyncAccess>> + Send + 'a>> {
        let token = self.0.lock().expect("token").clone();
        Box::pin(async move { Ok(SyncAccess::Granted { token }) })
    }
}

fn test_timing() -> SyncTiming {
    SyncTiming {
        idle_interval: Duration::from_millis(50),
        retry_base: Duration::from_millis(20),
        retry_max: Duration::from_millis(200),
        blocked_recheck: Duration::from_millis(50),
        network_wake_min_spacing: Duration::from_millis(10),
        media_retry_base: Duration::from_millis(20),
        media_retry_max: Duration::from_millis(200),
        timeouts: ClientTimeouts {
            connect: Duration::from_secs(2),
            request: Duration::from_secs(5),
            media_transfer: Duration::from_secs(5),
        },
    }
}

fn media_root() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("maintafox-sync-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("media root");
    dir
}

async fn setup_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.expect("in-memory sqlite");
    crate::migrations::Migrator::up(&db, None).await.expect("migrations");
    db
}

fn make_worker(
    db: &DatabaseConnection,
    base_url: &str,
    access: Arc<FixedAccess>,
    timing: SyncTiming,
) -> (SyncWorker, SyncWorkerHandle) {
    let handle = SyncWorkerHandle::new();
    let worker = SyncWorker::new(
        db.clone(),
        handle.clone(),
        SyncWorkerConfig {
            api_base_url: base_url.to_string(),
            media_root: media_root(),
            timing,
        },
        access,
    )
    .expect("worker");
    (worker, handle)
}

async fn exec(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
    db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .expect("fixture statement");
}

async fn scalar<T: sea_orm::TryGetable>(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) -> T {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
        .await
        .expect("query")
        .expect("row")
        .try_get_by_index::<T>(0)
        .expect("value")
}

async fn stage_team(db: &DatabaseConnection, sync_id: &str) -> SyncOutboxItem {
    queries::stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: "teams".to_string(),
            entity_sync_id: sync_id.to_string(),
            operation: "create".to_string(),
            payload_json: json!({ "entity_sync_id": sync_id, "code": sync_id }).to_string(),
        },
    )
    .await
    .expect("stage outbox item")
}

/// A device that already has a v2 delta checkpoint (no bootstrap snapshot).
async fn seed_checkpoint(db: &DatabaseConnection, token: &str) {
    queries::apply_snapshot_page(db, "seed", &[], Some(token))
        .await
        .expect("seed checkpoint");
}

async fn checkpoint(db: &DatabaseConnection) -> Option<String> {
    scalar::<Option<String>>(db, "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1", vec![]).await
}

// ── Tests ───────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn bootstrap_then_push_and_pull() {
    let db = setup_db().await;
    let staged = stage_team(&db, "team-1").await;
    let stub = start_stub(|req| match req.path.as_str() {
        "/api/v1/sync/snapshot" => ok(snapshot_page(vec![inbound("w-0", 1)], None, Some("v2.cp0.sig"))),
        "/api/v1/sync/exchange" => ok(exchange_answer(
            "v2.cp1.sig",
            acks_for(req, 41),
            vec![inbound("w-1", 42)],
            false,
        )),
        _ => status(404, json!({ "error": "not_found" })),
    })
    .await;
    let (worker, handle) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, wait) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    assert_eq!(wait, test_timing().idle_interval);

    let requests = stub.requests();
    assert_eq!(
        requests.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        vec!["/api/v1/sync/snapshot", "/api/v1/sync/exchange"]
    );
    let snapshot = requests[0].json();
    assert_eq!(snapshot["protocol_version"], "v2");
    assert_eq!(snapshot["scope"], "all");
    assert_eq!(snapshot["page_token"], Value::Null);
    let exchange = &requests[1];
    assert_eq!(exchange.method, "POST");
    assert_eq!(exchange.header("authorization"), Some("Bearer activation-token-a"));
    let body = exchange.json();
    assert_eq!(body["protocol_version"], "v2");
    assert_eq!(body["device_class"], "desktop");
    assert_eq!(body["checkpoint_token"], "v2.cp0.sig");
    assert_eq!(body["current_user_sync_ids"], json!([]));
    assert!(body["idempotency_key"].as_str().is_some_and(|k| k.len() >= 8));
    let item = &body["outbox_batch"][0];
    assert_eq!(item["idempotency_key"], staged.idempotency_key.as_str());
    assert_eq!(item["row_version"], 1);
    assert!(
        item.as_object().expect("item").contains_key("base_row_version") && item["base_row_version"].is_null(),
        "base_row_version must be sent as an explicit null for a create"
    );

    let (status, seq, duplicate): (String, Option<i64>, i64) = (
        scalar(
            &db,
            "SELECT status FROM sync_outbox WHERE id = ?",
            vec![staged.id.into()],
        )
        .await,
        scalar(
            &db,
            "SELECT server_seq FROM sync_outbox WHERE id = ?",
            vec![staged.id.into()],
        )
        .await,
        scalar(
            &db,
            "SELECT ack_duplicate FROM sync_outbox WHERE id = ?",
            vec![staged.id.into()],
        )
        .await,
    );
    assert_eq!((status.as_str(), seq, duplicate), ("acked", Some(41), 0));
    let inbox: i64 = scalar(
        &db,
        "SELECT COUNT(*) FROM sync_inbox WHERE entity_sync_id IN ('w-0', 'w-1')",
        vec![],
    )
    .await;
    assert_eq!(inbox, 2, "snapshot row and delta row are both in the inbox");
    assert_eq!(checkpoint(&db).await.as_deref(), Some("v2.cp1.sig"));
    assert!(store::load_state(&db).await.expect("state").pending.is_none());

    let published = handle.status();
    assert_eq!(published.state, SyncWorkerState::Idle);
    assert!(published.last_success_at.is_some());
    assert_eq!(published.pending_outbox_count, 0);
}

#[tokio::test]
async fn has_more_pages_through_the_delta() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let stub = start_stub(|req| {
        let body = req.json();
        match body["checkpoint_token"].as_str() {
            Some("v2.start") => ok(exchange_answer("v2.p1", vec![], vec![inbound("w-a", 1)], true)),
            Some("v2.p1") => ok(exchange_answer("v2.p2", vec![], vec![inbound("w-b", 2)], false)),
            other => status(400, json!({ "error": "unexpected", "message": format!("{other:?}") })),
        }
    })
    .await;
    let (worker, _) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    assert_eq!(stub.requests_to("/api/v1/sync/exchange").len(), 2);
    assert!(stub.requests_to("/api/v1/sync/snapshot").is_empty());
    assert_eq!(checkpoint(&db).await.as_deref(), Some("v2.p2"));
    let inbox: i64 = scalar(
        &db,
        "SELECT COUNT(*) FROM sync_inbox WHERE entity_type = 'future_widgets'",
        vec![],
    )
    .await;
    assert_eq!(inbox, 2);
}

#[tokio::test]
async fn lost_answer_is_resent_with_the_same_key_and_body_after_a_restart() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let staged = stage_team(&db, "team-1").await;
    // The server processes the request but the answer never arrives.
    let stub = start_stub(|_| Reply::Drop).await;
    {
        let (worker, _) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());
        let (outcome, _) = worker.cycle_once().await;
        assert!(matches!(outcome, CycleOutcome::Retry { .. }), "{outcome:?}");
    } // the app stops ("crash")
    let stored = store::load_state(&db)
        .await
        .expect("state")
        .pending
        .expect("request kept for a resend");

    stub.set_handler(|req| {
        let mut acks = acks_for(req, 7);
        for ack in &mut acks {
            ack["duplicate"] = json!(true);
        }
        ok(exchange_answer("v2.after", acks, vec![], false))
    });
    let (worker, _) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());
    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });

    let exchanges = stub.requests_to("/api/v1/sync/exchange");
    assert_eq!(exchanges.len(), 2);
    assert_eq!(exchanges[0].body, exchanges[1].body, "same body byte for byte");
    assert_eq!(exchanges[1].body, stored.body);
    assert_eq!(exchanges[1].json()["idempotency_key"], stored.idempotency_key.as_str());
    let (status, attempts, duplicate): (String, i64, i64) = (
        scalar(
            &db,
            "SELECT status FROM sync_outbox WHERE id = ?",
            vec![staged.id.into()],
        )
        .await,
        scalar(
            &db,
            "SELECT attempt_count FROM sync_outbox WHERE id = ?",
            vec![staged.id.into()],
        )
        .await,
        scalar(
            &db,
            "SELECT ack_duplicate FROM sync_outbox WHERE id = ?",
            vec![staged.id.into()],
        )
        .await,
    );
    assert_eq!((status.as_str(), attempts, duplicate), ("acked", 2, 1));
}

#[tokio::test]
async fn idempotency_key_reused_is_surfaced_and_rebuilt_with_a_new_key() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let staged = stage_team(&db, "team-1").await;
    let stub = start_stub(|_| {
        status(
            409,
            json!({ "error": "idempotency_key_reused", "message": "different body" }),
        )
    })
    .await;
    let (worker, handle) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    let CycleOutcome::Failed { error } = outcome else {
        panic!("expected a failure, got {outcome:?}");
    };
    assert_eq!(error.code, "idempotency_key_reused");
    assert_eq!(handle.status().state, SyncWorkerState::Error);
    assert_eq!(
        handle.status().last_error.map(|e| e.code).as_deref(),
        Some("idempotency_key_reused")
    );
    assert!(store::load_state(&db).await.expect("state").pending.is_none());
    let attempts: i64 = scalar(
        &db,
        "SELECT attempt_count FROM sync_outbox WHERE id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(attempts, 0, "a refused key is not a delivery attempt");

    stub.set_handler(|req| ok(exchange_answer("v2.after", acks_for(req, 1), vec![], false)));
    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let exchanges = stub.requests_to("/api/v1/sync/exchange");
    assert_eq!(exchanges.len(), 2, "one request per cycle: never retried blindly");
    assert_ne!(
        exchanges[0].json()["idempotency_key"],
        exchanges[1].json()["idempotency_key"]
    );
}

#[tokio::test]
async fn refused_checkpoint_bootstraps_with_a_paged_snapshot() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.stale").await;
    let stub = start_stub(|req| match (req.path.as_str(), req.json()) {
        ("/api/v1/sync/exchange", body) if body["checkpoint_token"] == "v2.stale" => status(
            400,
            json!({ "error": "invalid_checkpoint_token", "message": "issued to another device" }),
        ),
        ("/api/v1/sync/exchange", body) if body["checkpoint_token"] == "v2.fresh" => {
            ok(exchange_answer("v2.after", vec![], vec![], false))
        }
        ("/api/v1/sync/snapshot", body) if body["page_token"].is_null() => {
            ok(snapshot_page(vec![inbound("w-x", 1)], Some("page-2"), None))
        }
        ("/api/v1/sync/snapshot", body) if body["page_token"] == "page-2" => {
            ok(snapshot_page(vec![inbound("w-y", 2)], None, Some("v2.fresh")))
        }
        _ => status(400, json!({ "error": "unexpected" })),
    })
    .await;
    let (worker, _) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let paths: Vec<String> = stub.requests().iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths,
        vec![
            "/api/v1/sync/exchange",
            "/api/v1/sync/snapshot",
            "/api/v1/sync/snapshot",
            "/api/v1/sync/exchange"
        ]
    );
    assert_eq!(checkpoint(&db).await.as_deref(), Some("v2.after"));
    let inbox: i64 = scalar(
        &db,
        "SELECT COUNT(*) FROM sync_inbox WHERE entity_sync_id IN ('w-x', 'w-y')",
        vec![],
    )
    .await;
    assert_eq!(inbox, 2);
}

#[tokio::test]
async fn conflict_items_are_recorded_for_a_person() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let staged = stage_team(&db, "team-1").await;
    let stub = start_stub(|req| {
        let item = &outbox_of(req)[0];
        let mut answer = exchange_answer("v2.after", vec![], vec![], false);
        answer["conflict_items"] = json!([{
            "idempotency_key": item["idempotency_key"],
            "entity_type": "teams",
            "entity_sync_id": "team-1",
            "local_row_version": 1,
            "local_base_row_version": null,
            "server_row_version": 3,
            "server_operation": "update",
            "server_payload_json": "{\"entity_sync_id\":\"team-1\",\"code\":\"SERVER\"}",
            "server_payload_hash": null,
            "server_owner_user_sync_id": null,
            "server_deleted": false,
        }]);
        ok(answer)
    })
    .await;
    let (worker, handle) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let status: String = scalar(
        &db,
        "SELECT status FROM sync_outbox WHERE id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(status, "conflict");
    let (kind, remote): (String, i64) = (
        scalar(
            &db,
            "SELECT conflict_type FROM sync_conflicts WHERE linked_outbox_id = ?",
            vec![staged.id.into()],
        )
        .await,
        scalar(
            &db,
            "SELECT inbound_row_version FROM sync_conflicts WHERE linked_outbox_id = ?",
            vec![staged.id.into()],
        )
        .await,
    );
    assert_eq!((kind.as_str(), remote), ("SERVER_CONFLICT", 3));
    assert_eq!(handle.status().unresolved_conflict_count, 1);
}

#[tokio::test]
async fn rate_limit_and_server_errors_back_off() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let staged = stage_team(&db, "team-1").await;
    let stub = start_stub(|_| Reply::Json {
        status: 429,
        body: json!({ "error": "rate_limited", "message": "Too many requests; retry later." }),
        headers: vec![("Retry-After", "7".to_string())],
    })
    .await;
    let timing = test_timing();
    let (worker, handle) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), timing);

    let (outcome, wait) = worker.cycle_once().await;
    assert!(
        matches!(&outcome, CycleOutcome::Retry { retry_after: Some(d), .. } if *d == Duration::from_secs(7)),
        "{outcome:?}"
    );
    assert!(wait >= Duration::from_secs(7), "Retry-After is honoured: {wait:?}");
    let attempts: i64 = scalar(
        &db,
        "SELECT attempt_count FROM sync_outbox WHERE id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(attempts, 0, "a rate-limited request is not a delivery attempt");

    stub.set_handler(|_| status(503, json!({ "error": "sync_exchange_failed" })));
    let (outcome, wait) = worker.cycle_once().await;
    assert!(
        matches!(outcome, CycleOutcome::Retry { retry_after: None, .. }),
        "{outcome:?}"
    );
    // Second consecutive failure: base × 2, ±25 % jitter, capped.
    let nominal = timing.retry_base * 2;
    assert!(
        wait >= nominal.mul_f64(0.75) && wait <= nominal.mul_f64(1.25),
        "{wait:?}"
    );
    let attempts: i64 = scalar(
        &db,
        "SELECT attempt_count FROM sync_outbox WHERE id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(attempts, 1, "a 5xx counts as a delivery attempt");
    let status = handle.status();
    assert_eq!(status.state, SyncWorkerState::Retrying);
    assert_eq!(status.consecutive_failures, 2);
    assert_eq!(status.last_error.map(|e| e.http_status), Some(Some(503)));
}

#[tokio::test]
async fn offline_worker_waits_and_resumes_when_the_network_returns() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let staged = stage_team(&db, "team-1").await;
    // A port with nothing listening: connections are refused.
    let addr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        probe.local_addr().expect("addr")
    };
    // Long backoff: only the network event can bring the worker back quickly.
    let timing = SyncTiming {
        retry_base: Duration::from_secs(60),
        retry_max: Duration::from_secs(60),
        ..test_timing()
    };
    let (worker, handle) = make_worker(&db, &format!("http://{addr}"), FixedAccess::new(TOKEN), timing);
    let commands = handle.take_receiver().expect("receiver");
    let cancel = CancellationToken::new();
    let task = tokio::spawn(worker.run(commands, cancel.clone()));

    let mut status = handle.subscribe();
    tokio::time::timeout(
        Duration::from_secs(10),
        status.wait_for(|s| s.state == SyncWorkerState::Offline),
    )
    .await
    .expect("worker goes offline")
    .expect("status channel");
    let attempts: i64 = scalar(
        &db,
        "SELECT attempt_count FROM sync_outbox WHERE id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(attempts, 0, "nothing was sent while offline");

    let listener = TcpListener::bind(addr).await.expect("rebind the port");
    let stub = start_stub_on(listener, |req| {
        ok(exchange_answer("v2.after", acks_for(req, 1), vec![], false))
    })
    .await;
    handle.send(SyncCommand::NetworkOnline);
    tokio::time::timeout(
        Duration::from_secs(10),
        status.wait_for(|s| s.state == SyncWorkerState::Idle),
    )
    .await
    .expect("worker syncs once the network is back")
    .expect("status channel");
    assert_eq!(stub.requests_to("/api/v1/sync/exchange").len(), 1);
    let item_status: String = scalar(
        &db,
        "SELECT status FROM sync_outbox WHERE id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(item_status, "acked");

    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("worker stops on cancel")
        .expect("worker task");
}

#[tokio::test]
async fn refused_activation_stops_until_the_token_changes() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    stage_team(&db, "team-1").await;
    let stub = start_stub(|_| status(403, json!({ "error": "activation_revoked" }))).await;
    let access = FixedAccess::new(TOKEN);
    let (worker, handle) = make_worker(&db, &stub.base_url, access.clone(), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    assert!(
        matches!(&outcome, CycleOutcome::Blocked { reason, .. } if reason == BLOCKER_ACTIVATION_REFUSED),
        "{outcome:?}"
    );
    assert_eq!(handle.status().blocker.as_deref(), Some(BLOCKER_ACTIVATION_REFUSED));
    assert_eq!(
        handle.status().last_error.map(|e| e.code).as_deref(),
        Some("activation_revoked")
    );
    let stored = store::load_state(&db).await.expect("state");
    assert_eq!(
        stored.refused_token_sha256.as_deref(),
        Some(hash_payload(TOKEN).as_str())
    );
    let first_key = stored.pending.expect("request kept").idempotency_key;

    // Same token: the worker stays stopped and does not call the server.
    let (outcome, _) = worker.cycle_once().await;
    assert!(matches!(outcome, CycleOutcome::Blocked { .. }));
    assert_eq!(stub.requests().len(), 1);

    // The licence check re-claimed: new token, the stored request goes out unchanged.
    access.set("activation-token-b");
    stub.set_handler(|req| ok(exchange_answer("v2.after", acks_for(req, 1), vec![], false)));
    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let exchanges = stub.requests_to("/api/v1/sync/exchange");
    assert_eq!(exchanges.len(), 2);
    assert_eq!(exchanges[1].json()["idempotency_key"], first_key.as_str());
    assert_eq!(exchanges[1].header("authorization"), Some("Bearer activation-token-b"));
    assert!(store::load_state(&db)
        .await
        .expect("state")
        .refused_token_sha256
        .is_none());
}

#[tokio::test]
async fn repeated_server_failures_dead_letter_the_item() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let staged = stage_team(&db, "team-1").await;
    exec(
        &db,
        "UPDATE sync_outbox SET attempt_count = ? WHERE id = ?",
        vec![(OUTBOX_MAX_DELIVERY_ATTEMPTS - 1).into(), staged.id.into()],
    )
    .await;
    let stub = start_stub(|_| status(500, json!({ "error": "sync_exchange_failed" }))).await;
    let (worker, handle) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    assert!(matches!(outcome, CycleOutcome::Retry { .. }), "{outcome:?}");
    let status: String = scalar(
        &db,
        "SELECT status FROM sync_outbox WHERE id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(status, "dead_letter");
    let conflict: String = scalar(
        &db,
        "SELECT conflict_type FROM sync_conflicts WHERE linked_outbox_id = ?",
        vec![staged.id.into()],
    )
    .await;
    assert_eq!(conflict, "OUTBOX_DEAD_LETTER");
    assert!(
        store::load_state(&db).await.expect("state").pending.is_none(),
        "the next request is rebuilt without the dead-lettered item"
    );
    assert_eq!(handle.status().dead_letter_outbox_count, 1);
}

#[tokio::test]
async fn user_scope_fields_and_user_snapshot() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    exec(
        &db,
        "INSERT INTO user_accounts (entity_sync_id, username, display_name, identity_mode, is_active, is_admin,
                                    force_password_change, created_at, updated_at)
         VALUES ('user-a', 'user_a', 'User A', 'local', 1, 0, 0, '2026-10-11T00:00:00Z', '2026-10-11T00:00:00Z')",
        vec![],
    )
    .await;
    let user_id: i64 = scalar(
        &db,
        "SELECT id FROM user_accounts WHERE entity_sync_id = 'user-a'",
        vec![],
    )
    .await;
    exec(
        &db,
        "INSERT INTO trusted_devices (id, device_fingerprint, user_id, trusted_at, is_revoked)
         VALUES ('td-1', 'fp-1', ?, '2026-10-11T00:00:00Z', 0)",
        vec![user_id.to_string().into()],
    )
    .await;
    let stub = start_stub(|req| match req.path.as_str() {
        "/api/v1/sync/exchange" => {
            let mut answer = exchange_answer("v2.after", vec![], vec![], false);
            answer["user_snapshot_required"] = json!(["user-a"]);
            ok(answer)
        }
        "/api/v1/sync/snapshot" => ok(snapshot_page(vec![inbound("w-user", 3)], None, Some("v2.user-only"))),
        _ => status(404, json!({ "error": "not_found" })),
    })
    .await;
    let (worker, _) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let exchange = stub.requests_to("/api/v1/sync/exchange")[0].json();
    assert_eq!(exchange["current_user_sync_ids"], json!(["user-a"]));
    let snapshot = stub.requests_to("/api/v1/sync/snapshot")[0].json();
    assert_eq!(snapshot["scope"], "user");
    assert_eq!(snapshot["current_user_sync_ids"], json!(["user-a"]));
    assert_eq!(
        checkpoint(&db).await.as_deref(),
        Some("v2.after"),
        "a user snapshot never replaces the delta checkpoint"
    );
    let inbox: i64 = scalar(
        &db,
        "SELECT COUNT(*) FROM sync_inbox WHERE entity_sync_id = 'w-user'",
        vec![],
    )
    .await;
    assert_eq!(inbox, 1);
}

#[test]
fn outbox_items_carry_owner_and_explicit_base_version() {
    let item = SyncOutboxItem {
        id: 1,
        idempotency_key: "k-1".into(),
        entity_type: "user_dismissals".into(),
        entity_sync_id: "d-1".into(),
        operation: "update".into(),
        row_version: 4,
        payload_json: "{}".into(),
        payload_hash: hash_payload("{}"),
        status: "pending".into(),
        acknowledged_at: None,
        rejection_code: None,
        rejection_message: None,
        origin_machine_id: None,
        base_row_version: Some(3),
        owner_user_sync_id: Some("user-a".into()),
        attempt_count: 0,
        last_attempt_at: None,
        last_error: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let wire = serde_json::to_value(ExchangeOutboxItem::from(&item)).expect("serialize");
    assert_eq!(wire["owner_user_sync_id"], "user-a");
    assert_eq!(wire["base_row_version"], 3);
    assert_eq!(wire["row_version"], 4);
    let keys: Vec<&String> = wire.as_object().expect("object").keys().collect();
    assert_eq!(keys.len(), 9, "only the contract's item keys are sent: {keys:?}");
}

#[tokio::test]
async fn pause_and_resume() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let stub = start_stub(|_| ok(exchange_answer("v2.after", vec![], vec![], false))).await;
    let (worker, handle) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let until = pause_sync(&db, &handle, 30).await.expect("pause");
    let (outcome, wait) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Paused { until: until.clone() });
    assert!(wait > Duration::from_secs(29 * 60));
    assert_eq!(handle.status().state, SyncWorkerState::Paused);
    assert_eq!(handle.status().paused_until.as_deref(), Some(until.as_str()));
    assert!(stub.requests().is_empty());
    assert!(pause_sync(&db, &handle, 0).await.is_err());

    resume_sync(&db, &handle).await.expect("resume");
    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    assert_eq!(stub.requests().len(), 1);
}

#[tokio::test]
async fn media_is_uploaded_through_a_presigned_url() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let bytes = b"photo bytes".to_vec();
    let sha = crate::sync::media::content_sha256_hex(&bytes);
    let object_key = format!("prod/media/tenant-1/equipment/eq-1/{sha}");
    let root = media_root();
    std::fs::create_dir_all(root.join("asset_photos/1")).expect("dir");
    std::fs::write(root.join("asset_photos/1/p.jpg"), &bytes).expect("file");
    crate::sync::media::upsert_media_attachment_row(
        &db,
        &crate::sync::domain::MediaAttachmentSyncPayload {
            entity_sync_id: "media-1".into(),
            row_version: 1,
            parent_entity_type: "equipment".into(),
            parent_entity_sync_id: "eq-1".into(),
            content_sha256: sha.clone(),
            mime_type: "image/jpeg".into(),
            byte_size: i64::try_from(bytes.len()).expect("size"),
            object_key: object_key.clone(),
            file_name: "p.jpg".into(),
            relative_path: Some("asset_photos/1/p.jpg".into()),
            capture_meta_json: None,
            updated_at: None,
        },
    )
    .await
    .expect("media row");
    store::record_media_manifest(
        &db,
        &[crate::sync::domain::SyncMediaManifestItem {
            entity_sync_id: "media-1".into(),
            content_sha256: sha.clone(),
            object_key: object_key.clone(),
            byte_size: i64::try_from(bytes.len()).expect("size"),
        }],
    )
    .await
    .expect("manifest");

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let upload_url = format!("{base}/object-storage/upload-1");
    let key_for_stub = object_key.clone();
    let stub = start_stub_on(listener, move |req| match req.path.as_str() {
        "/api/v1/sync/media/presign-put" => ok(json!({
            "url": upload_url,
            "method": "PUT",
            "bucket": "media",
            "object_key": key_for_stub,
            "expires_in_seconds": 900,
        })),
        "/object-storage/upload-1" => ok(json!({})),
        "/api/v1/sync/exchange" => ok(exchange_answer("v2.after", vec![], vec![], false)),
        _ => status(404, json!({ "error": "not_found" })),
    })
    .await;
    let handle = SyncWorkerHandle::new();
    let worker = SyncWorker::new(
        db.clone(),
        handle.clone(),
        SyncWorkerConfig {
            api_base_url: stub.base_url.clone(),
            media_root: root,
            timing: test_timing(),
        },
        FixedAccess::new(TOKEN),
    )
    .expect("worker");

    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let presign = stub.requests_to("/api/v1/sync/media/presign-put");
    assert_eq!(presign.len(), 1);
    assert_eq!(presign[0].json()["object_key"], object_key.as_str());
    assert_eq!(presign[0].json()["content_sha256"], sha.as_str());
    let put = stub.requests_to("/object-storage/upload-1");
    assert_eq!(put.len(), 1);
    assert_eq!(put[0].method, "PUT");
    assert_eq!(put[0].body.as_bytes(), bytes.as_slice());
    assert_eq!(
        put[0].header("authorization"),
        None,
        "the presigned URL is the credential"
    );
    let state: String = scalar(
        &db,
        "SELECT status FROM sync_media_uploads WHERE object_key = ?",
        vec![object_key.into()],
    )
    .await;
    assert_eq!(state, "uploaded");
    assert_eq!(handle.status().media_pending_count, 0);
}

#[tokio::test]
async fn control_rows_are_recreated_after_a_local_data_wipe() {
    let db = setup_db().await;
    // `reset_local_tenant_runtime_data` deletes every row of these tables.
    exec(&db, "DELETE FROM sync_replica_identity", vec![]).await;
    exec(&db, "DELETE FROM sync_transport_state", vec![]).await;
    assert!(store::load_state(&db).await.is_err());

    queries::ensure_sync_control_rows(&db).await.expect("re-seed");
    queries::ensure_sync_control_rows(&db).await.expect("idempotent");
    stage_team(&db, "team-after-reset").await;
    let state = store::load_state(&db).await.expect("state");
    assert!(state.pending.is_none() && state.paused_until.is_none());
}

#[tokio::test]
async fn received_media_is_downloaded_verified_and_retried() {
    let db = setup_db().await;
    seed_checkpoint(&db, "v2.start").await;
    let bytes = b"%PDF report from device A".to_vec();
    let sha = crate::sync::media::content_sha256_hex(&bytes);
    let object_key = format!("prod/media/tenant-1/work_orders/wo-1/{sha}");
    let relative = "wo_attachments/7/0b7c-report.pdf".to_string();
    crate::sync::media::queue_media_download(
        &db,
        "media-in-1",
        &object_key,
        &sha,
        i64::try_from(bytes.len()).expect("size"),
        Some(&relative),
    )
    .await
    .expect("queue");
    // A row whose storage location would leave the media directories is never written.
    crate::sync::media::queue_media_download(&db, "media-in-2", &object_key, &sha, 3, Some("maintafox.db"))
        .await
        .expect("queue");

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let get_url = format!("{base}/object-storage/get-1");
    let key_for_stub = object_key.clone();
    // Object storage answers: not uploaded yet, then wrong bytes, then the file.
    let answers = Arc::new(Mutex::new(vec![
        Reply::Bytes { status: 200, body: bytes.clone() },
        Reply::Bytes { status: 200, body: b"%PDF REPORT from device A".to_vec() },
        Reply::Bytes { status: 404, body: b"<Error><Code>NoSuchKey</Code></Error>".to_vec() },
    ]));
    let stub = start_stub_on(listener, move |req| match req.path.as_str() {
        "/api/v1/sync/media/presign-get" => ok(json!({
            "url": get_url,
            "method": "GET",
            "bucket": "media",
            "object_key": key_for_stub,
            "expires_in_seconds": 900,
        })),
        "/object-storage/get-1" => answers.lock().expect("answers").pop().expect("an answer"),
        "/api/v1/sync/exchange" => ok(exchange_answer("v2.after", vec![], vec![], false)),
        _ => status(404, json!({ "error": "not_found" })),
    })
    .await;
    let root = media_root();
    let handle = SyncWorkerHandle::new();
    let worker = SyncWorker::new(
        db.clone(),
        handle.clone(),
        SyncWorkerConfig {
            api_base_url: stub.base_url.clone(),
            media_root: root.clone(),
            timing: test_timing(),
        },
        FixedAccess::new(TOKEN),
    )
    .expect("worker");
    let file_state = |db: DatabaseConnection, root: PathBuf, path: String| async move {
        crate::sync::media::media_file_states(&db, &root, &[path])
            .await
            .expect("states")
            .remove(0)
    };
    let due_now = || exec(&db, "UPDATE sync_media_downloads SET next_attempt_at = NULL", vec![]);

    // 1. Not on object storage yet: stays pending, retried later, never given up.
    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let state = file_state(db.clone(), root.clone(), relative.clone()).await;
    assert_eq!(state.state, "pending");
    assert!(state.last_error.as_deref().unwrap_or_default().contains("not on the server yet"));
    let next: Option<String> = scalar(
        &db,
        "SELECT next_attempt_at FROM sync_media_downloads WHERE entity_sync_id = 'media-in-1'",
        vec![],
    )
    .await;
    assert!(next.is_some(), "a retry is scheduled");
    let bad: String = scalar(
        &db,
        "SELECT status FROM sync_media_downloads WHERE entity_sync_id = 'media-in-2'",
        vec![],
    )
    .await;
    assert_eq!(bad, "failed", "a path outside the media directories is refused");
    assert!(!root.join("maintafox.db").exists());
    assert_eq!(handle.status().media_download_pending_count, 1);
    assert_eq!(handle.status().media_download_failed_count, 1);

    // 2. Wrong content: refused, nothing left on disk.
    due_now().await;
    worker.cycle_once().await;
    let state = file_state(db.clone(), root.clone(), relative.clone()).await;
    assert_eq!(state.state, "pending");
    assert!(state.last_error.as_deref().unwrap_or_default().contains("content hash mismatch"));
    let leftovers = std::fs::read_dir(root.join("wo_attachments/7")).expect("dir").count();
    assert_eq!(leftovers, 0, "a refused download leaves no file");

    // 3. The file: stored at its place, verified.
    due_now().await;
    worker.cycle_once().await;
    assert_eq!(std::fs::read(root.join(&relative)).expect("file"), bytes);
    let state = file_state(db.clone(), root.clone(), relative.clone()).await;
    assert_eq!(state.state, "available");
    assert_eq!(stub.requests_to("/object-storage/get-1").len(), 3);
    for get in stub.requests_to("/object-storage/get-1") {
        assert_eq!(get.header("authorization"), None, "the presigned URL is the credential");
    }
    assert_eq!(handle.status().media_download_pending_count, 0);

    // 4. A failed download can be queued again by a person.
    assert_eq!(
        crate::sync::media::retry_media_download(&db, "maintafox.db").await.expect("retry"),
        1
    );
    let again: String = scalar(
        &db,
        "SELECT status FROM sync_media_downloads WHERE entity_sync_id = 'media-in-2'",
        vec![],
    )
    .await;
    assert_eq!(again, "pending");
}

#[tokio::test]
async fn a_rate_limited_snapshot_continues_from_the_stored_page() {
    let db = setup_db().await;
    let refused_once = Arc::new(Mutex::new(false));
    let refused = refused_once.clone();
    let stub = start_stub(move |req| match req.path.as_str() {
        "/api/v1/sync/snapshot" => match req.json()["page_token"].as_str() {
            None => ok(snapshot_page(vec![inbound("w-0", 1)], Some("page-2"), None)),
            Some("page-2") => {
                let mut refused = refused.lock().expect("flag");
                if *refused {
                    ok(snapshot_page(vec![inbound("w-1", 2)], None, Some("v2.cp.sig")))
                } else {
                    *refused = true;
                    Reply::Json {
                        status: 429,
                        body: json!({ "error": "rate_limited", "message": "Too many requests; retry later." }),
                        headers: vec![("Retry-After", "1".to_string())],
                    }
                }
            }
            Some(other) => status(400, json!({ "error": "invalid_page_token", "message": other })),
        },
        "/api/v1/sync/exchange" => ok(exchange_answer("v2.cp2.sig", vec![], vec![], false)),
        _ => status(404, json!({ "error": "not_found" })),
    })
    .await;
    let (worker, _handle) = make_worker(&db, &stub.base_url, FixedAccess::new(TOKEN), test_timing());

    let (outcome, _) = worker.cycle_once().await;
    assert!(
        matches!(outcome, CycleOutcome::Retry { retry_after: Some(d), .. } if d == Duration::from_secs(1)),
        "{outcome:?}"
    );
    assert_eq!(
        store::load_snapshot_progress(&db).await.expect("progress").map(|(_, t)| t).as_deref(),
        Some("page-2"),
        "the position of the snapshot is kept"
    );

    let (outcome, _) = worker.cycle_once().await;
    assert_eq!(outcome, CycleOutcome::Synced { more_work: false });
    let tokens: Vec<Value> = stub
        .requests_to("/api/v1/sync/snapshot")
        .iter()
        .map(|r| r.json()["page_token"].clone())
        .collect();
    assert_eq!(
        tokens,
        vec![Value::Null, json!("page-2"), json!("page-2")],
        "the second cycle continues with page 2, not from the first page"
    );
    assert_eq!(store::load_snapshot_progress(&db).await.expect("progress"), None);
    let received: i64 = scalar(
        &db,
        "SELECT COUNT(*) FROM sync_inbox WHERE entity_sync_id IN ('w-0', 'w-1')",
        vec![],
    )
    .await;
    assert_eq!(received, 2, "the rows of both pages are kept");
}
