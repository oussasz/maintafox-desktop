//! Multi-device harness (Phase 2.5): three desktop databases sync through the real sync worker
//! against the real control API on a real Postgres.
//!
//! Run it with `pnpm sync:harness` (`scripts/sync-harness.mjs`): the script starts the database
//! and the API, activates devices A, B and C, and passes their tokens in the file named by
//! `MAINTAFOX_SYNC_HARNESS_CONFIG`. Without that file the test is ignored by `cargo test`.
//!
//! Scenarios, in order (they share the tenant on the server):
//! 1. first sync of A and B; a row created on A reaches B;
//! 2. the same row edited on A and B: B gets a conflict, resolved once keeping B's version and
//!    once taking the server's version; both devices end equal;
//! 3. B offline: its change waits, then reaches A once B is back online;
//! 4. per-user rows: reach the devices where their owner is signed in, and only those;
//! 5. delete: the tombstone removes the row on B;
//! 6. new device C: bootstraps from a snapshot (live rows, no deleted row, no foreign personal row);
//! 7. crash mid-exchange: the server commits A's push but the answer is lost; A resends the same
//!    exchange after a restart and nothing is applied twice;
//! 8. backup restore: B returns to a state before its own latest change; after the reset startup
//!    does for a restored database (`store::forget_server_position`), the snapshot brings it back;
//! 9. revoked device: C is revoked by the operator; it is refused and its new row reaches no one.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sea_orm::DatabaseConnection;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::domain::{ResolveSyncConflictInput, SyncConflictFilter, SyncConflictRecord};
use super::queries::{list_sync_conflicts, resolve_sync_conflict};
use super::registry;
use super::test_support::{
    current_payload, editable_text_column, exec, scalar_i64, scalar_opt_text, started_db, CreatedRow, RowFactory,
};
use super::transport::worker::{CycleOutcome, SyncAccess, SyncAccessProvider};
use super::transport::{store, SyncTiming, SyncWorker, SyncWorkerConfig, SyncWorkerHandle};
use crate::errors::AppResult;

const CONFIG_ENV: &str = "MAINTAFOX_SYNC_HARNESS_CONFIG";

#[derive(Debug, Deserialize)]
struct HarnessConfig {
    api_base_url: String,
    control_url: String,
    devices: Vec<DeviceConfig>,
}

#[derive(Debug, Deserialize)]
struct DeviceConfig {
    name: String,
    fingerprint: String,
    token: String,
}

/// The harness only ever talks to the API the script started on this machine.
fn require_local(url: &str) {
    let rest = url
        .strip_prefix("http://")
        .unwrap_or_else(|| panic!("harness URL must be plain http on this machine: {url}"));
    let host = rest.split([':', '/']).next().unwrap_or_default();
    assert!(
        host == "127.0.0.1" || host == "localhost",
        "the harness only talks to a local control API, not {url}"
    );
}

struct ActivationToken(String);

impl SyncAccessProvider for ActivationToken {
    fn access<'a>(
        &'a self,
        _db: &'a DatabaseConnection,
    ) -> Pin<Box<dyn Future<Output = AppResult<SyncAccess>> + Send + 'a>> {
        let token = self.0.clone();
        Box::pin(async move { Ok(SyncAccess::Granted { token }) })
    }
}

struct Device {
    name: String,
    fingerprint: String,
    token: String,
    db: DatabaseConnection,
    factory: RowFactory,
}

