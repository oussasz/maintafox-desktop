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
use super::removal::{self, PhoneRemovalStatus, Probe, RemovalKind, RemovalOutcome, RemovalSteps, UploadRound};
use super::store::{self, PendingExchange, PendingMediaDownload, PendingMediaUpload};
use super::wire::{
    ExchangeOutboxItem, ExchangeRequest, PresignGetRequest, PresignPutRequest, SnapshotRequest, StoredExchangeRequest,
    DEVICE_CLASS, MAX_CURRENT_USER_SYNC_IDS, SNAPSHOT_SCOPE_ALL, SNAPSHOT_SCOPE_USER,
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
/// The phone erased itself (`removal`): it must be enrolled again.
pub const BLOCKER_PHONE_REMOVED: &str = "phone_removed";
/// The licence server refused the phone without its erase marker (`removal`,
/// `RevokedUnconfirmed`): locked, data kept, asked again at each re-check.
pub const BLOCKER_PHONE_LOCKED: &str = "phone_locked";

/// How often a phone asks the server whether it was removed, when nothing else it sent this
/// cycle would have told it: its push-only exchanges keep working once it is removed.
pub const REMOVAL_PROBE_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// A read-only route every activated device may call: answered `device_wipe_required` or
/// `activation_revoked` to a removed phone.
const REMOVAL_PROBE_PATH: &str = "/api/v1/activation/tenant-status";

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
    /// Why the attempt failed, as a stable reason the screens translate (`error_kind`).
    pub kind: &'static str,
    /// Raw text ("HTTP 503: …", the network library's): log and technical details only.
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
    /// The phone's removal (`removal`), for the screens.
    removal: watch::Sender<PhoneRemovalStatus>,
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
        let (removal, _) = watch::channel(PhoneRemovalStatus::none());
        Self {
            inner: Arc::new(HandleInner {
                commands,
                receiver: Mutex::new(Some(receiver)),
                status,
                removal,
            }),
        }
    }

    /// Where the phone's removal stands (`PhoneRemovalStatus::none()` when there is none).
    pub fn removal_status(&self) -> PhoneRemovalStatus {
        self.inner.removal.borrow().clone()
    }

    pub fn subscribe_removal(&self) -> watch::Receiver<PhoneRemovalStatus> {
        self.inner.removal.subscribe()
    }

    fn publish_removal(&self, status: PhoneRemovalStatus) {
        self.inner.removal.send_replace(status);
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
                kind: error_kind(BLOCKER_NOT_CONFIGURED, None),
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
            "GATE_SYNC_PAUSE_RANGE:max={SYNC_MAX_PAUSE_MINUTES}: Pause duration must be between 1 and {SYNC_MAX_PAUSE_MINUTES} minutes."
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

/// Stable reason of a failed attempt (`SyncWorkerError::kind`) from its raw code and HTTP status.
/// The server's codes are open-ended, so an HTTP failure is read by its status first; an error
/// read back from the worker state has no status, and the server codes known to the worker
/// ([`classify`]) give its reason then.
pub fn error_kind(code: &str, http_status: Option<u16>) -> &'static str {
    match (http_status, code) {
        (Some(401 | 403), _) => "access_refused",
        (Some(408 | 429 | 500..=599), _) => "server_unavailable",
        (Some(_), _) => "request_refused",
        (None, "rate_limited" | "sync_exchange_failed") => "server_unavailable",
        (None, "invalid_token" | "activation_revoked") => "access_refused",
        (
            None,
            "tenant_config_mismatch"
            | "invalid_checkpoint_token"
            | "checkpoint_ahead_of_server"
            | "idempotency_key_reused"
            | "validation_error",
        ) => "request_refused",
        (None, "offline") => "network_unreachable",
        (None, "interrupted") => "connection_interrupted",
        (None, "invalid_server_response") => "invalid_server_response",
        (None, "local_error") => "local_error",
        (None, BLOCKER_NOT_CONFIGURED) => "not_configured",
        _ => "other",
    }
}

fn worker_error(err: &TransportError) -> SyncWorkerError {
    SyncWorkerError {
        code: err.code().to_string(),
        kind: error_kind(err.code(), err.http_status()),
        message: err.message(),
        http_status: err.http_status(),
        at: store::now_utc(),
    }
}

