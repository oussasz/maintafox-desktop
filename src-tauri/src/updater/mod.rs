//! In-app updates (master plan Phase 5.5).
//!
//! - **Check:** the control API's manifest `GET /api/v1/updates/{target}/{arch}/{version}` with this
//!   device's activation token. The server decides (ring, rollout %, pause, recall, minimum
//!   version) and answers in the Tauri updater format plus a `maintafox` object: upgrade or
//!   rollback, mandatory or not, notes in French and English.
//! - **Install:** the manifest is checked again (a recall or pause stops the install, and an offer
//!   other than the one the administrator confirmed is never installed), the signed installer is
//!   downloaded with progress and verified against the public key built into this app, and only
//!   then the database is backed up (`pre_update` run, Phase 1.3 backup service) or its restore
//!   staged, the journal (`journal.rs`) written and the installer started at once (the app
//!   restarts). From the backup on, the app accepts no change (`WriteBarrier`: the window shows a
//!   blocking screen, session commands are refused, the sync worker pauses), so a change made
//!   during the download is in the backup. A save already running when the barrier went up can
//!   still finish during the backup (milliseconds).
//! - **Failed database update:** migrations are forward-only. When the new version cannot update
//!   the database, it stages the pre-update backup (Phase 1.3 restore) and asks for the previous
//!   signed installer (`X-Maintafox-Rollback-To`); the previous version starts on the backup.
//! - **Recall of the running version:** the server offers the previous release (kind rollback).
//!   If the database schema did not change since that version, it is kept; otherwise the
//!   pre-update backup is restored, only when nothing is waiting to be synchronised (changes made
//!   since the update come back from the server's snapshot after the restore).
//!
//! Builds without the owner's updater public key (`build.rs`) have no updater plugin at all and
//! say so (`not_configured`); nothing pretends to check.

pub mod journal;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::errors::{AppError, AppResult};
use journal::{JournalState, StartupAction, UpdateJournal, UpdateKind};

/// The owner's updater public key, embedded by `build.rs` (None: this build has no updater).
pub const UPDATER_PUBLIC_KEY: Option<&str> = option_env!("MAINTAFOX_UPDATER_PUBKEY");

/// In-app updates: desktop builds that carry the owner's updater public key. Never the phone app
/// (its updates come with a new APK; the plugin's HTTPS client relies on the platform certificate
/// verifier, which the Android build does not initialise).
pub fn updater_enabled() -> bool {
    UPDATER_PUBLIC_KEY.is_some() && !cfg!(mobile)
}
/// Header asking the manifest for the version this device came from (failed database update).
pub const ROLLBACK_HEADER: &str = "X-Maintafox-Rollback-To";
/// NSIS update mode: progress window only, no questions (the user already confirmed in the app).
const WINDOWS_INSTALL_MODE: &str = "basicUi";

// ── Plugin registration ─────────────────────────────────────────────────────────────────────

/// Adds the updater plugin configuration to the app context when this build has a public key.
/// The key is not in tauri.conf.json: it comes from the owner's key file (no placeholder).
pub fn add_plugin_config<R: Runtime>(context: &mut tauri::Context<R>) {
    if let Some(pubkey) = UPDATER_PUBLIC_KEY.filter(|_| updater_enabled()) {
        context.config_mut().plugins.0.insert(
            "updater".to_string(),
            serde_json::json!({ "pubkey": pubkey, "windows": { "installMode": WINDOWS_INSTALL_MODE } }),
        );
    }
}

/// Registers the updater plugin only when in-app updates are enabled ([`updater_enabled`]).
pub fn register_plugin<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    if updater_enabled() {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        tracing::warn!("updater: this build has no updater public key; in-app updates are disabled");
        builder
    }
}

// ── Manifest ────────────────────────────────────────────────────────────────────────────────

/// Endpoint template given to the Tauri updater (it fills target, arch and current version).
pub fn manifest_endpoint(api_base_url: &str) -> AppResult<tauri::Url> {
    let base = api_base_url.trim().trim_end_matches('/');
    tauri::Url::parse(&format!(
        "{base}/api/v1/updates/{{{{target}}}}/{{{{arch}}}}/{{{{current_version}}}}"
    ))
    .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid control API URL {base}: {e}")))
}

/// The `maintafox` object the control API adds to the Tauri manifest (vendor-console
/// `routes/updates.ts`). `mandatory` equals the licence policy refresh's `force_update_required`
/// for the same version.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestExtension {
    /// Always present.
    pub release_id: String,
    pub kind: UpdateKind,
    pub mandatory: bool,
    /// Why the update is mandatory, as a code (`forced_update_floor`, `emergency`,
    /// `rollback_requested`, `recalled_version`; the UI translates it), null when optional.
    pub mandatory_reason: Option<String>,
    /// Optional free text from the operator, shown as plain text.
    #[serde(default)]
    pub mandatory_message: Option<String>,
    /// The floor as the operator typed it (e.g. "1.0") when the floor makes it mandatory.
    pub min_version: Option<String>,
    pub notes_en: String,
    pub notes_fr: String,
}

pub fn parse_extension(raw: &serde_json::Value) -> AppResult<ManifestExtension> {
    let ext = raw.get("maintafox").cloned().ok_or_else(|| {
        AppError::Internal(anyhow::anyhow!(
            "the update server answered without the maintafox release information"
        ))
    })?;
    serde_json::from_value(ext).map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
            "the update server's release information is invalid: {e}"
        ))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Available,
    UpToDate,
    /// This build has no updater public key.
    NotConfigured,
    /// The device has no activation yet: the server cannot tell which releases it may have.
    NotActivated,
    /// The update server could not be reached or answered wrongly (details in `message`).
    Unreachable,
}

/// Result of a check, for the UI (`UpdateCheckResult` in shared/ipc-types.ts).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckResult {
    pub status: CheckStatus,
    pub available: bool,
    pub current_version: String,
    pub version: Option<String>,
    /// The catalog release offered: the install names it back (`ConfirmedOffer`).
    pub release_id: Option<String>,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
    pub kind: Option<UpdateKind>,
    pub mandatory: bool,
    pub mandatory_reason: Option<String>,
    pub mandatory_message: Option<String>,
    pub notes_en: Option<String>,
    pub notes_fr: Option<String>,
    pub message: Option<String>,
}

impl UpdateCheckResult {
    fn without_update(status: CheckStatus, current_version: &str, message: Option<String>) -> Self {
        Self {
            status,
            available: false,
            current_version: current_version.to_string(),
            version: None,
            release_id: None,
            notes: None,
            pub_date: None,
            kind: None,
            mandatory: false,
            mandatory_reason: None,
            mandatory_message: None,
            notes_en: None,
            notes_fr: None,
            message,
        }
    }
}

/// The version this app runs: the one the licence's forced-update rule reads
/// (`product_license::running_app_version`). The updater plugin puts the Tauri package version in
/// the manifest URL; both are the one release version (`tauri.conf.json` and `Cargo.toml`, checked
/// equal by the release workflow and by `tests::one_version_source`).
pub fn running_version() -> String {
    crate::commands::product_license::running_app_version().to_string()
}

fn app_data_dir(app: &AppHandle) -> AppResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app data directory unavailable: {e}")))
}

/// Outcome of one manifest request.
pub enum ManifestAnswer {
    NotConfigured,
    NotActivated,
    /// 204: nothing to install.
    None,
    /// The server answered with a refusal status (e.g. 409 `rollback_target_not_allowed`, 400, 403):
    /// the updater plugin reports any non-success status as `ReleaseNotFound`.
    ServerRefused,
    /// The server offered a version this device does not accept (`accept_offer`); signed or not,
    /// it is never installed.
    NotAcceptable(String),
    Offer(Box<Update>, ManifestExtension),
}

/// Mandatory reason the server gives with the rollback offered off a recalled running version.
pub const REASON_RECALLED_VERSION: &str = "recalled_version";

/// Which offered version this device accepts (vendor-console PR #3, security review). With a
/// rollback request: exactly the requested version, kind rollback. Without: kind upgrade to a
/// higher version, or the server's rollback off a recalled running version (lower, kind rollback,
/// reason `recalled_version`). Any other version is refused, whatever its signature.
pub fn accept_offer(
    running: &str,
    offered: &str,
    ext: &ManifestExtension,
    rollback_to: Option<&str>,
) -> Result<(), String> {
    let parse = |v: &str| semver::Version::parse(v.trim()).map_err(|e| format!("unreadable version {v}: {e}"));
    let (run, off) = (parse(running)?, parse(offered)?);
    match rollback_to {
        Some(requested) => {
            if ext.kind != UpdateKind::Rollback {
                return Err(format!(
                    "a rollback to {requested} was requested, the server answered an upgrade"
                ));
            }
            if offered.trim() != requested.trim() {
                return Err(format!(
                    "a rollback to {requested} was requested, the server offered {offered}"
                ));
            }
            Ok(())
        }
        None => match ext.kind {
            UpdateKind::Upgrade if off > run => Ok(()),
            UpdateKind::Upgrade => Err(format!(
                "an upgrade must be newer than {running}, the server offered {offered}"
            )),
            UpdateKind::Rollback if off < run && ext.mandatory_reason.as_deref() == Some(REASON_RECALLED_VERSION) => {
                Ok(())
            }
            UpdateKind::Rollback => Err(format!(
                "an unrequested rollback to {offered} is accepted only off a recalled version older than {running}"
            )),
        },
    }
}

/// The installer name the release workflow signs and the control API requires for `version`.
pub fn installer_file_name(version: &str) -> String {
    format!("Maintafox_{}_x64-setup.exe", version.trim())
}

/// The file a Tauri updater signature was made for: the `file:` field of the minisign trusted
/// comment (`trusted comment: timestamp:<n>\tfile:<name>`; the signature is base64 of the minisign
/// text). The trusted comment is covered by the signature's global signature, which the updater
/// plugin verifies against this build's public key before it returns the downloaded bytes.
pub fn signed_file_name(signature_base64: &str) -> Result<String, String> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(signature_base64.trim())
        .map_err(|e| format!("the update signature is not base64: {e}"))?;
    let text = String::from_utf8(raw).map_err(|_| "the update signature is not text".to_string())?;
    let trusted = text
        .lines()
        .find_map(|line| line.strip_prefix("trusted comment:"))
        .ok_or_else(|| "the update signature has no trusted comment".to_string())?;
    trusted
        .trim()
        .split('\t')
        .find_map(|field| field.strip_prefix("file:"))
        .map(str::to_string)
        .ok_or_else(|| "the update signature does not name the signed file".to_string())
}

/// Binds the signature to the offered version (security review S2): a correctly signed installer
/// of another version offered under this version number is refused. Case-sensitive, exact.
pub fn check_signed_file(signature_base64: &str, offered: &str) -> Result<(), String> {
    let expected = installer_file_name(offered);
    let named = signed_file_name(signature_base64)?;
    if named == expected {
        Ok(())
    } else {
        Err(format!("the update signature was made for {named}, not for {expected}"))
    }
}

/// Asks the manifest. What it offers is checked by `accept_offer` and `check_signed_file` here,
/// and the installer's signature against this build's public key before anything is installed.
pub async fn ask_manifest(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    rollback_to: Option<&str>,
) -> AppResult<ManifestAnswer> {
    if !updater_enabled() {
        return Ok(ManifestAnswer::NotConfigured);
    }
    // An active licence, or one that requires an update (installing it ends that denial).
    let Some(token) = crate::commands::product_license::update_manifest_bearer_token(db)
        .await?
        .filter(|t| !t.trim().is_empty())
    else {
        return Ok(ManifestAnswer::NotActivated);
    };
    let endpoint = manifest_endpoint(env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL"))?;
    let language = if locale.is_some_and(|l| l.to_ascii_lowercase().starts_with("fr")) {
        "fr"
    } else {
        "en"
    };
    let request_err = |e: tauri_plugin_updater::Error| AppError::Internal(anyhow::anyhow!("updater request: {e}"));
    let mut builder = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(request_err)?
        .header("Authorization", format!("Bearer {token}"))
        .map_err(request_err)?
        .header("Accept-Language", language)
        .map_err(request_err)?
        .version_comparator(|current, remote| remote.version != current);
    if let Some(version) = rollback_to {
        builder = builder.header(ROLLBACK_HEADER, version).map_err(request_err)?;
    }
    let updater = builder
        .build()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("updater: {e}")))?;
    let answer = match updater.check().await {
        Ok(answer) => answer,
        // Any non-success status (409, 400, 403, 5xx): the plugin keeps no body or status.
        Err(tauri_plugin_updater::Error::ReleaseNotFound) => return Ok(ManifestAnswer::ServerRefused),
        Err(e) => return Err(AppError::Internal(anyhow::anyhow!("update server: {e}"))),
    };
    match answer {
        None => Ok(ManifestAnswer::None),
        Some(update) => {
            let ext = parse_extension(&update.raw_json)?;
            let accepted = accept_offer(&running_version(), &update.version, &ext, rollback_to)
                .and_then(|()| check_signed_file(&update.signature, &update.version));
            if let Err(why) = accepted {
                tracing::warn!(offered = %update.version, "update: offer refused: {why}");
                return Ok(ManifestAnswer::NotAcceptable(why));
            }
            Ok(ManifestAnswer::Offer(Box::new(update), ext))
        }
    }
}

