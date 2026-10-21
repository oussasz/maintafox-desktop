//! In-app updates (master plan Phase 5.5).
//!
//! - **Check:** the control API's manifest `GET /api/v1/updates/{target}/{arch}/{version}` with this
//!   device's activation token. The server decides (ring, rollout %, pause, recall, minimum
//!   version) and answers in the Tauri updater format plus a `maintafox` object: upgrade or
//!   rollback, mandatory or not, notes in French and English.
//! - **Install:** the manifest is checked again right before downloading (a recall or pause stops
//!   the install), the database is backed up (`pre_update` run, Phase 1.3 backup service), the
//!   journal (`journal.rs`) is written, the signed installer is downloaded with progress and
//!   verified against the public key built into this app, then installed (the app restarts).
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
/// Header asking the manifest for the version this device came from (failed database update).
pub const ROLLBACK_HEADER: &str = "X-Maintafox-Rollback-To";
/// NSIS update mode: progress window only, no questions (the user already confirmed in the app).
const WINDOWS_INSTALL_MODE: &str = "basicUi";

// ── Plugin registration ─────────────────────────────────────────────────────────────────────

/// Adds the updater plugin configuration to the app context when this build has a public key.
/// The key is not in tauri.conf.json: it comes from the owner's key file (no placeholder).
pub fn add_plugin_config<R: Runtime>(context: &mut tauri::Context<R>) {
    if let Some(pubkey) = UPDATER_PUBLIC_KEY {
        context.config_mut().plugins.0.insert(
            "updater".to_string(),
            serde_json::json!({ "pubkey": pubkey, "windows": { "installMode": WINDOWS_INSTALL_MODE } }),
        );
    }
}

