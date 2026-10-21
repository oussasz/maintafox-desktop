//! Restore from backup (Phase 1.3).
//!
//! The live connection is shared by the background loops, so the database file cannot be swapped
//! while the app runs. Restore is therefore two-phase:
//! 1. `stage_restore` (app running): validate the backup, refuse a newer schema, take a safety
//!    backup of the current database, then copy the backup next to the live file as
//!    `maintafox.db.restore-pending`. The caller restarts the app.
//! 2. `apply_pending_restore` (next startup, before the database is opened): swap the pending
//!    file in, rolling back if the swap fails. Migrations then upgrade an older schema.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::db::encryption::{sibling, DatabaseKey};
use crate::errors::{AppError, AppResult};

const PENDING_SUFFIX: &str = ".restore-pending";
const STAGING_SUFFIX: &str = ".restore-staging";
const PREVIOUS_SUFFIX: &str = ".pre-restore.bak";

#[derive(Debug, Clone, Serialize)]
pub struct RestoreStaged {
    pub backup_path: String,
    pub safety_backup_path: String,
}

pub fn pending_restore_path(db_path: &Path) -> PathBuf {
    sibling(db_path, PENDING_SUFFIX)
}

/// Migration names known to this build.
fn known_migrations() -> HashSet<String> {
    use sea_orm_migration::MigratorTrait;
    crate::migrations::Migrator::migrations()
        .iter()
        .map(|m| m.name().to_string())
        .collect()
}

/// Fails if `backup_db` contains migrations this build does not know: it was written by a newer
/// app version, and opening it here could silently break data.
async fn ensure_schema_not_newer(backup_db: &DatabaseConnection) -> AppResult<()> {
    let rows = backup_db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT version FROM seaql_migrations".to_string(),
        ))
        .await
        .map_err(|e| {
            AppError::ValidationFailed(vec![format!(
                "the file is not a Maintafox backup (no migration history): {e}"
            )])
        })?;
    let known = known_migrations();
    let unknown: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String>("", "version").ok())
        .filter(|v| !known.contains(v))
        .collect();
    match unknown.first() {
        None => Ok(()),
        Some(first) => Err(AppError::ValidationFailed(vec![format!(
            "this backup was made by a newer Maintafox version ({} unknown migration(s), e.g. {first}); \
             update the application before restoring it",
            unknown.len()
        )])),
    }
}

/// Checks a backup can be restored: integrity OK, opens with this device's key, schema not newer.
pub async fn preflight(db: &DatabaseConnection, backup_path: &Path, key: &DatabaseKey) -> AppResult<()> {
    let path_str = backup_path.to_string_lossy().to_string();
    let report = super::validate_backup_file(db, &path_str, Some(key)).await?;
    if !report.integrity_ok {
        return Err(AppError::ValidationFailed(vec![format!(
            "backup failed the integrity check (it may be damaged or encrypted with another \
             installation's key): {}",
            report.integrity_check_output
        )]));
    }
    let backup_db = crate::db::connect_sqlcipher(backup_path, Some(key), false).await?;
    let result = ensure_schema_not_newer(&backup_db).await;
    backup_db.close().await.ok();
    result
}