pub async fn check(app: &AppHandle, db: &sea_orm::DatabaseConnection, locale: Option<&str>) -> UpdateCheckResult {
    let current = running_version();
    match ask_manifest(app, db, locale, None).await {
        Ok(ManifestAnswer::NotConfigured) => {
            UpdateCheckResult::without_update(CheckStatus::NotConfigured, &current, None)
        }
        Ok(ManifestAnswer::NotActivated) => {
            UpdateCheckResult::without_update(CheckStatus::NotActivated, &current, None)
        }
        Ok(ManifestAnswer::None) => UpdateCheckResult::without_update(CheckStatus::UpToDate, &current, None),
        Ok(ManifestAnswer::ServerRefused) => UpdateCheckResult::without_update(
            CheckStatus::Unreachable,
            &current,
            Some("the update server refused the request".to_string()),
        ),
        Ok(ManifestAnswer::NotAcceptable(why)) => {
            UpdateCheckResult::without_update(CheckStatus::Unreachable, &current, Some(why))
        }
        Ok(ManifestAnswer::Offer(update, ext)) => UpdateCheckResult {
            status: CheckStatus::Available,
            available: true,
            current_version: current,
            version: Some(update.version.clone()),
            release_id: Some(ext.release_id.clone()),
            notes: update.body.clone(),
            pub_date: update.date.map(|d| d.to_string()),
            kind: Some(ext.kind),
            mandatory: ext.mandatory,
            mandatory_reason: ext.mandatory_reason,
            mandatory_message: ext.mandatory_message,
            notes_en: Some(ext.notes_en),
            notes_fr: Some(ext.notes_fr),
            message: None,
        },
        Err(e) => {
            // The application stays usable without the update server; the UI shows why.
            tracing::warn!("update check failed: {e}");
            UpdateCheckResult::without_update(CheckStatus::Unreachable, &current, Some(e.to_string()))
        }
    }
}

// ── Downgrade safety ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DowngradePlan {
    /// No migration ran since `target`: the older version opens this database as it is.
    KeepDatabase,
    /// Migrations ran since `target`: its pre-update backup must be restored first. `sha256`: the
    /// checksum recorded when the backup was taken (checked again before it is staged).
    RestoreBackup { backup_path: PathBuf, sha256: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum InstallRefusal {
    /// No pre-update backup of `target` is known on this device, or no checksum was recorded
    /// for it.
    RollbackBackupMissing,
    /// The pre-update backup of `target` is there, but it changed since it was taken (checksum
    /// mismatch): it is never restored (security review N5).
    RollbackBackupChanged,
    /// A database restore is staged for the next start (an administrator's, Settings > Backup, or
    /// one the updater could not cancel): no update runs until Maintafox has restarted (N1).
    RestorePending,
    /// Changes not yet synchronised would be lost by the restore.
    RollbackUnsyncedChanges { count: i64 },
    /// The server no longer offers this update (recalled or paused since the check), offers
    /// something this device does not accept, or offers something other than what the
    /// administrator confirmed (they are asked again).
    UpdateWithdrawn,
    /// The rollback after a failed database update was refused (none offered, a refusal status,
    /// another version): keep this PC as it is and contact the vendor.
    RollbackRefused,
}

/// Decides how to go back from `running` to the older `target` (forward-only migrations: an
/// older version cannot open a database migrated by a newer one).
pub fn plan_downgrade(
    journal: Option<&UpdateJournal>,
    running: &str,
    target: &str,
    current_schema_version: i64,
    unsynced_changes: i64,
) -> Result<DowngradePlan, InstallRefusal> {
    let upgrade = journal.filter(|j| {
        j.kind == UpdateKind::Upgrade
            && j.state == JournalState::Completed
            && j.from_version == target
            && j.to_version == running
    });
    let Some(j) = upgrade else {
        return Err(InstallRefusal::RollbackBackupMissing);
    };
    if j.schema_version_before == Some(current_schema_version) {
        return Ok(DowngradePlan::KeepDatabase);
    }
    let Some(path) = j.backup_path.as_ref().map(PathBuf::from).filter(|p| p.is_file()) else {
        return Err(InstallRefusal::RollbackBackupMissing);
    };
    // A backup whose checksum was not recorded cannot be shown to be the one that was taken.
    let Some(sha256) = j.backup_sha256.clone().filter(|s| !s.trim().is_empty()) else {
        return Err(InstallRefusal::RollbackBackupMissing);
    };
    if unsynced_changes > 0 {
        return Err(InstallRefusal::RollbackUnsyncedChanges {
            count: unsynced_changes,
        });
    }
    Ok(DowngradePlan::RestoreBackup {
        backup_path: path,
        sha256,
    })
}

/// Whether the file at `path` is the backup that was taken (the checksum recorded then).
pub fn backup_matches_checksum(path: &Path, expected_sha256: &str) -> AppResult<bool> {
    let actual = crate::backup::file_sha256(path)?;
    Ok(actual.eq_ignore_ascii_case(expected_sha256.trim()))
}

pub async fn applied_migration_count(db: &sea_orm::DatabaseConnection) -> AppResult<i64> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM seaql_migrations".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("seaql_migrations count returned no row")))?;
    Ok(row.try_get::<i64>("", "cnt")?)
}

/// Local changes not yet synchronised (captured, pending or in flight): a restore would lose them.
async fn unsynced_change_count(db: &sea_orm::DatabaseConnection) -> AppResult<i64> {
    let summary = crate::sync::queries::get_sync_state_summary(db).await?;
    Ok(summary.pending_outbox_count + summary.in_flight_outbox_count)
}

// ── Install ─────────────────────────────────────────────────────────────────────────────────

/// Progress sent to the window while installing (`UpdateProgressEvent` in shared/ipc-types.ts).
/// Order: downloading, then backing up (upgrade) or restoring the backup (rollback), then
/// installing.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum UpdateProgressEvent {
    BackingUp,
    RestoringBackup,
    Downloading {
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Installing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum InstallOutcome {
    /// The installer runs; this process exits.
    Restarting,
    Refused {
        refusal: InstallRefusal,
    },
}

/// The offer the administrator saw and confirmed (the last check's `version`, `kind` and
/// `release_id`). The install re-asks the manifest and installs only this offer: anything else
/// (another version, a rollback instead of an upgrade, another catalog release) is refused with
/// `UpdateWithdrawn`, and the UI shows the new offer for a new confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConfirmedOffer {
    pub version: String,
    pub kind: UpdateKind,
    pub release_id: String,
}

/// Whether what the manifest offers now is exactly what the administrator confirmed.
pub fn offer_matches_confirmation(offered_version: &str, ext: &ManifestExtension, confirmed: &ConfirmedOffer) -> bool {
    offered_version.trim() == confirmed.version.trim()
        && ext.kind == confirmed.kind
        && ext.release_id == confirmed.release_id
}

/// Where an update keeps its files: the journal (app data folder), the pre-update backups (the
/// application backup folder of `commands::backup`, as every backup the app takes on its own) and
/// the live database (whose staged restore a rollback writes next to it).
#[derive(Debug, Clone)]
pub struct UpdatePaths {
    pub data_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub live_db: PathBuf,
}

impl UpdatePaths {
    pub fn of(app: &AppHandle) -> AppResult<Self> {
        Ok(Self {
            data_dir: app_data_dir(app)?,
            backup_dir: crate::commands::backup::app_backup_dir(app)?,
            live_db: PathBuf::from(crate::startup::resolve_db_path(app)?),
        })
    }
}

/// What the manifest offers, as the install needs it (the plugin's `Update` stays with the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    pub running: String,
    pub version: String,
    pub kind: UpdateKind,
}

/// Outcome of the preparation that runs right before the installer.
#[derive(Debug)]
pub enum Prepared {
    /// The journal is written; the installer must start now. `staged_restore`: a rollback staged
    /// the pre-update backup (cancelled if the installer does not start).
    Ready {
        entry: Box<UpdateJournal>,
        staged_restore: bool,
    },
    /// Nothing was changed (a restore staged on the way was cancelled again).
    Refused(InstallRefusal),
}

/// Loads this installation's database key (the OS credential store in the app).
pub type KeyLoader = dyn Fn() -> AppResult<crate::db::encryption::DatabaseKey> + Sync;

/// Only one install or reinstall runs at a time in this process (security review N2): two would
/// race on the backup, the staged restore, the journal and the installer.
static INSTALL_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Held while an install or reinstall runs; released when dropped.
#[derive(Debug)]
pub struct InstallSlot(&'static std::sync::atomic::AtomicBool);

impl InstallSlot {
    /// The process-wide slot of `install` / `reinstall_previous`.
    pub fn acquire() -> AppResult<Self> {
        Self::acquire_on(&INSTALL_RUNNING)
    }

    fn acquire_on(flag: &'static std::sync::atomic::AtomicBool) -> AppResult<Self> {
        use std::sync::atomic::Ordering;
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self(flag))
            .map_err(|_| {
                AppError::ValidationFailed(vec![
                    "GATE_UPD_INSTALL_BUSY: an update is already being installed on this computer".to_string(),
                ])
            })
    }
}

impl Drop for InstallSlot {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Raised from the pre-update backup (or the staged restore) until the process exits or the
/// install stops (security review SF1): a change made then would be missing from the backup, or
/// lost with the restore. While it is up, every IPC command that needs a session is refused
/// (`require_session!`), the sync worker skips its cycles and the window cannot be closed; the UI
/// shows a blocking screen (`UpdateInstallLock`).
static UPDATE_WRITE_BARRIER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The refusal while the barrier is up.
pub const UPDATE_IN_PROGRESS_MESSAGE: &str =
    "Une mise à jour est en cours d'installation et Maintafox va redémarrer : aucune modification n'est possible maintenant.";

/// The barrier, lowered when dropped (the install stopped; on success the process exits first).
#[derive(Debug)]
pub struct WriteBarrier(&'static std::sync::atomic::AtomicBool);

impl WriteBarrier {
    fn raise_on(flag: &'static std::sync::atomic::AtomicBool) -> Self {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        Self(flag)
    }
}

impl Drop for WriteBarrier {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Whether an update is between its backup (or staged restore) and the restart.
pub fn writes_barred() -> bool {
    UPDATE_WRITE_BARRIER.load(std::sync::atomic::Ordering::SeqCst)
}

/// Refuses a command while an update is being installed (`require_session!` calls it first).
pub fn refuse_while_installing() -> AppResult<()> {
    refuse_while_installing_on(&UPDATE_WRITE_BARRIER)
}

/// The process-wide barrier, for the commands that check it through a seam taking the flag
/// (sign-in and unlock: [`refuse_sign_in_while_installing_on`]).
pub(crate) fn update_write_barrier() -> &'static std::sync::atomic::AtomicBool {
    &UPDATE_WRITE_BARRIER
}

/// The refusal of a sign-in or an unlock while the barrier is up (AU-109).
pub const SIGN_IN_UPDATE_IN_PROGRESS_MESSAGE: &str =
    "AUTH_UPDATE_IN_PROGRESS: Mise à jour en cours — reconnectez-vous après le redémarrage.";

/// Refuses a sign-in or an unlock while an update is being installed (AU-109): the same check as
/// every command behind a session and the forced password change ([`refuse_while_installing`]),
/// coded (`AUTH_UPDATE_IN_PROGRESS`) so the sign-in and lock screens show it translated. Called
/// first, before any lockout count, audit row or session is written. Signing out stays allowed.
pub(crate) fn refuse_sign_in_while_installing_on(flag: &std::sync::atomic::AtomicBool) -> AppResult<()> {
    refuse_while_installing_on(flag)
        .map_err(|_| AppError::UpdateInProgress(SIGN_IN_UPDATE_IN_PROGRESS_MESSAGE.to_string()))
}

fn refuse_while_installing_on(flag: &std::sync::atomic::AtomicBool) -> AppResult<()> {
    if flag.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::ValidationFailed(vec![UPDATE_IN_PROGRESS_MESSAGE.to_string()]));
    }
    Ok(())
}

/// A database restore is staged for the next start.
fn restore_pending(paths: &UpdatePaths) -> bool {
    crate::backup::restore::pending_restore_path(&paths.live_db).exists()
}

/// Before the download, without changing anything: a rollback this device would refuse anyway
/// (no verifiable pre-update backup, unsynchronised changes) is refused before minutes of
/// download. `prepare_install` decides again, authoritatively, after the download.
async fn downgrade_precheck(
    db: &sea_orm::DatabaseConnection,
    paths: &UpdatePaths,
    target: &InstallTarget,
) -> AppResult<Option<InstallRefusal>> {
    if target.kind != UpdateKind::Rollback {
        return Ok(None);
    }
    let previous = journal::load(&paths.data_dir)?;
    let schema_now = applied_migration_count(db).await?;
    let pending = unsynced_change_count(db).await?;
    Ok(plan_downgrade(previous.as_ref(), &target.running, &target.version, schema_now, pending).err())
}

