//! Scheduled backups (Phase 1.3).
//!
//! - Schedule: the active `backup` policy's `schedule_cron`, in local time. Both the standard
//!   5-field form (`0 2 * * *`) and the `cron` crate's 6/7-field form (seconds first) are accepted.
//! - A run is due when a scheduled time has passed since the last successful scheduled backup, so
//!   a backup missed while the app was closed runs at the next tick.
//! - Output: `<app_data_dir>/backups/maintafox-YYYYMMDD-HHMMSS.db`, recorded with
//!   `trigger = 'scheduled'`. A failure is recorded as a `failed` run and retried after
//!   `RETRY_AFTER_FAILURE`.
//! - Retention: grandfather-father-son over the `backup.retention_daily/weekly/monthly` settings.
//!   Only scheduled backup files inside the backup folder are ever deleted.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use cron::Schedule;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::{AppError, AppResult};
use crate::settings::BackupPolicy;

const TICK: TokioDuration = TokioDuration::from_secs(60);
const RETRY_AFTER_FAILURE: chrono::Duration = chrono::Duration::hours(1);

/// Parses a backup schedule. A 5-field expression gets a leading `0` seconds field.
pub fn parse_backup_schedule(expr: &str) -> Result<Schedule, String> {
    let expr = expr.trim();
    let normalized = if expr.split_whitespace().count() == 5 {
        format!("0 {expr}")
    } else {
        expr.to_string()
    };
    Schedule::from_str(&normalized).map_err(|e| format!("invalid backup schedule '{expr}': {e}"))
}

/// True when a scheduled time falls in `(since, now]`.
pub fn is_due(schedule: &Schedule, since: DateTime<Local>, now: DateTime<Local>) -> bool {
    schedule.after(&since).next().is_some_and(|t| t <= now)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    pub daily: usize,
    pub weekly: usize,
    pub monthly: usize,
}

fn keep_newest_per_bucket(
    sorted: &[&(i64, DateTime<Local>)],
    limit: usize,
    bucket: fn(NaiveDate) -> (i32, u32),
    keep: &mut HashSet<i64>,
) {
    let mut seen: Vec<(i32, u32)> = Vec::new();
    for (id, at) in sorted {
        let b = bucket(at.date_naive());
        if seen.contains(&b) {
            continue;
        }
        if seen.len() == limit {
            break;
        }
        seen.push(b);
        keep.insert(*id);
    }
}

/// Grandfather-father-son retention. `runs` are `(id, started_at)`; returns the ids to delete.
///
/// Keeps the newest backup of each of the newest `daily` days, `weekly` ISO weeks and `monthly`
/// months. Everything else is pruned.
pub fn select_for_pruning(runs: &[(i64, DateTime<Local>)], retention: Retention) -> Vec<i64> {
    let mut sorted: Vec<&(i64, DateTime<Local>)> = runs.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));

    let mut keep = HashSet::new();
    keep_newest_per_bucket(&sorted, retention.daily, |d| (d.year(), d.ordinal()), &mut keep);
    keep_newest_per_bucket(
        &sorted,
        retention.weekly,
        |d| (d.iso_week().year(), d.iso_week().week()),
        &mut keep,
    );
    keep_newest_per_bucket(&sorted, retention.monthly, |d| (d.year(), d.month()), &mut keep);

    sorted
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !keep.contains(id))
        .collect()
}

pub async fn start_backup_scheduler(db: DatabaseConnection, backup_dir: PathBuf) {
    let started = Local::now();
    let mut last_failure: Option<DateTime<Local>> = None;
    let mut ticker = interval(TICK);
    loop {
        ticker.tick().await;
        match run_due_backup(&db, &backup_dir, started, last_failure).await {
            Ok(_) => last_failure = None,
            Err(err) => {
                tracing::error!(error = %err, "backup::scheduler tick failed");
                last_failure = Some(Local::now());
            }
        }
    }
}

async fn load_schedule(db: &DatabaseConnection) -> AppResult<Option<Schedule>> {
    let Some(snapshot) = crate::settings::get_active_policy(db, "backup").await? else {
        return Ok(None);
    };
    let policy: BackupPolicy = serde_json::from_str(&snapshot.snapshot_json)?;
    if policy.schedule_cron.trim().is_empty() {
        return Ok(None);
    }
    parse_backup_schedule(&policy.schedule_cron)
        .map(Some)
        .map_err(|e| AppError::ValidationFailed(vec![e]))
}

