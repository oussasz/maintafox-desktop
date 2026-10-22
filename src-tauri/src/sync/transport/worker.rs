//! Sync worker: runs as background task `sync` and speaks protocol v2 with the control API.
//!
//! One cycle:
//! 1. Stop early when sync is paused, not licensed, not activated, or the server refused this
//!    activation token (it waits for another token, e.g. after the licence check re-claims).
//! 2. Without a v2 delta checkpoint, bootstrap with a scope `all` snapshot.
//! 3. Exchange until the server has nothing more (`has_more`) and the outbox is drained:
//!    - the request (idempotency key + exact body + outbox ids) is stored before it is sent, so a
//!      crash or a failure resends the same key with the same body;
//!    - the answer is applied (`apply_sync_batch`, which stores the checkpoint in the same
//!      transaction), then the stored request is cleared and the inbox materialized;
//!    - users in `user_snapshot_required` get a scope `user` snapshot.
//! 4. Media announced in the manifest is uploaded before each exchange (failures never block it).
//! 5. Media received from other devices (`sync_media_downloads`) is downloaded at the end of a
//!    cycle that synced: presigned GET, streamed to a temporary file, size and SHA-256 checked,
//!    then moved into place; failures are retried per file with backoff and never block sync.
//! 6. A cycle whose exchanges reached the server head records whether the organisation has user
//!    accounts on the server: the first administrator of a new PC waits for that (`plant_join`).
//!
//! Failures: no connection → `offline`, retried with backoff and woken by the webview `online`
//! event; 5xx / interrupted → retried with the same key and counted as a delivery attempt
//! (`record_outbox_delivery_failure`, dead letter after `OUTBOX_MAX_DELIVERY_ATTEMPTS`); 429 →
//! `Retry-After`; 401/403 → stopped until the activation token changes (the UI starts a licence
//! check); contract errors (other 4xx) → shown as an error, never resent unchanged.

use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::Rng;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use super::client::{ClientTimeouts, ControlPlaneClient, TransportError, OBJECT_NOT_FOUND};
use super::store::{self, PendingExchange, PendingMediaDownload, PendingMediaUpload};
use super::wire::{
    ExchangeOutboxItem, ExchangeRequest, PresignGetRequest, PresignPutRequest, SnapshotRequest, StoredExchangeRequest,
    DEVICE_CLASS_DESKTOP, MAX_CURRENT_USER_SYNC_IDS, SNAPSHOT_SCOPE_ALL, SNAPSHOT_SCOPE_USER,
};
use crate::errors::{AppError, AppResult};
use crate::sync::domain::{SyncMediaManifestItem, SYNC_PROTOCOL_VERSION_V2};
use crate::sync::materialize::materialize_inbox;
use crate::sync::queries::{self, hash_payload, OUTBOX_MAX_DELIVERY_ATTEMPTS};

/// Background task id in `BackgroundTaskSupervisor`.
pub const SYNC_TASK_ID: &str = "sync";

/// Tauri event carrying every `SyncWorkerStatus` change to the webview.
pub const SYNC_STATUS_EVENT: &str = "sync-worker-status";

// ── Timing (production values; tests pass shorter ones through `SyncTiming`) ────────────────

/// Pause between two cycles when everything is in sync (the previous webview heartbeat was 60 s).
pub const SYNC_IDLE_INTERVAL: Duration = Duration::from_secs(60);
/// First retry delay after a failure; doubles per consecutive failure.
pub const SYNC_RETRY_BASE_DELAY: Duration = Duration::from_secs(2);
/// Longest retry delay (also the slowest offline probe).
pub const SYNC_RETRY_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
/// Retry delays vary randomly by ±25 % so devices do not retry in lockstep after an outage.
pub const SYNC_RETRY_JITTER_RATIO: f64 = 0.25;
/// How often a blocked worker (not activated, not licensed, token refused) re-reads local state.
pub const SYNC_BLOCKED_RECHECK_INTERVAL: Duration = Duration::from_secs(60);
/// A network `online` event wakes an offline worker, but never sooner than this after the last
/// attempt started (a flapping network must not hammer the server).
pub const SYNC_NETWORK_WAKE_MIN_SPACING: Duration = Duration::from_secs(5);
/// Connection set-up limit; past it the server counts as unreachable.
pub const SYNC_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Whole-request limit for exchange and snapshot calls (same as the previous webview transport).
pub const SYNC_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Whole-request limit for one media upload or download to/from object storage.
pub const SYNC_MEDIA_TRANSFER_TIMEOUT: Duration = Duration::from_secs(10 * 60);

// ── Sizes ───────────────────────────────────────────────────────────────────────────────────

/// Outbox items per exchange (the server accepts up to 1000).
pub const SYNC_OUTBOX_BATCH_SIZE: i64 = 200;
/// Snapshot page size: the server's maximum (1..1000), so a large tenant needs as few requests as
/// possible under the per-device request limit.
pub const SYNC_SNAPSHOT_PAGE_SIZE: u32 = 1000;
/// Exchanges in one cycle before the worker yields (it starts the next cycle at once).
pub const SYNC_MAX_EXCHANGES_PER_CYCLE: usize = 50;
/// Inbox rows materialized per pass (passes continue until nothing more can be applied).
pub const SYNC_MATERIALIZE_BATCH: i64 = 500;
/// A snapshot whose page token the server refuses restarts from page 1 at most this many times.
pub const SYNC_MAX_SNAPSHOT_RESTARTS: u32 = 3;
/// Media uploads tried per exchange.
pub const SYNC_MEDIA_UPLOADS_PER_CYCLE: u32 = 20;
/// Media downloads tried per cycle.
pub const SYNC_MEDIA_DOWNLOADS_PER_CYCLE: u32 = 20;
/// First retry delay of a failed media download; doubles per attempt of that file.
pub const SYNC_MEDIA_RETRY_BASE_DELAY: Duration = Duration::from_secs(30);
/// Longest retry delay of a media download.
pub const SYNC_MEDIA_RETRY_MAX_DELAY: Duration = Duration::from_secs(60 * 60);
/// Longest operator pause (same bound as the previous Sync Center pause).
pub const SYNC_MAX_PAUSE_MINUTES: u32 = 24 * 60;

// ── Blocker codes shown by the UI (`shell:sync.worker.blocker.*`) ─────────────────────────

pub const BLOCKER_NOT_LICENSED: &str = "not_licensed";
pub const BLOCKER_NOT_ACTIVATED: &str = "not_activated";
pub const BLOCKER_ACTIVATION_REFUSED: &str = "activation_refused";
pub const BLOCKER_TENANT_MISMATCH: &str = "tenant_config_mismatch";
pub const BLOCKER_NOT_CONFIGURED: &str = "not_configured";

#[derive(Debug, Clone, Copy)]
pub struct SyncTiming {
    pub idle_interval: Duration,
    pub retry_base: Duration,
    pub retry_max: Duration,
    pub blocked_recheck: Duration,
    pub network_wake_min_spacing: Duration,
    /// Per-file retry delays of media downloads.
    pub media_retry_base: Duration,
    pub media_retry_max: Duration,
    pub timeouts: ClientTimeouts,
}