/// Right before the installer (after the download): the pre-update backup (upgrade) or the
/// downgrade plan and its staged restore (rollback), then the journal. Every refusal or failure
/// leaves nothing behind (no journal entry, no staged restore), so the next start does not believe
/// an update ran and never applies a restore nobody installed. A restore is named in the journal
/// before it is staged (`UpdateJournal::staged_restore`): if it cannot be cancelled again, the
/// journal does not justify it and the next start cancels it (security review SF2).
pub async fn prepare_install(
    db: &sea_orm::DatabaseConnection,
    paths: &UpdatePaths,
    target: &InstallTarget,
    actor: Option<i32>,
    load_key: &KeyLoader,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
) -> AppResult<Prepared> {
    // An administrator may have staged a restore during the download (N1).
    if restore_pending(paths) {
        return Ok(Prepared::Refused(InstallRefusal::RestorePending));
    }
    let previous = journal::load(&paths.data_dir)?;
    let schema_now = applied_migration_count(db).await?;

    let mut entry = UpdateJournal {
        from_version: target.running.clone(),
        to_version: target.version.clone(),
        kind: target.kind,
        state: JournalState::Installing,
        backup_path: None,
        backup_sha256: None,
        schema_version_before: Some(schema_now),
        started_at: chrono::Utc::now().to_rfc3339(),
        finished_at: None,
        failure: None,
        rollback_refusal: None,
        rollback_allowed_version: None,
        backup_refusal: None,
        notice_acknowledged: false,
        staged_restore: None,
    };
    let mut staged_restore = false;
    match target.kind {
        UpdateKind::Upgrade => {
            progress(UpdateProgressEvent::BackingUp);
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            let path = paths.backup_dir.join(format!(
                "maintafox-pre-update-{}-to-{}-{stamp}.db",
                target.running, target.version
            ));
            // The backup service's own run (encrypted copy, checksum, `backup_runs` record).
            let backup =
                crate::backup::run_manual_backup(db, &path.to_string_lossy(), crate::backup::TRIGGER_PRE_UPDATE, actor)
                    .await?;
            entry.backup_path = Some(backup.output_path);
            entry.backup_sha256 = Some(backup.sha256_checksum);
        }
        UpdateKind::Rollback => {
            let pending = unsynced_change_count(db).await?;
            match plan_downgrade(previous.as_ref(), &target.running, &target.version, schema_now, pending) {
                Err(refusal) => return Ok(Prepared::Refused(refusal)),
                Ok(DowngradePlan::KeepDatabase) => {}
                Ok(DowngradePlan::RestoreBackup { backup_path, sha256 }) => {
                    // The file must be the backup that was taken (security review S8).
                    if !backup_matches_checksum(&backup_path, &sha256)? {
                        tracing::error!(backup = %backup_path.display(), "update: pre-update backup checksum mismatch; rollback refused");
                        return Ok(Prepared::Refused(InstallRefusal::RollbackBackupChanged));
                    }
                    // `plan_downgrade` restores only from the completed upgrade it read.
                    let Some(previous) = previous.as_ref() else {
                        return Ok(Prepared::Refused(InstallRefusal::RollbackBackupMissing));
                    };
                    let record = journal::StagedRestore {
                        pending_path: crate::backup::restore::pending_restore_path(&paths.live_db)
                            .to_string_lossy()
                            .to_string(),
                        sha256: sha256.clone(),
                    };
                    // Written before the restore is staged (SF2): whatever fails from here on, a
                    // failed cancel included, the next start finds a restore the journal does not
                    // justify (the completed upgrade) and cancels it.
                    let mut marked = previous.clone();
                    marked.staged_restore = Some(record.clone());
                    journal::save(&paths.data_dir, &marked)?;
                    progress(UpdateProgressEvent::RestoringBackup);
                    // The restore service's checks (integrity, this installation's key, schema not
                    // newer) and its safety backup of the current database.
                    let staged = match load_key() {
                        Ok(key) => crate::backup::restore::stage_restore(
                            db,
                            &paths.live_db,
                            &backup_path,
                            &key,
                            &paths.backup_dir,
                            actor,
                        )
                        .await
                        .map(|_| ()),
                        Err(e) => Err(e),
                    };
                    if let Err(e) = staged {
                        withdraw_staged_restore(paths, previous);
                        return Err(e);
                    }
                    // A change made while the restore was being staged would be lost with it.
                    let pending_now = match unsynced_change_count(db).await {
                        Ok(count) => count,
                        Err(e) => {
                            withdraw_staged_restore(paths, previous);
                            return Err(e);
                        }
                    };
                    if pending_now > 0 {
                        withdraw_staged_restore(paths, previous);
                        return Ok(Prepared::Refused(InstallRefusal::RollbackUnsyncedChanges {
                            count: pending_now,
                        }));
                    }
                    staged_restore = true;
                    entry.backup_path = Some(backup_path.to_string_lossy().to_string());
                    entry.backup_sha256 = Some(sha256);
                    entry.staged_restore = Some(record);
                }
            }
        }
    }
    if let Err(e) = journal::save(&paths.data_dir, &entry) {
        // Without its journal entry the staged restore would not be justified at the next start.
        if let (true, Some(previous)) = (staged_restore, previous.as_ref()) {
            withdraw_staged_restore(paths, previous);
        }
        return Err(e);
    }
    Ok(Prepared::Ready {
        entry: Box::new(entry),
        staged_restore,
    })
}

/// Abandons a restore `prepare_install` staged (a refusal or a failure after the staging): the
/// staged file is removed, then the journal the install started from is put back. When the file
/// cannot be removed, the journal written before the staging stays: it names the restore without
/// justifying it, so the next start cancels it, and no update runs before that restart
/// (`RestorePending`). Returns whether the restore was removed.
fn withdraw_staged_restore(paths: &UpdatePaths, previous: &UpdateJournal) -> bool {
    match crate::backup::restore::cancel_pending_restore(&paths.live_db) {
        Ok(_) => {
            if let Err(e) = journal::save(&paths.data_dir, previous) {
                // Harmless: no restore is pending any more.
                tracing::warn!("update: the journal still names the cancelled restore: {e}");
            }
            true
        }
        Err(e) => {
            tracing::error!(
                "update: the staged restore could not be cancelled; the next start cancels it (the journal does not justify it): {e}"
            );
            false
        }
    }
}

/// The installer did not start: a restore staged for the older version that was not installed is
/// removed first, then the journal says so. If the restore cannot be removed, the entry keeps
/// naming it: `InstallNotCompleted` does not justify it, so the next start cancels it (SF2).
pub fn record_install_not_started(paths: &UpdatePaths, entry: &mut UpdateJournal, failure: &str) -> AppResult<()> {
    let cancelled = if entry.staged_restore.is_some() {
        crate::backup::restore::cancel_pending_restore(&paths.live_db).map(|_| ())
    } else {
        Ok(())
    };
    if cancelled.is_ok() {
        entry.staged_restore = None;
    }
    entry.state = JournalState::InstallNotCompleted;
    entry.failure = Some(failure.to_string());
    entry.finished_at = Some(chrono::Utc::now().to_rfc3339());
    entry.notice_acknowledged = true;
    let saved = journal::save(&paths.data_dir, entry);
    cancelled.and(saved)
}

/// Downloads with progress; the plugin verifies the minisign signature against this build's
/// public key before it returns the bytes. The download URL is a presigned object-storage URL:
/// the caller has removed the device token from the update's headers.
async fn download_verified(update: &Update, progress: &(dyn Fn(UpdateProgressEvent) + Sync)) -> AppResult<Vec<u8>> {
    let mut downloaded: u64 = 0;
    update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                progress(UpdateProgressEvent::Downloading {
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                });
            },
            || {},
        )
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("download or signature check failed: {e}")))
}

/// The install sequence (security review B1): download and verify first, while the app stays
/// usable and nothing is written; then the write barrier goes up (SF1) and the install prepares
/// (backup or staged restore, journal); then the installer at once, with nothing awaited in
/// between. If the installer cannot start, the staged restore is cancelled, the journal says so
/// and the barrier comes down. `download` and `install` are the updater plugin's in the app;
/// `barrier` is the process-wide one (`writes_barred`) in the app.
#[allow(clippy::too_many_arguments)]
pub async fn run_install<Dl, Inst>(
    db: &sea_orm::DatabaseConnection,
    paths: &UpdatePaths,
    target: &InstallTarget,
    actor: Option<i32>,
    load_key: &KeyLoader,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
    barrier: &'static std::sync::atomic::AtomicBool,
    download: Dl,
    install: Inst,
) -> AppResult<InstallOutcome>
where
    Dl: std::future::Future<Output = AppResult<Vec<u8>>>,
    Inst: FnOnce(Vec<u8>) -> AppResult<()>,
{
    // A restore staged for the next start comes first: an update now would back up or replace
    // the database it is about to swap out (N1).
    if restore_pending(paths) {
        return Ok(InstallOutcome::Refused {
            refusal: InstallRefusal::RestorePending,
        });
    }
    if let Some(refusal) = downgrade_precheck(db, paths, target).await? {
        return Ok(InstallOutcome::Refused { refusal });
    }
    // Nothing is written before or during the download: a failed download leaves no trace.
    let bytes = download.await?;
    // From the backup until the process exits, nothing can be changed in the app.
    let _barrier = WriteBarrier::raise_on(barrier);
    let mut entry = match prepare_install(db, paths, target, actor, load_key, progress).await? {
        Prepared::Refused(refusal) => return Ok(InstallOutcome::Refused { refusal }),
        Prepared::Ready { entry, .. } => *entry,
    };
    tracing::info!(from = %entry.from_version, to = %entry.to_version, kind = ?entry.kind, "update: installing");
    progress(UpdateProgressEvent::Installing);
    // On Windows the plugin starts the installer and exits this process: it returns only on error.
    if let Err(e) = install(bytes) {
        // Nothing was installed: no restore stays staged and the next start knows it.
        record_install_not_started(paths, &mut entry, &e.to_string())?;
        return Err(e);
    }
    Ok(InstallOutcome::Restarting)
}

/// Install the offer the administrator confirmed, if the manifest still offers exactly that. The
/// caller (`commands::updater`) has checked who may install and holds the install slot.
pub async fn install(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    actor: Option<i32>,
    confirmed: &ConfirmedOffer,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
) -> AppResult<InstallOutcome> {
    let (mut update, ext) = match ask_manifest(app, db, locale, None).await? {
        ManifestAnswer::Offer(update, ext) => (*update, ext),
        ManifestAnswer::NotConfigured => {
            return Err(AppError::ValidationFailed(vec![
                "GATE_UPD_NOT_CONFIGURED: this build has no in-app updates".to_string(),
            ]))
        }
        ManifestAnswer::NotActivated => {
            return Err(AppError::ValidationFailed(vec![
                "GATE_UPD_NOT_ACTIVATED: this device is not activated".to_string(),
            ]))
        }
        ManifestAnswer::None | ManifestAnswer::ServerRefused | ManifestAnswer::NotAcceptable(_) => {
            return Ok(InstallOutcome::Refused {
                refusal: InstallRefusal::UpdateWithdrawn,
            })
        }
    };
    if !offer_matches_confirmation(&update.version, &ext, confirmed) {
        tracing::warn!(
            confirmed = %confirmed.version, confirmed_kind = ?confirmed.kind, offered = %update.version,
            offered_kind = ?ext.kind, "update: the offer changed since it was confirmed; asking again"
        );
        return Ok(InstallOutcome::Refused {
            refusal: InstallRefusal::UpdateWithdrawn,
        });
    }
    let paths = UpdatePaths::of(app)?;
    let target = InstallTarget {
        running: running_version(),
        version: update.version.clone(),
        kind: ext.kind,
    };
    // The download URL is a presigned object-storage URL: it must not carry the device token.
    update.headers.clear();
    let update = &update;
    run_install(
        db,
        &paths,
        &target,
        actor,
        &crate::commands::backup::require_database_key,
        progress,
        &UPDATE_WRITE_BARRIER,
        download_verified(update, progress),
        |bytes| {
            update
                .install(bytes)
                .map_err(|e| AppError::Internal(anyhow::anyhow!("the installer could not be started: {e}")))
        },
    )
    .await
}

/// What the rollback request after a failed database update got (`reinstall_previous`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackAnswer {
    /// The server offers exactly the requested version, kind rollback.
    Install,
    /// No acceptable answer (204, a refusal status such as 409, another version or kind, or this
    /// device cannot ask): the reason, recorded in the journal.
    Refused(String),
}

/// Reads a manifest answer to the rollback request. Never another version: no retry with other
/// versions is made.
pub fn rollback_answer(answer: &ManifestAnswer, requested: &str) -> RollbackAnswer {
    match answer {
        ManifestAnswer::Offer(update, ext) if update.version == requested && ext.kind == UpdateKind::Rollback => {
            RollbackAnswer::Install
        }
        ManifestAnswer::Offer(update, _) => {
            RollbackAnswer::Refused(format!("the server offered {} instead of {requested}", update.version))
        }
        ManifestAnswer::None => RollbackAnswer::Refused("the server has nothing to install (204)".into()),
        ManifestAnswer::ServerRefused => RollbackAnswer::Refused("the server refused the rollback request".into()),
        ManifestAnswer::NotAcceptable(why) => RollbackAnswer::Refused(why.clone()),
        ManifestAnswer::NotConfigured => RollbackAnswer::Refused("this build has no in-app updates".into()),
        ManifestAnswer::NotActivated => RollbackAnswer::Refused("this device cannot ask the update server".into()),
    }
}