/// Validates `backup_path`, writes a safety backup of the live database into `safety_dir`, and
/// stages the backup for the next startup. Nothing in the live database changes.
pub async fn stage_restore(
    db: &DatabaseConnection,
    live_db_path: &Path,
    backup_path: &Path,
    key: &DatabaseKey,
    safety_dir: &Path,
    actor: Option<i32>,
) -> AppResult<RestoreStaged> {
    if !backup_path.is_file() {
        return Err(AppError::NotFound {
            entity: "backup_file".to_string(),
            id: backup_path.to_string_lossy().to_string(),
        });
    }
    if same_file(backup_path, live_db_path) {
        return Err(AppError::ValidationFailed(vec![
            "the selected file is the live database, not a backup".to_string(),
        ]));
    }
    preflight(db, backup_path, key).await?;

    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let safety_path = safety_dir.join(format!("maintafox-pre-restore-{stamp}.db"));
    super::run_manual_backup(db, &safety_path.to_string_lossy(), super::TRIGGER_PRE_RESTORE, actor).await?;

    // Copy under a staging name first so a crash never leaves a half-written pending file.
    let staging = sibling(live_db_path, STAGING_SUFFIX);
    let pending = pending_restore_path(live_db_path);
    fs::copy(backup_path, &staging)?;
    if let Err(e) = fs::rename(&staging, &pending) {
        fs::remove_file(&staging).ok();
        return Err(e.into());
    }
    tracing::warn!(
        event = "restore_staged",
        backup = %backup_path.display(),
        safety_backup = %safety_path.display(),
        actor = ?actor,
        "restore staged; it is applied at the next startup"
    );
    Ok(RestoreStaged {
        backup_path: backup_path.to_string_lossy().to_string(),
        safety_backup_path: safety_path.to_string_lossy().to_string(),
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Removes a staged restore that must not be applied any more (the update it was staged for did
/// not install). Returns whether one was removed.
pub fn cancel_pending_restore(live_db_path: &Path) -> AppResult<bool> {
    let pending = pending_restore_path(live_db_path);
    if !pending.exists() {
        return Ok(false);
    }
    fs::remove_file(&pending)?;
    tracing::warn!(event = "restore_cancelled", "staged restore removed");
    Ok(true)
}

/// Swaps a staged restore into place. Must run before the database is opened.
///
/// Returns `true` when a restore was applied. On failure the original database is put back and the
/// pending file is kept so the error can be investigated.
pub fn apply_pending_restore(live_db_path: &Path) -> AppResult<bool> {
    let pending = pending_restore_path(live_db_path);
    if !pending.exists() {
        return Ok(false);
    }
    let previous = sibling(live_db_path, PREVIOUS_SUFFIX);
    if previous.exists() {
        fs::remove_file(&previous)?;
    }
    let had_live = live_db_path.exists();
    if had_live {
        fs::rename(live_db_path, &previous)?;
    }
    if let Err(e) = fs::rename(&pending, live_db_path) {
        if had_live {
            fs::rename(&previous, live_db_path).ok();
        }
        return Err(AppError::Internal(anyhow::anyhow!(
            "could not apply the staged restore (original database kept): {e}"
        )));
    }
    // The old WAL/SHM belong to the replaced file; SQLite would replay them into the restored one.
    for stale in [sibling(live_db_path, "-wal"), sibling(live_db_path, "-shm")] {
        if stale.exists() {
            fs::remove_file(stale)?;
        }
    }
    // `stage_restore` wrote a safety backup, so the replaced file is not needed.
    if had_live {
        fs::remove_file(&previous).ok();
    }
    tracing::warn!(event = "restore_applied", "database restored from backup");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm_migration::MigratorTrait;

    async fn migrated_encrypted_db(path: &Path, key: &DatabaseKey) -> DatabaseConnection {
        let db = crate::db::connect_sqlcipher(path, Some(key), true).await.unwrap();
        db.execute_unprepared("PRAGMA journal_mode=WAL;").await.unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        db
    }

    async fn marker(db: &DatabaseConnection) -> Option<String> {
        db.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT note FROM restore_marker".to_string(),
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String>("", "note").ok())
    }

    #[tokio::test]
    async fn staged_restore_is_applied_at_next_open_and_safety_backup_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("maintafox.db");
        let backup = dir.path().join("backup.db");
        let safety_dir = dir.path().join("backups");
        let key = DatabaseKey::generate();

        let db = migrated_encrypted_db(&live, &key).await;
        db.execute_unprepared("CREATE TABLE restore_marker (note TEXT); INSERT INTO restore_marker VALUES ('old');")
            .await
            .unwrap();
        crate::backup::run_manual_backup(&db, &backup.to_string_lossy(), crate::backup::TRIGGER_MANUAL, None)
            .await
            .unwrap();
        db.execute_unprepared("UPDATE restore_marker SET note = 'new';")
            .await
            .unwrap();

        let staged = stage_restore(&db, &live, &backup, &key, &safety_dir, Some(1))
            .await
            .unwrap();
        assert!(Path::new(&staged.safety_backup_path).is_file());
        assert!(pending_restore_path(&live).is_file());
        assert_eq!(
            marker(&db).await.as_deref(),
            Some("new"),
            "live DB untouched until restart"
        );
        db.close().await.unwrap();

        assert!(apply_pending_restore(&live).unwrap());
        assert!(!pending_restore_path(&live).exists());
        let reopened = crate::db::connect_sqlcipher(&live, Some(&key), false).await.unwrap();
        assert_eq!(marker(&reopened).await.as_deref(), Some("old"));
        reopened.close().await.unwrap();

        let safety = crate::db::connect_sqlcipher(Path::new(&staged.safety_backup_path), Some(&key), false)
            .await
            .unwrap();
        assert_eq!(
            marker(&safety).await.as_deref(),
            Some("new"),
            "safety backup holds pre-restore data"
        );
        safety.close().await.unwrap();
    }

    #[tokio::test]
    async fn backup_with_unknown_migration_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("maintafox.db");
        let backup = dir.path().join("backup.db");
        let key = DatabaseKey::generate();

        let db = migrated_encrypted_db(&live, &key).await;
        db.execute_unprepared(
            "INSERT INTO seaql_migrations (version, applied_at) VALUES ('m29990101_000001_from_the_future', 0);",
        )
        .await
        .unwrap();
        crate::backup::run_manual_backup(&db, &backup.to_string_lossy(), crate::backup::TRIGGER_MANUAL, None)
            .await
            .unwrap();
        db.execute_unprepared("DELETE FROM seaql_migrations WHERE version LIKE 'm2999%';")
            .await
            .unwrap();

        let err = stage_restore(&db, &live, &backup, &key, &dir.path().join("backups"), None)
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("newer Maintafox version"), "{err:?}");
        assert!(!pending_restore_path(&live).exists());
        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn backup_encrypted_with_another_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("maintafox.db");
        let foreign = dir.path().join("foreign.db");
        let key = DatabaseKey::generate();

        let other = migrated_encrypted_db(&foreign, &DatabaseKey::generate()).await;
        other.close().await.unwrap();
        let db = migrated_encrypted_db(&live, &key).await;

        let err = stage_restore(&db, &live, &foreign, &key, &dir.path().join("backups"), None)
            .await
            .unwrap_err();
        assert!(format!("{err:?}").contains("integrity check"), "{err:?}");
        assert!(!pending_restore_path(&live).exists());
        db.close().await.unwrap();
    }

    /// Two checks started together (same second) used to share one temp copy named after the
    /// timestamp: one overwrote or deleted the other's file (CI on Linux).
    #[tokio::test]
    async fn concurrent_backup_checks_do_not_share_their_temp_copy() {
        let dir = tempfile::tempdir().unwrap();
        let key = DatabaseKey::generate();
        let db = migrated_encrypted_db(&dir.path().join("maintafox.db"), &key).await;
        let a = dir.path().join("a.db");
        let b = dir.path().join("b.db");
        for p in [&a, &b] {
            crate::backup::run_manual_backup(&db, &p.to_string_lossy(), crate::backup::TRIGGER_MANUAL, None)
                .await
                .unwrap();
        }
        let (a, b) = (a.to_string_lossy().to_string(), b.to_string_lossy().to_string());
        for _ in 0..3 {
            let (ra, rb) = tokio::join!(
                crate::backup::validate_backup_file(&db, &a, Some(&key)),
                crate::backup::validate_backup_file(&db, &b, Some(&key)),
            );
            let (ra, rb) = (ra.unwrap(), rb.unwrap());
            assert!(ra.integrity_ok, "{}", ra.integrity_check_output);
            assert!(rb.integrity_ok, "{}", rb.integrity_check_output);
        }
        db.close().await.unwrap();
    }

    #[test]
    fn no_pending_file_is_a_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("maintafox.db");
        fs::write(&live, b"current").unwrap();
        assert!(!apply_pending_restore(&live).unwrap());
        assert_eq!(fs::read(&live).unwrap(), b"current");
    }

    #[test]
    fn pending_file_replaces_live_and_stale_wal_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("maintafox.db");
        fs::write(&live, b"current").unwrap();
        fs::write(sibling(&live, "-wal"), b"old wal").unwrap();
        fs::write(pending_restore_path(&live), b"restored").unwrap();

        assert!(apply_pending_restore(&live).unwrap());
        assert_eq!(fs::read(&live).unwrap(), b"restored");
        assert!(!sibling(&live, "-wal").exists());
        assert!(!sibling(&live, PREVIOUS_SUFFIX).exists());
    }
}