impl SyncTiming {
    pub const fn production() -> Self {
        Self {
            idle_interval: SYNC_IDLE_INTERVAL,
            retry_base: SYNC_RETRY_BASE_DELAY,
            retry_max: SYNC_RETRY_MAX_DELAY,
            blocked_recheck: SYNC_BLOCKED_RECHECK_INTERVAL,
            network_wake_min_spacing: SYNC_NETWORK_WAKE_MIN_SPACING,
            media_retry_base: SYNC_MEDIA_RETRY_BASE_DELAY,
            media_retry_max: SYNC_MEDIA_RETRY_MAX_DELAY,
            timeouts: ClientTimeouts {
                connect: SYNC_CONNECT_TIMEOUT,
                request: SYNC_REQUEST_TIMEOUT,
                media_transfer: SYNC_MEDIA_TRANSFER_TIMEOUT,
            },
        }
    }
}

/// Delay before retry number `failures` (1 = first retry): exponential, capped, with jitter.
/// `jitter` is a random value in `[0, 1)`.
pub fn retry_delay(failures: u32, timing: &SyncTiming, jitter: f64) -> Duration {
    backoff(failures, timing.retry_base, timing.retry_max, jitter)
}

/// Delay before attempt number `attempts + 1` of one media download.
pub fn media_retry_delay(attempts: u32, timing: &SyncTiming, jitter: f64) -> Duration {
    backoff(attempts, timing.media_retry_base, timing.media_retry_max, jitter)
}

fn backoff(failures: u32, base: Duration, max: Duration, jitter: f64) -> Duration {
    let exponent = failures.saturating_sub(1).min(20);
    let exponential = base.saturating_mul(1_u32 << exponent);
    let capped = exponential.min(max);
    let factor = 1.0 + SYNC_RETRY_JITTER_RATIO * (2.0 * jitter.clamp(0.0, 1.0) - 1.0);
    capped.mul_f64(factor).max(base.min(capped))
}

#[derive(Debug, Clone)]
pub struct SyncWorkerConfig {
    /// Control API origin (`build.rs` → `MAINTAFOX_CONTROL_PLANE_API_BASE_URL`).
    pub api_base_url: String,
    /// Directory that `media_attachments.relative_path` is relative to (the app data directory).
    pub media_root: PathBuf,
    pub timing: SyncTiming,
}

// ── Access (licence + activation) ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAccess {
    Granted { token: String },
    NotLicensed,
    NotActivated,
}

type AccessFuture<'a> = Pin<Box<dyn Future<Output = AppResult<SyncAccess>> + Send + 'a>>;

/// Decides whether this device may sync now and with which activation token.
pub trait SyncAccessProvider: Send + Sync {
    fn access<'a>(&'a self, db: &'a DatabaseConnection) -> AccessFuture<'a>;
}

/// Production access: the entitlement must allow the `sync` capability and the activation must be
/// active (the same rules the previous webview transport applied).
pub struct LicenseSyncAccess;

impl SyncAccessProvider for LicenseSyncAccess {
    fn access<'a>(&'a self, db: &'a DatabaseConnection) -> AccessFuture<'a> {
        Box::pin(async move {
            let capability = crate::entitlements::queries::check_entitlement_capability(db, "sync".to_string()).await?;
            if !capability.allowed {
                return Ok(SyncAccess::NotLicensed);
            }
            match crate::commands::product_license::sync_activation_bearer_token(db).await? {
                Some(token) if !token.trim().is_empty() => Ok(SyncAccess::Granted { token }),
                _ => Ok(SyncAccess::NotActivated),
            }
        })
    }
}

// ── Status and control ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncWorkerState {
    /// The worker has not finished its first cycle yet.
    Starting,
    Idle,
    Running,
    Paused,
    Offline,
    /// A temporary failure; the next attempt is scheduled.
    Retrying,
    /// Waiting for something only a person or the licence check can change (see `blocker`).
    Blocked,
    /// The server refused the request as invalid, or a local error happened; retried with backoff.
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncWorkerError {
    pub code: String,
    pub message: String,
    pub http_status: Option<u16>,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncWorkerStatus {
    pub state: SyncWorkerState,
    pub blocker: Option<String>,
    pub paused_until: Option<String>,
    pub last_success_at: Option<String>,
    pub last_attempt_at: Option<String>,
    pub next_attempt_at: Option<String>,
    /// Local changes not yet acknowledged (pending + being sent).
    pub pending_outbox_count: i64,
    pub dead_letter_outbox_count: i64,
    pub unresolved_conflict_count: i64,
    pub media_pending_count: i64,
    pub media_failed_count: i64,
    /// Files received from other devices and not stored here yet / given up.
    pub media_download_pending_count: i64,
    pub media_download_failed_count: i64,
    pub consecutive_failures: i64,
    pub last_error: Option<SyncWorkerError>,
}

impl SyncWorkerStatus {
    fn starting() -> Self {
        Self {
            state: SyncWorkerState::Starting,
            blocker: None,
            paused_until: None,
            last_success_at: None,
            last_attempt_at: None,
            next_attempt_at: None,
            pending_outbox_count: 0,
            dead_letter_outbox_count: 0,
            unresolved_conflict_count: 0,
            media_pending_count: 0,
            media_failed_count: 0,
            media_download_pending_count: 0,
            media_download_failed_count: 0,
            consecutive_failures: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncCommand {
    /// An operator asked for a sync now: forget the backoff and run a cycle.
    RunNow,
    /// Pause or resume changed: evaluate the state again now.
    Reevaluate,
    /// The webview saw the network come back.
    NetworkOnline,
    /// The signed-out first-start screen asks to join the plant (`request_plant_join`): run a
    /// cycle soon, but never sooner than `network_wake_min_spacing` after the last one started,
    /// never before a server `Retry-After`, never during a pause, and without forgetting the
    /// failure count (unlike `RunNow`). Repeated requests therefore coalesce into one cycle.
    JoinPlant,
}

struct HandleInner {
    commands: mpsc::UnboundedSender<SyncCommand>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<SyncCommand>>>,
    status: watch::Sender<SyncWorkerStatus>,
}

/// Shared handle to the worker (in `AppState`): status for the UI, commands for the worker.
#[derive(Clone)]
pub struct SyncWorkerHandle {
    inner: Arc<HandleInner>,
}

impl std::fmt::Debug for SyncWorkerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncWorkerHandle")
            .field("state", &self.inner.status.borrow().state)
            .finish_non_exhaustive()
    }
}

impl Default for SyncWorkerHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncWorkerHandle {
    pub fn new() -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (status, _) = watch::channel(SyncWorkerStatus::starting());
        Self {
            inner: Arc::new(HandleInner {
                commands,
                receiver: Mutex::new(Some(receiver)),
                status,
            }),
        }
    }

    pub fn status(&self) -> SyncWorkerStatus {
        self.inner.status.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<SyncWorkerStatus> {
        self.inner.status.subscribe()
    }

    pub fn send(&self, command: SyncCommand) {
        // The receiver lives as long as the worker; after shutdown there is nobody to wake.
        let _ = self.inner.commands.send(command);
    }

    /// The command receiver, handed to the worker once.
    pub fn take_receiver(&self) -> Option<mpsc::UnboundedReceiver<SyncCommand>> {
        self.inner.receiver.lock().map_or(None, |mut guard| guard.take())
    }

    fn publish(&self, status: SyncWorkerStatus) {
        self.inner.status.send_replace(status);
    }

    fn publish_with(&self, update: impl FnOnce(&mut SyncWorkerStatus)) {
        self.inner.status.send_modify(update);
    }

    /// The worker could not start (invalid configuration): say so instead of staying "starting".
    pub fn publish_not_configured(&self, message: &str) {
        self.publish_with(|s| {
            s.state = SyncWorkerState::Blocked;
            s.blocker = Some(BLOCKER_NOT_CONFIGURED.to_string());
            s.last_error = Some(SyncWorkerError {
                code: BLOCKER_NOT_CONFIGURED.to_string(),
                message: message.to_string(),
                http_status: None,
                at: store::now_utc(),
            });
        });
    }
}

/// Pause sync for `minutes` (1..=`SYNC_MAX_PAUSE_MINUTES`). Returns the end of the pause.
pub async fn pause_sync(db: &DatabaseConnection, handle: &SyncWorkerHandle, minutes: u32) -> AppResult<String> {
    if minutes == 0 || minutes > SYNC_MAX_PAUSE_MINUTES {
        return Err(AppError::ValidationFailed(vec![format!(
            "Pause duration must be between 1 and {SYNC_MAX_PAUSE_MINUTES} minutes."
        )]));
    }
    let until = (chrono::Utc::now() + chrono::Duration::minutes(i64::from(minutes)))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    store::set_paused_until(db, Some(&until)).await?;
    handle.send(SyncCommand::Reevaluate);
    Ok(until)
}

/// End a pause and forget a refused token, then sync now.
pub async fn resume_sync(db: &DatabaseConnection, handle: &SyncWorkerHandle) -> AppResult<()> {
    store::set_paused_until(db, None).await?;
    store::clear_refused(db).await?;
    handle.send(SyncCommand::RunNow);
    Ok(())
}

// ── Worker ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleOutcome {
    Synced {
        more_work: bool,
    },
    Paused {
        until: String,
    },
    Blocked {
        reason: String,
        error: Option<SyncWorkerError>,
    },
    Offline {
        error: SyncWorkerError,
    },
    Retry {
        error: SyncWorkerError,
        retry_after: Option<Duration>,
    },
    Failed {
        error: SyncWorkerError,
    },
}

/// How the worker reacts to a transport error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    /// Nothing reached the server.
    Offline,
    /// The server asked to slow down; nothing was processed.
    RateLimited,
    /// May have reached the server; resend the same request later.
    Retryable,
    /// 401/403: the activation is refused.
    ActivationRefused,
    /// The delta checkpoint is refused: bootstrap with a snapshot.
    CheckpointRefused,
    /// Same key with another body: a bug signal (the body is stored before sending).
    IdempotencyKeyReused,
    TenantMismatch,
    /// The request itself is invalid; resending it unchanged cannot help.
    Permanent,
}

