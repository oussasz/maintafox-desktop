//! A phone that may no longer work for its plant uploads what it still holds, then erases itself;
//! a phone the licence server refuses without saying it may be erased locks and keeps its data.
//!
//! Four causes, recorded as a [`PendingRemoval`] (device setting, kept across restarts):
//! - [`RemovalKind::Removed`]: its administrator removed it (licence server 403
//!   `device_wipe_required`). For 168 h the server still takes this phone's push-only sync
//!   exchanges, the upload of its own files (`presign-put`) and its wipe confirmation, and refuses
//!   everything else.
//! - [`RemovalKind::Revoked`]: 403 `activation_revoked` WITH the server's erase marker
//!   (`wipe: "without_upload"`, `reason: "lost" | "upload_window_closed"`): reported lost, or removed
//!   and the window passed. The only answer on which unsent work is erased without being uploaded.
//! - [`RemovalKind::RevokedUnconfirmed`]: 403 `activation_revoked` WITHOUT the marker (no activation
//!   row after a server restore, a vendor revocation, a released slot). The server did not say this
//!   phone may be erased: it is locked (nobody signs in, no new work), its data is kept, and the
//!   worker asks the server again at each re-check ([`RemovalSteps::probe`]): accepted again, the
//!   lock is lifted; `device_wipe_required`, the upload starts; the marker, the erase.
//! - [`RemovalKind::Unbound`]: activated with the licence key before enrolment existed, so it is
//!   bound to nobody (`auth::phone_binding::unbound_phone_policy`). Its server still takes normal
//!   exchanges.
//!
//! The procedure ([`run_removal`], run by the sync worker, Android only):
//! 1. The session is ended; while a removal is pending nobody signs in (`auth::phone_binding`).
//! 2. Removed / Unbound: the outbox items and files given up before ([`RemovalSteps::requeue_failures`]:
//!    dead letters, failed uploads) are put back once; then exchanges (records and media manifest
//!    first), then the files of the phone's own records, until nothing is left unsent, the server
//!    having acknowledged each item and object storage each file. An exchange that fails stops the
//!    attempt: the data is kept and the worker retries with its backoff, for as long as it takes
//!    (never erased on a timer; a server error never gives an item up during a removal). Only a
//!    request the server refuses as invalid (400, 409, 413, 422) or a file it refuses for good is
//!    left behind, logged and counted as dropped. Revoked: nothing is sent.
//! 3. Right before the erase, newly captured work sends the procedure back to step 2. What will
//!    not reach the server is counted per kind (Revoked: everything unsent; otherwise items and
//!    files refused for good) and logged.
//! 4. Local data erased (`reset_to_first_run_in` and the synchronised files), then the
//!    installation identity regenerated: the next enrolment is a new phone for the server, and the
//!    phone is "not activated" again (enrolment, never sign-in).
//! 5. Removed only: the wipe confirmed with the OLD device token, held in memory since step 2 and
//!    dropped right after, with the dropped counts (`dropped: { total, by_kind }`). Never before
//!    step 4.
//!
//! The licence server has no route a phone can call to free its own licence slot: an Unbound
//! phone's former activation keeps its slot until an administrator removes it from the phone list.
//!
//! The screens follow [`PhoneRemovalStatus`] (event [`PHONE_REMOVAL_STATUS_EVENT`], command
//! `get_phone_removal_status`).

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};

use super::client::TransportError;
use super::worker::SyncWorkerError;
use crate::errors::AppResult;

/// Tauri event carrying every [`PhoneRemovalStatus`] change to the webview.
pub const PHONE_REMOVAL_STATUS_EVENT: &str = "phone-removal-status";

const REMOVAL_KEY: &str = "mobile.phone_removal";
const REMOVAL_SCOPE: &str = "device";

/// Exchanges in one attempt before the worker yields (it starts the next attempt at once).
pub const REMOVAL_UPLOAD_ROUNDS_PER_ATTEMPT: u32 = 50;