/// Registers the updater plugin only when the build has a public key.
pub fn register_plugin<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    if UPDATER_PUBLIC_KEY.is_some() {
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

/// Asks the manifest. What it offers is checked by `accept_offer` here, and the installer's
/// signature against this build's public key before anything is installed.
pub async fn ask_manifest(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    rollback_to: Option<&str>,
) -> AppResult<ManifestAnswer> {
    if UPDATER_PUBLIC_KEY.is_none() {
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
            if let Err(why) = accept_offer(&running_version(), &update.version, &ext, rollback_to) {
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
    /// Migrations ran since `target`: its pre-update backup must be restored first.
    RestoreBackup { backup_path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum InstallRefusal {
    /// No pre-update backup of `target` is known on this device.
    RollbackBackupMissing,
    /// Changes not yet synchronised would be lost by the restore.
    RollbackUnsyncedChanges { count: i64 },
    /// The server no longer offers this update (recalled or paused since the check), or offers
    /// something this device does not accept.
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
    if unsynced_changes > 0 {
        return Err(InstallRefusal::RollbackUnsyncedChanges {
            count: unsynced_changes,
        });
    }
    Ok(DowngradePlan::RestoreBackup { backup_path: path })
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

// ── Install ─────────────────────────────────────────────────────────────────────────────────

/// Progress sent to the window while installing (`UpdateProgressEvent` in shared/ipc-types.ts).
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

/// Outcome of everything that runs before the download.
#[derive(Debug)]
pub enum Prepared {
    /// The journal is written; the download may start. `staged_restore`: a rollback staged the
    /// pre-update backup (to cancel if the installer does not run).
    Ready {
        entry: Box<UpdateJournal>,
        staged_restore: bool,
    },
    /// Nothing was changed.
    Refused(InstallRefusal),
}

/// Loads this installation's database key (the OS credential store in the app).
pub type KeyLoader = dyn Fn() -> AppResult<crate::db::encryption::DatabaseKey> + Sync;

/// Before the download: the pre-update backup (upgrade) or the downgrade plan (rollback), then the
/// journal. Every failure returns before the journal is written, so nothing is downloaded or
/// installed without its backup, and the next start does not believe an update ran.
pub async fn prepare_install(
    db: &sea_orm::DatabaseConnection,
    paths: &UpdatePaths,
    target: &InstallTarget,
    actor: Option<i32>,
    load_key: &KeyLoader,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
) -> AppResult<Prepared> {
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
        notice_acknowledged: false,
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
            let summary = crate::sync::queries::get_sync_state_summary(db).await?;
            let pending = summary.pending_outbox_count + summary.in_flight_outbox_count;
            match plan_downgrade(previous.as_ref(), &target.running, &target.version, schema_now, pending) {
                Err(refusal) => return Ok(Prepared::Refused(refusal)),
                Ok(DowngradePlan::KeepDatabase) => {}
                Ok(DowngradePlan::RestoreBackup { backup_path }) => {
                    progress(UpdateProgressEvent::RestoringBackup);
                    // The restore service's checks (integrity, this installation's key, schema not
                    // newer) and its safety backup of the current database.
                    crate::backup::restore::stage_restore(
                        db,
                        &paths.live_db,
                        &backup_path,
                        &load_key()?,
                        &paths.backup_dir,
                        actor,
                    )
                    .await?;
                    staged_restore = true;
                    entry.backup_path = Some(backup_path.to_string_lossy().to_string());
                }
            }
        }
    }
    journal::save(&paths.data_dir, &entry)?;
    Ok(Prepared::Ready {
        entry: Box::new(entry),
        staged_restore,
    })
}

/// The installer did not start: the journal says so, and a restore staged for the older version
/// that was not installed is removed.
pub fn record_install_not_started(
    paths: &UpdatePaths,
    entry: &mut UpdateJournal,
    staged_restore: bool,
    failure: &str,
) -> AppResult<()> {
    entry.state = JournalState::InstallNotCompleted;
    entry.failure = Some(failure.to_string());
    entry.finished_at = Some(chrono::Utc::now().to_rfc3339());
    entry.notice_acknowledged = true;
    journal::save(&paths.data_dir, entry)?;
    if staged_restore {
        crate::backup::restore::cancel_pending_restore(&paths.live_db)?;
    }
    Ok(())
}

/// Downloads (progress), then hands the installer to Windows; the app exits on success.
async fn download_and_install(mut update: Update, progress: &(dyn Fn(UpdateProgressEvent) + Sync)) -> AppResult<()> {
    // The download URL is a presigned object-storage URL: it must not carry the device token.
    update.headers.clear();
    let mut downloaded: u64 = 0;
    let bytes = update
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
        .map_err(|e| AppError::Internal(anyhow::anyhow!("download or signature check failed: {e}")))?;
    progress(UpdateProgressEvent::Installing);
    update
        .install(bytes)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("the installer could not be started: {e}")))?;
    Ok(())
}

/// Install what the manifest offers now. The caller (`commands::updater`) has checked who may
/// install; the user confirmed in the UI.
pub async fn install(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    actor: Option<i32>,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
) -> AppResult<InstallOutcome> {
    let (update, ext) = match ask_manifest(app, db, locale, None).await? {
        ManifestAnswer::Offer(update, ext) => (*update, ext),
        ManifestAnswer::NotConfigured => {
            return Err(AppError::ValidationFailed(vec![
                "this build has no in-app updates".to_string()
            ]))
        }
        ManifestAnswer::NotActivated => {
            return Err(AppError::ValidationFailed(vec![
                "this device is not activated".to_string()
            ]))
        }
        ManifestAnswer::None | ManifestAnswer::ServerRefused | ManifestAnswer::NotAcceptable(_) => {
            return Ok(InstallOutcome::Refused {
                refusal: InstallRefusal::UpdateWithdrawn,
            })
        }
    };
    let paths = UpdatePaths::of(app)?;
    let target = InstallTarget {
        running: running_version(),
        version: update.version.clone(),
        kind: ext.kind,
    };
    let prepared = prepare_install(
        db,
        &paths,
        &target,
        actor,
        &crate::commands::backup::require_database_key,
        progress,
    )
    .await?;
    let (mut entry, staged_restore) = match prepared {
        Prepared::Refused(refusal) => return Ok(InstallOutcome::Refused { refusal }),
        Prepared::Ready { entry, staged_restore } => (*entry, staged_restore),
    };
    tracing::info!(from = %entry.from_version, to = %entry.to_version, kind = ?entry.kind, "update: installing");
    if let Err(e) = download_and_install(update, progress).await {
        // Nothing was installed: the next start of this version must not believe otherwise.
        record_install_not_started(&paths, &mut entry, staged_restore, &e.to_string())?;
        return Err(e);
    }
    Ok(InstallOutcome::Restarting)
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

/// From the recovery screen: reinstall the version this device came from after its database
/// update failed (the pre-update backup is already staged).
pub async fn reinstall_previous(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
) -> AppResult<InstallOutcome> {
    let data_dir = app_data_dir(app)?;
    let Some(entry) = journal::load(&data_dir)?.filter(|j| j.state == JournalState::RollbackRequired) else {
        return Err(AppError::ValidationFailed(vec![
            "no failed update to roll back".to_string()
        ]));
    };
    let answer = ask_manifest(app, db, locale, Some(&entry.from_version)).await?;
    match rollback_answer(&answer, &entry.from_version) {
        RollbackAnswer::Install => {
            let ManifestAnswer::Offer(update, _) = answer else {
                unreachable!("rollback_answer accepts only an offer")
            };
            download_and_install(*update, progress).await?;
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

/// Journal-driven rollback: checks the pre-update backup the journal names is the file that was
/// taken (checksum), stages it through the restore service (`backup::restore::stage_restore`:
/// integrity, key, schema not newer, safety backup of the half-migrated database), and records
/// `RollbackRequired`. Nothing is staged and the journal is unchanged when a check fails.
pub async fn stage_pre_update_backup(
    db: &sea_orm::DatabaseConnection,
    paths: &UpdatePaths,
    mut j: UpdateJournal,
    failure: &str,
    key: &crate::db::encryption::DatabaseKey,
) -> AppResult<UpdateJournalView> {
    let backup = j
        .backup_path
        .as_ref()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| AppError::NotFound {
            entity: "pre_update_backup".to_string(),
            id: j.from_version.clone(),
        })?;
    if let Some(expected) = &j.backup_sha256 {
        let actual = crate::backup::file_sha256(&backup)?;
        if &actual != expected {
            return Err(AppError::ValidationFailed(vec![format!(
                "the pre-update backup {} has changed since it was taken (checksum mismatch)",
                backup.display()
            )]));
        }
    }
    crate::backup::restore::stage_restore(db, &paths.live_db, &backup, key, &paths.backup_dir, None).await?;
    j.state = JournalState::RollbackRequired;
    j.failure = Some(failure.to_string());
    j.finished_at = Some(chrono::Utc::now().to_rfc3339());
    journal::save(&paths.data_dir, &j)?;
    tracing::error!(from = %j.from_version, to = %j.to_version, "update: database update failed; pre-update backup staged");
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
            backup_sha256: None,
            schema_version_before: Some(schema_before),
            started_at: "2026-10-13T10:00:00Z".into(),
            finished_at: Some("2026-10-13T10:05:00Z".into()),
            failure: None,
            rollback_refusal: None,
            rollback_allowed_version: None,
            notice_acknowledged: false,
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
                backup_path: backup.clone()
            })
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

        // A backup that changed since it was taken is never staged.
        let mut tampered = entry.clone();
        tampered.backup_sha256 = Some("0".repeat(64));
        let err = stage_pre_update_backup(&db, &paths, tampered, "Migration failed", &key)
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("checksum mismatch"), "{err:?}");
        assert!(!crate::backup::restore::pending_restore_path(&paths.live_db).exists());
        assert_eq!(
            journal::load(&paths.data_dir).unwrap().unwrap().state,
            JournalState::Installing
        );

        let view = stage_pre_update_backup(&db, &paths, entry, "Migration failed: m1", &key)
            .await
            .unwrap();
        assert_eq!(view.state, JournalState::RollbackRequired);
        assert_eq!(view.failure.as_deref(), Some("Migration failed: m1"));
        assert!(crate::backup::restore::pending_restore_path(&paths.live_db).is_file());
        let stored = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(stored.state, JournalState::RollbackRequired);
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
        record_install_not_started(&paths, &mut entry, true, "download failed").unwrap();
        assert!(!crate::backup::restore::pending_restore_path(&paths.live_db).exists());
        let stored = journal::load(&paths.data_dir).unwrap().unwrap();
        assert_eq!(stored.state, JournalState::InstallNotCompleted);
        assert!(stored.notice_acknowledged);
        assert_eq!(stored.failure.as_deref(), Some("download failed"));
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
}