fn classify(err: &TransportError) -> FailureKind {
    match err {
        TransportError::Offline(_) => FailureKind::Offline,
        TransportError::Interrupted(_) | TransportError::Contract(_) => FailureKind::Retryable,
        TransportError::Http { status, code, .. } => match (*status, code.as_deref()) {
            (401 | 403, _) => FailureKind::ActivationRefused,
            (429, _) => FailureKind::RateLimited,
            (400, Some("invalid_checkpoint_token")) | (409, Some("checkpoint_ahead_of_server")) => {
                FailureKind::CheckpointRefused
            }
            (409, Some("idempotency_key_reused")) => FailureKind::IdempotencyKeyReused,
            (409, Some("tenant_config_mismatch")) => FailureKind::TenantMismatch,
            (408, _) | (500..=599, _) => FailureKind::Retryable,
            _ => FailureKind::Permanent,
        },
    }
}

fn retry_after_of(err: &TransportError) -> Option<Duration> {
    match err {
        TransportError::Http { retry_after, .. } => *retry_after,
        _ => None,
    }
}

fn worker_error(err: &TransportError) -> SyncWorkerError {
    SyncWorkerError {
        code: err.code().to_string(),
        message: err.message(),
        http_status: err.http_status(),
        at: store::now_utc(),
    }
}

fn local_error(err: &AppError) -> SyncWorkerError {
    SyncWorkerError {
        code: "local_error".to_string(),
        message: err.to_string(),
        http_status: None,
        at: store::now_utc(),
    }
}

/// What to do after one exchange.
enum Step {
    Continue,
    Done,
    Stop(CycleOutcome),
}

enum MediaFailure {
    /// Nothing more can be transferred now (no connection, or the control API refuses this
    /// device); not counted as an attempt.
    Offline,
    /// The file is not on object storage yet (its device has not uploaded it): retried with
    /// backoff, never given up automatically.
    NotYetAvailable(String),
    Retry(String),
    Permanent(String),
}

/// When a plant-join request (`SyncCommand::JoinPlant`) may run the next cycle: `None` during a
/// pause; otherwise the wake spacing after the last cycle started, and not before the server's
/// `Retry-After` (counted from the end of the cycle that received it) has passed.
fn join_wake_at(outcome: &CycleOutcome, started: Instant, ended: Instant, timing: &SyncTiming) -> Option<Instant> {
    let spaced = started + timing.network_wake_min_spacing;
    match outcome {
        CycleOutcome::Paused { .. } => None,
        CycleOutcome::Retry {
            retry_after: Some(after),
            ..
        } => Some(spaced.max(ended + *after)),
        _ => Some(spaced),
    }
}

/// Mirrors the server's `assertTenantOwnsObjectKey`: `{env}/media/{tenant}/…`.
fn object_key_in_tenant(object_key: &str, tenant_id: &str) -> bool {
    let parts: Vec<&str> = object_key.split('/').collect();
    parts.len() >= 4 && parts[1] == "media" && parts[2] == tenant_id
}

/// A `relative_path` from the database must stay inside the media root.
fn safe_relative_path(relative: &str) -> Option<&Path> {
    let path = Path::new(relative);
    path.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(path)
}

fn cap_users(mut users: Vec<String>) -> Vec<String> {
    if users.len() > MAX_CURRENT_USER_SYNC_IDS {
        warn!(
            target: "maintafox::sync",
            users = users.len(),
            max = MAX_CURRENT_USER_SYNC_IDS,
            "more users on this device than the sync contract allows; personal items of the others are not requested"
        );
        users.truncate(MAX_CURRENT_USER_SYNC_IDS);
    }
    users
}

async fn has_v2_checkpoint(db: &DatabaseConnection) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
            [],
        ))
        .await?;
    let token: Option<String> = row
        .map(|r| r.try_get::<Option<String>>("", "checkpoint_token"))
        .transpose()
        .map_err(|e| AppError::SyncError(format!("sync_checkpoint.checkpoint_token: {e}")))?
        .flatten();
    // A v1 token (`cp-…`) is refused by v2 exchanges: bootstrap instead of failing once.
    Ok(token.is_some_and(|t| t.starts_with("v2.")))
}

pub struct SyncWorker {
    db: DatabaseConnection,
    client: ControlPlaneClient,
    access: Arc<dyn SyncAccessProvider>,
    handle: SyncWorkerHandle,
    media_root: PathBuf,
    timing: SyncTiming,
}

