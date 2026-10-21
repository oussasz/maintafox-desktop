//! Update journal: what the running version knows about the update in progress or just done.
//!
//! It is a small JSON file next to the database (`update-journal.json`), not a table: the version
//! that starts after an update must read it before (and even when) its database migrations fail.
//! It is written before the installer starts and changed once by the next version that starts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::AppResult;

pub const JOURNAL_FILE: &str = "update-journal.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateKind {
    /// A newer release.
    Upgrade,
    /// An older signed release: after a recall, or after a failed database update.
    Rollback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalState {
    /// The installer of `to_version` was started by `from_version`.
    Installing,
    /// `to_version` started and its database is up to date.
    Completed,
    /// `to_version` could not update the database: the pre-update backup is staged and the
    /// previous version must be reinstalled.
    RollbackRequired,
    /// The previous version runs again on the restored pre-update backup.
    RolledBack,
    /// `from_version` started again: the installer did not complete (cancelled or failed).
    InstallNotCompleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateJournal {
    pub from_version: String,
    pub to_version: String,
    pub kind: UpdateKind,
    pub state: JournalState,
    /// Backup of the database taken by `from_version` just before installing (`pre_update` run).
    pub backup_path: Option<String>,
    pub backup_sha256: Option<String>,
    /// Applied migrations when the backup was taken.
    pub schema_version_before: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Why the update failed (shown to the user, kept for support).
    pub failure: Option<String>,
    /// Why the rollback after a failed database update was refused (kept for support).
    #[serde(default)]
    pub rollback_refusal: Option<String>,
    /// The version the server's refusal named as allowed instead (information only).
    #[serde(default)]
    pub rollback_allowed_version: Option<String>,
    /// Why the pre-update backup was not restored after a failed database update (it is missing,
    /// has no recorded checksum, or changed since it was taken). Nothing was staged, and the
    /// recovery reinstall is refused: the older version cannot open the half-updated database.
    #[serde(default)]
    pub backup_refusal: Option<String>,
    /// The user has seen the outcome notice (rolled back / installer not completed).
    #[serde(default)]
    pub notice_acknowledged: bool,
}

pub fn journal_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(JOURNAL_FILE)
}

pub fn load(app_data_dir: &Path) -> AppResult<Option<UpdateJournal>> {
    let path = journal_path(app_data_dir);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let journal = serde_json::from_str(&text).map_err(|e| {
        crate::errors::AppError::Internal(anyhow::anyhow!("update journal {} is unreadable: {e}", path.display()))
    })?;
    Ok(Some(journal))
}

/// Writes the journal atomically (temporary file, flushed, then renamed over the old one).
pub fn save(app_data_dir: &Path, journal: &UpdateJournal) -> AppResult<()> {
    use std::io::Write;
    std::fs::create_dir_all(app_data_dir)?;
    let path = journal_path(app_data_dir);
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(journal)
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("update journal: {e}")))?;
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// What startup must do about the journal, given the version that is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupAction {
    /// No journal, or nothing left to do.
    Nothing,
    /// First start of the installed version: migrate; success completes the journal, failure
    /// restores the pre-update backup and asks for the previous version.
    VerifyNewVersion,
    /// This version could not update the database earlier: do not touch the database again,
    /// show the recovery screen that reinstalls `from_version`.
    RecoveryPending,
    /// The previous version runs again after a failed update: the backup was swapped in.
    ReturnedToPrevious,
    /// The installer did not complete: the same version started again.
    InstallNotCompleted,
}

pub fn classify(journal: Option<&UpdateJournal>, running_version: &str) -> StartupAction {
    let Some(j) = journal else {
        return StartupAction::Nothing;
    };
    match j.state {
        JournalState::Installing if running_version == j.to_version => StartupAction::VerifyNewVersion,
        JournalState::Installing if running_version == j.from_version => StartupAction::InstallNotCompleted,
        JournalState::RollbackRequired if running_version == j.to_version => StartupAction::RecoveryPending,
        JournalState::RollbackRequired if running_version == j.from_version => StartupAction::ReturnedToPrevious,
        _ => StartupAction::Nothing,
    }
}