/// The `reason` values of the licence server's erase marker (`wipe: "without_upload"`). A marker
/// with any other reason is not read as one: the phone then locks and keeps its data.
pub const WIPE_REASONS: [&str; 2] = ["lost", "upload_window_closed"];

/// Key of `dropped_by_kind` / `uploaded_by_kind` counting files (photos, documents).
pub const MEDIA_FILES_KIND: &str = "media_files";

/// The wipe confirmation's `dropped.by_kind` takes at most this many kinds of at most this many
/// characters (server limits); the rest is summed under [`OTHER_KINDS`].
const WIPE_CONFIRMATION_MAX_KINDS: usize = 32;
const WIPE_CONFIRMATION_KIND_MAX_CHARS: usize = 64;
const OTHER_KINDS: &str = "other";

/// Why this phone erases itself, or is locked (see the module comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalKind {
    /// Activated with the licence key, bound to nobody: uploads, then resets for an enrolment.
    Unbound,
    /// Removed by its administrator: uploads (push only, then its files), erases, confirms.
    Removed,
    /// Refused without the erase marker: locked, data kept, the server asked again.
    RevokedUnconfirmed,
    /// The server's erase marker: erases without uploading.
    Revoked,
}

impl RemovalKind {
    /// Whether a refusal meaning `self`, seen after `current` was recorded, replaces it. Revoked
    /// (the marker) is final; Removed and RevokedUnconfirmed follow the server's latest answer (a
    /// lock turns into the upload when the server says the phone was removed, an upload is locked
    /// when the server refuses the phone without the marker); Unbound never replaces anything.
    pub fn supersedes(self, current: RemovalKind) -> bool {
        match (current, self) {
            (RemovalKind::Revoked, _) => false,
            (_, RemovalKind::Revoked) => true,
            (RemovalKind::Unbound | RemovalKind::RevokedUnconfirmed, RemovalKind::Removed)
            | (RemovalKind::Unbound | RemovalKind::Removed, RemovalKind::RevokedUnconfirmed) => true,
            _ => false,
        }
    }

    /// What `seen` becomes once `next` is seen too: `next` when it supersedes.
    pub fn combine(seen: Option<RemovalKind>, next: RemovalKind) -> RemovalKind {
        match seen {
            Some(seen) if seen == next || !next.supersedes(seen) => seen,
            _ => next,
        }
    }
}

/// The removal under way on this phone (device setting [`REMOVAL_KEY`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRemoval {
    pub kind: RemovalKind,
    pub detected_at: String,
    /// Items and files the server acknowledged during the upload.
    #[serde(default)]
    pub uploaded_count: i64,
    /// The same per kind (entity type, or [`MEDIA_FILES_KIND`]).
    #[serde(default)]
    pub uploaded_by_kind: BTreeMap<String, i64>,
    /// What was not uploaded, per kind (entity type, or [`MEDIA_FILES_KIND`]), counted before the erase.
    #[serde(default)]
    pub dropped_by_kind: BTreeMap<String, i64>,
    /// The items and files given up before the removal were put back to be sent (once).
    #[serde(default)]
    pub failures_requeued: bool,
    /// The local data is erased; what remains is the new identity and the confirmation.
    #[serde(default)]
    pub erased: bool,
}

impl PendingRemoval {
    fn new(kind: RemovalKind) -> Self {
        Self {
            kind,
            detected_at: crate::sync::transport::store::now_utc(),
            uploaded_count: 0,
            uploaded_by_kind: BTreeMap::new(),
            dropped_by_kind: BTreeMap::new(),
            failures_requeued: false,
            erased: false,
        }
    }

    fn add_uploaded(&mut self, uploaded: &Uploaded) {
        for (kind, n) in uploaded {
            if *n > 0 {
                *self.uploaded_by_kind.entry(kind.clone()).or_insert(0) += n;
                self.uploaded_count += n;
            }
        }
    }
}

/// Whether this build erases itself when removed: the Android field app only. A desktop keeps
/// its data; a refused desktop stays blocked until its licence is put right.
pub const fn applies_on_this_device() -> bool {
    cfg!(target_os = "android")
}

