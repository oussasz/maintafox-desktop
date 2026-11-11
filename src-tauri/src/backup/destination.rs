//! Where scheduled backups go, and whether a recent copy exists outside this PC's disk (L7-03).
//!
//! - `backup.external_dir` (device setting, this PC only): a folder on another disk, a USB disk or
//!   a network share. Scheduled backups are written there; when it cannot be reached (share down,
//!   disk unplugged) the backup is written in the application folder instead, so a backup is never
//!   skipped, and the screen warns that the last copy outside the PC is getting old.
//! - A folder on the drive of the application's data is refused: it fails with the PC.
//! - A copy "outside" is a successful backup whose file is on another drive or a network share
//!   than the application's data (scheduled to the external folder, or a manual backup saved there).
//!   On systems without drive letters (tests on Linux) any file outside the data folder counts.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};

/// The device setting holding the external folder (JSON string, or `null` for none).
pub const EXTERNAL_DIR_KEY: &str = "backup.external_dir";

/// A copy outside the PC older than this is reported as too old.
pub const OUTSIDE_COPY_MAX_AGE_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize)]
pub struct BackupOverview {
    /// The chosen external folder, if any.
    pub external_dir: Option<String>,
    /// The external folder can be written right now.
    pub external_reachable: bool,
    /// Folder of the backups the application writes by itself (on this PC's disk).
    pub app_backup_dir: String,
    pub last_success_at: Option<String>,
    /// Last successful backup on another drive or a network share.
    pub last_outside_at: Option<String>,
    pub last_outside_path: Option<String>,
    /// No copy outside the PC, or the last one is older than [`OUTSIDE_COPY_MAX_AGE_DAYS`].
    pub outside_copy_missing_or_old: bool,
    pub outside_copy_max_age_days: i64,
}

/// The external folder of this PC, if one is set.
pub async fn external_dir(db: &impl ConnectionTrait) -> AppResult<Option<PathBuf>> {
    let Some(setting) = crate::settings::get_setting(db, EXTERNAL_DIR_KEY, "device").await? else {
        return Ok(None);
    };
    let value: Option<String> = serde_json::from_str(&setting.setting_value_json).unwrap_or(None);
    Ok(value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(PathBuf::from))
}

/// A canonical Windows path as the user writes it: `\\?\UNC\srv\share\x` → `\\srv\share\x`,
/// `\\?\D:\x` → `D:\x` (other paths unchanged).
fn plain_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Lower-cased plain path with `/` separators and no trailing separator (Windows paths compare
/// case-insensitively).
fn comparable(path: &Path) -> String {
    let s = plain_path(&path.to_string_lossy()).replace('\\', "/").to_lowercase();
    s.trim_end_matches('/').to_string()
}

/// Whether `path` is inside `folder` (or is it).
fn is_inside(path: &Path, folder: &Path) -> bool {
    let p = comparable(path);
    let f = comparable(folder);
    p == f || p.starts_with(&format!("{f}/"))
}

/// The drive (`c:`) or network share (`//srv/share`) of a path; `None` without either.
fn volume_of(path: &Path) -> Option<String> {
    let c = comparable(path);
    if let Some(rest) = c.strip_prefix("//") {
        let mut parts = rest.split('/');
        let server = parts.next().filter(|s| !s.is_empty())?;
        let share = parts.next().filter(|s| !s.is_empty())?;
        return Some(format!("//{server}/{share}"));
    }
    let bytes = c.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':').then(|| c[..2].to_string())
}

/// Whether a backup file is outside this PC's disk: on another drive or share than the data
/// folder, or (no drive letters) anywhere outside the data folder.
fn is_outside(path: &Path, app_data_dir: &Path) -> bool {
    match (volume_of(path), volume_of(app_data_dir)) {
        (Some(file), Some(data)) => file != data,
        _ => !is_inside(path, app_data_dir),
    }
}

/// The folder the user chose, checked: it exists, it is a folder, it is not on the drive of the
/// application's data, and a file can be written in it. Blocking (file system): call from
/// `spawn_blocking` in async code.
pub fn check_external_dir(path: &Path, app_data_dir: &Path) -> AppResult<PathBuf> {
    let refuse = |code: &str, why: &str| AppError::ValidationFailed(vec![format!("{code}: {why}")]);
    if !path.is_absolute() {
        return Err(refuse(
            "GATE_BACKUP_DIR_NOT_ABSOLUTE",
            "choisissez un dossier complet (avec son lecteur ou son partage).",
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        refuse(
            "GATE_BACKUP_DIR_UNREACHABLE",
            "ce dossier est introuvable ou inaccessible.",
        )
    })?;
    if !canonical.is_dir() {
        return Err(refuse("GATE_BACKUP_DIR_NOT_FOLDER", "ce chemin n'est pas un dossier."));
    }
    let canonical = PathBuf::from(plain_path(&canonical.to_string_lossy()));
    let app_canonical = std::fs::canonicalize(app_data_dir).unwrap_or_else(|_| app_data_dir.to_path_buf());
    if is_inside(&canonical, &app_canonical) || !is_outside(&canonical, &app_canonical) {
        return Err(refuse(
            "GATE_BACKUP_DIR_SAME_DISK",
            "ce dossier est sur le disque de l'application : choisissez un autre disque, une clé USB ou un dossier réseau.",
        ));
    }
    // A new file of its own (never an existing file or link of the share), then removed.
    let probe = canonical.join(format!(
        ".maintafox-write-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|_| {
            refuse(
                "GATE_BACKUP_DIR_NOT_WRITABLE",
                "Maintafox ne peut pas écrire dans ce dossier.",
            )
        })?;
    std::fs::remove_file(&probe).ok();
    Ok(canonical)
}

/// [`check_external_dir`] off the async workers (a network share that does not answer can block
/// for many seconds).
async fn check_external_dir_async(path: PathBuf, app_data_dir: PathBuf) -> AppResult<PathBuf> {
    tokio::task::spawn_blocking(move || check_external_dir(&path, &app_data_dir))
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("backup folder check: {e}")))?
}