/// The rollback was refused: the journal records why and stays `RollbackRequired` (this version
/// keeps showing the recovery screen and never migrates); the pre-update backup stays staged (or,
/// after a restart, is the database), nothing else is tried. `allowed_version`: the version the
/// server's 409 names as allowed, kept as information only (never installed automatically).
pub fn record_rollback_refusal(data_dir: &Path, reason: &str, allowed_version: Option<&str>) -> AppResult<()> {
    if let Some(mut j) = journal::load(data_dir)?.filter(|j| j.state == JournalState::RollbackRequired) {
        j.rollback_refusal = Some(reason.to_string());
        j.rollback_allowed_version = allowed_version.map(str::to_string);
        journal::save(data_dir, &j)?;
    }
    Ok(())
}

/// The body of a manifest refusal (vendor-console: `{ "error": "rollback_target_not_allowed",
/// "allowed_version": "x.y.z" }` on 409).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ManifestRefusalBody {
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub allowed_version: Option<String>,
}

/// Reads a refusal body; `allowed_version` is kept only when it is a plain release version
/// (MAJOR.MINOR.PATCH), so nothing else from the body reaches the screen.
pub fn parse_refusal_body(text: &str) -> ManifestRefusalBody {
    let mut body: ManifestRefusalBody = serde_json::from_str(text).unwrap_or_default();
    body.allowed_version = body
        .allowed_version
        .map(|v| v.trim().to_string())
        .filter(|v| is_plain_release_version(v));
    body
}

/// MAJOR.MINOR.PATCH, digits only (the catalog's release versions).
pub fn is_plain_release_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The `{target}/{arch}` the updater plugin puts in the manifest URL on this build.
fn platform_segments() -> (&'static str, &'static str) {
    let target = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    (target, std::env::consts::ARCH)
}

/// The updater plugin keeps no body of a refusal status: asks the same manifest request once more
/// to read why (409 `rollback_target_not_allowed` and its `allowed_version`). Information only;
/// any failure here gives an empty body.
async fn read_rollback_refusal(db: &sea_orm::DatabaseConnection, rollback_to: &str) -> ManifestRefusalBody {
    let Ok(Some(token)) = crate::commands::product_license::update_manifest_bearer_token(db).await else {
        return ManifestRefusalBody::default();
    };
    let base = env!("MAINTAFOX_CONTROL_PLANE_API_BASE_URL")
        .trim()
        .trim_end_matches('/');
    let (target, arch) = platform_segments();
    let url = format!("{base}/api/v1/updates/{target}/{arch}/{}", running_version());
    let _ = rustls::crypto::ring::default_provider().install_default();
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
    else {
        return ManifestRefusalBody::default();
    };
    match client
        .get(url)
        .bearer_auth(token)
        .header(ROLLBACK_HEADER, rollback_to)
        .send()
        .await
    {
        Ok(response) if response.status().as_u16() == 409 => {
            parse_refusal_body(&response.text().await.unwrap_or_default())
        }
        _ => ManifestRefusalBody::default(),
    }
}

/// What the sessionless recovery reinstall may do with this journal (security review N1, S8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReinstallCheck {
    /// Ask the server for exactly the journal's `from_version`.
    Ask(Box<UpdateJournal>),
    /// The pre-update backup could not be verified and was not staged: reinstalling the older
    /// version would put it on a database it cannot open. Keep the PC as it is (support).
    BackupNotVerified,
}

/// Only while the journal is `RollbackRequired`, only on the version that failed to update the
/// database (`to_version`), and only when its pre-update backup was verified and staged.
pub fn reinstall_check(journal: Option<UpdateJournal>, running: &str) -> AppResult<ReinstallCheck> {
    let Some(entry) = journal.filter(|j| j.state == JournalState::RollbackRequired) else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_UPD_NO_FAILED_UPDATE: no failed update to roll back".to_string(),
        ]));
    };
    if entry.to_version != running {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_UPD_ROLLBACK_WRONG_VERSION: the failed update was to {}, this is version {running}: nothing to roll back here",
            entry.to_version
        )]));
    }
    if entry.backup_refusal.is_some() {
        return Ok(ReinstallCheck::BackupNotVerified);
    }
    Ok(ReinstallCheck::Ask(Box::new(entry)))
}

/// From the recovery screen: reinstall the version this device came from after its database
/// update failed (the pre-update backup is already staged). The caller holds the install slot.
pub async fn reinstall_previous(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
) -> AppResult<InstallOutcome> {
    let data_dir = app_data_dir(app)?;
    let entry = match reinstall_check(journal::load(&data_dir)?, &running_version())? {
        ReinstallCheck::Ask(entry) => *entry,
        ReinstallCheck::BackupNotVerified => {
            return Ok(InstallOutcome::Refused {
                refusal: InstallRefusal::RollbackBackupMissing,
            })
        }
    };
    let answer = ask_manifest(app, db, locale, Some(&entry.from_version)).await?;
    match rollback_answer(&answer, &entry.from_version) {
        RollbackAnswer::Install => {
            let ManifestAnswer::Offer(update, _) = answer else {
                unreachable!("rollback_answer accepts only an offer")
            };
            let mut update = *update;
            // The download URL is a presigned object-storage URL: it must not carry the token.
            update.headers.clear();
            let bytes = download_verified(&update, progress).await?;
            progress(UpdateProgressEvent::Installing);
            update
                .install(bytes)
                .map_err(|e| AppError::Internal(anyhow::anyhow!("the installer could not be started: {e}")))?;
            Ok(InstallOutcome::Restarting)
        }
        RollbackAnswer::Refused(mut reason) => {
            let mut allowed = None;
            if matches!(answer, ManifestAnswer::ServerRefused) {
                let body = read_rollback_refusal(db, &entry.from_version).await;
                if let Some(code) = body.error {
                    reason = format!("{reason} ({code})");
                }
                allowed = body.allowed_version;
            }
            tracing::error!(to = %entry.from_version, allowed = ?allowed, "update: rollback refused: {reason}");
            record_rollback_refusal(&data_dir, &reason, allowed.as_deref())?;
            Ok(InstallOutcome::Refused {
                refusal: InstallRefusal::RollbackRefused,
            })
        }
    }
}

// ── Startup ─────────────────────────────────────────────────────────────────────────────────

/// Journal state for the UI (recovery screen, "update rolled back" notice).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateJournalView {
    pub state: JournalState,
    pub kind: UpdateKind,
    pub from_version: String,
    pub to_version: String,
    pub failure: Option<String>,
    pub rollback_refusal: Option<String>,
    pub rollback_allowed_version: Option<String>,
    /// Why the pre-update backup was not restored (no checksum, or the file changed): the
    /// recovery screen then offers no reinstall.
    pub backup_refusal: Option<String>,
    pub notice_acknowledged: bool,
}

impl From<&UpdateJournal> for UpdateJournalView {
    fn from(j: &UpdateJournal) -> Self {
        Self {
            state: j.state,
            kind: j.kind,
            from_version: j.from_version.clone(),
            to_version: j.to_version.clone(),
            failure: j.failure.clone(),
            rollback_refusal: j.rollback_refusal.clone(),
            rollback_allowed_version: j.rollback_allowed_version.clone(),
            backup_refusal: j.backup_refusal.clone(),
            notice_acknowledged: j.notice_acknowledged,
        }
    }
}

pub fn journal_view(app: &AppHandle) -> AppResult<Option<UpdateJournalView>> {
    Ok(journal::load(&app_data_dir(app)?)?
        .as_ref()
        .map(UpdateJournalView::from))
}

pub fn acknowledge_notice(app: &AppHandle) -> AppResult<()> {
    let dir = app_data_dir(app)?;
    if let Some(mut j) = journal::load(&dir)? {
        j.notice_acknowledged = true;
        journal::save(&dir, &j)?;
    }
    Ok(())
}

/// Before the database opens, a restore the updater staged is applied only when the journal
/// justifies it (security review B1, SF2): the journal names it (`staged_restore`, same checksum)
/// and `journal::staged_restore_is_justified` holds. Otherwise it is cancelled, the journal stops
/// naming it, and an `Installing` entry is closed as `InstallNotCompleted`. A pending restore the
/// journal does not name (no record, or another checksum) is one an administrator staged and is
/// left to be applied. Without a staged restore nothing is read. Returns whether one was
/// cancelled. An unreadable journal, or a staged file that cannot be read, is an error: the
/// restore is not applied (support procedure in docs/release/ROLLBACK.md).
pub fn reconcile_staged_restore(data_dir: &Path, live_db: &Path, running: &str) -> AppResult<bool> {
    let pending = crate::backup::restore::pending_restore_path(live_db);
    if !pending.exists() {
        return Ok(false);
    }
    let Some(mut j) = journal::load(data_dir)? else {
        // No update in progress: a restore staged by an administrator (Settings > Backup).
        return Ok(false);
    };
    let Some(record) = j.staged_restore.clone() else {
        // The updater staged nothing for this entry: an administrator's restore.
        return Ok(false);
    };
    if !backup_matches_checksum(&pending, &record.sha256)? {
        // Another file replaced the updater's: an administrator staged it afterwards.
        return Ok(false);
    }
    if journal::staged_restore_is_justified(&j, running) {
        return Ok(false);
    }
    crate::backup::restore::cancel_pending_restore(live_db)?;
    j.staged_restore = None;
    if j.state == JournalState::Installing {
        j.state = JournalState::InstallNotCompleted;
        j.failure = Some(format!(
            "the installer of {} did not complete; the database restore staged for it was cancelled",
            j.to_version
        ));
        j.finished_at = Some(chrono::Utc::now().to_rfc3339());
    }
    if let Err(e) = journal::save(data_dir, &j) {
        // The restore is already cancelled: the stale record no longer matches anything pending.
        tracing::warn!("update: the journal still names the cancelled restore: {e}");
    }
    tracing::warn!(from = %j.from_version, to = %j.to_version, state = ?j.state, running, "update: a restore the journal does not justify was cancelled");
    Ok(true)
}

/// An administrator staged a restore (Settings > Backup): it replaced any restore the updater had
/// staged, so the journal stops naming one and the administrator's restore is applied at the next
/// start, even when it is the same backup file.
pub fn forget_staged_restore(data_dir: &Path) -> AppResult<()> {
    if let Some(mut j) = journal::load(data_dir)?.filter(|j| j.staged_restore.is_some()) {
        j.staged_restore = None;
        journal::save(data_dir, &j)?;
    }
    Ok(())
}

/// `forget_staged_restore` for this app's data folder.
pub fn forget_staged_restore_for_app(app: &AppHandle) -> AppResult<()> {
    forget_staged_restore(&app_data_dir(app)?)
}

/// `reconcile_staged_restore` for this app's folders and running version.
pub fn reconcile_staged_restore_at_startup(app: &AppHandle, live_db: &Path) -> AppResult<bool> {
    reconcile_staged_restore(&app_data_dir(app)?, live_db, &running_version())
}

/// The journal and what startup must do with it.
pub fn startup_action(app: &AppHandle) -> AppResult<(Option<UpdateJournal>, StartupAction)> {
    let journal = journal::load(&app_data_dir(app)?)?;
    let action = journal::classify(journal.as_ref(), &running_version());
    Ok((journal, action))
}

/// After migrations succeeded: closes the journal entry.
pub fn on_migrations_succeeded(
    app: &AppHandle,
    journal: Option<UpdateJournal>,
    action: &StartupAction,
) -> AppResult<()> {
    let Some(mut j) = journal else { return Ok(()) };
    j.state = match action {
        StartupAction::VerifyNewVersion => JournalState::Completed,
        StartupAction::ReturnedToPrevious => JournalState::RolledBack,
        StartupAction::InstallNotCompleted => JournalState::InstallNotCompleted,
        StartupAction::Nothing | StartupAction::RecoveryPending => return Ok(()),
    };
    // A restore staged for this entry was applied before the database opened.
    j.staged_restore = None;
    j.finished_at = Some(chrono::Utc::now().to_rfc3339());
    tracing::info!(state = ?j.state, from = %j.from_version, to = %j.to_version, "update: journal closed");
    journal::save(&app_data_dir(app)?, &j)
}

/// The new version could not migrate the database: stage the pre-update backup and record that
/// the previous version must be reinstalled. Returns the journal view for the recovery screen.
pub async fn on_migration_failed(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    j: UpdateJournal,
    failure: &str,
) -> AppResult<UpdateJournalView> {
    let paths = UpdatePaths::of(app)?;
    let key = crate::commands::backup::require_database_key()?;
    stage_pre_update_backup(db, &paths, j, failure, &key).await
}

/// Why the pre-update backup the journal names cannot be shown to be the backup that was taken
/// (security review S8): it is gone, no checksum was recorded, or the file changed since.
pub fn unverified_pre_update_backup(j: &UpdateJournal) -> AppResult<Result<PathBuf, String>> {
    let Some(backup) = j.backup_path.as_ref().map(PathBuf::from).filter(|p| p.is_file()) else {
        return Ok(Err("the backup taken before the update is missing".to_string()));
    };
    let Some(expected) = j.backup_sha256.as_deref().filter(|s| !s.trim().is_empty()) else {
        return Ok(Err(format!(
            "no checksum was recorded for the backup {} taken before the update",
            backup.display()
        )));
    };
    if !backup_matches_checksum(&backup, expected)? {
        return Ok(Err(format!(
            "the backup {} taken before the update has changed since it was taken (checksum mismatch)",
            backup.display()
        )));
    }
    Ok(Ok(backup))
}

