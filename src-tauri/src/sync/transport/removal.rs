//! A phone that may no longer work for its plant uploads what it still holds, then erases itself.
//!
//! Three causes, recorded as a [`PendingRemoval`] (device setting, kept across restarts):
//! - [`RemovalKind::Removed`]: its administrator removed it (licence server 403
//!   `device_wipe_required`). For 168 h the server still takes this phone's push-only sync
//!   exchanges and its wipe confirmation, and refuses everything else.
//! - [`RemovalKind::Revoked`]: reported lost, or removed and the window passed (403
//!   `activation_revoked` on a phone). Nothing can be uploaded any more.
//! - [`RemovalKind::Unbound`]: activated with the licence key before enrolment existed, so it is
//!   bound to nobody (`auth::phone_binding::unbound_phone_policy`). Its server still takes normal
//!   exchanges.
//!
//! The procedure ([`run_removal`], run by the sync worker, Android only):
//! 1. The session is ended; while a removal is pending nobody signs in (`auth::phone_binding`).
//! 2. Removed / Unbound: exchanges until the outbox is empty, the server having acknowledged each
//!    one. An exchange that fails stops the attempt: the data is kept and the worker retries with
//!    its backoff, for as long as it takes (never erased on a timer). Revoked: nothing is sent.
//! 3. What will not reach the server is logged per kind (Revoked: everything unsent; otherwise
//!    items the server refused for good, and files the server no longer takes from a removed
//!    phone).
//! 4. Local data erased (`reset_to_first_run_in` and the synchronised files), then the
//!    installation identity regenerated: the next enrolment is a new phone for the server.
//! 5. Removed only: the wipe confirmed with the OLD device token, held in memory since step 2 and
//!    dropped right after, whether or not the confirmation got through (the server's window then
//!    simply expires). Never before step 4.
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

/// Why this phone erases itself. Ordered: a later cause replaces an earlier one, never the
/// reverse (a revoked phone can upload nothing, whatever was recorded before).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalKind {
    /// Activated with the licence key, bound to nobody: uploads, then resets for an enrolment.
    Unbound,
    /// Removed by its administrator: uploads (push only), erases, confirms.
    Removed,
    /// Lost, or removed past the server's window: erases without uploading.
    Revoked,
}

/// The removal under way on this phone (device setting [`REMOVAL_KEY`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRemoval {
    pub kind: RemovalKind,
    pub detected_at: String,
    /// Items the server acknowledged during the upload.
    #[serde(default)]
    pub uploaded_count: i64,
    /// What was not uploaded, per kind (entity type, or `media_files`), counted before the erase.
    #[serde(default)]
    pub dropped_by_kind: BTreeMap<String, i64>,
    /// The local data is erased; what remains is the new identity and the confirmation.
    #[serde(default)]
    pub erased: bool,
}

/// Whether this build erases itself when removed: the Android field app only. A desktop keeps
/// its data; a refused desktop stays blocked until its licence is put right.
pub const fn applies_on_this_device() -> bool {
    cfg!(target_os = "android")
}

/// The removal a licence server refusal means for a phone, if any.
pub fn kind_of_refusal(status: u16, code: Option<&str>) -> Option<RemovalKind> {
    match (status, code) {
        (403, Some("device_wipe_required")) => Some(RemovalKind::Removed),
        (403, Some("activation_revoked")) => Some(RemovalKind::Revoked),
        _ => None,
    }
}

/// [`kind_of_refusal`] of a transport error.
pub fn kind_of_transport_error(err: &TransportError) -> Option<RemovalKind> {
    match err {
        TransportError::Http { status, code, .. } => kind_of_refusal(*status, code.as_deref()),
        _ => None,
    }
}