impl Device {
    /// One worker cycle against `base_url`, by a worker created for it (as after a restart).
    async fn cycle(&self, base_url: &str) -> CycleOutcome {
        let media_root = std::env::temp_dir().join(format!("mfx-sync-harness-{}-{}", self.name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&media_root).expect("media root");
        let worker = SyncWorker::new(
            self.db.clone(),
            SyncWorkerHandle::new(),
            SyncWorkerConfig {
                api_base_url: base_url.to_string(),
                media_root,
                timing: SyncTiming::production(),
            },
            Arc::new(ActivationToken(self.token.clone())),
        )
        .expect("sync worker");
        worker.cycle_once().await.0
    }

    /// Cycles until the device has nothing left to send or receive.
    async fn sync(&self, base_url: &str) -> CycleOutcome {
        for _ in 0..30 {
            match self.cycle(base_url).await {
                CycleOutcome::Synced { more_work: true } => continue,
                other => return other,
            }
        }
        panic!("device {} still had work after 30 cycles", self.name);
    }

    async fn synced(&self, base_url: &str) {
        let outcome = self.sync(base_url).await;
        assert!(
            matches!(outcome, CycleOutcome::Synced { more_work: false }),
            "device {}: sync did not complete: {outcome:?}",
            self.name
        );
    }

    async fn create(&mut self, entity_type: &str) -> CreatedRow {
        self.factory
            .create(&self.db, entity_type)
            .await
            .unwrap_or_else(|e| panic!("device {}: create {entity_type}: {e}", self.name))
    }

    async fn payload(&self, row: &CreatedRow) -> Option<serde_json::Map<String, serde_json::Value>> {
        current_payload(&self.db, row.entity_type, &row.entity_sync_id, &row.payload)
            .await
            .unwrap_or_else(|e| panic!("device {}: read {}: {e}", self.name, row.entity_type))
    }

    async fn text(&self, row: &CreatedRow, column: &str) -> Option<String> {
        let table = registry::entity(row.entity_type).expect("registered").table;
        scalar_opt_text(
            &self.db,
            &format!("SELECT \"{column}\" FROM \"{table}\" WHERE entity_sync_id = ?"),
            vec![row.entity_sync_id.clone().into()],
        )
        .await
    }

    async fn set_text(&self, row: &CreatedRow, column: &str, value: &str) {
        let table = registry::entity(row.entity_type).expect("registered").table;
        exec(
            &self.db,
            &format!("UPDATE \"{table}\" SET \"{column}\" = ? WHERE entity_sync_id = ?"),
            vec![value.into(), row.entity_sync_id.clone().into()],
        )
        .await;
    }

    async fn conflicts_of(&self, row: &CreatedRow) -> Vec<SyncConflictRecord> {
        list_sync_conflicts(
            &self.db,
            SyncConflictFilter {
                statuses: None,
                conflict_type: None,
                requires_operator_review: None,
                limit: Some(200),
            },
        )
        .await
        .expect("list conflicts")
        .into_iter()
        .filter(|c| c.entity_sync_id == row.entity_sync_id)
        .collect()
    }

    async fn open_conflict(&self, row: &CreatedRow) -> SyncConflictRecord {
        self.conflicts_of(row)
            .await
            .into_iter()
            .find(|c| c.resolved_at.is_none())
            .unwrap_or_else(|| panic!("device {}: no open conflict for {}", self.name, row.entity_type))
    }

    async fn resolve(&self, conflict: &SyncConflictRecord, action: &str) {
        let person = scalar_i64(&self.db, "SELECT id FROM user_accounts ORDER BY id LIMIT 1", vec![]).await;
        resolve_sync_conflict(
            &self.db,
            person,
            ResolveSyncConflictInput {
                conflict_id: conflict.id,
                expected_row_version: conflict.row_version,
                action: action.to_string(),
                resolution_note: Some(format!("sync harness: {action}")),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("device {}: resolve {action}: {e}", self.name));
    }

    /// Signs `user_sync_id` in on this device (trusted device), so its personal rows come here.
    async fn trust(&self, user_sync_id: &str) {
        let user_id = scalar_i64(
            &self.db,
            "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
            vec![user_sync_id.into()],
        )
        .await;
        exec(
            &self.db,
            "INSERT INTO trusted_devices (id, device_fingerprint, user_id, trusted_at, is_revoked)
             VALUES (?, ?, ?, '2026-10-11T00:00:00Z', 0)",
            vec![
                format!("harness-{}-{user_sync_id}", self.name).into(),
                self.fingerprint.clone().into(),
                user_id.into(),
            ],
        )
        .await;
    }
}

async fn assert_same(a: &Device, b: &Device, row: &CreatedRow, stage: &str) {
    let on_a = a.payload(row).await;
    let on_b = b.payload(row).await;
    assert!(on_a.is_some(), "{stage}: {} missing on {}", row.entity_type, a.name);
    assert_eq!(on_a, on_b, "{stage}: {} differs between {} and {}", row.entity_type, a.name, b.name);
}

/// A local proxy to the API that lets the first exchange reach the server, waits for the server's
/// complete answer, and then drops the connection without passing it on: the device sees the
/// exchange fail exactly as if it had crashed (or lost the network) after sending it.
async fn answer_losing_proxy(upstream: &str) -> String {
    let upstream = upstream
        .strip_prefix("http://")
        .expect("http upstream")
        .trim_end_matches('/')
        .to_string();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("proxy listener");
    let url = format!("http://{}", listener.local_addr().expect("proxy address"));
    let lost_one = Arc::new(AtomicBool::new(false));
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            let upstream = upstream.clone();
            let lost_one = lost_one.clone();
            tokio::spawn(async move {
                let Some(request) = read_http_message(&mut client).await else {
                    return;
                };
                let Ok(mut server) = TcpStream::connect(&upstream).await else {
                    return;
                };
                if server.write_all(&request).await.is_err() {
                    return;
                }
                let is_exchange = request.starts_with(b"POST /api/v1/sync/exchange ");
                if is_exchange && !lost_one.swap(true, Ordering::SeqCst) {
                    // The server processes the whole exchange; its answer never reaches the device.
                    let _ = read_http_message(&mut server).await;
                    return;
                }
                let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            });
        }
    });
    url
}