async fn last_scheduled_success(db: &DatabaseConnection) -> AppResult<Option<DateTime<Local>>> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT started_at FROM backup_runs WHERE trigger = 'scheduled' AND status = 'success' \
             ORDER BY id DESC LIMIT 1"
                .to_string(),
        ))
        .await?;
    Ok(row
        .and_then(|r| r.try_get::<String>("", "started_at").ok())
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Local)))
}

/// Runs one scheduler tick. Returns `true` when a backup was written.
pub async fn run_due_backup(
    db: &DatabaseConnection,
    backup_dir: &Path,
    scheduler_started: DateTime<Local>,
    last_failure: Option<DateTime<Local>>,
) -> AppResult<bool> {
    let now = Local::now();
    if last_failure.is_some_and(|f| now - f < RETRY_AFTER_FAILURE) {
        return Ok(false);
    }
    let Some(schedule) = load_schedule(db).await? else {
        return Ok(false);
    };
    // Without any scheduled history, start counting from when the scheduler started.
    let since = last_scheduled_success(db).await?.unwrap_or(scheduler_started);
    if !is_due(&schedule, since, now) {
        return Ok(false);
    }

    let target = backup_dir.join(format!("maintafox-{}.db", now.format("%Y%m%d-%H%M%S")));
    let target_str = target.to_string_lossy().to_string();
    if let Err(err) = super::run_manual_backup(db, &target_str, super::TRIGGER_SCHEDULED, None).await {
        record_failed_run(db, &target_str, &err.to_string()).await;
        return Err(err);
    }
    prune_scheduled_backups(db, backup_dir).await?;
    Ok(true)
}

async fn record_failed_run(db: &DatabaseConnection, target: &str, error: &str) {
    let now = Utc::now().to_rfc3339();
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO backup_runs (trigger, status, output_path, encryption_mode, started_at, \
             completed_at, error_message) VALUES ('scheduled', 'failed', ?, 'aes256', ?, ?, ?)",
            [target.into(), now.clone().into(), now.into(), error.into()],
        ))
        .await;
    if let Err(e) = result {
        tracing::error!(error = %e, "could not record the failed scheduled backup");
    }
}

async fn load_retention(db: &DatabaseConnection) -> AppResult<Option<Retention>> {
    let mut values = [0usize; 3];
    for (slot, key) in [
        "backup.retention_daily",
        "backup.retention_weekly",
        "backup.retention_monthly",
    ]
    .iter()
    .enumerate()
    {
        let Some(setting) = crate::settings::get_setting(db, key, "tenant").await? else {
            tracing::warn!(setting = key, "backup retention setting missing; no backups pruned");
            return Ok(None);
        };
        let n: u64 = serde_json::from_str(&setting.setting_value_json)
            .map_err(|e| AppError::ValidationFailed(vec![format!("{key} must be a non-negative integer: {e}")]))?;
        values[slot] = usize::try_from(n).unwrap_or(usize::MAX);
    }
    Ok(Some(Retention {
        daily: values[0],
        weekly: values[1],
        monthly: values[2],
    }))
}