/// The removal a licence server refusal means for a phone, if any. `wipe_marked`: the answer
/// carries the erase marker ([`wipe_marker_of_body`]).
pub fn kind_of_refusal(status: u16, code: Option<&str>, wipe_marked: bool) -> Option<RemovalKind> {
    match (status, code) {
        (403, Some("device_wipe_required")) => Some(RemovalKind::Removed),
        (403, Some("activation_revoked")) if wipe_marked => Some(RemovalKind::Revoked),
        (403, Some("activation_revoked")) => Some(RemovalKind::RevokedUnconfirmed),
        _ => None,
    }
}

/// The erase marker's `reason` in the answer `body` of `status`: only a 403 `activation_revoked`
/// with `wipe: "without_upload"` and a reason of [`WIPE_REASONS`].
pub fn wipe_marker_of_body(status: u16, body: &str) -> Option<String> {
    if status != 403 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let field = |name: &str| value.get(name).and_then(serde_json::Value::as_str);
    if field("error") != Some("activation_revoked") || field("wipe") != Some("without_upload") {
        return None;
    }
    let reason = field("reason").unwrap_or_default();
    if !WIPE_REASONS.contains(&reason) {
        tracing::warn!(target: "maintafox::sync", reason, "erase marker with an unknown reason: read as a refusal without marker");
        return None;
    }
    Some(reason.to_string())
}

/// [`kind_of_refusal`] of a raw answer.
pub fn kind_of_refusal_body(status: u16, body: &str) -> Option<RemovalKind> {
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(serde_json::Value::as_str).map(str::to_string));
    kind_of_refusal(status, code.as_deref(), wipe_marker_of_body(status, body).is_some())
}

/// [`kind_of_refusal`] of a transport error.
pub fn kind_of_transport_error(err: &TransportError) -> Option<RemovalKind> {
    match err {
        TransportError::Http {
            status,
            code,
            wipe_without_upload,
            ..
        } => kind_of_refusal(*status, code.as_deref(), wipe_without_upload.is_some()),
        _ => None,
    }
}

pub async fn pending(db: &impl ConnectionTrait) -> AppResult<Option<PendingRemoval>> {
    let Some(row) = crate::settings::get_setting(db, REMOVAL_KEY, REMOVAL_SCOPE).await? else {
        return Ok(None);
    };
    match serde_json::from_str(&row.setting_value_json) {
        Ok(removal) => Ok(Some(removal)),
        // Unreadable: the phone is never left unlocked, nor erased on a guess. Read as a refusal
        // without marker: locked, data kept, and the server is asked what this phone is.
        Err(e) => {
            tracing::error!(target: "maintafox::sync", error = %e, "phone removal record unreadable: the phone is locked and asks the server");
            Ok(Some(PendingRemoval::new(RemovalKind::RevokedUnconfirmed)))
        }
    }
}

async fn save(db: &DatabaseConnection, removal: &PendingRemoval, summary: &str) -> AppResult<()> {
    crate::settings::set_setting(
        db,
        REMOVAL_KEY,
        REMOVAL_SCOPE,
        &serde_json::to_string(removal)?,
        None,
        summary,
    )
    .await
}

async fn clear(db: &DatabaseConnection, summary: &str) -> AppResult<()> {
    crate::settings::delete_setting(db, REMOVAL_KEY, REMOVAL_SCOPE, None, summary).await?;
    Ok(())
}

/// Records that this phone must erase itself (or lock) for `kind`, unless the cause already
/// recorded stays ([`RemovalKind::supersedes`]). Returns the removal now pending.
pub async fn record(db: &DatabaseConnection, kind: RemovalKind) -> AppResult<PendingRemoval> {
    let removal = match pending(db).await? {
        Some(current) if current.kind == kind || !kind.supersedes(current.kind) => return Ok(current),
        Some(mut current) => {
            current.kind = kind;
            current
        }
        None => PendingRemoval::new(kind),
    };
    tracing::warn!(target: "maintafox::sync", kind = ?kind, "phone removal recorded");
    save(db, &removal, "phone removal recorded").await?;
    Ok(removal)
}