/// A restore staged for a rollback whose installer never ran: the journal says a rollback was
/// being installed, and the version that starts is not the rollback's target (the installer was
/// cancelled, failed, or another version was installed). Such a restore is not justified and must
/// not be applied: it would put back an old backup under the running version.
pub fn staged_restore_is_orphaned(j: &UpdateJournal, running_version: &str) -> bool {
    j.state == JournalState::Installing && j.kind == UpdateKind::Rollback && running_version != j.to_version
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(state: JournalState) -> UpdateJournal {
        UpdateJournal {
            from_version: "1.0.0".into(),
            to_version: "1.1.0".into(),
            kind: UpdateKind::Upgrade,
            state,
            backup_path: Some("backups/pre-update.db".into()),
            backup_sha256: Some("ab".repeat(32)),
            schema_version_before: Some(120),
            started_at: "2026-10-13T10:00:00Z".into(),
            finished_at: None,
            failure: None,
            rollback_refusal: None,
            rollback_allowed_version: None,
            backup_refusal: None,
            notice_acknowledged: false,
        }
    }

    #[test]
    fn a_restore_staged_for_an_uninstalled_rollback_is_orphaned() {
        let mut rollback = journal(JournalState::Installing);
        rollback.kind = UpdateKind::Rollback;
        rollback.from_version = "1.1.0".into();
        rollback.to_version = "1.0.0".into();
        assert!(staged_restore_is_orphaned(&rollback, "1.1.0"), "installer never ran");
        assert!(
            staged_restore_is_orphaned(&rollback, "1.2.0"),
            "another version installed"
        );
        assert!(
            !staged_restore_is_orphaned(&rollback, "1.0.0"),
            "the rollback ran: apply it"
        );
        let upgrade = journal(JournalState::Installing);
        assert!(
            !staged_restore_is_orphaned(&upgrade, "1.0.0"),
            "upgrades stage no restore"
        );
        let mut failed = journal(JournalState::RollbackRequired);
        failed.kind = UpdateKind::Rollback;
        assert!(
            !staged_restore_is_orphaned(&failed, "1.1.0"),
            "failed migration: justified"
        );
    }

    #[test]
    fn classify_covers_every_transition() {
        assert_eq!(classify(None, "1.0.0"), StartupAction::Nothing);
        let installing = journal(JournalState::Installing);
        assert_eq!(classify(Some(&installing), "1.1.0"), StartupAction::VerifyNewVersion);
        assert_eq!(classify(Some(&installing), "1.0.0"), StartupAction::InstallNotCompleted);
        assert_eq!(
            classify(Some(&installing), "2.0.0"),
            StartupAction::Nothing,
            "a later manual install"
        );
        let failed = journal(JournalState::RollbackRequired);
        assert_eq!(classify(Some(&failed), "1.1.0"), StartupAction::RecoveryPending);
        assert_eq!(classify(Some(&failed), "1.0.0"), StartupAction::ReturnedToPrevious);
        for done in [
            JournalState::Completed,
            JournalState::RolledBack,
            JournalState::InstallNotCompleted,
        ] {
            assert_eq!(classify(Some(&journal(done)), "1.1.0"), StartupAction::Nothing);
            assert_eq!(classify(Some(&journal(done)), "1.0.0"), StartupAction::Nothing);
        }
    }

    #[test]
    fn save_and_load_round_trip_atomically() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), None);
        let j = journal(JournalState::Installing);
        save(dir.path(), &j).unwrap();
        assert_eq!(load(dir.path()).unwrap(), Some(j.clone()));
        assert!(!journal_path(dir.path()).with_extension("json.tmp").exists());
        let mut next = j;
        next.state = JournalState::Completed;
        save(dir.path(), &next).unwrap();
        assert_eq!(load(dir.path()).unwrap().unwrap().state, JournalState::Completed);
    }

    #[test]
    fn unreadable_journal_is_an_error_not_silently_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(journal_path(dir.path()), "{not json").unwrap();
        assert!(load(dir.path()).is_err());
    }
}