fn local_error(err: &AppError) -> SyncWorkerError {
    SyncWorkerError {
        code: "local_error".to_string(),
        kind: error_kind("local_error", None),
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

/// Run after a materialization that wrote a row the effective rights are read from
/// (`MaterializeInboxResult::rights_changed`, AU-107): the app drops its cached permissions and
/// tells the window, so a right withdrawn on another PC stops here at once.
pub type RightsChangedHook = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub struct SyncWorker {
    db: DatabaseConnection,
    client: ControlPlaneClient,
    access: Arc<dyn SyncAccessProvider>,
    handle: SyncWorkerHandle,
    media_root: PathBuf,
    timing: SyncTiming,
    on_rights_changed: Option<RightsChangedHook>,
    /// The app's session, ended when the phone starts erasing itself (`removal`).
    session: Option<Arc<tokio::sync::RwLock<crate::auth::session_manager::SessionManager>>>,
    /// A removal a server answer of this cycle revealed, recorded when the cycle ends.
    removal_seen: Mutex<Option<RemovalKind>>,
    /// When the phone last asked whether it was removed (`REMOVAL_PROBE_INTERVAL`).
    removal_probed_at: Mutex<Option<Instant>>,
    /// Whether this worker runs on a phone (`removal::applies_on_this_device`): it then removes
    /// itself when its server says so. Tests set it on any host (`as_phone`).
    phone: bool,
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
            on_rights_changed: None,
            session: None,
            removal_seen: Mutex::new(None),
            removal_probed_at: Mutex::new(None),
            phone: removal::applies_on_this_device(),
        })
    }

    /// The worker of a phone on any host (tests of the removal against the HTTP stub).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn as_phone(mut self) -> Self {
        self.phone = true;
        self
    }

    /// The session a phone's removal ends (`removal`): the app's own.
    #[must_use]
    pub fn with_session(
        mut self,
        session: Arc<tokio::sync::RwLock<crate::auth::session_manager::SessionManager>>,
    ) -> Self {
        self.session = Some(session);
        self
    }

    /// Called once per materialization that changed the effective rights ([`RightsChangedHook`]).
    #[must_use]
    pub fn on_rights_changed(mut self, hook: RightsChangedHook) -> Self {
        self.on_rights_changed = Some(hook);
        self
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
                // `biased`: commands already queued are all read before a due deadline starts the
                // cycle, so requests sent together (three join requests from the first-start
                // screen) make one cycle, never one cycle each by the luck of `select!`'s order.
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => break 'cycles,
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
                    () = tokio::time::sleep_until(deadline) => break,
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
        let removal_recorded = self.record_seen_removal().await;
        let wait = match self.settle(&outcome).await {
            Ok(wait) => wait,
            Err(e) => {
                error!(target: "maintafox::sync", "cannot record the sync outcome: {e}");
                self.timing.retry_max
            }
        };
        // A removal revealed by this cycle starts at once.
        (outcome, if removal_recorded { Duration::ZERO } else { wait })
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
            (Some(code), Some(message), Some(at)) => {
                let http_status = error.as_ref().and_then(|e| e.http_status);
                Some(SyncWorkerError {
                    code: code.clone(),
                    kind: error_kind(code, http_status),
                    message: message.clone(),
                    http_status,
                    at: at.clone(),
                })
            }
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
        // A phone being removed does nothing else (not even during a pause): it uploads, then erases.
        if self.phone {
            if let Some(outcome) = self.phone_removal().await? {
                return Ok(outcome);
            }
        }
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
            match self
                .exchange_once(&token, &token_hash, &mut snapshot_done, ExchangeMode::Normal)
                .await?
            {
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
        if self.phone {
            self.probe_removal(&token).await;
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
        // Grants still waiting for an author or for rights that a complete pull did not bring wait
        // no longer (`sync::grant_check`).
        // Accounts, grants and role permissions released here change the rights: refreshed as after
        // a run, also when the release failed part-way (each of its runs commits pass by pass).
        let released = crate::sync::grant_check::release_after_complete_pull(&self.db).await;
        self.refresh_rights(released.as_ref().map_or(true, |changed| *changed))
            .await;
        released?;
        // Decisions sealed by the upgrade are sent once more by the PC whose version the server
        // holds, now that this PC knows it (migration 264, review R2-S8).
        crate::reliability::study::decision_sync::resend_sealed_after_complete_pull(&self.db).await?;
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
        self.note_removal(err);
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

    // ── Phone removal (`removal`) ────────────────────────────────────────────────────────────

    /// Keeps the removal a server answer reveals, on a phone, for the end of the cycle (or for the
    /// removal round that made the request: `take_seen_removal`).
    fn note_removal(&self, err: &TransportError) {
        if !self.phone {
            return;
        }
        if let Some(kind) = removal::kind_of_transport_error(err) {
            self.note_removal_kind(kind);
        }
    }

    fn note_removal_kind(&self, kind: RemovalKind) {
        if let Ok(mut seen) = self.removal_seen.lock() {
            *seen = Some(RemovalKind::combine(*seen, kind));
        }
    }

    fn take_seen_removal(&self) -> Option<RemovalKind> {
        self.removal_seen.lock().ok().and_then(|mut seen| seen.take())
    }

    /// Records the removal noted during the cycle, if any. Whether one was recorded.
    async fn record_seen_removal(&self) -> bool {
        let Some(kind) = self.take_seen_removal() else {
            return false;
        };
        match removal::record(&self.db, kind).await {
            Ok(_) => true,
            Err(e) => {
                error!(target: "maintafox::sync", "cannot record the phone removal: {e}");
                false
            }
        }
    }

    /// Asks the server whether it still accepts this phone (a read-only route every activated
    /// device may call; answered `device_wipe_required` or `activation_revoked` to a removed one).
    async fn probe_once(&self, token: &str) -> Probe {
        match self.client.get_json_raw(REMOVAL_PROBE_PATH, Some(token)).await {
            Ok((status, _)) if (200..300).contains(&status) => Probe::Accepted,
            Ok((status, body)) => match removal::kind_of_refusal_body(status, &body) {
                Some(kind) => Probe::Refused(kind),
                None => Probe::Unanswered(worker_error(&super::client::http_error(status, &body, None))),
            },
            Err(e) => Probe::Unanswered(worker_error(&e)),
        }
    }

    /// At most every [`REMOVAL_PROBE_INTERVAL`], asks the server whether this phone was removed (a
    /// removed phone's exchanges are still answered). A failure says nothing: the next probe asks
    /// again.
    async fn probe_removal(&self, token: &str) {
        let due = self.removal_probed_at.lock().is_ok_and(|mut last| {
            let due = last.is_none_or(|at| at.elapsed() >= REMOVAL_PROBE_INTERVAL);
            if due {
                *last = Some(Instant::now());
            }
            due
        });
        if !due {
            return;
        }
        if let Probe::Refused(kind) = self.probe_once(token).await {
            self.note_removal_kind(kind);
        }
    }

    /// The phone's removal, when one is recorded or this phone is activated without an enrolment
    /// (`auth::phone_binding::unbound_phone_policy`): one attempt of `removal::run_removal`.
    /// `None` when there is nothing to remove (or a lock was just lifted).
    async fn phone_removal(&self) -> AppResult<Option<CycleOutcome>> {
        let pending = match removal::pending(&self.db).await? {
            Some(pending) => pending,
            None => match crate::auth::phone_binding::unbound_phone_removal() {
                Some(kind) if crate::auth::phone_binding::unbound_phone_needs_removal(&self.db).await? => {
                    removal::record(&self.db, kind).await?
                }
                _ => return Ok(None),
            },
        };
        // Read before anything is erased; dropped when this attempt ends.
        let token = crate::commands::product_license::stored_activation_token(&self.db).await?;
        let mut steps = WorkerRemoval {
            worker: self,
            token: token.clone(),
        };
        Ok(
            match removal::run_removal(&self.db, &mut steps, pending, token).await? {
                RemovalOutcome::Done(_) => Some(CycleOutcome::Blocked {
                    reason: BLOCKER_PHONE_REMOVED.to_string(),
                    error: None,
                }),
                RemovalOutcome::Waiting {
                    error: Some(error),
                    retry_after,
                } => Some(if error.code == "offline" {
                    CycleOutcome::Offline { error }
                } else {
                    CycleOutcome::Retry { error, retry_after }
                }),
                // More to upload than one attempt sends: the next attempt starts at once.
                RemovalOutcome::Waiting { error: None, .. } => Some(CycleOutcome::Synced { more_work: true }),
                RemovalOutcome::Blocked { error } => Some(CycleOutcome::Blocked {
                    reason: BLOCKER_PHONE_LOCKED.to_string(),
                    error,
                }),
                // The server accepts this phone again: the normal cycle goes on.
                RemovalOutcome::Lifted => None,
            },
        )
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
                    device_class: crate::sync::transport::wire::device_class_if_not_desktop(),
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

    /// The cached permissions are dropped and the window reloads its own ([`RightsChangedHook`]).
    async fn refresh_rights(&self, needed: bool) {
        if needed {
            if let Some(hook) = &self.on_rights_changed {
                hook().await;
            }
        }
    }

    /// Apply every received row: pending rows parents first, then (`retry_deferred`) the rows that
    /// waited for a parent, until nothing more can be applied.
    async fn materialize(&self, retry_deferred: bool) -> AppResult<()> {
        let run = materialize_inbox(&self.db, SYNC_MATERIALIZE_BATCH, retry_deferred).await;
        // Once per run, not per row, also when the run failed after a pass that wrote rights rows
        // had committed.
        self.refresh_rights(crate::sync::materialize::rights_refresh_needed(&run))
            .await;
        let result = run?;
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
        // Reserved stock marks left by a write outside a pass (`inventory::reservations`), every cycle,
        // also when nothing was received; no write when there is no mark.
        if let Err(e) = crate::inventory::reservations::recompute_dirty_reserved(&self.db).await {
            warn!(target: "maintafox::sync", "reserved stock recompute failed: {e}");
        }
        // D13: a close, a cancellation or a reservation received from another PC may leave a
        // reservation open on an ended work order; it is released now, not at the next start. Run
        // every cycle (it reads the open reservations only), also when the cycle only settled
        // conflicts.
        match crate::inventory::reservations::release_reservations_of_ended_work_orders(
            &self.db,
            crate::inventory::reservations::SweepOrigin::AfterSync,
        )
        .await
        {
            Ok(sweep) => {
                for (wo_id, error) in &sweep.failures {
                    warn!(target: "maintafox::sync", wo_id = *wo_id, error = %error, "reservations of an ended work order not released");
                }
            }
            Err(e) => warn!(target: "maintafox::sync", "releasing reservations of ended work orders failed: {e}"),
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
            device_class: DEVICE_CLASS,
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

    /// One exchange. [`ExchangeMode::Normal`]: the files announced are uploaded first.
    /// [`ExchangeMode::Removal`]: a phone's final upload; its files go after the exchange (the
    /// server takes a removed phone's file only once its record and manifest entry are in), and a
    /// failure never gives an item up unless the server refused the request as invalid.
    async fn exchange_once(
        &self,
        token: &str,
        token_hash: &str,
        snapshot_done: &mut bool,
        mode: ExchangeMode,
    ) -> AppResult<Step> {
        let pending = match store::load_state(&self.db).await?.pending {
            Some(pending) => {
                queries::mark_outbox_items_resent(&self.db, &pending.outbox_ids).await?;
                pending
            }
            None => self.build_exchange().await?,
        };
        if mode == ExchangeMode::Normal {
            self.upload_pending_media(token, false).await?;
        }

        let response = match self.client.exchange(token, &pending.body).await {
            Ok(response) => response,
            Err(e) => {
                return self
                    .exchange_failure(&e, &pending, token, token_hash, snapshot_done, mode)
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
        mode: ExchangeMode,
    ) -> AppResult<Step> {
        self.note_removal(err);
        let error = worker_error(err);
        let ids = &pending.outbox_ids;
        if mode == ExchangeMode::Removal {
            return self.removal_exchange_failure(err, error, pending).await.map(Step::Stop);
        }
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

    /// A failed exchange of a phone's final upload (`removal`). The data is kept for every failure
    /// but one: the server refused the request itself as invalid ([`removal_refusal_is_final`]),
    /// whose items are given up at once, logged, and counted as dropped at the erase. Nothing is
    /// ever given up for a server error or a lost answer, however often it happens.
    async fn removal_exchange_failure(
        &self,
        err: &TransportError,
        error: SyncWorkerError,
        pending: &PendingExchange,
    ) -> AppResult<CycleOutcome> {
        let ids = &pending.outbox_ids;
        Ok(match classify(err) {
            FailureKind::Offline => {
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                CycleOutcome::Offline { error }
            }
            FailureKind::Permanent if removal_refusal_is_final(err) => {
                let dropped = queries::give_up_outbox_items(&self.db, ids, &error.message).await?;
                store::clear_pending(&self.db).await?;
                error!(
                    target: "maintafox::sync",
                    dropped,
                    code = error.code.as_str(),
                    http_status = ?error.http_status,
                    "phone removal: the server refused this request as invalid; its items are left behind and counted as dropped"
                );
                CycleOutcome::Failed { error }
            }
            FailureKind::Retryable | FailureKind::Permanent => {
                // Same key and body next time; never given up during a removal.
                queries::keep_outbox_items_after_failure(&self.db, ids, &error.message).await?;
                CycleOutcome::Retry {
                    retry_after: retry_after_of(err),
                    error,
                }
            }
            FailureKind::TenantMismatch | FailureKind::CheckpointRefused | FailureKind::IdempotencyKeyReused => {
                // The stored request is rebuilt (a removed phone's push needs no checkpoint).
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                store::clear_pending(&self.db).await?;
                CycleOutcome::Retry {
                    retry_after: retry_after_of(err),
                    error,
                }
            }
            FailureKind::RateLimited | FailureKind::ActivationRefused => {
                // Nothing processed; the removal reads a refusal it acts on (`note_removal`).
                queries::return_unsent_outbox_items(&self.db, ids).await?;
                CycleOutcome::Retry {
                    retry_after: retry_after_of(err),
                    error,
                }
            }
        })
    }

    /// Uploads the files due. `removal`: a phone's final upload, where a failed file is never given
    /// up unless it can never be uploaded (missing, altered, refused for good by the server).
    async fn upload_pending_media(&self, token: &str, removal: bool) -> AppResult<MediaPass> {
        let mut pass = MediaPass::default();
        let uploads = store::list_pending_media(&self.db, SYNC_MEDIA_UPLOADS_PER_CYCLE).await?;
        for upload in uploads {
            match self.upload_one(token, &upload).await {
                Ok(()) => store::mark_media_uploaded(&self.db, &upload.object_key).await?,
                // Nothing more can be uploaded now; not counted as an attempt.
                Err(MediaFailure::Offline) => {
                    pass.offline = true;
                    break;
                }
                Err(MediaFailure::Retry(message) | MediaFailure::NotYetAvailable(message)) => {
                    let give_up = !removal && upload.attempt_count + 1 >= OUTBOX_MAX_DELIVERY_ATTEMPTS;
                    warn!(target: "maintafox::sync", object_key = upload.object_key.as_str(), give_up, "media upload failed: {message}");
                    store::mark_media_failed(&self.db, &upload.object_key, &message, give_up).await?;
                    pass.failed = Some(message);
                }
                Err(MediaFailure::Permanent(message)) => {
                    warn!(target: "maintafox::sync", object_key = upload.object_key.as_str(), removal, "media upload impossible: {message}");
                    store::mark_media_failed(&self.db, &upload.object_key, &message, true).await?;
                }
            }
        }
        Ok(pass)
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

        let media_failure = |e: TransportError| {
            self.note_removal(&e);
            match (e.code(), classify(&e)) {
                // A removed phone's file the server does not take from it (not its own work).
                (MEDIA_NOT_FROM_THIS_DEVICE, _) => MediaFailure::Permanent(e.message()),
                (_, FailureKind::Offline) => MediaFailure::Offline,
                (_, FailureKind::Permanent) => MediaFailure::Permanent(e.message()),
                _ => MediaFailure::Retry(e.message()),
            }
        };
        let presigned = match self
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
        {
            Ok(presigned) => presigned,
            // The object is already stored (keys are content-addressed): this file is uploaded.
            Err(e) if e.code() == MEDIA_ALREADY_STORED => {
                info!(target: "maintafox::sync", object_key = upload.object_key.as_str(), "media already stored on the server");
                return Ok(());
            }
            Err(e) => return Err(media_failure(e)),
        };
        if !presigned.method.eq_ignore_ascii_case("PUT") || presigned.object_key != upload.object_key {
            return Err(MediaFailure::Permanent(format!(
                "presign answered {} {} for {}",
                presigned.method, presigned.object_key, upload.object_key
            )));
        }
        self.client
            .put_object(&presigned.url, bytes, &presigned.headers)
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
            .map_err(|e| {
                self.note_removal(&e);
                match (e.code(), classify(&e)) {
                    ("object_key_tenant_mismatch", _) => MediaFailure::Permanent(e.message()),
                    (_, FailureKind::Offline | FailureKind::ActivationRefused | FailureKind::TenantMismatch) => {
                        MediaFailure::Offline
                    }
                    (_, FailureKind::Permanent) => MediaFailure::Permanent(e.message()),
                    _ => MediaFailure::Retry(e.message()),
                }
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

/// The removal steps as the worker carries them out on this phone.
struct WorkerRemoval<'a> {
    worker: &'a SyncWorker,
    /// The device token of the activation being removed (read before the erase).
    token: Option<String>,
}

/// Wipe confirmation of a removed phone (the server takes it with the old device token).
const WIPE_CONFIRMED_PATH: &str = "/api/v1/mobile-device/wipe-confirmed";
/// Sends of the wipe confirmation when no answer comes back.
const WIPE_CONFIRMATION_ATTEMPTS: u32 = 3;

/// How a phone's final upload sends one exchange (`SyncWorker::exchange_once`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeMode {
    Normal,
    Removal,
}

/// What one pass over the files due did.
#[derive(Debug, Default)]
struct MediaPass {
    /// Stopped: nothing more can be transferred now.
    offline: bool,
    /// A file failed and is retried (the last reason).
    failed: Option<String>,
}

/// presign-put refusal of a removed phone's file that is not its own work (permanent).
const MEDIA_NOT_FROM_THIS_DEVICE: &str = "media_not_from_this_device";
/// presign-put answer for an object already stored (keys are content-addressed): uploaded.
const MEDIA_ALREADY_STORED: &str = "media_already_stored";

/// Whether a removed phone's exchange refused as invalid is left behind (its items given up and
/// counted as dropped): only a refusal of the request's content (400, 409, 413, 422). Any other
/// status (a missing route behind a proxy, a gone server) keeps the data.
fn removal_refusal_is_final(err: &TransportError) -> bool {
    matches!(err.http_status(), Some(400 | 409 | 413 | 422))
}

fn removal_error(code: &str, message: String) -> SyncWorkerError {
    SyncWorkerError {
        code: code.to_string(),
        kind: error_kind(code, None),
        message,
        http_status: None,
        at: store::now_utc(),
    }
}

fn no_device_token() -> SyncWorkerError {
    removal_error("no_device_token", "no device token left to upload with".to_string())
}

fn offline_error() -> SyncWorkerError {
    removal_error(
        "offline",
        "the files cannot be uploaded now (no connection)".to_string(),
    )
}

fn media_error(message: String) -> SyncWorkerError {
    removal_error("media_upload_failed", message)
}

/// The error a stopped exchange reports, with its `Retry-After`.
fn failure_of(outcome: CycleOutcome) -> Option<(SyncWorkerError, Option<Duration>)> {
    match outcome {
        CycleOutcome::Offline { error } | CycleOutcome::Failed { error } => Some((error, None)),
        CycleOutcome::Retry { error, retry_after } => Some((error, retry_after)),
        CycleOutcome::Blocked { error: Some(error), .. } => Some((error, None)),
        CycleOutcome::Blocked { reason, error: None } => Some((removal_error(&reason, reason.clone()), None)),
        CycleOutcome::Synced { .. } | CycleOutcome::Paused { .. } => None,
    }
}

impl RemovalSteps for WorkerRemoval<'_> {
    async fn end_session(&mut self) {
        if let Some(session) = &self.worker.session {
            if session.write().await.clear_session().is_some() {
                warn!(target: "maintafox::sync", "phone removal: the session was ended");
            }
        }
    }

    async fn probe(&mut self) -> Probe {
        match self.token.as_deref() {
            Some(token) => self.worker.probe_once(token).await,
            None => Probe::Unanswered(no_device_token()),
        }
    }

    async fn requeue_failures(&mut self) -> AppResult<i64> {
        let db = &self.worker.db;
        Ok(queries::requeue_dead_letters(db).await? + store::requeue_failed_media(db).await?)
    }

    /// One exchange of the records (with the media manifest), then the files due; never a
    /// snapshot (the server refuses one to a removed phone).
    async fn upload_round(&mut self, _kind: RemovalKind) -> AppResult<UploadRound> {
        let worker = self.worker;
        let before = removal::work_by_kind(&worker.db).await?;
        let stored = store::load_state(&worker.db).await?.pending.is_some();
        if before.unsent_total() == 0 && !stored {
            return Ok(UploadRound::Empty {
                uploaded: removal::Uploaded::new(),
            });
        }
        let Some(token) = self.token.as_deref() else {
            // Never erased for want of a token: the work stays, the phone stays locked.
            warn!(target: "maintafox::sync", "phone removal: no device token left to upload with; data kept");
            return Ok(UploadRound::Failed {
                uploaded: removal::Uploaded::new(),
                error: no_device_token(),
                retry_after: None,
            });
        };
        // A refusal left from earlier in the cycle is not this round's.
        let _ = worker.take_seen_removal();
        let mut failure: Option<(SyncWorkerError, Option<Duration>)> = None;
        if stored || before.unsent_records() > 0 {
            let token_hash = hash_payload(token);
            if let Step::Stop(outcome) = worker
                .exchange_once(token, &token_hash, &mut true, ExchangeMode::Removal)
                .await?
            {
                failure = failure_of(outcome);
            }
        }
        if failure.is_none() && removal::work_by_kind(&worker.db).await?.unsent_files() > 0 {
            let pass = worker.upload_pending_media(token, true).await?;
            if pass.offline {
                failure = Some((offline_error(), None));
            } else if let Some(message) = pass.failed {
                failure = Some((media_error(message), None));
            }
        }
        let after = removal::work_by_kind(&worker.db).await?;
        let uploaded = before.uploaded_until(&after);
        if let Some(kind) = worker.take_seen_removal() {
            return Ok(UploadRound::Refused {
                uploaded,
                kind,
                error: failure.map(|(error, _)| error),
            });
        }
        if let Some((error, retry_after)) = failure {
            return Ok(UploadRound::Failed {
                uploaded,
                error,
                retry_after,
            });
        }
        let stored = store::load_state(&worker.db).await?.pending.is_some();
        Ok(if after.unsent_total() == 0 && !stored {
            UploadRound::Empty { uploaded }
        } else {
            UploadRound::More { uploaded }
        })
    }

    async fn unsent_remaining(&mut self) -> AppResult<i64> {
        let db = &self.worker.db;
        let stored = i64::from(store::load_state(db).await?.pending.is_some());
        Ok(removal::work_by_kind(db).await?.unsent_total() + stored)
    }

    async fn leftover(&mut self) -> AppResult<std::collections::BTreeMap<String, i64>> {
        removal::unsent_by_kind(&self.worker.db).await
    }

    async fn erase(&mut self) -> AppResult<()> {
        let worker = self.worker;
        let own_session;
        let session = match &worker.session {
            Some(session) => session.as_ref(),
            None => {
                own_session = tokio::sync::RwLock::new(crate::auth::session_manager::SessionManager::new());
                &own_session
            }
        };
        crate::commands::product_license::reset_to_first_run_in(&worker.db, session, "phone removed").await?;
        // The synchronised files (photos, documents) go with the data that named them.
        for dir in crate::sync::media::MEDIA_STORAGE_DIRECTORIES {
            match tokio::fs::remove_dir_all(worker.media_root.join(dir)).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(AppError::Io(e)),
            }
        }
        Ok(())
    }

    async fn regenerate_identity(&mut self) -> AppResult<()> {
        // Off a phone (the worker run as a phone by the tests) there is no installation identity:
        // a desktop's fingerprint comes from its OS and is never regenerated.
        if !removal::applies_on_this_device() {
            return Ok(());
        }
        crate::auth::device::regenerate_installation_identity()
    }

    /// Sent with the dropped counts; again when no answer came (a lost answer is answered the
    /// erase marker the second time: the server took the first one); once more without the counts
    /// if the server finds them invalid, so the confirmation itself is never lost to them.
    async fn confirm_wipe(&mut self, old_token: &str, dropped: &std::collections::BTreeMap<String, i64>) -> bool {
        let mut body = removal::wipe_confirmation_body(dropped);
        for _ in 0..WIPE_CONFIRMATION_ATTEMPTS {
            match self
                .worker
                .client
                .post_json_raw(WIPE_CONFIRMED_PATH, Some(old_token), body.to_string())
                .await
            {
                Ok((status, _)) if (200..300).contains(&status) => return true,
                Ok((status, answer)) if removal::wipe_marker_of_body(status, &answer).is_some() => {
                    info!(target: "maintafox::sync", "phone removal: the wipe was already confirmed");
                    return true;
                }
                Ok((400, _)) if body != serde_json::json!({}) => {
                    warn!(target: "maintafox::sync", "phone removal: the dropped counts were refused; confirming without them");
                    body = serde_json::json!({});
                }
                Ok((status, _)) => {
                    warn!(target: "maintafox::sync", status, "phone removal: the wipe confirmation was refused");
                    return false;
                }
                Err(e) => {
                    warn!(target: "maintafox::sync", error = %e.message(), "phone removal: the wipe confirmation got no answer");
                }
            }
        }
        false
    }

    fn publish(&mut self, status: PhoneRemovalStatus) {
        self.worker.handle.publish_removal(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A worker whose rights hook counts its calls (`SyncWorker::on_rights_changed`, AU-107).
    fn counting_worker(db: &DatabaseConnection) -> (SyncWorker, Arc<std::sync::atomic::AtomicUsize>) {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = calls.clone();
        let worker = SyncWorker::new(
            db.clone(),
            SyncWorkerHandle::new(),
            SyncWorkerConfig {
                api_base_url: "http://127.0.0.1:9".to_string(),
                media_root: std::env::temp_dir(),
                timing: timing(),
            },
            Arc::new(LicenseSyncAccess),
        )
        .expect("worker")
        .on_rights_changed(Arc::new(move || {
            let counter = counter.clone();
            Box::pin(async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })
        }));
        (worker, calls)
    }

    async fn run_sql(db: &DatabaseConnection, sql: &str, values: Vec<sea_orm::Value>) {
        db.execute(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    /// A role made on another PC, received as new: its row is written here when applied.
    async fn receive_role(db: &DatabaseConnection, sync_id: &str) {
        let at = chrono::Utc::now().to_rfc3339();
        run_sql(
            db,
            "INSERT INTO roles (entity_sync_id, name, role_type, status, is_system, created_at, updated_at, row_version) \
             VALUES (?, ?, 'custom', 'active', 0, ?, ?, 1)",
            vec![sync_id.into(), sync_id.into(), at.clone().into(), at.into()],
        )
        .await;
        let payload = crate::sync::test_support::current_payload(db, "roles", sync_id, &serde_json::Map::new())
            .await
            .expect("payload")
            .expect("row");
        // Removed again, with what this PC staged for it, so that it arrives as new.
        run_sql(db, "DELETE FROM roles WHERE entity_sync_id = ?", vec![sync_id.into()]).await;
        crate::sync::capture::drain_capture_journal(db).await.expect("drain");
        run_sql(
            db,
            "DELETE FROM sync_outbox WHERE entity_sync_id = ?",
            vec![sync_id.into()],
        )
        .await;
        queries::apply_sync_batch(
            db,
            crate::sync::domain::ApplySyncBatchInput {
                protocol_version: SYNC_PROTOCOL_VERSION_V2.to_string(),
                server_batch_id: format!("b-{sync_id}"),
                checkpoint_token: format!("v2.test.{sync_id}"),
                acknowledged_items: vec![],
                rejected_items: vec![],
                inbound_items: vec![crate::sync::domain::SyncInboundItemInput {
                    entity_type: "roles".into(),
                    entity_sync_id: sync_id.into(),
                    operation: "upsert".into(),
                    row_version: 2,
                    payload_json: serde_json::Value::Object(payload).to_string(),
                    ..Default::default()
                }],
                conflict_items: vec![],
                policy_metadata_json: None,
            },
        )
        .await
        .expect("ingest");
    }

    /// AU-107 (review of 461146ef): the rights hook runs once per run that wrote a rights row, never
    /// for a run without one, and once for a run that failed (passes committed before the failure
    /// may hold rights rows: the refresh does not depend on what the run reported).
    #[tokio::test]
    async fn the_rights_hook_runs_once_per_rights_run_and_on_a_failed_run() {
        use std::sync::atomic::Ordering::SeqCst;
        let db = crate::sync::test_support::started_db().await;
        let (worker, calls) = counting_worker(&db);

        worker.materialize(true).await.expect("empty run");
        assert_eq!(calls.load(SeqCst), 0, "nothing received");

        receive_role(&db, "role-from-other-pc").await;
        worker.materialize(true).await.expect("rights run");
        assert_eq!(calls.load(SeqCst), 1, "one refresh for the run");
        let held = db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS h FROM roles WHERE entity_sync_id = 'role-from-other-pc'".to_string(),
            ))
            .await
            .expect("role");
        assert!(held.is_some(), "the received role is written here");

        // Every later step of a run fails (the inbox is gone): an error, refreshed anyway.
        db.execute_unprepared("ALTER TABLE sync_inbox RENAME TO sync_inbox_gone")
            .await
            .expect("break the inbox");
        assert!(worker.materialize(true).await.is_err());
        assert_eq!(calls.load(SeqCst), 2, "refreshed once for the failed run");

        // The release after a complete pull (grants and accounts waiting for their author), failed.
        assert!(worker.after_complete_pull(None).await.is_err());
        assert_eq!(calls.load(SeqCst), 3, "refreshed once for the failed release");
    }

    /// The release after a complete pull with nothing waiting changes no right: no refresh.
    #[tokio::test]
    async fn a_release_with_nothing_waiting_does_not_refresh() {
        let db = crate::sync::test_support::started_db().await;
        let (worker, calls) = counting_worker(&db);
        worker.after_complete_pull(None).await.expect("release");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

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
    fn worker_errors_carry_a_stable_reason_and_keep_the_raw_text_as_detail() {
        let http = |status: u16, code: Option<&str>| TransportError::Http {
            status,
            code: code.map(str::to_string),
            message: "upstream down".to_string(),
            retry_after: None,
            wipe_without_upload: None,
        };
        let cases = [
            (TransportError::Offline("dns error".into()), "network_unreachable"),
            (
                TransportError::Interrupted("timed out".into()),
                "connection_interrupted",
            ),
            (TransportError::Contract("bad json".into()), "invalid_server_response"),
            (http(503, None), "server_unavailable"),
            (http(500, Some("sync_exchange_failed")), "server_unavailable"),
            (http(429, Some("rate_limited")), "server_unavailable"),
            (http(408, None), "server_unavailable"),
            (http(401, Some("invalid_token")), "access_refused"),
            (http(403, None), "access_refused"),
            (http(409, Some("tenant_config_mismatch")), "request_refused"),
            (http(400, None), "request_refused"),
        ];
        for (err, kind) in cases {
            let worker = worker_error(&err);
            assert_eq!(worker.kind, kind, "{err:?}");
            // The raw text stays available for the log and the technical details.
            assert_eq!(worker.message, err.message());
            assert_eq!(worker.code, err.code());
        }
        assert_eq!(worker_error(&http(503, None)).message, "HTTP 503: upstream down");
        assert_eq!(
            local_error(&AppError::SyncError("disk full".into())).kind,
            "local_error"
        );
        // A persisted error read back without its HTTP status.
        assert_eq!(error_kind(BLOCKER_NOT_CONFIGURED, None), "not_configured");
        assert_eq!(error_kind("rate_limited", None), "server_unavailable");
        assert_eq!(error_kind("invalid_token", None), "access_refused");
        assert_eq!(error_kind("tenant_config_mismatch", None), "request_refused");
        assert_eq!(error_kind("http_error", None), "other");
        assert_eq!(error_kind("something_new", None), "other");
        assert_eq!(error_kind("offline", None), "network_unreachable");
    }

    #[test]
    fn classification_follows_the_contract() {
        let http = |status: u16, code: Option<&str>| TransportError::Http {
            status,
            code: code.map(str::to_string),
            message: String::new(),
            retry_after: None,
            wipe_without_upload: None,
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