/// Records the removal a licence server refusal observed in Rust means, on a phone (`body`: the
/// answer, read for the erase marker). A failure to record is logged (the next refusal records it
/// again).
pub async fn note_refusal(db: &DatabaseConnection, status: u16, code: Option<&str>, body: Option<&str>) {
    if !applies_on_this_device() {
        return;
    }
    let wipe_marked = body.is_some_and(|b| wipe_marker_of_body(status, b).is_some());
    if let Some(kind) = kind_of_refusal(status, code, wipe_marked) {
        if let Err(e) = record(db, kind).await {
            tracing::error!(target: "maintafox::sync", error = %e, "cannot record the phone removal");
        }
    }
}

// ── Status for the screens ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalPhase {
    /// No removal.
    None,
    /// Sending the pending work.
    Uploading,
    /// The upload failed (offline, server error): data kept, the phone locked, retried with backoff.
    WaitingToUpload,
    /// Refused by the server without the erase marker (`RevokedUnconfirmed`): the phone locked,
    /// its data kept, the server asked again at each re-check.
    Blocked,
    /// Erasing the local data.
    Erasing,
    /// Erased; the phone is back at its first start (enrolment).
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhoneRemovalStatus {
    pub phase: RemovalPhase,
    pub kind: Option<RemovalKind>,
    /// Items and files the server acknowledged during the final upload.
    pub uploaded_count: i64,
    /// The same per kind (entity type, or `media_files` for the photos and documents).
    pub uploaded_by_kind: BTreeMap<String, i64>,
    /// Items that will not reach the server (lost, refused for good, or files refused for good),
    /// and the same per kind.
    pub dropped_count: i64,
    pub dropped_by_kind: BTreeMap<String, i64>,
    /// Why the last upload attempt failed (`WaitingToUpload`), or why the server could not be
    /// asked (`Blocked`).
    pub last_error: Option<SyncWorkerError>,
    /// Removed only, once `Done`: whether the server took the wipe confirmation.
    pub wipe_confirmed: Option<bool>,
}

impl PhoneRemovalStatus {
    pub fn none() -> Self {
        Self {
            phase: RemovalPhase::None,
            kind: None,
            uploaded_count: 0,
            uploaded_by_kind: BTreeMap::new(),
            dropped_count: 0,
            dropped_by_kind: BTreeMap::new(),
            last_error: None,
            wipe_confirmed: None,
        }
    }

    fn of(removal: &PendingRemoval, phase: RemovalPhase) -> Self {
        Self {
            phase,
            kind: Some(removal.kind),
            uploaded_count: removal.uploaded_count,
            uploaded_by_kind: removal.uploaded_by_kind.clone(),
            dropped_count: removal.dropped_by_kind.values().sum(),
            dropped_by_kind: removal.dropped_by_kind.clone(),
            last_error: None,
            wipe_confirmed: None,
        }
    }
}

// ── Procedure ───────────────────────────────────────────────────────────────────────────────────

/// Acknowledged per kind during one round.
pub(crate) type Uploaded = BTreeMap<String, i64>;

/// What one upload round (an exchange, then files) did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UploadRound {
    /// Nothing unsent is left.
    Empty { uploaded: Uploaded },
    /// Acknowledged; more remains.
    More { uploaded: Uploaded },
    /// No acknowledgement for the rest: data kept, retried later.
    Failed {
        uploaded: Uploaded,
        error: SyncWorkerError,
        retry_after: Option<std::time::Duration>,
    },
    /// The server answered a refusal meaning `kind` ([`kind_of_refusal`]).
    Refused {
        uploaded: Uploaded,
        kind: RemovalKind,
        error: Option<SyncWorkerError>,
    },
}

/// What the server says of this phone when asked ([`RemovalSteps::probe`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    /// The server accepts this phone's activation again.
    Accepted,
    /// A refusal meaning this kind.
    Refused(RemovalKind),
    /// No answer, or another answer: nothing changes.
    Unanswered(SyncWorkerError),
}

