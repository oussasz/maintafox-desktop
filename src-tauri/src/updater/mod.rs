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

use std::path::PathBuf;

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

/// The `maintafox` object the control API adds to the Tauri manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestExtension {
    pub release_id: String,
    pub kind: UpdateKind,
    pub mandatory: bool,
    pub mandatory_reason: Option<String>,
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
            notes_en: None,
            notes_fr: None,
            message,
        }
    }
}

pub fn running_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
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
    None,
    Offer(Box<Update>, ManifestExtension),
}

/// Asks the manifest. The server is the authority on which version to install (it may be older,
/// kind rollback), so any version different from the running one is accepted; the signature is
/// still checked against this build's public key before anything is installed.
pub async fn ask_manifest(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    rollback_to: Option<&str>,
) -> AppResult<ManifestAnswer> {
    if UPDATER_PUBLIC_KEY.is_none() {
        return Ok(ManifestAnswer::NotConfigured);
    }
    let Some(token) = crate::commands::product_license::sync_activation_bearer_token(db)
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
    match updater
        .check()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("update server: {e}")))?
    {
        None => Ok(ManifestAnswer::None),
        Some(update) => {
            let ext = parse_extension(&update.raw_json)?;
            Ok(ManifestAnswer::Offer(Box::new(update), ext))
        }
    }
}

pub async fn check(app: &AppHandle, db: &sea_orm::DatabaseConnection, locale: Option<&str>) -> UpdateCheckResult {
    let current = running_version(app);
    match ask_manifest(app, db, locale, None).await {
        Ok(ManifestAnswer::NotConfigured) => {
            UpdateCheckResult::without_update(CheckStatus::NotConfigured, &current, None)
        }
        Ok(ManifestAnswer::NotActivated) => {
            UpdateCheckResult::without_update(CheckStatus::NotActivated, &current, None)
        }
        Ok(ManifestAnswer::None) => UpdateCheckResult::without_update(CheckStatus::UpToDate, &current, None),
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
    /// The server no longer offers this update (recalled or paused since the check).
    UpdateWithdrawn,
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

fn backup_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app_data_dir(app)?.join("backups");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn live_db_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(PathBuf::from(crate::startup::resolve_db_path(app)?))
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

/// Install what the manifest offers now (signed-in user, confirmed in the UI).
pub async fn install(
    app: &AppHandle,
    db: &sea_orm::DatabaseConnection,
    locale: Option<&str>,
    actor: Option<i32>,
    progress: &(dyn Fn(UpdateProgressEvent) + Sync),
) -> AppResult<InstallOutcome> {
    let running = running_version(app);
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
        ManifestAnswer::None => {
            return Ok(InstallOutcome::Refused {
                refusal: InstallRefusal::UpdateWithdrawn,
            })
        }
    };
    let data_dir = app_data_dir(app)?;
    let previous = journal::load(&data_dir)?;
    let schema_now = applied_migration_count(db).await?;

    let mut entry = UpdateJournal {
        from_version: running.clone(),
        to_version: update.version.clone(),
        kind: ext.kind,
        state: JournalState::Installing,
        backup_path: None,
        backup_sha256: None,
        schema_version_before: Some(schema_now),
        started_at: chrono::Utc::now().to_rfc3339(),
        finished_at: None,
        failure: None,
        notice_acknowledged: false,
    };
    let mut staged_restore = false;
    match ext.kind {
        UpdateKind::Upgrade => {
            progress(UpdateProgressEvent::BackingUp);
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            let path = backup_dir(app)?.join(format!(
                "maintafox-pre-update-{running}-to-{}-{stamp}.db",
                update.version
            ));
            let backup =
                crate::backup::run_manual_backup(db, &path.to_string_lossy(), crate::backup::TRIGGER_PRE_UPDATE, actor)
                    .await?;
            entry.backup_path = Some(backup.output_path);
            entry.backup_sha256 = Some(backup.sha256_checksum);
        }
        UpdateKind::Rollback => {
            let summary = crate::sync::queries::get_sync_state_summary(db).await?;
            let pending = summary.pending_outbox_count + summary.in_flight_outbox_count;
            match plan_downgrade(previous.as_ref(), &running, &update.version, schema_now, pending) {
                Err(refusal) => return Ok(InstallOutcome::Refused { refusal }),
                Ok(DowngradePlan::KeepDatabase) => {}
                Ok(DowngradePlan::RestoreBackup { backup_path }) => {
                    progress(UpdateProgressEvent::RestoringBackup);
                    let key = crate::db::encryption::load_stored_key()?.ok_or_else(|| {
                        AppError::DatabaseKeyUnavailable("no database key is stored in the OS credential store".into())
                    })?;
                    crate::backup::restore::stage_restore(
                        db,
                        &live_db_path(app)?,
                        &backup_path,
                        &key,
                        &backup_dir(app)?,
                        actor,
                    )
                    .await?;
                    staged_restore = true;
                    entry.backup_path = Some(backup_path.to_string_lossy().to_string());
                }
            }
        }
    }
    journal::save(&data_dir, &entry)?;
    tracing::info!(from = %entry.from_version, to = %entry.to_version, kind = ?entry.kind, "update: installing");
    if let Err(e) = download_and_install(update, progress).await {
        // Nothing was installed: the next start of this version must not believe otherwise.
        entry.state = JournalState::InstallNotCompleted;
        entry.failure = Some(e.to_string());
        entry.finished_at = Some(chrono::Utc::now().to_rfc3339());
        entry.notice_acknowledged = true;
        journal::save(&data_dir, &entry)?;
        if staged_restore {
            // The staged restore belongs to the older version that was not installed.
            crate::backup::restore::cancel_pending_restore(&live_db_path(app)?)?;
        }
        return Err(e);
    }
    Ok(InstallOutcome::Restarting)
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
    match ask_manifest(app, db, locale, Some(&entry.from_version)).await? {
        ManifestAnswer::Offer(update, _) if update.version == entry.from_version => {
            download_and_install(*update, progress).await?;
            Ok(InstallOutcome::Restarting)
        }
        ManifestAnswer::Offer(..) | ManifestAnswer::None => Ok(InstallOutcome::Refused {
            refusal: InstallRefusal::UpdateWithdrawn,
        }),
        ManifestAnswer::NotConfigured | ManifestAnswer::NotActivated => Ok(InstallOutcome::Refused {
            refusal: InstallRefusal::RollbackBackupMissing,
        }),
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
    let action = journal::classify(journal.as_ref(), &running_version(app));
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
    mut j: UpdateJournal,
    failure: &str,
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
    let key = crate::db::encryption::load_stored_key()?.ok_or_else(|| {
        AppError::DatabaseKeyUnavailable("no database key is stored in the OS credential store".into())
    })?;
    crate::backup::restore::stage_restore(db, &live_db_path(app)?, &backup, &key, &backup_dir(app)?, None).await?;
    j.state = JournalState::RollbackRequired;
    j.failure = Some(failure.to_string());
    j.finished_at = Some(chrono::Utc::now().to_rfc3339());
    journal::save(&app_data_dir(app)?, &j)?;
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
}