/// Sets (or clears, with `None`) the external folder of this PC after checking it.
pub async fn set_external_dir(
    db: &DatabaseConnection,
    path: Option<&str>,
    app_data_dir: &Path,
    user_id: i32,
) -> AppResult<Option<String>> {
    let stored = match path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => Some(
            check_external_dir_async(PathBuf::from(p), app_data_dir.to_path_buf())
                .await?
                .to_string_lossy()
                .to_string(),
        ),
        None => None,
    };
    crate::settings::set_setting_recorded(
        db,
        EXTERNAL_DIR_KEY,
        "device",
        &serde_json::to_string(&stored)?,
        Some(user_id),
        match &stored {
            Some(_) => "Dossier de sauvegarde hors du PC choisi",
            None => "Dossier de sauvegarde hors du PC retiré",
        },
        true,
    )
    .await?;
    Ok(stored)
}

/// Where the next scheduled backup goes: the external folder when it can be written, else the
/// application folder (a backup is never skipped because a share is down).
pub async fn scheduled_target_dir(db: &DatabaseConnection, app_backup_dir: &Path) -> PathBuf {
    let app_data_dir = app_backup_dir.parent().unwrap_or(app_backup_dir).to_path_buf();
    match external_dir(db).await {
        Ok(Some(dir)) => match check_external_dir_async(dir.clone(), app_data_dir).await {
            Ok(canonical) => canonical,
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "backup: external folder unreachable, writing locally");
                app_backup_dir.to_path_buf()
            }
        },
        Ok(None) => app_backup_dir.to_path_buf(),
        Err(e) => {
            tracing::warn!(error = %e, "backup: external folder setting unreadable, writing locally");
            app_backup_dir.to_path_buf()
        }
    }
}