/// The steps the procedure orders. The sync worker carries them out; tests record their order.
pub(crate) trait RemovalSteps {
    /// Signs the current user out.
    async fn end_session(&mut self);
    /// Asks the server what this phone is (a locked phone, `RevokedUnconfirmed`).
    async fn probe(&mut self) -> Probe;
    /// Puts the outbox items and files given up before back to be sent. How many.
    async fn requeue_failures(&mut self) -> AppResult<i64>;
    async fn upload_round(&mut self, kind: RemovalKind) -> AppResult<UploadRound>;
    /// How much is still to be sent (items, captured changes, a stored request, files).
    async fn unsent_remaining(&mut self) -> AppResult<i64>;
    /// What is still unsent or will never be sent, per kind.
    async fn leftover(&mut self) -> AppResult<BTreeMap<String, i64>>;
    async fn erase(&mut self) -> AppResult<()>;
    async fn regenerate_identity(&mut self) -> AppResult<()>;
    /// `POST /api/v1/mobile-device/wipe-confirmed` with `old_token` and the dropped counts
    /// ([`wipe_confirmation_body`]): whether the server took it.
    async fn confirm_wipe(&mut self, old_token: &str, dropped: &BTreeMap<String, i64>) -> bool;
    fn publish(&mut self, status: PhoneRemovalStatus);
}

/// How an attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RemovalOutcome {
    /// Erased (and confirmed when it applies).
    Done(PhoneRemovalStatus),
    /// The upload is not finished: data kept, the phone stays locked. `error` when it failed.
    Waiting {
        error: Option<SyncWorkerError>,
        retry_after: Option<std::time::Duration>,
    },
    /// Refused without the erase marker: locked, data kept, asked again at the next re-check.
    Blocked { error: Option<SyncWorkerError> },
    /// The server accepts this phone again: the lock is lifted, nothing was erased.
    Lifted,
}

/// One attempt of the removal recorded in `removal` (see the module comment for the order).
/// `old_token` is the device token read before anything is erased; it is used for the wipe
/// confirmation only and dropped when the attempt ends.
pub(crate) async fn run_removal(
    db: &DatabaseConnection,
    steps: &mut impl RemovalSteps,
    mut removal: PendingRemoval,
    old_token: Option<String>,
) -> AppResult<RemovalOutcome> {
    steps.end_session().await;
    if !removal.erased {
        if removal.kind == RemovalKind::RevokedUnconfirmed {
            match steps.probe().await {
                Probe::Accepted => {
                    clear(db, "phone lock lifted: the server accepts it again").await?;
                    steps.publish(PhoneRemovalStatus::none());
                    tracing::warn!(target: "maintafox::sync", "phone lock lifted: the server accepts this phone again; nothing was erased");
                    return Ok(RemovalOutcome::Lifted);
                }
                Probe::Refused(kind) if kind.supersedes(removal.kind) => {
                    tracing::warn!(target: "maintafox::sync", kind = ?kind, "locked phone: the server now says what it is");
                    removal.kind = kind;
                    save(db, &removal, "phone removal recorded").await?;
                }
                Probe::Refused(_) => return blocked(db, steps, &removal, None).await,
                Probe::Unanswered(error) => return blocked(db, steps, &removal, Some(error)).await,
            }
        }
        if removal.kind != RemovalKind::Revoked {
            if !removal.failures_requeued {
                let requeued = steps.requeue_failures().await?;
                removal.failures_requeued = true;
                save(db, &removal, "phone removal: given-up work put back").await?;
                if requeued > 0 {
                    tracing::warn!(target: "maintafox::sync", requeued, "phone removal: items and files given up before are sent again");
                }
            }
            if let Some(outcome) = upload(db, steps, &mut removal).await? {
                return Ok(outcome);
            }
        }
        // Work captured since the last round goes first, unless the server said this phone may
        // upload nothing.
        if removal.kind != RemovalKind::Revoked {
            let unsent = steps.unsent_remaining().await?;
            if unsent > 0 {
                save(db, &removal, "phone removal upload progress").await?;
                tracing::warn!(target: "maintafox::sync", unsent, "phone removal: new work before the erase; uploading it first");
                return Ok(RemovalOutcome::Waiting {
                    error: None,
                    retry_after: None,
                });
            }
        }
        removal.dropped_by_kind = steps.leftover().await?;
        for (kind, count) in &removal.dropped_by_kind {
            tracing::warn!(
                target: "maintafox::sync",
                kind = kind.as_str(),
                count,
                removal = ?removal.kind,
                "phone removal: unsent work dropped with the erase"
            );
        }
        save(db, &removal, "phone removal: erasing").await?;
        steps.publish(PhoneRemovalStatus::of(&removal, RemovalPhase::Erasing));
        steps.erase().await?;
        removal.erased = true;
        save(db, &removal, "phone removal: erased").await?;
    }
    // A new installation identity before the record goes: a phone that stops here regenerates it
    // at the next attempt, and cannot be enrolled meanwhile (the record refuses it).
    steps.regenerate_identity().await?;
    clear(db, "phone removal finished").await?;
    let wipe_confirmed = match (removal.kind, old_token) {
        (RemovalKind::Removed, Some(token)) => {
            let confirmed = steps.confirm_wipe(&token, &removal.dropped_by_kind).await;
            drop(token);
            Some(confirmed)
        }
        // After a restart past the erase the token is gone: the server's window expires instead.
        (RemovalKind::Removed, None) => Some(false),
        _ => None,
    };
    let mut status = PhoneRemovalStatus::of(&removal, RemovalPhase::Done);
    status.wipe_confirmed = wipe_confirmed;
    tracing::warn!(
        target: "maintafox::sync",
        kind = ?removal.kind,
        uploaded = status.uploaded_count,
        dropped = status.dropped_count,
        wipe_confirmed = ?wipe_confirmed,
        "phone removal finished: local data erased"
    );
    steps.publish(status.clone());
    Ok(RemovalOutcome::Done(status))
}