/// Reads one HTTP/1.1 message (head and body, by Content-Length or chunked encoding).
async fn read_http_message(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&data[..head_end]).to_ascii_lowercase();
    let content_length = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok());
    let chunked = head
        .lines()
        .any(|l| l.starts_with("transfer-encoding:") && l.contains("chunked"));
    loop {
        let body = &data[head_end..];
        let complete = match content_length {
            Some(len) => body.len() >= len,
            None if chunked => body.ends_with(b"0\r\n\r\n"),
            None => true,
        };
        if complete {
            return Some(data);
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return Some(data);
        }
        data.extend_from_slice(&chunk[..n]);
    }
}

/// A local address where nothing listens: the device is offline.
async fn unreachable_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("probe listener");
    let url = format!("http://{}", listener.local_addr().expect("probe address"));
    drop(listener);
    url
}

#[tokio::test]
#[ignore = "needs the control API and Postgres: run `pnpm sync:harness`"]
async fn multi_device_harness() {
    let path =
        std::env::var(CONFIG_ENV).unwrap_or_else(|_| panic!("{CONFIG_ENV} is not set: run `pnpm sync:harness`"));
    let config: HarnessConfig =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("harness config")).expect("harness config JSON");
    require_local(&config.api_base_url);
    require_local(&config.control_url);
    let api = config.api_base_url.as_str();
    let mut devices = Vec::new();
    for (index, d) in (1_i64..).zip(config.devices) {
        devices.push(Device {
            name: d.name,
            fingerprint: d.fingerprint,
            token: d.token,
            db: started_db().await,
            factory: RowFactory::for_device(index),
        });
    }
    let [mut a, mut b, mut c]: [Device; 3] = devices
        .try_into()
        .unwrap_or_else(|_| panic!("three devices expected"));

    // 1. First sync; a row created on A reaches B.
    a.synced(api).await;
    b.synced(api).await;
    let team = a.create("teams").await;
    a.synced(api).await;
    b.synced(api).await;
    assert_same(&a, &b, &team, "1. create").await;
    println!("[harness] 1. create on A reached B");

    // 2. Concurrent edit, resolved both ways by a person.
    let column = editable_text_column(&a.db, "teams")
        .await
        .expect("editable text column of teams");
    a.set_text(&team, &column, "edited on A").await;
    b.set_text(&team, &column, "edited on B").await;
    a.synced(api).await;
    b.synced(api).await;
    let conflict = b.open_conflict(&team).await;
    b.resolve(&conflict, "accept_local").await;
    b.synced(api).await;
    a.synced(api).await;
    assert_eq!(
        a.text(&team, &column).await.as_deref(),
        Some("edited on B"),
        "B's kept version reaches A"
    );
    assert_same(&a, &b, &team, "2a. keep local").await;

    a.set_text(&team, &column, "second edit on A").await;
    b.set_text(&team, &column, "second edit on B").await;
    a.synced(api).await;
    b.synced(api).await;
    let conflict = b.open_conflict(&team).await;
    b.resolve(&conflict, "accept_remote").await;
    b.synced(api).await;
    a.synced(api).await;
    assert_eq!(
        b.text(&team, &column).await.as_deref(),
        Some("second edit on A"),
        "B takes the server's version"
    );
    assert_same(&a, &b, &team, "2b. take remote").await;
    println!("[harness] 2. concurrent edit: conflict shown on B, resolved keeping B's version and taking A's");

    // 3. Offline, then reconnect.
    let offline = unreachable_url().await;
    let made_offline = b.create("teams").await;
    let outcome = b.sync(&offline).await;
    assert!(
        matches!(outcome, CycleOutcome::Offline { .. } | CycleOutcome::Retry { .. }),
        "an unreachable API is offline, not an error: {outcome:?}"
    );
    assert!(a.payload(&made_offline).await.is_none(), "nothing travelled while B was offline");
    b.synced(api).await;
    a.synced(api).await;
    assert_same(&a, &b, &made_offline, "3. reconnect").await;
    println!("[harness] 3. offline change on B reached A after reconnecting");

    // 4. Per-user rows go only where their owner is signed in.
    let preference = a.create("notification_preferences").await;
    let owner = scalar_opt_text(
        &a.db,
        "SELECT u.entity_sync_id FROM notification_preferences p JOIN user_accounts u ON u.id = p.user_id
         WHERE p.entity_sync_id = ?",
        vec![preference.entity_sync_id.clone().into()],
    )
    .await
    .expect("owner of the preference");
    a.trust(&owner).await;
    a.synced(api).await;
    b.synced(api).await;
    assert!(
        b.payload(&preference).await.is_none(),
        "B does not get the personal row of someone not signed in there"
    );
    b.trust(&owner).await;
    b.synced(api).await;
    assert_same(&a, &b, &preference, "4. per-user").await;
    println!("[harness] 4. personal row reached B once its owner was signed in there");

    // 5. Delete: the tombstone removes the row elsewhere.
    let table = registry::entity("teams").expect("teams").table;
    exec(
        &a.db,
        &format!("DELETE FROM \"{table}\" WHERE entity_sync_id = ?"),
        vec![team.entity_sync_id.clone().into()],
    )
    .await;
    a.synced(api).await;
    b.synced(api).await;
    assert!(b.payload(&team).await.is_none(), "the deleted row is gone on B");
    println!("[harness] 5. delete on A removed the row on B");

    // 6. A new device bootstraps from a snapshot.
    c.synced(api).await;
    assert_same(&a, &c, &made_offline, "6. snapshot").await;
    assert!(c.payload(&team).await.is_none(), "a deleted row is not in the snapshot");
    assert!(
        c.payload(&preference).await.is_none(),
        "the snapshot of C holds no personal row of a person not signed in on C"
    );
    println!("[harness] 6. new device C bootstrapped from the snapshot");

    // 7. Crash mid-exchange: the server commits, the answer is lost, the device resends.
    let proxy = answer_losing_proxy(api).await;
    let crash_row = a.create("teams").await;
    let outcome = a.cycle(&proxy).await;
    assert!(
        !matches!(outcome, CycleOutcome::Synced { .. }),
        "the device never saw the answer: {outcome:?}"
    );
    assert!(
        store::load_state(&a.db).await.expect("transport state").pending.is_some(),
        "the unanswered exchange is kept to be sent again"
    );
    b.synced(api).await;
    assert_same(&a, &b, &crash_row, "7. committed before the answer was lost").await;
    a.synced(api).await;
    assert!(
        store::load_state(&a.db).await.expect("transport state").pending.is_none(),
        "the resent exchange was answered"
    );
    assert!(a.conflicts_of(&crash_row).await.is_empty(), "the resent exchange is not a conflict");
    b.synced(api).await;
    let copies = scalar_i64(
        &b.db,
        &format!("SELECT COUNT(*) FROM \"{table}\" WHERE entity_sync_id = ?"),
        vec![crash_row.entity_sync_id.clone().into()],
    )
    .await;
    assert_eq!(copies, 1, "the resent exchange is applied once");
    println!("[harness] 7. exchange resent after a lost answer: applied once, no conflict");

    // 8. Backup restore: B goes back to a state before its own latest change.
    let after_backup = b.create("teams").await;
    b.synced(api).await;
    // What the restored file lacks: the row and its sync bookkeeping (written after the backup).
    exec(&b.db, "INSERT INTO sync_capture_suppression (id) VALUES (1)", vec![]).await;
    exec(
        &b.db,
        &format!("DELETE FROM \"{table}\" WHERE entity_sync_id = ?"),
        vec![after_backup.entity_sync_id.clone().into()],
    )
    .await;
    exec(&b.db, "DELETE FROM sync_capture_suppression", vec![]).await;
    exec(
        &b.db,
        "DELETE FROM sync_entity_state WHERE entity_sync_id = ?",
        vec![after_backup.entity_sync_id.clone().into()],
    )
    .await;
    b.synced(api).await;
    assert!(
        b.payload(&after_backup).await.is_none(),
        "without a reset, the server never sends a device its own change back"
    );
    // What startup does after swapping a restored backup in (`startup::run_startup_sequence`).
    store::forget_server_position(&b.db).await.expect("forget server position");
    b.synced(api).await;
    a.synced(api).await;
    assert_same(&a, &b, &after_backup, "8. after restore").await;
    println!("[harness] 8. after a backup restore, B got its own later change back from the snapshot");

    // 9. Revoked device.
    let response = reqwest::Client::new()
        .post(format!("{}/machines/{}/revoke", config.control_url, c.fingerprint))
        .send()
        .await
        .expect("control endpoint");
    assert!(response.status().is_success(), "revoke: HTTP {}", response.status());
    let after_revoke = c.create("teams").await;
    let outcome = c.sync(api).await;
    assert!(
        matches!(outcome, CycleOutcome::Blocked { .. }),
        "a revoked device is refused: {outcome:?}"
    );
    a.synced(api).await;
    assert!(
        a.payload(&after_revoke).await.is_none(),
        "nothing from the revoked device reaches A"
    );
    println!("[harness] 9. revoked device C was refused; its change reached no one");
}