/// Deletes scheduled backup files that fall outside the retention policy.
pub async fn prune_scheduled_backups(db: &DatabaseConnection, backup_dir: &Path) -> AppResult<usize> {
    let Some(retention) = load_retention(db).await? else {
        return Ok(0);
    };
    let Ok(backup_dir) = std::fs::canonicalize(backup_dir) else {
        return Ok(0);
    };
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, output_path, started_at FROM backup_runs \
             WHERE trigger = 'scheduled' AND status = 'success'"
                .to_string(),
        ))
        .await?;

    let mut runs = Vec::new();
    let mut paths = HashMap::new();
    for row in rows {
        let id: i64 = row.try_get("", "id")?;
        let path: String = row.try_get("", "output_path")?;
        let started: String = row.try_get("", "started_at")?;
        let Ok(started) = DateTime::parse_from_rfc3339(&started) else {
            continue;
        };
        // Only files that still exist directly inside the backup folder are candidates.
        let Ok(canonical) = std::fs::canonicalize(&path) else {
            continue;
        };
        if canonical.parent() != Some(backup_dir.as_path()) {
            continue;
        }
        runs.push((id, started.with_timezone(&Local)));
        paths.insert(id, canonical);
    }

    let mut deleted = 0;
    for id in select_for_pruning(&runs, retention) {
        if let Some(path) = paths.get(&id) {
            match std::fs::remove_file(path) {
                Ok(()) => deleted += 1,
                Err(e) => tracing::warn!(path = %path.display(), error = %e, "could not prune backup"),
            }
        }
    }
    if deleted > 0 {
        tracing::info!(deleted, "pruned scheduled backups outside retention");
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use sea_orm_migration::MigratorTrait;

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, 0, 0).single().unwrap()
    }

    #[test]
    fn five_field_cron_is_normalized() {
        let s = parse_backup_schedule("0 2 * * *").unwrap();
        let next = s.after(&at(2026, 10, 10, 3)).next().unwrap();
        assert_eq!(next, at(2026, 10, 11, 2));
    }

    #[test]
    fn six_field_cron_is_accepted_and_garbage_rejected() {
        assert!(parse_backup_schedule("0 30 1 * * *").is_ok());
        assert!(parse_backup_schedule("every night").is_err());
        assert!(parse_backup_schedule("").is_err());
    }

    #[test]
    fn due_only_after_a_scheduled_time_has_passed() {
        let s = parse_backup_schedule("0 2 * * *").unwrap();
        assert!(!is_due(&s, at(2026, 10, 10, 3), at(2026, 10, 11, 1)));
        assert!(is_due(&s, at(2026, 10, 10, 3), at(2026, 10, 11, 2)));
        // Missed while closed for days: due once at the next tick.
        assert!(is_due(&s, at(2026, 10, 1, 3), at(2026, 10, 10, 9)));
    }

    #[test]
    fn retention_keeps_daily_weekly_and_monthly_representatives() {
        // One backup per day for 60 days; id 60 = 2026-10-10 (a Saturday), id 1 = 2026-08-12.
        let runs: Vec<(i64, DateTime<Local>)> = (0..60)
            .map(|i| (60 - i, at(2026, 10, 10, 2) - chrono::Duration::days(i)))
            .collect();
        let retention = Retention {
            daily: 7,
            weekly: 4,
            monthly: 3,
        };
        let pruned: HashSet<i64> = select_for_pruning(&runs, retention).into_iter().collect();
        let kept: HashSet<i64> = runs.iter().map(|r| r.0).filter(|id| !pruned.contains(id)).collect();

        let mut expected: HashSet<i64> = (54..=60).collect(); // last 7 days
        expected.extend([60, 54, 47, 40]); // newest of the last 4 ISO weeks (Sundays before)
        expected.extend([60, 50, 20]); // newest of Oct, Sep (30th), Aug (31st)
        assert_eq!(kept, expected);
    }

    #[test]
    fn zero_retention_prunes_everything() {
        let runs = vec![(1, at(2026, 10, 10, 2)), (2, at(2026, 10, 9, 2))];
        let pruned = select_for_pruning(
            &runs,
            Retention {
                daily: 0,
                weekly: 0,
                monthly: 0,
            },
        );
        assert_eq!(pruned.len(), 2);
    }

    #[tokio::test]
    async fn scheduled_tick_writes_backup_and_prunes_old_ones() {
        let dir = tempfile::tempdir().unwrap();
        let key = crate::db::encryption::DatabaseKey::generate();
        let db = crate::db::connect_sqlcipher(&dir.path().join("live.db"), Some(&key), true)
            .await
            .unwrap();
        crate::migrations::Migrator::up(&db, None).await.unwrap();
        crate::db::seeder::seed_system_data(&db).await.unwrap();
        crate::settings::ensure_default_policies(&db).await.unwrap();
        db.execute_unprepared(
            "UPDATE app_settings SET setting_value_json = '1' WHERE setting_key LIKE 'backup.retention_%'",
        )
        .await
        .unwrap();
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();

        // The scheduler started two days ago: the nightly run is overdue.
        let started = Local::now() - chrono::Duration::days(2);
        assert!(run_due_backup(&db, &backups, started, None).await.unwrap());
        assert!(
            !run_due_backup(&db, &backups, started, None).await.unwrap(),
            "not due again"
        );

        // An older scheduled backup in the same folder is pruned (retention 1/1/1).
        let old = backups.join("maintafox-old.db");
        crate::backup::run_manual_backup(&db, &old.to_string_lossy(), crate::backup::TRIGGER_SCHEDULED, None)
            .await
            .unwrap();
        db.execute_unprepared(
            "UPDATE backup_runs SET started_at = '2020-01-01T02:00:00+00:00' WHERE output_path LIKE '%maintafox-old.db'",
        )
        .await
        .unwrap();
        assert_eq!(prune_scheduled_backups(&db, &backups).await.unwrap(), 1);
        assert!(!old.exists());
        assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1);
        db.close().await.unwrap();
    }
}