/// The upload rounds of one attempt (Removed, Unbound). `None` when nothing is left to send (or
/// the server's marker says nothing more may be: `removal.kind` is then Revoked); otherwise how
/// the attempt ends.
async fn upload(
    db: &DatabaseConnection,
    steps: &mut impl RemovalSteps,
    removal: &mut PendingRemoval,
) -> AppResult<Option<RemovalOutcome>> {
    steps.publish(PhoneRemovalStatus::of(removal, RemovalPhase::Uploading));
    for _ in 0..REMOVAL_UPLOAD_ROUNDS_PER_ATTEMPT {
        match steps.upload_round(removal.kind).await? {
            UploadRound::Empty { uploaded } => {
                removal.add_uploaded(&uploaded);
                return Ok(None);
            }
            UploadRound::More { uploaded } => {
                removal.add_uploaded(&uploaded);
                save(db, removal, "phone removal upload progress").await?;
                steps.publish(PhoneRemovalStatus::of(removal, RemovalPhase::Uploading));
            }
            UploadRound::Failed {
                uploaded,
                error,
                retry_after,
            } => {
                removal.add_uploaded(&uploaded);
                return waiting(db, steps, removal, error, retry_after).await.map(Some);
            }
            UploadRound::Refused { uploaded, kind, error } => {
                removal.add_uploaded(&uploaded);
                match kind {
                    RemovalKind::Revoked => {
                        tracing::warn!(target: "maintafox::sync", "phone removal: the server's erase marker; erasing without uploading the rest");
                        removal.kind = RemovalKind::Revoked;
                        return Ok(None);
                    }
                    RemovalKind::RevokedUnconfirmed => {
                        removal.kind = RemovalKind::RevokedUnconfirmed;
                        return blocked(db, steps, removal, error).await.map(Some);
                    }
                    RemovalKind::Removed if removal.kind == RemovalKind::Unbound => {
                        tracing::warn!(target: "maintafox::sync", "phone removal: the unbound phone was removed by its administrator");
                        removal.kind = RemovalKind::Removed;
                        save(db, removal, "phone removal recorded").await?;
                    }
                    // Nothing new (the same cause again): an answer without acknowledgement.
                    _ => {
                        let error = error.unwrap_or_else(|| refusal_error(kind));
                        return waiting(db, steps, removal, error, None).await.map(Some);
                    }
                }
            }
        }
    }
    // More to upload than one attempt sends: the next attempt starts at once.
    save(db, removal, "phone removal upload progress").await?;
    Ok(Some(RemovalOutcome::Waiting {
        error: None,
        retry_after: None,
    }))
}