pub async fn overview(db: &DatabaseConnection, app_backup_dir: &Path) -> AppResult<BackupOverview> {
    let app_data_dir = app_backup_dir.parent().unwrap_or(app_backup_dir).to_path_buf();
    let external = external_dir(db).await?;
    let external_reachable = match &external {
        Some(d) => check_external_dir_async(d.clone(), app_data_dir.clone()).await.is_ok(),
        None => false,
    };
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT output_path, COALESCE(completed_at, started_at) AS at FROM backup_runs \
             WHERE status = 'success' ORDER BY id DESC LIMIT 500"
                .to_string(),
        ))
        .await?;
    let mut last_success_at = None;
    let mut last_outside: Option<(String, String)> = None;
    for r in &rows {
        let path: String = r.try_get("", "output_path")?;
        let at: String = r.try_get("", "at")?;
        if last_success_at.is_none() {
            last_success_at = Some(at.clone());
        }
        if is_outside(Path::new(&path), &app_data_dir) {
            last_outside = Some((at, path));
            break;
        }
    }
    let now = crate::clock::now();
    let outside_copy_missing_or_old = match &last_outside {
        None => true,
        Some((at, _)) => DateTime::parse_from_rfc3339(at)
            .map(|d| now - d.with_timezone(&Utc) > chrono::Duration::days(OUTSIDE_COPY_MAX_AGE_DAYS))
            .unwrap_or(true),
    };
    Ok(BackupOverview {
        external_dir: external.map(|p| plain_path(&p.to_string_lossy())),
        external_reachable,
        app_backup_dir: app_backup_dir.to_string_lossy().to_string(),
        last_success_at,
        last_outside_at: last_outside.as_ref().map(|(at, _)| at.clone()),
        last_outside_path: last_outside.map(|(_, p)| plain_path(&p)),
        outside_copy_missing_or_old,
        outside_copy_max_age_days: OUTSIDE_COPY_MAX_AGE_DAYS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_inside_the_application_folder_is_refused_and_another_is_accepted() {
        let app = tempfile::tempdir().unwrap();
        let inside = app.path().join("backups");
        std::fs::create_dir_all(&inside).unwrap();
        assert!(check_external_dir(&inside, app.path()).is_err());
        assert!(check_external_dir(app.path(), app.path()).is_err());
        let other = tempfile::tempdir().unwrap();
        // On a drive-letter system both temp folders share the drive: refused as the same disk.
        if volume_of(app.path()).is_none() {
            assert!(check_external_dir(other.path(), app.path()).is_ok());
            // The write probe leaves nothing behind.
            assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
        }
        assert!(check_external_dir(&other.path().join("missing"), app.path()).is_err());
        assert!(check_external_dir(Path::new("relative/folder"), app.path()).is_err());
    }

    #[test]
    fn windows_paths_compare_by_folder_drive_and_share() {
        assert!(is_inside(
            Path::new(r"C:\Data\App\backups\x.db"),
            Path::new(r"c:\data\app")
        ));
        assert!(!is_inside(Path::new(r"C:\Data\App2\x.db"), Path::new(r"C:\Data\App")));
        assert!(is_inside(Path::new(r"\\?\C:\Data\App\x.db"), Path::new(r"C:\Data\App")));
        assert_eq!(plain_path(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(plain_path(r"\\?\D:\Sauvegardes"), r"D:\Sauvegardes");
        assert_eq!(
            volume_of(Path::new(r"\\?\UNC\Srv\Share\x")).as_deref(),
            Some("//srv/share")
        );
        assert_eq!(volume_of(Path::new(r"E:\x")).as_deref(), Some("e:"));
        assert_eq!(volume_of(Path::new("/mnt/usb/x")), None);
        let data = Path::new(r"C:\Users\u\AppData\Roaming\systems.maintafox.desktop");
        // Same drive: not outside the PC, even outside the data folder.
        assert!(!is_outside(Path::new(r"C:\Sauvegardes\a.db"), data));
        assert!(is_outside(Path::new(r"E:\Sauvegardes\a.db"), data));
        assert!(is_outside(Path::new(r"\\nas\backup\a.db"), data));
        assert!(is_outside(Path::new(r"\\?\UNC\nas\backup\a.db"), data));
    }

    async fn insert_run(db: &DatabaseConnection, path: &str, at: &str) {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO backup_runs (trigger, status, output_path, encryption_mode, started_at, completed_at) \
             VALUES ('scheduled', 'success', ?, 'aes256', ?, ?)",
            [path.into(), at.into(), at.into()],
        ))
        .await
        .expect("backup run");
    }

    #[tokio::test]
    async fn the_overview_reports_a_missing_or_old_copy_outside_the_pc() {
        let db = crate::db::test_db::seeded().await;
        let app = tempfile::tempdir().unwrap();
        let backups = app.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        let recent = crate::clock::now().to_rfc3339();
        insert_run(&db, &backups.join("a.db").to_string_lossy(), &recent).await;
        let o = overview(&db, &backups).await.unwrap();
        assert_eq!(o.last_success_at.as_deref(), Some(recent.as_str()));
        assert!(o.last_outside_at.is_none());
        assert!(o.outside_copy_missing_or_old);

        let old = (crate::clock::now() - chrono::Duration::days(30)).to_rfc3339();
        insert_run(&db, r"\\nas\sauvegardes\old.db", &old).await;
        let o = overview(&db, &backups).await.unwrap();
        assert_eq!(o.last_outside_at.as_deref(), Some(old.as_str()));
        assert!(o.outside_copy_missing_or_old);

        insert_run(&db, r"\\nas\sauvegardes\new.db", &recent).await;
        let o = overview(&db, &backups).await.unwrap();
        assert_eq!(o.last_outside_path.as_deref(), Some(r"\\nas\sauvegardes\new.db"));
        assert!(!o.outside_copy_missing_or_old);
    }

    #[tokio::test]
    async fn scheduled_backups_go_to_a_reachable_external_folder_only() {
        let db = crate::db::test_db::seeded().await;
        let app = tempfile::tempdir().unwrap();
        let backups = app.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        assert_eq!(scheduled_target_dir(&db, &backups).await, backups);

        // A folder of the application is refused; clearing works.
        assert!(set_external_dir(&db, Some(&backups.to_string_lossy()), app.path(), 1)
            .await
            .is_err());
        assert_eq!(set_external_dir(&db, None, app.path(), 1).await.unwrap(), None);
        assert!(external_dir(&db).await.unwrap().is_none());

        // Another disk (only testable without drive letters: temp folders share the drive).
        if volume_of(app.path()).is_some() {
            return;
        }
        let external = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(external.path()).unwrap();
        let stored = set_external_dir(&db, Some(&external.path().to_string_lossy()), app.path(), 1)
            .await
            .unwrap()
            .expect("stored");
        assert_eq!(comparable(Path::new(&stored)), comparable(&canonical));
        assert_eq!(
            comparable(&scheduled_target_dir(&db, &backups).await),
            comparable(&canonical)
        );

        // Unplugged: the backup is written locally.
        let gone = external.path().to_path_buf();
        drop(external);
        assert!(!gone.exists());
        assert_eq!(scheduled_target_dir(&db, &backups).await, backups);
    }
}