impl SyncWorker {
    pub fn new(
        db: DatabaseConnection,
        handle: SyncWorkerHandle,
        config: SyncWorkerConfig,
        access: Arc<dyn SyncAccessProvider>,
    ) -> Result<Self, String> {
        let client = ControlPlaneClient::new(&config.api_base_url, config.timing.timeouts)?;
        Ok(Self {
            db,
            client,
            access,
            handle,
            media_root: config.media_root,
            timing: config.timing,
        })
    }

    /// Run until cancelled. `commands` comes from `SyncWorkerHandle::take_receiver`.
    pub async fn run(self, mut commands: mpsc::UnboundedReceiver<SyncCommand>, cancel: CancellationToken) {
        info!(target: "maintafox::sync", "sync worker started (protocol v2)");
        'cycles: loop {
            let cycle_started = Instant::now();
            let (outcome, wait) = tokio::select! {
                () = cancel.cancelled() => break 'cycles,
                result = self.cycle_once() => result,
            };
            let cycle_ended = Instant::now();
            let mut deadline = cycle_ended + wait;
            loop {
                tokio::select! {
                    () = cancel.cancelled() => break 'cycles,
                    () = tokio::time::sleep_until(deadline) => break,
                    command = commands.recv() => match command {
                        None => break 'cycles,
                        Some(SyncCommand::RunNow) => {
                            if let Err(e) = store::reset_failures(&self.db).await {
                                warn!(target: "maintafox::sync", "cannot reset the sync failure count: {e}");
                            }
                            break;
                        }
                        Some(SyncCommand::Reevaluate) => break,
                        Some(SyncCommand::NetworkOnline) => {
                            if matches!(outcome, CycleOutcome::Offline { .. } | CycleOutcome::Retry { .. }) {
                                deadline = deadline.min(cycle_started + self.timing.network_wake_min_spacing);
                            }
                        }
                        Some(SyncCommand::JoinPlant) => {
                            if let Some(at) = join_wake_at(&outcome, cycle_started, cycle_ended, &self.timing) {
                                deadline = deadline.min(at);
                            }
                        }
                    },
                }
            }
        }
        info!(target: "maintafox::sync", "sync worker stopped");
    }

    /// One cycle, then the persisted bookkeeping and the published status. Returns the outcome and
    /// how long to wait before the next cycle.
    pub async fn cycle_once(&self) -> (CycleOutcome, Duration) {
        // An update is between its backup and the restart (updater SF1): the database is left
        // alone, nothing is recorded, and the next cycle comes soon in case the install stops.
        if crate::updater::writes_barred() {
            let wait = Duration::from_secs(5);
            let until = (chrono::Utc::now() + chrono::Duration::seconds(5)).to_rfc3339();
            return (CycleOutcome::Paused { until }, wait);
        }
        let outcome = match self.try_cycle().await {
            Ok(outcome) => outcome,
            Err(e) => {
                error!(target: "maintafox::sync", "sync cycle failed locally: {e}");
                CycleOutcome::Failed { error: local_error(&e) }
            }
        };
        let wait = match self.settle(&outcome).await {
            Ok(wait) => wait,
            Err(e) => {
                error!(target: "maintafox::sync", "cannot record the sync outcome: {e}");
                self.timing.retry_max
            }
        };
        (outcome, wait)
    }

    async fn settle(&self, outcome: &CycleOutcome) -> AppResult<Duration> {
        let error = match outcome {
            CycleOutcome::Synced { .. } => {
                store::record_success(&self.db).await?;
                None
            }
            CycleOutcome::Paused { .. } => None,
            CycleOutcome::Blocked { error, .. } => error.clone(),
            CycleOutcome::Offline { error } | CycleOutcome::Retry { error, .. } | CycleOutcome::Failed { error } => {
                Some(error.clone())
            }
        };
        if let Some(e) = &error {
            store::record_failure(&self.db, &e.code, &e.message).await?;
        }
        let state = store::load_state(&self.db).await?;
        let failures = u32::try_from(state.consecutive_failures.max(1)).unwrap_or(u32::MAX);
        let backoff = || retry_delay(failures, &self.timing, rand::thread_rng().gen::<f64>());
        let (worker_state, blocker, wait) = match outcome {
            CycleOutcome::Synced { more_work: true } => (SyncWorkerState::Running, None, Duration::ZERO),
            CycleOutcome::Synced { more_work: false } => (SyncWorkerState::Idle, None, self.timing.idle_interval),
            CycleOutcome::Paused { until } => {
                let remaining = chrono::DateTime::parse_from_rfc3339(until)
                    .ok()
                    .and_then(|u| (u.with_timezone(&chrono::Utc) - chrono::Utc::now()).to_std().ok())
                    .unwrap_or(self.timing.blocked_recheck);
                (SyncWorkerState::Paused, None, remaining.max(Duration::from_secs(1)))
            }
            CycleOutcome::Blocked { reason, .. } => (
                SyncWorkerState::Blocked,
                Some(reason.clone()),
                self.timing.blocked_recheck,
            ),
            CycleOutcome::Offline { .. } => (SyncWorkerState::Offline, None, backoff()),
            CycleOutcome::Retry { retry_after, .. } => {
                let wait = backoff().max(retry_after.unwrap_or(Duration::ZERO));
                (SyncWorkerState::Retrying, None, wait)
            }
            CycleOutcome::Failed { .. } => (SyncWorkerState::Error, None, backoff()),
        };
        let summary = queries::get_sync_state_summary(&self.db).await?;
        let (media_pending, media_failed) = store::media_counts(&self.db).await?;
        let (download_pending, download_failed) = store::media_download_counts(&self.db).await?;
        let next_attempt_at = chrono::Duration::from_std(wait)
            .ok()
            .map(|d| (chrono::Utc::now() + d).format("%Y-%m-%dT%H:%M:%SZ").to_string());
        let last_error = match (&state.last_error_code, &state.last_error_message, &state.last_error_at) {
            (Some(code), Some(message), Some(at)) => Some(SyncWorkerError {
                code: code.clone(),
                message: message.clone(),
                http_status: error.as_ref().and_then(|e| e.http_status),
                at: at.clone(),
            }),
            _ => None,
        };
        self.handle.publish(SyncWorkerStatus {
            state: worker_state,
            blocker,
            paused_until: match outcome {
                CycleOutcome::Paused { until } => Some(until.clone()),
                _ => None,
            },
            last_success_at: state.last_success_at,
            last_attempt_at: state.last_attempt_at,
            next_attempt_at,
            pending_outbox_count: summary.pending_outbox_count + summary.in_flight_outbox_count,
            dead_letter_outbox_count: summary.dead_letter_outbox_count,
            unresolved_conflict_count: summary.unresolved_conflict_count,
            media_pending_count: media_pending,
            media_failed_count: media_failed,
            media_download_pending_count: download_pending,
            media_download_failed_count: download_failed,
            consecutive_failures: state.consecutive_failures,
            last_error,
        });
        Ok(wait)
    }

    async fn try_cycle(&self) -> AppResult<CycleOutcome> {
        let state = store::load_state(&self.db).await?;
        if let Some(until) = state.paused_until.as_deref() {
            let active = chrono::DateTime::parse_from_rfc3339(until).is_ok_and(|u| u > chrono::Utc::now());
            if active {
                return Ok(CycleOutcome::Paused {
                    until: until.to_string(),
                });
            }
            store::set_paused_until(&self.db, None).await?;
        }
        // The organisation this pull runs for, read before the activation token is: a complete pull
        // records "the server confirmed no user accounts" for it only, and only if the activation
        // is still the same at the end (`after_complete_pull`), so a swap of activation during the
        // cycle can never record the fact for the wrong organisation (`plant_join`).
        let pulled_for = crate::commands::product_license::get_activation_claim_tenant_id(&self.db).await?;
        let token = match self.access.access(&self.db).await? {
            SyncAccess::NotLicensed => {
                return Ok(CycleOutcome::Blocked {
                    reason: BLOCKER_NOT_LICENSED.to_string(),
                    error: None,
                })
            }
            SyncAccess::NotActivated => {
                return Ok(CycleOutcome::Blocked {
                    reason: BLOCKER_NOT_ACTIVATED.to_string(),
                    error: None,
                })
            }
            SyncAccess::Granted { token } => token,
        };
        let token_hash = hash_payload(&token);
        if let Some(refused) = &state.refused_token_sha256 {
            if *refused == token_hash {
                return Ok(CycleOutcome::Blocked {
                    reason: state
                        .refused_reason
                        .clone()
                        .unwrap_or_else(|| BLOCKER_ACTIVATION_REFUSED.to_string()),
                    error: None,
                });
            }
            info!(target: "maintafox::sync", "activation token changed since the server refused it; syncing again");
            store::clear_refused(&self.db).await?;
        }

        self.handle.publish_with(|s| {
            s.state = SyncWorkerState::Running;
            s.blocker = None;
            s.paused_until = None;
            s.next_attempt_at = None;
        });
        store::record_attempt(&self.db).await?;

        let mut snapshot_done = false;
        if !has_v2_checkpoint(&self.db).await? {
            if let Some(outcome) = self.bootstrap(&token, &token_hash).await? {
                return Ok(outcome);
            }
            snapshot_done = true;
        }

        let mut more_work = true;
        for _ in 0..SYNC_MAX_EXCHANGES_PER_CYCLE {
            match self.exchange_once(&token, &token_hash, &mut snapshot_done).await? {
                Step::Continue => {}
                Step::Done => {
                    more_work = false;
                    break;
                }
                Step::Stop(outcome) => return Ok(outcome),
            }
        }
        if !more_work {
            self.after_complete_pull(pulled_for.as_deref()).await?;
        }
        let more_downloads = self.download_pending_media(&token).await?;
        Ok(CycleOutcome::Synced {
            more_work: more_work || more_downloads,
        })
    }

    /// The exchanges of this cycle reached the server head (`Step::Done`: `has_more` false and the
    /// outbox drained, every received row materialized): this PC now holds everything the server
    /// has for its organisation, which is the only time "the server has no user accounts" can be
    /// told apart from "not received yet" (`plant_join::record_complete_pull`). Nothing is recorded
    /// when the activation changed during the cycle: the pull ran for another organisation.
    async fn after_complete_pull(&self, pulled_for: Option<&str>) -> AppResult<()> {
        let Some(tenant_id) = pulled_for else {
            return Ok(());
        };
        let current = crate::commands::product_license::get_activation_claim_tenant_id(&self.db).await?;
        if current.as_deref() != Some(tenant_id) {
            return Ok(());
        }
        crate::sync::plant_join::record_complete_pull(&self.db, tenant_id).await?;
        Ok(())
    }

    /// Scope `all` snapshot; stores the delta checkpoint on its last page. `Some(outcome)` stops the
    /// cycle.
    async fn bootstrap(&self, token: &str, token_hash: &str) -> AppResult<Option<CycleOutcome>> {
        info!(target: "maintafox::sync", "no v2 checkpoint: bootstrapping with a snapshot");
        let users = cap_users(queries::current_user_sync_ids(&self.db).await?);
        match self.run_snapshot(token, SNAPSHOT_SCOPE_ALL, &users).await? {
            Ok(()) => Ok(None),
            Err(e) => self.request_failure(&e, token_hash).await.map(Some),
        }
    }

    /// Outcome for a failed request that carried no outbox items (snapshot).
    async fn request_failure(&self, err: &TransportError, token_hash: &str) -> AppResult<CycleOutcome> {
        let error = worker_error(err);
        Ok(match classify(err) {
            FailureKind::Offline => CycleOutcome::Offline { error },
            FailureKind::RateLimited | FailureKind::Retryable => CycleOutcome::Retry {
                retry_after: retry_after_of(err),
                error,
            },
            FailureKind::ActivationRefused => self.refuse(token_hash, BLOCKER_ACTIVATION_REFUSED, error).await?,
            FailureKind::TenantMismatch => self.refuse(token_hash, BLOCKER_TENANT_MISMATCH, error).await?,
            FailureKind::CheckpointRefused | FailureKind::IdempotencyKeyReused | FailureKind::Permanent => {
                CycleOutcome::Failed { error }
            }
        })
    }

    async fn refuse(&self, token_hash: &str, reason: &str, error: SyncWorkerError) -> AppResult<CycleOutcome> {
        warn!(
            target: "maintafox::sync",
            reason,
            code = error.code.as_str(),
            "control API refused this device; sync stops until the activation changes"
        );
        store::set_refused(&self.db, token_hash, reason).await?;
        Ok(CycleOutcome::Blocked {
            reason: reason.to_string(),
            error: Some(error),
        })
    }

    async fn run_snapshot(
        &self,
        token: &str,
        scope: &'static str,
        users: &[String],
    ) -> AppResult<Result<(), TransportError>> {
        let mut restarts = 0;
        // Only a scope `all` snapshot can be long; it continues where an interrupted one stopped
        // (429 from the per-device request limit, restart, crash): its pages are in the inbox.
        let resumable = scope == SNAPSHOT_SCOPE_ALL;
        let mut resume = if resumable {
            store::load_snapshot_progress(&self.db).await?
        } else {
            None
        };
        'snapshot: loop {
            let (snapshot_id, mut page_token) = match resume.take() {
                Some((id, token)) => {
                    info!(target: "maintafox::sync", scope, "continuing an interrupted snapshot");
                    (id, Some(token))
                }
                None => (format!("snapshot-{scope}-{}", uuid::Uuid::new_v4()), None),
            };
            let mut pages = 0_u32;
            loop {
                let request = SnapshotRequest {
                    protocol_version: SYNC_PROTOCOL_VERSION_V2,
                    page_token: page_token.as_deref(),
                    page_size: SYNC_SNAPSHOT_PAGE_SIZE,
                    scope,
                    current_user_sync_ids: users,
                };
                let page = match self.client.snapshot(token, &request).await {
                    Ok(page) => page,
                    Err(e) if e.code() == "invalid_page_token" && restarts < SYNC_MAX_SNAPSHOT_RESTARTS => {
                        restarts += 1;
                        if resumable {
                            store::clear_snapshot_progress(&self.db).await?;
                        }
                        warn!(target: "maintafox::sync", scope, restarts, "snapshot page token refused; restarting the snapshot");
                        continue 'snapshot;
                    }
                    Err(e) => return Ok(Err(e)),
                };
                pages += 1;
                if pages % 50 == 0 {
                    info!(target: "maintafox::sync", scope, pages, "snapshot pages received");
                }
                let last = !page.has_more;
                let checkpoint = if scope == SNAPSHOT_SCOPE_ALL && last {
                    match page.checkpoint_token.as_deref().filter(|t| !t.trim().is_empty()) {
                        Some(token) => Some(token),
                        None => {
                            return Ok(Err(TransportError::Contract(
                                "the last snapshot page has no checkpoint_token".to_string(),
                            )))
                        }
                    }
                } else {
                    None
                };
                queries::apply_snapshot_page(&self.db, &snapshot_id, &page.items, checkpoint).await?;
                if last {
                    // Applied once every page is stored: all rows parents first, each tried once
                    // (pages come in entity type order, so applying page by page made most
                    // children wait for a later page and be tried again).
                    self.materialize(true).await?;
                    if resumable {
                        store::clear_snapshot_progress(&self.db).await?;
                    }
                    info!(target: "maintafox::sync", scope, pages, "snapshot complete");
                    return Ok(Ok(()));
                }
                page_token = match page.next_page_token.filter(|t| !t.trim().is_empty()) {
                    Some(next) => {
                        if resumable {
                            store::save_snapshot_progress(&self.db, &snapshot_id, &next).await?;
                        }
                        Some(next)
                    }
                    None => {
                        return Ok(Err(TransportError::Contract(
                            "a snapshot page has has_more without next_page_token".to_string(),
                        )))
                    }
                };
            }
        }
    }

    /// Apply every received row: pending rows parents first, then (`retry_deferred`) the rows that
    /// waited for a parent, until nothing more can be applied.
    async fn materialize(&self, retry_deferred: bool) -> AppResult<()> {
        let result = materialize_inbox(&self.db, SYNC_MATERIALIZE_BATCH, retry_deferred).await?;
        if result.processed > 0 {
            info!(
                target: "maintafox::sync",
                processed = result.processed,
                applied = result.applied,
                conflicts = result.conflicts,
                deferred = result.deferred,
                "inbox materialized"
            );
        }
        Ok(())
    }

    /// Build the next exchange request and store it (with the media manifest) before it is sent.
    async fn build_exchange(&self) -> AppResult<PendingExchange> {
        let payload = queries::get_sync_push_payload(&self.db, Some(SYNC_OUTBOX_BATCH_SIZE)).await?;
        let tenant_id = payload.tenant_config.as_ref().map(|t| t.tenant_id.clone());
        let mut manifest: Vec<SyncMediaManifestItem> = Vec::new();
        for item in payload.media_manifest.clone().unwrap_or_default() {
            // The server refuses the whole request for a key outside the tenant
            // (`media_manifest_invalid`), e.g. media staged before activation.
            match &tenant_id {
                Some(tenant) if object_key_in_tenant(&item.object_key, tenant) => manifest.push(item),
                _ => warn!(
                    target: "maintafox::sync",
                    entity_sync_id = item.entity_sync_id.as_str(),
                    "media object key is outside the activated tenant; left out of the manifest"
                ),
            }
        }
        let users = cap_users(payload.current_user_sync_ids.clone());
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let request = ExchangeRequest {
            protocol_version: SYNC_PROTOCOL_VERSION_V2,
            idempotency_key: &idempotency_key,
            checkpoint_token: payload.checkpoint_token.as_deref(),
            current_user_sync_ids: &users,
            device_class: DEVICE_CLASS_DESKTOP,
            tenant_config: payload.tenant_config.as_ref(),
            media_manifest: &manifest,
            outbox_batch: payload.outbox_batch.iter().map(ExchangeOutboxItem::from).collect(),
        };
        let pending = PendingExchange {
            body: serde_json::to_string(&request)?,
            idempotency_key,
            outbox_ids: payload.outbox_batch.iter().map(|i| i.id).collect(),
        };
        let tx = self.db.begin().await?;
        store::save_pending(&tx, &pending).await?;
        store::record_media_manifest(&tx, &manifest).await?;
        tx.commit().await?;
        Ok(pending)
    }

    async fn exchange_once(&self, token: &str, token_hash: &str, snapshot_done: &mut bool) -> AppResult<Step> {
        let pending = match store::load_state(&self.db).await?.pending {
            Some(pending) => {
                queries::mark_outbox_items_resent(&self.db, &pending.outbox_ids).await?;
                pending
            }
            None => self.build_exchange().await?,
        };
        self.upload_pending_media(token).await?;

        let response = match self.client.exchange(token, &pending.body).await {
            Ok(response) => response,
            Err(e) => {
                return self
                    .exchange_failure(&e, &pending, token, token_hash, snapshot_done)
                    .await
            }
        };

        let has_more = response.has_more;
        let users_to_snapshot = response.user_snapshot_required.clone();
        let applied = queries::apply_sync_batch(&self.db, response.into_apply_input()).await?;
        store::clear_pending(&self.db).await?;
        info!(
            target: "maintafox::sync",
            acknowledged = applied.acknowledged_count,
            rejected = applied.rejected_count,
            conflicts = applied.conflict_count,
            inbound = applied.inbound_applied_count,
            has_more,
            "sync exchange applied"
        );
        self.materialize(true).await?;

        if !users_to_snapshot.is_empty() {
            // Contract §4.1: the snapshot must list the same users as the exchange.
            let sent: StoredExchangeRequest = serde_json::from_str(&pending.body)?;
            info!(target: "maintafox::sync", users = users_to_snapshot.len(), "user snapshot required");
            if let Err(e) = self
                .run_snapshot(token, SNAPSHOT_SCOPE_USER, &sent.current_user_sync_ids)
                .await?
            {
                return Ok(Step::Stop(self.request_failure(&e, token_hash).await?));
            }
        }

        let full_batch = i64::try_from(pending.outbox_ids.len()).unwrap_or(i64::MAX) >= SYNC_OUTBOX_BATCH_SIZE;
        Ok(if has_more || full_batch {
            Step::Continue
        } else {
            Step::Done
        })
    }

    async fn exchange_failure(
        &self,
        err: &TransportError,
        pending: &PendingExchange,
        token: &str,
        token_hash: &str,
        snapshot_done: &mut bool,
    ) -> AppResult<Step> {
        let error = worker_error(err);
        let ids = &pending.outbox_ids;
        let outcome = match classify(err) {
            FailureKind::Offline => {
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                CycleOutcome::Offline { error }
            }
            FailureKind::RateLimited => {
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                CycleOutcome::Retry {
                    retry_after: retry_after_of(err),
                    error,
                }
            }
            FailureKind::Retryable => {
                // Same key and body next time; the attempt counts toward the dead-letter limit.
                let dead = queries::record_outbox_delivery_failure(&self.db, ids, &error.message).await?;
                if dead > 0 {
                    warn!(target: "maintafox::sync", dead, "outbox items moved to dead letter; rebuilding the request without them");
                    store::clear_pending(&self.db).await?;
                }
                CycleOutcome::Retry {
                    retry_after: retry_after_of(err),
                    error,
                }
            }
            FailureKind::ActivationRefused => {
                // The request stays stored: the same device resends it once it may sync again.
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                self.refuse(token_hash, BLOCKER_ACTIVATION_REFUSED, error).await?
            }
            FailureKind::TenantMismatch => {
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                store::clear_pending(&self.db).await?;
                self.refuse(token_hash, BLOCKER_TENANT_MISMATCH, error).await?
            }
            FailureKind::CheckpointRefused => {
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                store::clear_pending(&self.db).await?;
                queries::clear_sync_checkpoint(&self.db).await?;
                // The server's state no longer matches this device: a snapshot starts from page 1.
                store::clear_snapshot_progress(&self.db).await?;
                if *snapshot_done {
                    // Refused right after a fresh snapshot: something is wrong on the server side.
                    CycleOutcome::Failed { error }
                } else {
                    warn!(target: "maintafox::sync", code = error.code.as_str(), "checkpoint refused; bootstrapping with a snapshot");
                    *snapshot_done = true;
                    return Ok(match self.bootstrap(token, token_hash).await? {
                        Some(outcome) => Step::Stop(outcome),
                        None => Step::Continue,
                    });
                }
            }
            FailureKind::IdempotencyKeyReused => {
                error!(
                    target: "maintafox::sync",
                    idempotency_key = pending.idempotency_key.as_str(),
                    "server reports the idempotency key reused with another body; the stored request is dropped and rebuilt with a new key"
                );
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                store::clear_pending(&self.db).await?;
                CycleOutcome::Failed { error }
            }
            FailureKind::Permanent => {
                error!(
                    target: "maintafox::sync",
                    code = error.code.as_str(),
                    message = error.message.as_str(),
                    "control API refused the sync request as invalid"
                );
                let dead = queries::record_outbox_delivery_failure(&self.db, ids, &error.message).await?;
                if dead > 0 {
                    warn!(target: "maintafox::sync", dead, "outbox items moved to dead letter after repeated refusals");
                }
                store::clear_pending(&self.db).await?;
                CycleOutcome::Failed { error }
            }
        };
        Ok(Step::Stop(outcome))
    }

    async fn upload_pending_media(&self, token: &str) -> AppResult<()> {
        let uploads = store::list_pending_media(&self.db, SYNC_MEDIA_UPLOADS_PER_CYCLE).await?;
        for upload in uploads {
            match self.upload_one(token, &upload).await {
                Ok(()) => store::mark_media_uploaded(&self.db, &upload.object_key).await?,
                // Nothing more can be uploaded now; not counted as an attempt.
                Err(MediaFailure::Offline) => break,
                Err(MediaFailure::Retry(message) | MediaFailure::NotYetAvailable(message)) => {
                    let give_up = upload.attempt_count + 1 >= OUTBOX_MAX_DELIVERY_ATTEMPTS;
                    warn!(target: "maintafox::sync", object_key = upload.object_key.as_str(), give_up, "media upload failed: {message}");
                    store::mark_media_failed(&self.db, &upload.object_key, &message, give_up).await?;
                }
                Err(MediaFailure::Permanent(message)) => {
                    warn!(target: "maintafox::sync", object_key = upload.object_key.as_str(), "media upload impossible: {message}");
                    store::mark_media_failed(&self.db, &upload.object_key, &message, true).await?;
                }
            }
        }
        Ok(())
    }

    async fn upload_one(&self, token: &str, upload: &PendingMediaUpload) -> Result<(), MediaFailure> {
        let (Some(parent_type), Some(parent_sync_id), Some(relative_path)) = (
            upload.parent_entity_type.as_deref(),
            upload.parent_entity_sync_id.as_deref(),
            upload.relative_path.as_deref(),
        ) else {
            return Err(MediaFailure::Permanent(
                "the local media_attachments row or its file path is missing".to_string(),
            ));
        };
        let Some(relative) = safe_relative_path(relative_path) else {
            return Err(MediaFailure::Permanent(format!(
                "media path '{relative_path}' is not inside the media directory"
            )));
        };
        let bytes = match tokio::fs::read(self.media_root.join(relative)).await {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(MediaFailure::Permanent(format!(
                    "local media file '{relative_path}' is missing"
                )))
            }
            Err(e) => return Err(MediaFailure::Retry(format!("cannot read '{relative_path}': {e}"))),
        };
        crate::sync::media::verify_content_hash(&bytes, &upload.content_sha256)
            .map_err(|e| MediaFailure::Permanent(e.to_string()))?;

        let media_failure = |e: TransportError| match classify(&e) {
            FailureKind::Offline => MediaFailure::Offline,
            FailureKind::Permanent => MediaFailure::Permanent(e.message()),
            _ => MediaFailure::Retry(e.message()),
        };
        let presigned = self
            .client
            .presign_put(
                token,
                &PresignPutRequest {
                    parent_entity_type: parent_type,
                    parent_entity_sync_id: parent_sync_id,
                    content_sha256: &upload.content_sha256,
                    object_key: &upload.object_key,
                },
            )
            .await
            .map_err(media_failure)?;
        if !presigned.method.eq_ignore_ascii_case("PUT") || presigned.object_key != upload.object_key {
            return Err(MediaFailure::Permanent(format!(
                "presign answered {} {} for {}",
                presigned.method, presigned.object_key, upload.object_key
            )));
        }
        self.client
            .put_object(&presigned.url, bytes)
            .await
            .map_err(media_failure)
    }

    /// Download due files received from other devices. Returns whether more downloads are due
    /// than one cycle takes.
    async fn download_pending_media(&self, token: &str) -> AppResult<bool> {
        let due = store::list_due_media_downloads(&self.db, SYNC_MEDIA_DOWNLOADS_PER_CYCLE).await?;
        let full = u32::try_from(due.len()).unwrap_or(u32::MAX) >= SYNC_MEDIA_DOWNLOADS_PER_CYCLE;
        for download in due {
            let attempts = u32::try_from(download.attempt_count + 1).unwrap_or(u32::MAX);
            let next_attempt = || {
                let delay = media_retry_delay(attempts, &self.timing, rand::thread_rng().gen::<f64>());
                chrono::Duration::from_std(delay)
                    .ok()
                    .map(|d| (chrono::Utc::now() + d).format("%Y-%m-%dT%H:%M:%SZ").to_string())
            };
            let id = download.entity_sync_id.as_str();
            match self.download_one(token, &download).await {
                Ok(()) => store::mark_media_downloaded(&self.db, id).await?,
                Err(MediaFailure::Offline) => return Ok(false),
                Err(MediaFailure::NotYetAvailable(message)) => {
                    store::mark_media_download_failed(&self.db, id, &message, false, next_attempt().as_deref()).await?;
                }
                Err(MediaFailure::Retry(message)) => {
                    let give_up = download.attempt_count + 1 >= OUTBOX_MAX_DELIVERY_ATTEMPTS;
                    warn!(target: "maintafox::sync", object_key = download.object_key.as_str(), give_up, "media download failed: {message}");
                    let next = if give_up { None } else { next_attempt() };
                    store::mark_media_download_failed(&self.db, id, &message, give_up, next.as_deref()).await?;
                }
                Err(MediaFailure::Permanent(message)) => {
                    warn!(target: "maintafox::sync", object_key = download.object_key.as_str(), "media download impossible: {message}");
                    store::mark_media_download_failed(&self.db, id, &message, true, None).await?;
                }
            }
        }
        Ok(full)
    }

    async fn download_one(&self, token: &str, download: &PendingMediaDownload) -> Result<(), MediaFailure> {
        let Some(relative_path) = download.relative_path.as_deref() else {
            return Err(MediaFailure::Permanent(
                "the storage location of this file was not received (sent by an older app version)".to_string(),
            ));
        };
        let Some(relative) = crate::sync::media::media_storage_path(relative_path) else {
            return Err(MediaFailure::Permanent(format!(
                "media path '{relative_path}' is not inside a media directory"
            )));
        };
        let expected_size = u64::try_from(download.byte_size)
            .map_err(|_| MediaFailure::Permanent(format!("invalid byte size {}", download.byte_size)))?;
        let target = self.media_root.join(relative);

        // Already here (stored before, or uploaded from this device): never overwrite a file.
        match tokio::fs::read(&target).await {
            Ok(bytes) => {
                return crate::sync::media::verify_content_hash(&bytes, &download.content_sha256).map_err(|_| {
                    MediaFailure::Permanent(format!(
                        "another file with different content already exists at '{relative_path}'"
                    ))
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(MediaFailure::Retry(format!("cannot read '{relative_path}': {e}"))),
        }

        let presigned = self
            .client
            .presign_get(
                token,
                &PresignGetRequest {
                    object_key: &download.object_key,
                },
            )
            .await
            .map_err(|e| match (e.code(), classify(&e)) {
                ("object_key_tenant_mismatch", _) => MediaFailure::Permanent(e.message()),
                (_, FailureKind::Offline | FailureKind::ActivationRefused | FailureKind::TenantMismatch) => {
                    MediaFailure::Offline
                }
                (_, FailureKind::Permanent) => MediaFailure::Permanent(e.message()),
                _ => MediaFailure::Retry(e.message()),
            })?;
        if !presigned.method.eq_ignore_ascii_case("GET") || presigned.object_key != download.object_key {
            return Err(MediaFailure::Permanent(format!(
                "presign answered {} {} for {}",
                presigned.method, presigned.object_key, download.object_key
            )));
        }
        let mut response = self.client.get_object(&presigned.url).await.map_err(|e| {
            if e.code() == OBJECT_NOT_FOUND {
                MediaFailure::NotYetAvailable(
                    "the file is not on the server yet (the device that added it has not sent it)".to_string(),
                )
            } else if matches!(classify(&e), FailureKind::Offline) {
                MediaFailure::Offline
            } else {
                MediaFailure::Retry(e.message())
            }
        })?;
        if let Some(announced) = response.content_length().filter(|len| *len != expected_size) {
            return Err(MediaFailure::Retry(format!(
                "object storage announced {announced} bytes, expected {expected_size}"
            )));
        }

        let parent = target
            .parent()
            .ok_or_else(|| MediaFailure::Permanent(format!("media path '{relative_path}' has no directory")))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| MediaFailure::Retry(format!("cannot create the folder of '{relative_path}': {e}")))?;
        let part = parent.join(format!(".download-{}.part", uuid::Uuid::new_v4()));
        let write_error = |e: std::io::Error| MediaFailure::Retry(format!("cannot write '{relative_path}': {e}"));
        let result = async {
            use sha2::Digest;
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::File::create(&part).await.map_err(write_error)?;
            let mut hasher = sha2::Sha256::new();
            let mut received: u64 = 0;
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| MediaFailure::Retry(format!("download interrupted: {e}")))?
            {
                received += chunk.len() as u64;
                if received > expected_size {
                    return Err(MediaFailure::Retry(format!(
                        "object storage sent more than the expected {expected_size} bytes"
                    )));
                }
                hasher.update(&chunk);
                file.write_all(&chunk).await.map_err(write_error)?;
            }
            file.flush().await.map_err(write_error)?;
            file.sync_all().await.map_err(write_error)?;
            drop(file);
            if received != expected_size {
                return Err(MediaFailure::Retry(format!(
                    "received {received} bytes, expected {expected_size}"
                )));
            }
            let actual = format!("{:x}", hasher.finalize());
            if actual != download.content_sha256.trim().to_ascii_lowercase() {
                return Err(MediaFailure::Retry(format!(
                    "content hash mismatch: expected {}, got {actual}",
                    download.content_sha256
                )));
            }
            tokio::fs::rename(&part, &target)
                .await
                .map_err(|e| MediaFailure::Retry(format!("cannot move the file into '{relative_path}': {e}")))
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&part).await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing() -> SyncTiming {
        SyncTiming::production()
    }

    #[test]
    fn retry_delay_doubles_caps_and_jitters() {
        let t = timing();
        // jitter 0.5 = no jitter
        assert_eq!(retry_delay(1, &t, 0.5), SYNC_RETRY_BASE_DELAY);
        assert_eq!(retry_delay(2, &t, 0.5), SYNC_RETRY_BASE_DELAY * 2);
        assert_eq!(retry_delay(4, &t, 0.5), SYNC_RETRY_BASE_DELAY * 8);
        assert_eq!(retry_delay(30, &t, 0.5), SYNC_RETRY_MAX_DELAY);
        let low = retry_delay(30, &t, 0.0);
        let high = retry_delay(30, &t, 1.0);
        assert_eq!(low, SYNC_RETRY_MAX_DELAY.mul_f64(1.0 - SYNC_RETRY_JITTER_RATIO));
        assert_eq!(high, SYNC_RETRY_MAX_DELAY.mul_f64(1.0 + SYNC_RETRY_JITTER_RATIO));
        // Never below the base delay.
        assert!(retry_delay(1, &t, 0.0) >= SYNC_RETRY_BASE_DELAY);
    }

    #[test]
    fn classification_follows_the_contract() {
        let http = |status: u16, code: Option<&str>| TransportError::Http {
            status,
            code: code.map(str::to_string),
            message: String::new(),
            retry_after: None,
        };
        assert_eq!(classify(&TransportError::Offline(String::new())), FailureKind::Offline);
        assert_eq!(
            classify(&TransportError::Interrupted(String::new())),
            FailureKind::Retryable
        );
        assert_eq!(
            classify(&http(500, Some("sync_exchange_failed"))),
            FailureKind::Retryable
        );
        assert_eq!(classify(&http(503, None)), FailureKind::Retryable);
        assert_eq!(classify(&http(429, Some("rate_limited"))), FailureKind::RateLimited);
        assert_eq!(
            classify(&http(401, Some("token_superseded"))),
            FailureKind::ActivationRefused
        );
        assert_eq!(
            classify(&http(403, Some("license_revoked"))),
            FailureKind::ActivationRefused
        );
        assert_eq!(
            classify(&http(400, Some("invalid_checkpoint_token"))),
            FailureKind::CheckpointRefused
        );
        assert_eq!(
            classify(&http(409, Some("checkpoint_ahead_of_server"))),
            FailureKind::CheckpointRefused
        );
        assert_eq!(
            classify(&http(409, Some("idempotency_key_reused"))),
            FailureKind::IdempotencyKeyReused
        );
        assert_eq!(
            classify(&http(409, Some("tenant_config_mismatch"))),
            FailureKind::TenantMismatch
        );
        assert_eq!(classify(&http(400, Some("validation_error"))), FailureKind::Permanent);
    }

    #[test]
    fn object_keys_and_paths() {
        assert!(object_key_in_tenant("prod/media/t-1/equipment/e1/abc", "t-1"));
        assert!(!object_key_in_tenant("prod/media/local-tenant/equipment/e1/abc", "t-1"));
        assert!(!object_key_in_tenant("prod/backup/t-1/x", "t-1"));
        assert!(safe_relative_path("di_attachments/1/a.png").is_some());
        assert!(safe_relative_path("../secrets.txt").is_none());
        assert!(safe_relative_path("/etc/passwd").is_none());
    }
}