pub async fn pending(db: &impl ConnectionTrait) -> AppResult<Option<PendingRemoval>> {
    let Some(row) = crate::settings::get_setting(db, REMOVAL_KEY, REMOVAL_SCOPE).await? else {
        return Ok(None);
    };
    match serde_json::from_str(&row.setting_value_json) {
        Ok(removal) => Ok(Some(removal)),
        // Unreadable: the phone is never left half-removed. Read as a removal by the administrator:
        // the pending work is offered to the server, which refuses it if the phone is revoked.
        Err(e) => {
            tracing::error!(target: "maintafox::sync", error = %e, "phone removal record unreadable; treated as removed");
            Ok(Some(PendingRemoval {
                kind: RemovalKind::Removed,
                detected_at: crate::sync::transport::store::now_utc(),
                uploaded_count: 0,
                dropped_by_kind: BTreeMap::new(),
                erased: false,
            }))
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

async fn clear(db: &DatabaseConnection) -> AppResult<()> {
    crate::settings::delete_setting(db, REMOVAL_KEY, REMOVAL_SCOPE, None, "phone removal finished").await?;
    Ok(())
}

/// Records that this phone must erase itself for `kind`, keeping a stronger cause already recorded
/// (see [`RemovalKind`]). Returns the removal now pending.
pub async fn record(db: &DatabaseConnection, kind: RemovalKind) -> AppResult<PendingRemoval> {
    let current = pending(db).await?;
    if let Some(current) = &current {
        if current.kind >= kind {
            return Ok(current.clone());
        }
    }
    let removal = match current {
        Some(mut current) => {
            current.kind = kind;
            current
        }
        None => PendingRemoval {
            kind,
            detected_at: crate::sync::transport::store::now_utc(),
            uploaded_count: 0,
            dropped_by_kind: BTreeMap::new(),
            erased: false,
        },
    };
    tracing::warn!(target: "maintafox::sync", kind = ?kind, "phone removal recorded");
    save(db, &removal, "phone removal recorded").await?;
    Ok(removal)
}

/// Records the removal a licence server refusal observed in Rust means, on a phone. A failure to
/// record is logged (the next refusal records it again).
pub async fn note_refusal(db: &DatabaseConnection, status: u16, code: Option<&str>) {
    if !applies_on_this_device() {
        return;
    }
    if let Some(kind) = kind_of_refusal(status, code) {
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
    /// Erasing the local data.
    Erasing,
    /// Erased; the phone is back at its first start (enrolment).
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhoneRemovalStatus {
    pub phase: RemovalPhase,
    pub kind: Option<RemovalKind>,
    /// Items the server acknowledged during the final upload.
    pub uploaded_count: i64,
    /// Items that will not reach the server (lost, refused for good, or files a removed phone may
    /// no longer send), and the same per kind.
    pub dropped_count: i64,
    pub dropped_by_kind: BTreeMap<String, i64>,
    /// Why the last upload attempt failed (`WaitingToUpload`).
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
            dropped_count: removal.dropped_by_kind.values().sum(),
            dropped_by_kind: removal.dropped_by_kind.clone(),
            last_error: None,
            wipe_confirmed: None,
        }
    }
}

// ── Procedure ───────────────────────────────────────────────────────────────────────────────────

/// What one upload exchange did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UploadRound {
    /// Nothing unsent is left; `uploaded` items acknowledged by this round.
    Empty { uploaded: i64 },
    /// Acknowledged; more remains.
    More { uploaded: i64 },
    /// No acknowledgement: data kept, retried later.
    Failed {
        error: SyncWorkerError,
        retry_after: Option<std::time::Duration>,
    },
    /// The server takes nothing more from this phone (`activation_revoked`), or no device token is
    /// left to send with.
    Revoked,
}

/// The steps the procedure orders. The sync worker carries them out; tests record their order.
pub(crate) trait RemovalSteps {
    /// Signs the current user out.
    async fn end_session(&mut self);
    async fn upload_round(&mut self, kind: RemovalKind) -> AppResult<UploadRound>;
    /// What is still unsent or will never be sent, per kind.
    async fn leftover(&mut self) -> AppResult<BTreeMap<String, i64>>;
    async fn erase(&mut self) -> AppResult<()>;
    async fn regenerate_identity(&mut self) -> AppResult<()>;
    /// `POST /api/v1/mobile-device/wipe-confirmed` with `old_token`: whether the server took it.
    async fn confirm_wipe(&mut self, old_token: &str) -> bool;
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
        if removal.kind != RemovalKind::Revoked {
            steps.publish(PhoneRemovalStatus::of(&removal, RemovalPhase::Uploading));
            let mut emptied = false;
            for _ in 0..REMOVAL_UPLOAD_ROUNDS_PER_ATTEMPT {
                match steps.upload_round(removal.kind).await? {
                    UploadRound::Empty { uploaded } => {
                        removal.uploaded_count += uploaded;
                        emptied = true;
                        break;
                    }
                    UploadRound::More { uploaded } => {
                        removal.uploaded_count += uploaded;
                        save(db, &removal, "phone removal upload progress").await?;
                        steps.publish(PhoneRemovalStatus::of(&removal, RemovalPhase::Uploading));
                    }
                    UploadRound::Failed { error, retry_after } => {
                        save(db, &removal, "phone removal upload progress").await?;
                        let mut status = PhoneRemovalStatus::of(&removal, RemovalPhase::WaitingToUpload);
                        status.last_error = Some(error.clone());
                        steps.publish(status);
                        tracing::warn!(
                            target: "maintafox::sync",
                            code = error.code.as_str(),
                            "phone removal: upload failed; data kept, retried later"
                        );
                        return Ok(RemovalOutcome::Waiting {
                            error: Some(error),
                            retry_after,
                        });
                    }
                    UploadRound::Revoked => {
                        tracing::warn!(target: "maintafox::sync", "phone removal: the server takes nothing more from this phone; erasing without uploading the rest");
                        removal.kind = RemovalKind::Revoked;
                        emptied = true;
                        break;
                    }
                }
            }
            if !emptied {
                save(db, &removal, "phone removal upload progress").await?;
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
    clear(db).await?;
    let wipe_confirmed = match (removal.kind, old_token) {
        (RemovalKind::Removed, Some(token)) => {
            let confirmed = steps.confirm_wipe(&token).await;
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

/// Unsent work per kind: outbox items not acknowledged (pending, in flight, given up), captured
/// changes not drained into the outbox yet, and files not uploaded (`media_files`).
pub(crate) async fn unsent_by_kind(db: &impl ConnectionTrait) -> AppResult<BTreeMap<String, i64>> {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT kind, SUM(n) AS n FROM (
                 SELECT entity_type AS kind, COUNT(*) AS n FROM sync_outbox
                  WHERE status IN ('pending', 'in_flight', 'dead_letter') GROUP BY entity_type
                 UNION ALL
                 SELECT entity_type AS kind,
                        COUNT(DISTINCT entity_type || ':' || COALESCE(entity_sync_id, natural_key_json)) AS n
                   FROM sync_capture_journal GROUP BY entity_type
                 UNION ALL
                 SELECT 'media_files' AS kind, COUNT(*) AS n FROM sync_media_uploads
                  WHERE status IN ('pending', 'failed')
             ) GROUP BY kind HAVING SUM(n) > 0"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| Ok((r.try_get::<String>("", "kind")?, r.try_get::<i64>("", "n")?)))
        .collect()
}

#[cfg(test)]
#[path = "removal_tests.rs"]
mod tests;