async fn waiting(
    db: &DatabaseConnection,
    steps: &mut impl RemovalSteps,
    removal: &PendingRemoval,
    error: SyncWorkerError,
    retry_after: Option<std::time::Duration>,
) -> AppResult<RemovalOutcome> {
    save(db, removal, "phone removal upload progress").await?;
    let mut status = PhoneRemovalStatus::of(removal, RemovalPhase::WaitingToUpload);
    status.last_error = Some(error.clone());
    steps.publish(status);
    tracing::warn!(
        target: "maintafox::sync",
        code = error.code.as_str(),
        "phone removal: upload failed; data kept, retried later"
    );
    Ok(RemovalOutcome::Waiting {
        error: Some(error),
        retry_after,
    })
}

async fn blocked(
    db: &DatabaseConnection,
    steps: &mut impl RemovalSteps,
    removal: &PendingRemoval,
    error: Option<SyncWorkerError>,
) -> AppResult<RemovalOutcome> {
    save(db, removal, "phone locked: refused without the erase marker").await?;
    let mut status = PhoneRemovalStatus::of(removal, RemovalPhase::Blocked);
    status.last_error = error.clone();
    steps.publish(status);
    tracing::warn!(
        target: "maintafox::sync",
        "phone locked: the server refused it without saying it may be erased; data kept, asked again later"
    );
    Ok(RemovalOutcome::Blocked { error })
}

fn refusal_error(kind: RemovalKind) -> SyncWorkerError {
    let code = match kind {
        RemovalKind::Removed => "device_wipe_required",
        _ => "activation_revoked",
    };
    SyncWorkerError {
        code: code.to_string(),
        kind: super::worker::error_kind(code, Some(403)),
        message: code.to_string(),
        http_status: Some(403),
        at: crate::sync::transport::store::now_utc(),
    }
}

/// The wipe confirmation body (`POST /api/v1/mobile-device/wipe-confirmed`): `{}` when nothing
/// was dropped, otherwise `{ "dropped": { "total", "by_kind" } }` within the server's limits (at
/// most 32 kinds of 1 to 64 characters: the smallest counts are summed under `other`).
pub(crate) fn wipe_confirmation_body(dropped: &BTreeMap<String, i64>) -> serde_json::Value {
    let mut kinds: Vec<(String, i64)> = Vec::new();
    let mut other = 0_i64;
    for (kind, n) in dropped {
        let n = (*n).max(0);
        if n == 0 {
            continue;
        }
        if kind.is_empty() || kind.chars().count() > WIPE_CONFIRMATION_KIND_MAX_CHARS || kind == OTHER_KINDS {
            other += n;
        } else {
            kinds.push((kind.clone(), n));
        }
    }
    if kinds.is_empty() && other == 0 {
        return serde_json::json!({});
    }
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let room = if other > 0 || kinds.len() > WIPE_CONFIRMATION_MAX_KINDS {
        WIPE_CONFIRMATION_MAX_KINDS - 1
    } else {
        WIPE_CONFIRMATION_MAX_KINDS
    };
    let mut by_kind = serde_json::Map::new();
    for (i, (kind, n)) in kinds.into_iter().enumerate() {
        if i < room {
            by_kind.insert(kind, serde_json::Value::from(n));
        } else {
            other += n;
        }
    }
    if other > 0 {
        by_kind.insert(OTHER_KINDS.to_string(), serde_json::Value::from(other));
    }
    let total: i64 = by_kind.values().filter_map(serde_json::Value::as_i64).sum();
    serde_json::json!({ "dropped": { "total": total, "by_kind": by_kind } })
}