/// Journal-driven rollback: checks the pre-update backup the journal names is the file that was
/// taken (checksum, required), stages it through the restore service
/// (`backup::restore::stage_restore`: integrity, key, schema not newer, safety backup of the
/// half-migrated database), and records `RollbackRequired`.
///
/// When the backup cannot be verified nothing is staged, ever: the journal records
/// `RollbackRequired` with `backup_refusal`, so this version stays on the recovery screen without
/// touching the database again, and the screen offers no reinstall (contact support). When the
/// restore service refuses the backup, nothing is staged and the journal is unchanged.
pub async fn stage_pre_update_backup(
    db: &sea_orm::DatabaseConnection,
    paths: &UpdatePaths,
    mut j: UpdateJournal,
    failure: &str,
    key: &crate::db::encryption::DatabaseKey,
) -> AppResult<UpdateJournalView> {
    j.failure = Some(failure.to_string());
    j.finished_at = Some(chrono::Utc::now().to_rfc3339());
    j.state = JournalState::RollbackRequired;
    match unverified_pre_update_backup(&j)? {
        Err(why) => {
            j.backup_refusal = Some(why.clone());
            journal::save(&paths.data_dir, &j)?;
            tracing::error!(from = %j.from_version, to = %j.to_version, "update: database update failed; pre-update backup NOT restored: {why}");
        }
        Ok(backup) => {
            crate::backup::restore::stage_restore(db, &paths.live_db, &backup, key, &paths.backup_dir, None).await?;
            j.backup_refusal = None;
            // `RollbackRequired` justifies it whichever version starts next.
            j.staged_restore = j.backup_sha256.clone().map(|sha256| journal::StagedRestore {
                pending_path: crate::backup::restore::pending_restore_path(&paths.live_db)
                    .to_string_lossy()
                    .to_string(),
                sha256,
            });
            journal::save(&paths.data_dir, &j)?;
            tracing::error!(from = %j.from_version, to = %j.to_version, "update: database update failed; pre-update backup staged");
        }
    }
    Ok(UpdateJournalView::from(&j))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn completed_upgrade(backup: Option<&Path>, schema_before: i64) -> UpdateJournal {
        UpdateJournal {
            from_version: "1.0.0".into(),
            to_version: "1.1.0".into(),
            kind: UpdateKind::Upgrade,
            state: JournalState::Completed,
            backup_path: backup.map(|p| p.to_string_lossy().to_string()),
            backup_sha256: backup.map(|_| "ab".repeat(32)),
            schema_version_before: Some(schema_before),
            started_at: "2026-10-13T10:00:00Z".into(),
            finished_at: Some("2026-10-13T10:05:00Z".into()),
            failure: None,
            rollback_refusal: None,
            rollback_allowed_version: None,
            backup_refusal: None,
            notice_acknowledged: false,
            staged_restore: None,
        }
    }

    #[test]
    fn manifest_endpoint_keeps_the_tauri_placeholders() {
        let url = manifest_endpoint("https://api.maintafox.systems/").unwrap().to_string();
        assert!(
            url.starts_with("https://api.maintafox.systems/api/v1/updates/"),
            "{url}"
        );
        // The plugin replaces both the raw and the percent-encoded forms.
        assert!(
            url.contains("target") && url.contains("arch") && url.contains("current_version"),
            "{url}"
        );
        assert!(manifest_endpoint("not a url").is_err());
    }

    #[test]
    fn extension_is_required_and_parsed() {
        let raw = serde_json::json!({
            "version": "1.1.0", "url": "https://x", "signature": "s",
            "maintafox": { "release_id": "rel_1", "kind": "rollback", "mandatory": true, "mandatory_reason": null,
                           "min_version": "1.0.0", "notes_en": "EN", "notes_fr": "FR" }
        });
        let ext = parse_extension(&raw).unwrap();
        assert_eq!(ext.kind, UpdateKind::Rollback);
        assert!(ext.mandatory);
        assert_eq!(ext.notes_fr, "FR");
        assert!(
            parse_extension(&serde_json::json!({ "version": "1.1.0" })).is_err(),
            "a foreign server is refused"
        );
    }

    #[test]
    fn downgrade_keeps_the_database_when_no_migration_ran() {
        let j = completed_upgrade(None, 120);
        assert_eq!(
            plan_downgrade(Some(&j), "1.1.0", "1.0.0", 120, 5),
            Ok(DowngradePlan::KeepDatabase)
        );
    }

    #[test]
    fn downgrade_restores_the_pre_update_backup_after_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("pre-update.db");
        std::fs::write(&backup, b"backup").unwrap();
        let j = completed_upgrade(Some(&backup), 120);
        assert_eq!(
            plan_downgrade(Some(&j), "1.1.0", "1.0.0", 125, 0),
            Ok(DowngradePlan::RestoreBackup {
                backup_path: backup.clone(),
                sha256: "ab".repeat(32),
            })
        );
        let mut unverifiable = j.clone();
        unverifiable.backup_sha256 = None;
        assert_eq!(
            plan_downgrade(Some(&unverifiable), "1.1.0", "1.0.0", 125, 0),
            Err(InstallRefusal::RollbackBackupMissing),
            "a backup without a recorded checksum is never restored"
        );
        assert_eq!(
            plan_downgrade(Some(&j), "1.1.0", "1.0.0", 125, 3),
            Err(InstallRefusal::RollbackUnsyncedChanges { count: 3 }),
            "unsynchronised changes would be lost"
        );
    }

    #[test]
    fn downgrade_without_a_matching_backup_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            plan_downgrade(None, "1.1.0", "1.0.0", 125, 0),
            Err(InstallRefusal::RollbackBackupMissing)
        );
        let missing = completed_upgrade(Some(&dir.path().join("gone.db")), 120);
        assert_eq!(
            plan_downgrade(Some(&missing), "1.1.0", "1.0.0", 125, 0),
            Err(InstallRefusal::RollbackBackupMissing)
        );
        let other_pair = completed_upgrade(None, 120);
        assert_eq!(
            plan_downgrade(Some(&other_pair), "1.2.0", "1.0.0", 120, 0),
            Err(InstallRefusal::RollbackBackupMissing)
        );
    }

    // ── Release-chain port: version source, install preparation, journal-driven rollback ──

    /// The licence's forced-update rule and the update manifest read one version: the release
    /// version, the same in `tauri.conf.json` (what the updater plugin sends) and `Cargo.toml`.
    #[test]
    fn one_version_source() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json");
        assert_eq!(
            conf["version"].as_str(),
            Some(env!("CARGO_PKG_VERSION")),
            "tauri.conf.json and Cargo.toml must carry the same version"
        );
        assert_eq!(
            running_version(),
            crate::commands::product_license::running_app_version()
        );
    }

    /// The write barrier of these tests (the app's own is never raised by a test).
    static TEST_BARRIER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn paths_in(dir: &Path) -> UpdatePaths {
        UpdatePaths {
            data_dir: dir.join("data"),
            backup_dir: dir.join("backups"),
            live_db: dir.join("maintafox.db"),
        }
    }

    fn upgrade_target() -> InstallTarget {
        InstallTarget {
            running: "1.0.0".into(),
            version: "1.1.0".into(),
            kind: UpdateKind::Upgrade,
        }
    }

    fn never_downloaded() -> AppResult<Vec<u8>> {
        panic!("nothing is downloaded")
    }

    fn no_key() -> AppResult<crate::db::encryption::DatabaseKey> {
        panic!("this path must not need the database key")
    }

    fn recording() -> (
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        impl Fn(UpdateProgressEvent) + Sync,
    ) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        (seen, move |event: UpdateProgressEvent| {
            sink.lock().unwrap().push(format!("{event:?}"));
        })
    }

    /// A failed pre-update backup stops the install: no journal is written (the next start does
    /// not believe an update ran), the earlier journal is kept, nothing reaches the download.
    #[tokio::test]
    async fn a_failed_pre_update_backup_stops_the_install() {
        let db = crate::db::test_db::migrated().await;
        let dir = tempfile::tempdir().unwrap();
        let mut paths = paths_in(dir.path());
        // The backup folder cannot be created: a file stands where it should be.
        let blocker = dir.path().join("not-a-folder");
        std::fs::write(&blocker, b"file").unwrap();
        paths.backup_dir = blocker.join("backups");
        let earlier = completed_upgrade(None, 1);
        journal::save(&paths.data_dir, &earlier).unwrap();

        let (seen, progress) = recording();
        let outcome = prepare_install(&db, &paths, &upgrade_target(), Some(1), &no_key, &progress).await;
        assert!(outcome.is_err(), "the install must stop: {outcome:?}");
        assert_eq!(
            journal::load(&paths.data_dir).unwrap(),
            Some(earlier),
            "the journal is not rewritten when the backup fails"
        );
        let events = seen.lock().unwrap().clone();
        assert_eq!(events, vec!["BackingUp".to_string()], "nothing after the backup step");
    }

    /// A rollback with no pre-update backup of the target version is refused before anything is
    /// written; one whose schema did not change keeps the database (no restore staged).
    #[tokio::test]
    async fn rollback_preparation_follows_the_downgrade_plan() {
        let db = crate::db::test_db::migrated().await;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let target = InstallTarget {
            running: "1.1.0".into(),
            version: "1.0.0".into(),
            kind: UpdateKind::Rollback,
        };
        let (_, progress) = recording();
        match prepare_install(&db, &paths, &target, None, &no_key, &progress)
            .await
            .unwrap()
        {
            Prepared::Refused(InstallRefusal::RollbackBackupMissing) => {}
            other => panic!("refused without a backup: {other:?}"),
        }
        assert_eq!(journal::load(&paths.data_dir).unwrap(), None);

        let schema = applied_migration_count(&db).await.unwrap();
        journal::save(&paths.data_dir, &completed_upgrade(None, schema)).unwrap();
        match prepare_install(&db, &paths, &target, None, &no_key, &progress)
            .await
            .unwrap()
        {
            Prepared::Ready { entry, staged_restore } => {
                assert!(!staged_restore, "no migration ran since 1.0.0: the database is kept");
                assert_eq!(entry.kind, UpdateKind::Rollback);
                assert_eq!(entry.state, JournalState::Installing);
            }
            other => panic!("kept database: {other:?}"),
        }
        assert!(!crate::backup::restore::pending_restore_path(&paths.live_db).exists());
    }

    async fn encrypted_live_db(path: &Path, key: &crate::db::encryption::DatabaseKey) -> sea_orm::DatabaseConnection {
        use sea_orm::ConnectionTrait;
        use sea_orm_migration::MigratorTrait;
        let db = crate::db::connect_sqlcipher(path, Some(key), true).await.unwrap();
        db.execute_unprepared("PRAGMA journal_mode=WAL;").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        db
    }

    async fn marker(db: &sea_orm::DatabaseConnection) -> Option<String> {
        use sea_orm::{ConnectionTrait, DbBackend, Statement};
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT note FROM update_marker".to_string(),
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String>("", "note").ok())
    }

    /// Journal-driven rollback end to end: the old version takes the pre-update backup and writes
    /// the journal; the new version's migrations fail, so it stages that backup (checksum checked,
    /// restore service checks) and records `RollbackRequired`; the journal then sends the new
    /// version to the recovery screen and tells the previous version it came back; the next start
    /// swaps the backup in.
    #[tokio::test]
    async fn a_failed_migration_rolls_back_through_the_journal() {
        use sea_orm::ConnectionTrait;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let key = crate::db::encryption::DatabaseKey::generate();
        let db = encrypted_live_db(&paths.live_db, &key).await;
        db.execute_unprepared("CREATE TABLE update_marker (note TEXT); INSERT INTO update_marker VALUES ('before');")
            .await
            .unwrap();

        // 1.0.0 prepares the update to 1.1.0.
        let (seen, progress) = recording();
        let entry = match prepare_install(&db, &paths, &upgrade_target(), Some(1), &no_key, &progress)
            .await
            .unwrap()
        {
            Prepared::Ready { entry, staged_restore } => {
                assert!(!staged_restore);
                *entry
            }
            other => panic!("upgrade prepared: {other:?}"),
        };
        assert_eq!(seen.lock().unwrap().as_slice(), ["BackingUp".to_string()]);
        let backup = PathBuf::from(entry.backup_path.clone().expect("pre-update backup recorded"));
        assert!(backup.is_file());
        assert_eq!(journal::load(&paths.data_dir).unwrap(), Some(entry.clone()));

        // 1.1.0 starts: the journal says verify; its migrations fail half-way.
        assert_eq!(
            journal::classify(Some(&entry), "1.1.0"),
            StartupAction::VerifyNewVersion
        );
        db.execute_unprepared("UPDATE update_marker SET note = 'half-migrated';")
            .await
            .unwrap();

        // A backup that changed since it was taken, or without a recorded checksum, is never
        // staged: the journal records why, this version stays on the recovery screen, and the
        // recovery reinstall is refused.
        for (case, sha) in [("checksum mismatch", Some("0".repeat(64))), ("no checksum", None)] {
            let mut unverified = entry.clone();
            unverified.backup_sha256 = sha;
            let view = stage_pre_update_backup(&db, &paths, unverified, "Migration failed", &key)
                .await
                .unwrap();
            assert_eq!(view.state, JournalState::RollbackRequired, "{case}");
            assert!(
                view.backup_refusal.as_deref().is_some_and(|r| r.contains(case)),
                "{case}: {view:?}"
            );
            assert!(
                !crate::backup::restore::pending_restore_path(&paths.live_db).exists(),
                "{case}: nothing staged"
            );
            let stored = journal::load(&paths.data_dir).unwrap();
            assert_eq!(
                journal::classify(stored.as_ref(), "1.1.0"),
                StartupAction::RecoveryPending,
                "{case}: the database is not migrated again"
            );
            assert_eq!(
                reinstall_check(stored, "1.1.0").unwrap(),
                ReinstallCheck::BackupNotVerified,
                "{case}"
            );
        }
        journal::save(&paths.data_dir, &entry).unwrap();

        let view = stage_pre_update_backup(&db, &paths, entry, "Migration failed: m1", &key)
            .await
            .unwrap();
        assert_eq!(view.state, JournalState::RollbackRequired);
        assert_eq!(view.backup_refusal, None);
        assert_eq!(view.failure.as_deref(), Some("Migration failed: m1"));
        assert!(crate::backup::restore::pending_restore_path(&paths.live_db).is_file());
        let stored = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(stored.state, JournalState::RollbackRequired);
        assert!(stored.staged_restore.is_some(), "the journal names the staged backup");
        assert!(
            !reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.0.0").unwrap(),
            "justified"
        );
        assert_eq!(
            journal::classify(Some(&stored), "1.1.0"),
            StartupAction::RecoveryPending
        );
        assert_eq!(
            journal::classify(Some(&stored), "1.0.0"),
            StartupAction::ReturnedToPrevious
        );

        // The rollback request is refused (409, 204 or another version): nothing else is tried,
        // the refusal is recorded, the backup stays staged and this version stays in recovery.
        record_rollback_refusal(
            &paths.data_dir,
            "the server refused the rollback request",
            Some("1.0.1"),
        )
        .unwrap();
        let refused = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(refused.state, JournalState::RollbackRequired);
        assert_eq!(
            refused.rollback_refusal.as_deref(),
            Some("the server refused the rollback request")
        );
        assert_eq!(
            refused.rollback_allowed_version.as_deref(),
            Some("1.0.1"),
            "kept as information only"
        );
        assert_eq!(
            journal::classify(Some(&refused), "1.1.0"),
            StartupAction::RecoveryPending
        );
        assert!(crate::backup::restore::pending_restore_path(&paths.live_db).is_file());
        assert_eq!(
            marker(&db).await.as_deref(),
            Some("half-migrated"),
            "the live database is not touched while the recovery is pending"
        );
        db.close().await.unwrap();

        // The previous version starts: the staged backup is swapped in before the database opens.
        assert!(crate::backup::restore::apply_pending_restore(&paths.live_db).unwrap());
        let reopened = crate::db::connect_sqlcipher(&paths.live_db, Some(&key), false)
            .await
            .unwrap();
        assert_eq!(marker(&reopened).await.as_deref(), Some("before"));
        reopened.close().await.unwrap();
    }

    /// The installer did not start after a rollback staged a restore: the restore is cancelled
    /// and the journal records that nothing was installed (no notice to show).
    #[test]
    fn an_install_that_did_not_start_cancels_its_staged_restore() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        std::fs::write(crate::backup::restore::pending_restore_path(&paths.live_db), b"staged").unwrap();
        let mut entry = completed_upgrade(None, 1);
        entry.state = JournalState::Installing;
        entry.staged_restore = Some(staged_record(&paths, b"staged"));
        record_install_not_started(&paths, &mut entry, "download failed").unwrap();
        assert!(!crate::backup::restore::pending_restore_path(&paths.live_db).exists());
        let stored = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(stored.state, JournalState::InstallNotCompleted);
        assert!(stored.notice_acknowledged);
        assert_eq!(stored.failure.as_deref(), Some("download failed"));
        assert_eq!(stored.staged_restore, None, "cancelled: the journal no longer names it");
    }

    /// The vendor console's manifest (vendor-console `routes/updates.ts`): `mandatory_reason` and
    /// `min_version` may be null, and `pub_date` absent.
    #[test]
    fn extension_matches_the_vendor_manifest_contract() {
        let raw = serde_json::json!({
            "version": "1.2.0", "notes": "EN", "url": "https://objects.example/presigned", "signature": "s",
            "maintafox": { "release_id": "rel_abc", "kind": "upgrade", "mandatory": false,
                           "mandatory_reason": null, "min_version": null, "notes_en": "EN", "notes_fr": "FR" }
        });
        let ext = parse_extension(&raw).unwrap();
        assert_eq!(ext.kind, UpdateKind::Upgrade);
        assert_eq!(ext.mandatory_reason, None);
        assert_eq!(ext.mandatory_message, None, "absent: none");
        assert_eq!(ext.min_version, None);
        let forced = serde_json::json!({
            "version": "1.2.0", "url": "https://objects.example/presigned", "signature": "s",
            "maintafox": { "release_id": "rel_abc", "kind": "upgrade", "mandatory": true,
                           "mandatory_reason": "forced_update_floor", "mandatory_message": "Security fix",
                           "min_version": "1.0", "notes_en": "EN", "notes_fr": "FR" }
        });
        let ext = parse_extension(&forced).unwrap();
        assert_eq!(ext.mandatory_reason.as_deref(), Some("forced_update_floor"));
        assert_eq!(ext.mandatory_message.as_deref(), Some("Security fix"));
        assert_eq!(ext.min_version.as_deref(), Some("1.0"));
        assert!(
            parse_extension(
                &serde_json::json!({ "maintafox": { "kind": "upgrade", "mandatory": false,
                "notes_en": "", "notes_fr": "" } })
            )
            .is_err(),
            "release_id is always present"
        );
        assert!(
            parse_extension(
                &serde_json::json!({ "maintafox": { "release_id": "rel_abc", "kind": "sideways",
                "mandatory": false, "notes_en": "", "notes_fr": "" } })
            )
            .is_err(),
            "an unknown kind is refused"
        );
    }

    fn ext(kind: UpdateKind, reason: Option<&str>) -> ManifestExtension {
        ManifestExtension {
            release_id: "rel_1".into(),
            kind,
            mandatory: reason.is_some(),
            mandatory_reason: reason.map(str::to_string),
            mandatory_message: None,
            min_version: None,
            notes_en: String::new(),
            notes_fr: String::new(),
        }
    }

    /// Security review of vendor-console PR #3: never an arbitrary different version.
    #[test]
    fn only_the_offers_this_device_asked_for_are_accepted() {
        let upgrade = ext(UpdateKind::Upgrade, None);
        let recalled = ext(UpdateKind::Rollback, Some(REASON_RECALLED_VERSION));
        let other_rollback = ext(UpdateKind::Rollback, Some("emergency"));
        // No rollback requested.
        assert!(accept_offer("1.1.0", "1.2.0", &upgrade, None).is_ok());
        assert!(
            accept_offer("0.4.0-dev", "0.4.0", &upgrade, None).is_ok(),
            "pre-release below its release"
        );
        assert!(
            accept_offer("1.1.0", "1.0.0", &upgrade, None).is_err(),
            "an 'upgrade' to an older version"
        );
        assert!(accept_offer("1.1.0", "1.1.0", &upgrade, None).is_err());
        assert!(
            accept_offer("1.1.0", "1.0.0", &recalled, None).is_ok(),
            "rollback off a recalled version"
        );
        assert!(
            accept_offer("1.1.0", "1.2.0", &recalled, None).is_err(),
            "a 'rollback' to a newer one"
        );
        assert!(
            accept_offer("1.1.0", "1.0.0", &other_rollback, None).is_err(),
            "rollback without recall"
        );
        assert!(
            accept_offer("1.1.0", "latest", &upgrade, None).is_err(),
            "unreadable version"
        );
        // Rollback requested after a failed migration.
        let rollback = ext(UpdateKind::Rollback, Some("rollback_requested"));
        assert!(accept_offer("1.1.0", "1.0.0", &rollback, Some("1.0.0")).is_ok());
        assert!(
            accept_offer("1.1.0", "0.9.0", &rollback, Some("1.0.0")).is_err(),
            "version mismatch"
        );
        assert!(
            accept_offer("1.1.0", "1.2.0", &upgrade, Some("1.0.0")).is_err(),
            "an upgrade answer to a rollback request"
        );
        assert!(
            accept_offer("1.1.0", "1.0.0", &upgrade, Some("1.0.0")).is_err(),
            "the requested version, but not as a rollback"
        );
    }

    /// 409 (refusal status), 204, a version mismatch and an upgrade answer to the rollback request
    /// all end in a recorded refusal; nothing else is tried.
    #[test]
    fn every_unacceptable_rollback_answer_is_a_refusal() {
        let mismatch = accept_offer(
            "1.1.0",
            "0.9.0",
            &ext(UpdateKind::Rollback, Some("rollback_requested")),
            Some("1.0.0"),
        )
        .unwrap_err();
        let upgrade = accept_offer("1.1.0", "1.2.0", &ext(UpdateKind::Upgrade, None), Some("1.0.0")).unwrap_err();
        for (case, answer) in [
            ("409", ManifestAnswer::ServerRefused),
            ("204", ManifestAnswer::None),
            ("version mismatch", ManifestAnswer::NotAcceptable(mismatch)),
            ("upgrade answer", ManifestAnswer::NotAcceptable(upgrade)),
            ("not activated", ManifestAnswer::NotActivated),
        ] {
            assert!(
                matches!(rollback_answer(&answer, "1.0.0"), RollbackAnswer::Refused(_)),
                "{case}"
            );
        }
        // Outside RollbackRequired nothing is recorded.
        let dir = tempfile::tempdir().unwrap();
        let done = completed_upgrade(None, 1);
        journal::save(dir.path(), &done).unwrap();
        record_rollback_refusal(dir.path(), "x", None).unwrap();
        assert_eq!(journal::load(dir.path()).unwrap(), Some(done));
    }

    /// The 409 body: `allowed_version` is read when it is a plain release version, nothing else.
    #[test]
    fn a_refusal_body_names_the_allowed_version_as_information_only() {
        let body = parse_refusal_body(r#"{"error":"rollback_target_not_allowed","allowed_version":"1.0.1"}"#);
        assert_eq!(body.error.as_deref(), Some("rollback_target_not_allowed"));
        assert_eq!(body.allowed_version.as_deref(), Some("1.0.1"));
        for odd in [
            r#"{"allowed_version":"1.0.1-rc.1"}"#,
            r#"{"allowed_version":"<b>1</b>"}"#,
            "not json",
            "",
        ] {
            assert_eq!(parse_refusal_body(odd).allowed_version, None, "{odd}");
        }
        assert!(is_plain_release_version("10.2.30"));
        assert!(!is_plain_release_version("1.2"));
        assert!(!is_plain_release_version("0.4.0-dev"));
    }

    // ── Security review fixes (B1, S1, S2, S8, N1, N2) ──

    fn minisign_text(trusted: &str) -> String {
        use base64::Engine;
        let text = format!(
            "untrusted comment: signature from tauri secret key\nRUQ...\ntrusted comment: {trusted}\nglobal...\n"
        );
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    /// S2: the signature's trusted comment must name the installer of the offered version.
    #[test]
    fn the_signature_must_name_the_offered_installer() {
        use base64::Engine;
        let sig = minisign_text("timestamp:1760000000\tfile:Maintafox_1.2.0_x64-setup.exe");
        assert_eq!(signed_file_name(&sig).unwrap(), "Maintafox_1.2.0_x64-setup.exe");
        assert!(check_signed_file(&sig, "1.2.0").is_ok());
        let err = check_signed_file(&sig, "1.3.0").unwrap_err();
        assert!(err.contains("Maintafox_1.3.0_x64-setup.exe"), "{err}");
        // An old installer signed under its own name, offered as a newer version.
        let old = minisign_text("timestamp:1750000000\tfile:Maintafox_1.0.0_x64-setup.exe");
        assert!(check_signed_file(&old, "1.2.0").is_err());
        assert!(
            check_signed_file(
                &minisign_text("timestamp:1\tfile:maintafox_1.2.0_x64-setup.exe"),
                "1.2.0"
            )
            .is_err(),
            "case-sensitive"
        );
        assert!(
            check_signed_file(&minisign_text("timestamp:1"), "1.2.0").is_err(),
            "no file field"
        );
        assert!(check_signed_file("not base64 !!", "1.2.0").is_err());
        let no_trusted = base64::engine::general_purpose::STANDARD.encode("untrusted comment: x\nRUQ\n");
        assert!(check_signed_file(&no_trusted, "1.2.0").is_err(), "no trusted comment");
    }

    /// S1: the install is bound to the version, kind and release the administrator confirmed.
    #[test]
    fn the_install_is_bound_to_the_confirmed_offer() {
        let confirmed = ConfirmedOffer {
            version: "1.2.0".into(),
            kind: UpdateKind::Upgrade,
            release_id: "rel_1".into(),
        };
        let upgrade = ext(UpdateKind::Upgrade, None);
        let recalled = ext(UpdateKind::Rollback, Some(REASON_RECALLED_VERSION));
        assert!(offer_matches_confirmation("1.2.0", &upgrade, &confirmed));
        assert!(
            !offer_matches_confirmation("1.0.0", &recalled, &confirmed),
            "recalled since the confirmation: a rollback is never installed silently"
        );
        assert!(
            !offer_matches_confirmation("1.2.1", &upgrade, &confirmed),
            "another version"
        );
        let mut other_release = upgrade.clone();
        other_release.release_id = "rel_2".into();
        assert!(
            !offer_matches_confirmation("1.2.0", &other_release, &confirmed),
            "another release"
        );
        assert!(
            !offer_matches_confirmation("1.2.0", &recalled, &confirmed),
            "kind differs"
        );
        let mut confirmed_rollback = confirmed.clone();
        confirmed_rollback.kind = UpdateKind::Rollback;
        assert!(offer_matches_confirmation("1.2.0", &recalled, &confirmed_rollback));
    }

    /// N2: one install or reinstall at a time.
    #[test]
    fn only_one_install_runs_at_a_time() {
        static FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let first = InstallSlot::acquire_on(&FLAG).unwrap();
        assert!(matches!(
            InstallSlot::acquire_on(&FLAG),
            Err(AppError::ValidationFailed(_))
        ));
        drop(first);
        assert!(InstallSlot::acquire_on(&FLAG).is_ok(), "released when the install ends");
    }

    /// N1 and S8: the sessionless reinstall asks only on the version that failed to migrate, and
    /// never when the pre-update backup could not be verified.
    #[test]
    fn the_recovery_reinstall_needs_the_failed_version_and_a_verified_backup() {
        let mut failed = completed_upgrade(None, 1);
        failed.state = JournalState::RollbackRequired;
        assert!(matches!(
            reinstall_check(Some(failed.clone()), "1.1.0").unwrap(),
            ReinstallCheck::Ask(_)
        ));
        assert!(
            reinstall_check(Some(failed.clone()), "1.0.0").is_err(),
            "not the failed version"
        );
        assert!(reinstall_check(Some(failed.clone()), "1.2.0").is_err());
        assert!(
            reinstall_check(Some(completed_upgrade(None, 1)), "1.1.0").is_err(),
            "no failure"
        );
        assert!(reinstall_check(None, "1.1.0").is_err());
        failed.backup_refusal = Some("checksum mismatch".into());
        assert_eq!(
            reinstall_check(Some(failed), "1.1.0").unwrap(),
            ReinstallCheck::BackupNotVerified
        );
    }

    /// What the updater records for a staged restore whose content is `content`.
    fn staged_record(paths: &UpdatePaths, content: &[u8]) -> journal::StagedRestore {
        let pending = crate::backup::restore::pending_restore_path(&paths.live_db);
        let scratch = paths.live_db.with_extension("record-content");
        std::fs::write(&scratch, content).unwrap();
        let sha256 = crate::backup::file_sha256(&scratch).unwrap();
        std::fs::remove_file(&scratch).unwrap();
        journal::StagedRestore {
            pending_path: pending.to_string_lossy().to_string(),
            sha256,
        }
    }

    /// B1, SF2 at startup: a restore the updater staged is applied only when the journal justifies
    /// it, and cancelled otherwise before the database opens (the journal stops naming it, an
    /// `Installing` entry is closed); a restore the journal does not name is an administrator's.
    #[test]
    fn startup_applies_a_staged_restore_only_when_the_journal_justifies_it() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let pending = crate::backup::restore::pending_restore_path(&paths.live_db);
        std::fs::write(&paths.live_db, b"live with today's work").unwrap();
        let mut rollback = completed_upgrade(None, 1);
        rollback.kind = UpdateKind::Rollback;
        rollback.state = JournalState::Installing;
        rollback.from_version = "1.1.0".into();
        rollback.to_version = "1.0.0".into();
        rollback.staged_restore = Some(staged_record(&paths, b"old backup"));

        // The installer was cancelled: 1.1.0 starts again.
        journal::save(&paths.data_dir, &rollback).unwrap();
        std::fs::write(&pending, b"old backup").unwrap();
        assert!(reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(!pending.exists());
        assert!(!crate::backup::restore::apply_pending_restore(&paths.live_db).unwrap());
        assert_eq!(std::fs::read(&paths.live_db).unwrap(), b"live with today's work");
        let closed = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(closed.state, JournalState::InstallNotCompleted);
        assert!(closed.failure.clone().unwrap().contains("cancelled"));
        assert_eq!(closed.staged_restore, None);
        assert_eq!(journal::classify(Some(&closed), "1.1.0"), StartupAction::Nothing);

        // The rollback was installed: 1.0.0 starts on the staged backup.
        journal::save(&paths.data_dir, &rollback).unwrap();
        std::fs::write(&pending, b"old backup").unwrap();
        assert!(!reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.0.0").unwrap());
        assert!(pending.exists(), "justified: applied");

        // A failed database update: applied whichever version starts.
        let mut failed = rollback.clone();
        failed.state = JournalState::RollbackRequired;
        journal::save(&paths.data_dir, &failed).unwrap();
        assert!(!reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(pending.exists());

        // A closed entry still naming the restore (its cancel failed): cancelled, state kept.
        for state in [JournalState::Completed, JournalState::InstallNotCompleted] {
            let mut abandoned = rollback.clone();
            abandoned.state = state;
            journal::save(&paths.data_dir, &abandoned).unwrap();
            std::fs::write(&pending, b"old backup").unwrap();
            assert!(
                reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.0.0").unwrap(),
                "{state:?}"
            );
            assert!(!pending.exists(), "{state:?}");
            let stored = journal::load(&paths.data_dir).unwrap().unwrap();
            assert_eq!(stored.state, state);
            assert_eq!(stored.staged_restore, None);
        }

        // An administrator staged another backup after the updater's: theirs is applied.
        journal::save(&paths.data_dir, &rollback).unwrap();
        std::fs::write(&pending, b"the administrator's backup").unwrap();
        assert!(!reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(pending.exists());

        // The journal names no staged restore (an administrator's, Settings > Backup).
        let mut plain = rollback.clone();
        plain.staged_restore = None;
        journal::save(&paths.data_dir, &plain).unwrap();
        assert!(!reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(pending.exists());

        // No journal: a restore an administrator staged.
        std::fs::remove_file(journal::journal_path(&paths.data_dir)).unwrap();
        assert!(!reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(pending.exists());

        // An unreadable journal with a staged restore: the restore is not applied blindly.
        std::fs::write(journal::journal_path(&paths.data_dir), "{not json").unwrap();
        assert!(reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").is_err());
    }

    /// An administrator's restore (Settings > Backup) replaces the updater's: the journal stops
    /// naming one, so it is applied even when it is the same backup file.
    #[test]
    fn an_administrator_restore_is_never_taken_for_the_updater_s() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let pending = crate::backup::restore::pending_restore_path(&paths.live_db);
        let mut abandoned = completed_upgrade(None, 1);
        abandoned.staged_restore = Some(staged_record(&paths, b"pre-update backup"));
        journal::save(&paths.data_dir, &abandoned).unwrap();
        std::fs::write(&pending, b"pre-update backup").unwrap();
        forget_staged_restore(&paths.data_dir).unwrap();
        assert_eq!(journal::load(&paths.data_dir).unwrap().unwrap().staged_restore, None);
        assert!(!reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(pending.exists(), "the administrator's restore is applied");
        forget_staged_restore(&tempfile::tempdir().unwrap().path().join("none")).unwrap();
    }

    /// Makes the staged restore impossible to remove (a folder stands where the file was), as an
    /// antivirus lock would; `unlock` puts the staged file back.
    fn lock_pending(pending: &Path) -> PathBuf {
        let kept = pending.with_extension("kept");
        std::fs::rename(pending, &kept).unwrap();
        std::fs::create_dir(pending).unwrap();
        std::fs::write(pending.join("busy"), b"x").unwrap();
        kept
    }

    fn unlock_pending(pending: &Path, kept: &Path) {
        std::fs::remove_dir_all(pending).unwrap();
        std::fs::rename(kept, pending).unwrap();
    }

    /// SF2, first cancel path: a refusal or failure after the staging cannot remove the staged
    /// restore. The journal written before the staging names it and does not justify it, so the
    /// next start cancels it; no update runs before that restart.
    #[tokio::test]
    async fn a_staging_that_cannot_be_withdrawn_is_cancelled_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, key, db, target) = recall_setup(dir.path()).await;
        let pending = crate::backup::restore::pending_restore_path(&paths.live_db);
        let completed = journal::load(&paths.data_dir).unwrap().unwrap();
        let load_key = move || Ok(key.clone());

        // The journal names the restore before it is staged.
        let data_dir = paths.data_dir.clone();
        let named_before_staging = std::sync::atomic::AtomicBool::new(false);
        let progress = |event: UpdateProgressEvent| {
            if matches!(event, UpdateProgressEvent::RestoringBackup) {
                let j = journal::load(&data_dir).unwrap().unwrap();
                named_before_staging.store(
                    j.staged_restore.is_some() && j.state == JournalState::Completed,
                    std::sync::atomic::Ordering::SeqCst,
                );
            }
        };
        let prepared = prepare_install(&db, &paths, &target, None, &load_key, &progress)
            .await
            .unwrap();
        assert!(named_before_staging.load(std::sync::atomic::Ordering::SeqCst));
        assert!(matches!(
            prepared,
            Prepared::Ready {
                staged_restore: true,
                ..
            }
        ));
        assert!(pending.is_file());

        // Withdrawn normally: removed, and the journal is the completed upgrade again.
        journal::save(&paths.data_dir, &{
            let mut marked = completed.clone();
            marked.staged_restore = journal::load(&paths.data_dir).unwrap().unwrap().staged_restore;
            marked
        })
        .unwrap();
        assert!(withdraw_staged_restore(&paths, &completed));
        assert!(!pending.exists());
        assert_eq!(journal::load(&paths.data_dir).unwrap(), Some(completed.clone()));

        // The cancel fails: the marked journal stays and the restore stays pending.
        remove_safety_backups(&paths.backup_dir);
        let prepared = prepare_install(&db, &paths, &target, None, &load_key, &|_| {})
            .await
            .unwrap();
        let Prepared::Ready { entry, .. } = prepared else {
            panic!("staged")
        };
        let mut marked = completed.clone();
        marked.staged_restore = entry.staged_restore.clone();
        journal::save(&paths.data_dir, &marked).unwrap();
        let kept = lock_pending(&pending);
        assert!(!withdraw_staged_restore(&paths, &completed));
        let stored = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(stored.state, JournalState::Completed);
        assert!(stored.staged_restore.is_some(), "the journal still names it");

        // No update runs before the restart.
        let outcome = run_install(
            &db,
            &paths,
            &target,
            None,
            &load_key,
            &|_| {},
            &TEST_BARRIER,
            async { never_downloaded() },
            |_| -> AppResult<()> { panic!("never installed") },
        )
        .await
        .unwrap();
        assert!(matches!(
            outcome,
            InstallOutcome::Refused {
                refusal: InstallRefusal::RestorePending
            }
        ));

        // The next start (1.1.0, nothing installed) cancels it and keeps the completed upgrade.
        unlock_pending(&pending, &kept);
        assert!(reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(!pending.exists());
        assert_eq!(journal::load(&paths.data_dir).unwrap(), Some(completed));
        db.close().await.unwrap();
    }

    /// SF2, second cancel path: the installer did not start and its staged restore cannot be
    /// removed. The journal says the install did not complete and still names the restore, so the
    /// next start cancels it instead of applying it.
    #[tokio::test]
    async fn a_failed_installer_whose_restore_cannot_be_cancelled_is_cancelled_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, key, db, target) = recall_setup(dir.path()).await;
        let pending = crate::backup::restore::pending_restore_path(&paths.live_db);
        let load_key = move || Ok(key.clone());
        let kept = std::sync::Mutex::new(None);
        let install = |_: Vec<u8>| -> AppResult<()> {
            *kept.lock().unwrap() = Some(lock_pending(&pending));
            Err(AppError::Internal(anyhow::anyhow!(
                "the installer could not be started"
            )))
        };
        let result = run_install(
            &db,
            &paths,
            &target,
            None,
            &load_key,
            &|_| {},
            &TEST_BARRIER,
            async { Ok(Vec::new()) },
            install,
        )
        .await;
        assert!(result.is_err(), "the cancel failure is reported");
        let stored = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(stored.state, JournalState::InstallNotCompleted);
        assert!(stored.staged_restore.is_some(), "the journal still names the restore");
        assert!(!journal::staged_restore_is_justified(&stored, "1.1.0"));
        assert!(!journal::staged_restore_is_justified(&stored, "1.0.0"));

        unlock_pending(&pending, &kept.lock().unwrap().take().unwrap());
        assert!(reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.1.0").unwrap());
        assert!(!pending.exists());
        assert!(!crate::backup::restore::apply_pending_restore(&paths.live_db).unwrap());
        let closed = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(closed.state, JournalState::InstallNotCompleted);
        assert_eq!(closed.staged_restore, None);
        db.close().await.unwrap();
    }

    /// N1: an update does not start while a restore is staged for the next start, and one staged
    /// during the download stops it before the backup.
    #[tokio::test]
    async fn no_update_runs_while_a_restore_is_staged() {
        let db = crate::db::test_db::migrated().await;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let pending = crate::backup::restore::pending_restore_path(&paths.live_db);
        std::fs::write(&pending, b"the administrator's restore").unwrap();
        let outcome = run_install(
            &db,
            &paths,
            &upgrade_target(),
            Some(1),
            &no_key,
            &|_| {},
            &TEST_BARRIER,
            async { never_downloaded() },
            |_| -> AppResult<()> { panic!("never installed") },
        )
        .await
        .unwrap();
        assert!(matches!(
            outcome,
            InstallOutcome::Refused {
                refusal: InstallRefusal::RestorePending
            }
        ));
        std::fs::remove_file(&pending).unwrap();

        let staged_meanwhile = async {
            std::fs::write(&pending, b"staged during the download").unwrap();
            Ok(b"verified installer".to_vec())
        };
        let (seen, progress) = recording();
        let outcome = run_install(
            &db,
            &paths,
            &upgrade_target(),
            Some(1),
            &no_key,
            &progress,
            &TEST_BARRIER,
            staged_meanwhile,
            |_| -> AppResult<()> { panic!("never installed") },
        )
        .await
        .unwrap();
        assert!(matches!(
            outcome,
            InstallOutcome::Refused {
                refusal: InstallRefusal::RestorePending
            }
        ));
        assert!(seen.lock().unwrap().is_empty(), "no backup was taken");
        assert_eq!(journal::load(&paths.data_dir).unwrap(), None);
    }

    /// SF1: the write barrier is up from the backup until the installer, down during the
    /// download and again when the install stops.
    #[test]
    fn the_write_barrier_refuses_commands_while_it_is_up() {
        static FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        assert!(refuse_while_installing_on(&FLAG).is_ok());
        let barrier = WriteBarrier::raise_on(&FLAG);
        match refuse_while_installing_on(&FLAG) {
            Err(AppError::ValidationFailed(messages)) => {
                assert_eq!(messages, vec![UPDATE_IN_PROGRESS_MESSAGE.to_string()]);
            }
            other => panic!("refused while installing: {other:?}"),
        }
        drop(barrier);
        assert!(
            refuse_while_installing_on(&FLAG).is_ok(),
            "lowered when the install stops"
        );
    }

    type Log = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

    fn logging(log: &Log) -> impl Fn(UpdateProgressEvent) + Sync {
        let sink = log.clone();
        move |event: UpdateProgressEvent| sink.lock().unwrap().push(format!("{event:?}"))
    }

    /// B1, upgrade: the download comes first and the backup after it, so a work order closed
    /// during the download is in the pre-update backup; the installer starts right after the
    /// journal is written.
    #[tokio::test]
    async fn an_upgrade_backs_up_after_the_download_so_changes_made_meanwhile_are_kept() {
        // Its own barrier: TEST_BARRIER is shared by tests that run in parallel, and this test
        // asserts when the barrier is down.
        static BARRIER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        use sea_orm::ConnectionTrait;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let key = crate::db::encryption::DatabaseKey::generate();
        let db = encrypted_live_db(&paths.live_db, &key).await;
        db.execute_unprepared("CREATE TABLE update_marker (note TEXT); INSERT INTO update_marker VALUES ('before');")
            .await
            .unwrap();

        let log: Log = Default::default();
        let progress = logging(&log);
        let download = async {
            log.lock().unwrap().push("download".into());
            assert!(
                refuse_while_installing_on(&BARRIER).is_ok(),
                "the app stays usable during the download"
            );
            // The app stays usable during the download: a technician closes a work order.
            db.execute_unprepared("UPDATE update_marker SET note = 'closed during the download';")
                .await
                .unwrap();
            Ok(b"verified installer".to_vec())
        };
        let data_dir = paths.data_dir.clone();
        let install_log = log.clone();
        let install = move |bytes: Vec<u8>| {
            assert_eq!(bytes, b"verified installer");
            let j = journal::load(&data_dir)
                .unwrap()
                .expect("journal written before the installer");
            assert_eq!(j.state, JournalState::Installing);
            assert!(j.backup_path.is_some() && j.backup_sha256.is_some());
            assert!(
                refuse_while_installing_on(&BARRIER).is_err(),
                "nothing can be changed from the backup on"
            );
            install_log.lock().unwrap().push("install".into());
            Ok(())
        };
        let outcome = run_install(
            &db,
            &paths,
            &upgrade_target(),
            Some(1),
            &no_key,
            &progress,
            &BARRIER,
            download,
            install,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, InstallOutcome::Restarting));
        assert_eq!(
            log.lock().unwrap().as_slice(),
            ["download", "BackingUp", "Installing", "install"],
            "download, then backup, then the installer at once"
        );
        let entry = journal::load(&paths.data_dir).unwrap().unwrap();
        let backup = PathBuf::from(entry.backup_path.unwrap());
        let copy = crate::db::connect_sqlcipher(&backup, Some(&key), false).await.unwrap();
        assert_eq!(
            marker(&copy).await.as_deref(),
            Some("closed during the download"),
            "the pre-update backup holds the change made during the download"
        );
        copy.close().await.unwrap();
        db.close().await.unwrap();
    }

    /// A failed download changes nothing: no journal entry, no backup step.
    #[tokio::test]
    async fn a_failed_download_writes_nothing() {
        let db = crate::db::test_db::migrated().await;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let log: Log = Default::default();
        let progress = logging(&log);
        let download = async { Err(AppError::Internal(anyhow::anyhow!("signature check failed"))) };
        let install = |_: Vec<u8>| -> AppResult<()> { panic!("nothing is installed") };
        let result = run_install(
            &db,
            &paths,
            &upgrade_target(),
            Some(1),
            &no_key,
            &progress,
            &TEST_BARRIER,
            download,
            install,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(journal::load(&paths.data_dir).unwrap(), None);
        assert!(log.lock().unwrap().is_empty(), "no backup was taken");
    }

    /// A recall rollback on a real encrypted database: 1.0.0's pre-update backup (checksum
    /// recorded) and a Completed journal, then 1.1.0 is running with one more migration.
    async fn recall_setup(
        dir: &Path,
    ) -> (
        UpdatePaths,
        crate::db::encryption::DatabaseKey,
        sea_orm::DatabaseConnection,
        InstallTarget,
    ) {
        use sea_orm::ConnectionTrait;
        let paths = paths_in(dir);
        let key = crate::db::encryption::DatabaseKey::generate();
        let db = encrypted_live_db(&paths.live_db, &key).await;
        db.execute_unprepared("CREATE TABLE update_marker (note TEXT); INSERT INTO update_marker VALUES ('1.0.0');")
            .await
            .unwrap();
        std::fs::create_dir_all(&paths.backup_dir).unwrap();
        let backup_path = paths.backup_dir.join("maintafox-pre-update-1.0.0-to-1.1.0.db");
        let backup = crate::backup::run_manual_backup(
            &db,
            &backup_path.to_string_lossy(),
            crate::backup::TRIGGER_PRE_UPDATE,
            None,
        )
        .await
        .unwrap();
        let schema = applied_migration_count(&db).await.unwrap();
        let mut done = completed_upgrade(Some(&backup_path), schema - 1);
        done.backup_sha256 = Some(backup.sha256_checksum);
        journal::save(&paths.data_dir, &done).unwrap();
        let target = InstallTarget {
            running: "1.1.0".into(),
            version: "1.0.0".into(),
            kind: UpdateKind::Rollback,
        };
        (paths, key, db, target)
    }

    fn remove_safety_backups(backup_dir: &Path) {
        for entry in std::fs::read_dir(backup_dir).unwrap().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("maintafox-pre-restore-")
            {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
    }

    /// B1, recall rollback: a change made during the download is waiting to sync, so the restore
    /// would lose it: refused after the download, nothing staged, the journal unchanged.
    #[tokio::test]
    async fn a_rollback_refuses_when_changes_were_made_during_the_download() {
        use sea_orm::ConnectionTrait;
        let dir = tempfile::tempdir().unwrap();
        let (paths, key, db, target) = recall_setup(dir.path()).await;
        let before = journal::load(&paths.data_dir).unwrap();
        let load_key = move || Ok(key.clone());
        let (_, progress) = recording();
        let download = async {
            // A change captured for sync while the installer downloads.
            db.execute_unprepared(
                "INSERT INTO sync_capture_journal (entity_type, entity_sync_id, operation) \
                 VALUES ('work_orders', 'wo-1', 'upsert')",
            )
            .await
            .unwrap();
            Ok(b"verified installer".to_vec())
        };
        let install = |_: Vec<u8>| -> AppResult<()> { panic!("never installed") };
        let outcome = run_install(
            &db,
            &paths,
            &target,
            None,
            &load_key,
            &progress,
            &TEST_BARRIER,
            download,
            install,
        )
        .await
        .unwrap();
        assert!(
            matches!(
                outcome,
                InstallOutcome::Refused {
                    refusal: InstallRefusal::RollbackUnsyncedChanges { count: 1 }
                }
            ),
            "{outcome:?}"
        );
        assert!(!crate::backup::restore::pending_restore_path(&paths.live_db).exists());
        assert_eq!(journal::load(&paths.data_dir).unwrap(), before);
        db.close().await.unwrap();
    }

    /// B1, recall rollback: the restore is staged only after the download and the installer
    /// starts right after; if it cannot start, the staged restore is cancelled. S8: a backup that
    /// changed since it was taken is never staged.
    #[tokio::test]
    async fn a_rollback_stages_its_restore_after_the_download_and_cancels_it_if_the_installer_fails() {
        // Its own barrier: TEST_BARRIER is shared by tests that run in parallel, and this test
        // asserts when the barrier is down.
        static BARRIER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let (paths, key, db, target) = recall_setup(dir.path()).await;
        let pending = crate::backup::restore::pending_restore_path(&paths.live_db);
        let completed = journal::load(&paths.data_dir).unwrap().unwrap();
        let load_key = move || Ok(key.clone());

        // The installer cannot start.
        let log: Log = Default::default();
        let progress = logging(&log);
        let pending_at_download = AtomicBool::new(true);
        let download = async {
            pending_at_download.store(pending.exists(), Ordering::SeqCst);
            Ok(b"verified installer".to_vec())
        };
        let staged_at_install = AtomicBool::new(false);
        let install = |_: Vec<u8>| -> AppResult<()> {
            staged_at_install.store(pending.is_file(), Ordering::SeqCst);
            Err(AppError::Internal(anyhow::anyhow!(
                "the installer could not be started"
            )))
        };
        let result = run_install(
            &db, &paths, &target, None, &load_key, &progress, &BARRIER, download, install,
        )
        .await;
        assert!(result.is_err());
        assert!(
            !pending_at_download.load(Ordering::SeqCst),
            "nothing staged during the download"
        );
        assert!(
            staged_at_install.load(Ordering::SeqCst),
            "staged right before the installer"
        );
        assert!(!pending.exists(), "a failed install leaves no staged restore");
        let j = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(j.state, JournalState::InstallNotCompleted);
        assert_eq!(j.staged_restore, None);
        assert_eq!(j.kind, UpdateKind::Rollback);
        assert_eq!(log.lock().unwrap().as_slice(), ["RestoringBackup", "Installing"]);

        // The installer starts (the process would exit): the restore stays staged for 1.0.0.
        // (The restore service names its safety backup by the second: the first one is removed so
        // a second staging within the same second does not collide with it.)
        remove_safety_backups(&paths.backup_dir);
        journal::save(&paths.data_dir, &completed).unwrap();
        let (_, progress) = recording();
        let outcome = run_install(
            &db,
            &paths,
            &target,
            None,
            &load_key,
            &progress,
            &BARRIER,
            async { Ok(Vec::new()) },
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, InstallOutcome::Restarting));
        assert!(pending.is_file());
        let installing = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(installing.state, JournalState::Installing);
        let record = installing
            .staged_restore
            .clone()
            .expect("the journal names the staged restore");
        assert!(backup_matches_checksum(&pending, &record.sha256).unwrap());
        assert!(
            !reconcile_staged_restore(&paths.data_dir, &paths.live_db, "1.0.0").unwrap(),
            "applied by the rollback's version"
        );
        crate::backup::restore::cancel_pending_restore(&paths.live_db).unwrap();

        // S8: the pre-update backup changed since it was taken.
        let mut tampered = completed.clone();
        tampered.backup_sha256 = Some("0".repeat(64));
        journal::save(&paths.data_dir, &tampered).unwrap();
        let (_, progress) = recording();
        let outcome = run_install(
            &db,
            &paths,
            &target,
            None,
            &load_key,
            &progress,
            &BARRIER,
            async { Ok(Vec::new()) },
            |_| -> AppResult<()> { panic!("never installed") },
        )
        .await
        .unwrap();
        assert!(matches!(
            outcome,
            InstallOutcome::Refused {
                refusal: InstallRefusal::RollbackBackupChanged
            }
        ));
        assert!(!pending.exists());
        assert!(
            refuse_while_installing_on(&BARRIER).is_ok(),
            "the barrier comes down when the install stops"
        );
        db.close().await.unwrap();
    }
}