/// This phone's work per kind (entity type, or [`MEDIA_FILES_KIND`]): what is still to be sent,
/// and what was given up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WorkCounts {
    /// Outbox items pending or in flight, captured changes not drained yet, files not uploaded.
    pub unsent: BTreeMap<String, i64>,
    /// Outbox items dead-lettered or rejected by the server, files given up.
    pub given_up: BTreeMap<String, i64>,
}

impl WorkCounts {
    pub fn unsent_total(&self) -> i64 {
        self.unsent.values().sum()
    }

    /// Unsent records (files excluded).
    pub fn unsent_records(&self) -> i64 {
        self.unsent_total() - self.unsent_files()
    }

    pub fn unsent_files(&self) -> i64 {
        self.unsent.get(MEDIA_FILES_KIND).copied().unwrap_or(0)
    }

    /// What reached the server between `self` and `after`, per kind: the unsent that went, less
    /// what was given up meanwhile.
    pub fn uploaded_until(&self, after: &WorkCounts) -> Uploaded {
        let mut uploaded = Uploaded::new();
        for (kind, before) in &self.unsent {
            let still = after.unsent.get(kind).copied().unwrap_or(0);
            let given_up =
                after.given_up.get(kind).copied().unwrap_or(0) - self.given_up.get(kind).copied().unwrap_or(0);
            let n = before - still - given_up.max(0);
            if n > 0 {
                uploaded.insert(kind.clone(), n);
            }
        }
        uploaded
    }

    /// Everything that will not reach the server, per kind.
    pub fn leftover(&self) -> BTreeMap<String, i64> {
        let mut all = self.unsent.clone();
        for (kind, n) in &self.given_up {
            *all.entry(kind.clone()).or_insert(0) += n;
        }
        all.retain(|_, n| *n > 0);
        all
    }
}

async fn counts_by_kind(db: &impl ConnectionTrait, sql: &str) -> AppResult<BTreeMap<String, i64>> {
    let rows = db
        .query_all(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
        .await?;
    rows.iter()
        .map(|r| Ok((r.try_get::<String>("", "kind")?, r.try_get::<i64>("", "n")?)))
        .collect()
}

/// [`WorkCounts`] of this phone.
pub(crate) async fn work_by_kind(db: &impl ConnectionTrait) -> AppResult<WorkCounts> {
    let unsent = counts_by_kind(
        db,
        "SELECT kind, SUM(n) AS n FROM (
             SELECT entity_type AS kind, COUNT(*) AS n FROM sync_outbox
              WHERE status IN ('pending', 'in_flight') GROUP BY entity_type
             UNION ALL
             SELECT entity_type AS kind,
                    COUNT(DISTINCT entity_type || ':' || COALESCE(entity_sync_id, natural_key_json)) AS n
               FROM sync_capture_journal GROUP BY entity_type
             UNION ALL
             SELECT 'media_files' AS kind, COUNT(*) AS n FROM sync_media_uploads WHERE status = 'pending'
         ) GROUP BY kind HAVING SUM(n) > 0",
    )
    .await?;
    let given_up = counts_by_kind(
        db,
        "SELECT kind, SUM(n) AS n FROM (
             SELECT entity_type AS kind, COUNT(*) AS n FROM sync_outbox
              WHERE status IN ('dead_letter', 'rejected') GROUP BY entity_type
             UNION ALL
             SELECT 'media_files' AS kind, COUNT(*) AS n FROM sync_media_uploads WHERE status = 'failed'
         ) GROUP BY kind HAVING SUM(n) > 0",
    )
    .await?;
    Ok(WorkCounts { unsent, given_up })
}

/// Unsent or given-up work per kind: outbox items not acknowledged (pending, in flight,
/// dead-lettered, rejected by the server), captured changes not drained into the outbox yet, and
/// files not uploaded (`media_files`).
pub(crate) async fn unsent_by_kind(db: &impl ConnectionTrait) -> AppResult<BTreeMap<String, i64>> {
    Ok(work_by_kind(db).await?.leftover())
}

#[cfg(test)]
#[path = "removal_tests.rs"]
mod tests;
