//! Migration 298 — a week frozen automatically stores no freeze time (sync review of desktop #360).
//!
//! The sync server takes a week's `frozen_at` only from the person named in `frozen_by` who signed
//! the change (`author_not_signer` otherwise). The automatic freeze (`planning::weekly_schedule`,
//! `auto_freeze_week`) is made by nobody: it now stores `frozen_at` NULL with `frozen_by_id` NULL,
//! and its time is read from its "frozen automatically" event (`schedule_cols`). Migration 212 refused
//! a frozen or published week without `frozen_at`
//! (`CHECK (status IN ('draft', 'approved') OR frozen_at IS NOT NULL)`). That check becomes:
//! a frozen or published week has a freeze time, or nobody froze it
//! (`... OR frozen_at IS NOT NULL OR frozen_by_id IS NULL`): a person's freeze still always records
//! when it was made.
//!
//! SQLite cannot change a table constraint: the table is rebuilt as migration 254 does (new table
//! with the new check, every row copied as it is, ids included, old table dropped, new one renamed,
//! its AUTOINCREMENT counter kept), then its indexes and triggers are created again from their own
//! SQL. References from other tables (`weekly_schedule_events`, `weekly_schedule_baseline_items`)
//! keep their name (`legacy_alter_table` on during the rename) and still point to the same ids;
//! foreign keys are off during the rebuild only. A table already carrying the new check is left
//! alone; a table with neither check stops the migration with an error.
//!
//! One value changes: a week frozen automatically by an earlier version (frozen or published,
//! nobody in `frozen_by`, with its "frozen automatically" event for the current baseline) loses its
//! stored `frozen_at`, as the automatic freeze now writes it. Its shown time stays the same (the
//! event was written in the same second), and its next change by a person no longer carries a
//! freeze time nobody signed (the server refused such a week from a PC that joined the plant on
//! 2026-11-17; clearing a freeze time that names nobody is accepted). Capture is off while
//! migrations run: the cleared value is not sent by itself.

use regex::Regex;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261118_000298_weekly_automatic_freeze_time"
    }
}

const TABLE: &str = "weekly_schedules";

/// Weeks frozen automatically by an earlier version: no stored freeze time (module documentation).
const AUTOMATIC_FREEZE_TIME_CLEARED: &str = "UPDATE weekly_schedules SET frozen_at = NULL
     WHERE status IN ('frozen', 'published') AND frozen_by_id IS NULL AND frozen_at IS NOT NULL
       AND EXISTS (SELECT 1 FROM weekly_schedule_events e
                    WHERE e.weekly_schedule_id = weekly_schedules.id AND e.baseline_no = weekly_schedules.baseline_no
                      AND e.action = 'auto_frozen')";

/// The check that replaces migration 212's.
const NEW_CHECK: &str = "CHECK (status IN ('draft', 'approved') OR frozen_at IS NOT NULL OR frozen_by_id IS NULL)";

async fn texts(conn: &impl ConnectionTrait, sql: &str) -> Result<Vec<String>, DbErr> {
    conn.query_all(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        [TABLE.into()],
    ))
    .await?
    .iter()
    .map(|r| r.try_get_by_index::<String>(0))
    .collect()
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let Some(create_sql) = texts(db, "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?")
            .await?
            .into_iter()
            .next()
        else {
            return Ok(());
        };
        let old_check = Regex::new(
            r"(?i)CHECK\s*\(\s*status\s+IN\s*\(\s*'draft'\s*,\s*'approved'\s*\)\s+OR\s+frozen_at\s+IS\s+NOT\s+NULL\s*\)",
        )
        .expect("regex");
        if !old_check.is_match(&create_sql) {
            // Already rebuilt (this migration's check is there); anything else is a table this
            // migration does not know, never rebuilt blindly.
            if create_sql.contains(NEW_CHECK) {
                return Ok(());
            }
            return Err(DbErr::Migration(
                "migration 298: weekly_schedules has neither the freeze-time check of migration 212 nor its \
                 replacement; table left unchanged"
                    .to_string(),
            ));
        }
        let name =
            Regex::new(r#"(?i)^\s*CREATE\s+TABLE\s+(IF\s+NOT\s+EXISTS\s+)?"?weekly_schedules"?"#).expect("regex");
        let relaxed = old_check.replace(&create_sql, NEW_CHECK).into_owned();
        let rebuilt_sql = name
            .replace(&relaxed, "CREATE TABLE weekly_schedules_rebuilt")
            .into_owned();
        // Indexes and triggers of the table (the automatic indexes of its constraints have no SQL).
        let kept = texts(
            db,
            "SELECT sql FROM sqlite_master WHERE tbl_name = ? AND type IN ('index', 'trigger') AND sql IS NOT NULL
             ORDER BY type, name",
        )
        .await?;

        db.execute_unprepared("PRAGMA foreign_keys = OFF").await?;
        db.execute_unprepared("PRAGMA legacy_alter_table = ON").await?;
        let rebuilt = async {
            let txn = db.begin().await?;
            // The AUTOINCREMENT counter goes with the dropped table: kept, so the ids of deleted
            // weeks are never given again (events and baseline items still name them).
            let sequence: Option<i64> = txn
                .query_one(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "SELECT seq FROM sqlite_sequence WHERE name = 'weekly_schedules'".to_string(),
                ))
                .await?
                .map(|r| r.try_get_by_index::<i64>(0))
                .transpose()?;
            txn.execute_unprepared(&rebuilt_sql).await?;
            txn.execute_unprepared("INSERT INTO weekly_schedules_rebuilt SELECT * FROM weekly_schedules")
                .await?;
            txn.execute_unprepared("DROP TABLE weekly_schedules").await?;
            txn.execute_unprepared("ALTER TABLE weekly_schedules_rebuilt RENAME TO weekly_schedules")
                .await?;
            for sql in &kept {
                txn.execute_unprepared(sql).await?;
            }
            txn.execute_unprepared(AUTOMATIC_FREEZE_TIME_CLEARED).await?;
            if let Some(seq) = sequence {
                txn.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sqlite_sequence SET seq = MAX(seq, ?) WHERE name = 'weekly_schedules'",
                    [seq.into()],
                ))
                .await?;
                txn.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "INSERT INTO sqlite_sequence (name, seq) SELECT 'weekly_schedules', ?
                     WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'weekly_schedules')",
                    [seq.into()],
                ))
                .await?;
            }
            txn.commit().await
        }
        .await;
        db.execute_unprepared("PRAGMA legacy_alter_table = OFF").await?;
        db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
        rebuilt
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::sync::test_support::{exec, scalar_i64, started_db_before_migration, RowFactory};

    async fn strings(db: &DatabaseConnection, sql: &str) -> Vec<String> {
        db.query_all(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .iter()
            .map(|r| r.try_get_by_index::<String>(0).expect("text"))
            .collect()
    }

    /// Every row of the table, every column, in id order.
    async fn weeks_snapshot(db: &DatabaseConnection) -> Vec<String> {
        let columns = strings(
            db,
            "SELECT name FROM pragma_table_info('weekly_schedules') ORDER BY cid",
        )
        .await;
        let quoted: Vec<String> = columns.iter().map(|c| format!("quote(\"{c}\")")).collect();
        strings(
            db,
            &format!(
                "SELECT {} FROM weekly_schedules ORDER BY id",
                quoted.join(" || '|' || ")
            ),
        )
        .await
    }

    /// `(table, column)` of every foreign key that points to `weekly_schedules`.
    async fn references_to_weeks(db: &DatabaseConnection) -> Vec<(String, String)> {
        let tables = strings(db, "SELECT name FROM sqlite_master WHERE type = 'table'").await;
        let mut out = Vec::new();
        for table in tables {
            for column in strings(
                db,
                &format!(
                    "SELECT \"from\" FROM pragma_foreign_key_list('{table}') WHERE \"table\" = 'weekly_schedules'"
                ),
            )
            .await
            {
                out.push((table.clone(), column));
            }
        }
        out.sort();
        out
    }

    /// A frozen week with no freeze time is accepted once nobody froze it; a person's freeze still
    /// needs its time. Every row, id, reference and the id counter are kept.
    #[tokio::test]
    async fn an_automatic_freeze_may_store_no_time_and_every_week_is_kept() {
        let db = started_db_before_migration("m20261118_000298_weekly_automatic_freeze_time").await;
        let mut factory = RowFactory::new();
        let week = factory.create(&db, "weekly_schedules").await.expect("a week");
        // The factory gives every week the same site and Monday: move this one to the week before, so the
        // second week below does not break the one-week-per-site-and-Monday rule.
        exec(
            &db,
            "UPDATE weekly_schedules SET status = 'frozen', frozen_at = '2026-11-15T00:00:00Z', baseline_no = 1,
                    week_start = '2026-11-02', week_end = '2026-11-08'
              WHERE id = ?",
            vec![week.rowid.into()],
        )
        .await;
        let refused_before = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE weekly_schedules SET frozen_at = NULL WHERE id = ?",
                [week.rowid.into()],
            ))
            .await;
        assert!(
            refused_before.is_err(),
            "migration 212 refuses a frozen week without time"
        );
        // A week an earlier version froze automatically: its stored time goes, its event stays.
        let automatic = factory.create(&db, "weekly_schedules").await.expect("another week");
        exec(
            &db,
            "UPDATE weekly_schedules SET status = 'published', frozen_at = '2026-11-15T00:00:00Z', baseline_no = 1
              WHERE id = ?",
            vec![automatic.rowid.into()],
        )
        .await;
        exec(
            &db,
            "INSERT INTO weekly_schedule_events (entity_sync_id, weekly_schedule_id, action, baseline_no, created_at)
             VALUES ('m298-auto-frozen', ?, 'auto_frozen', 1, '2026-11-15T00:00:00Z')",
            vec![automatic.rowid.into()],
        )
        .await;
        let before = weeks_snapshot(&db).await;
        let references = references_to_weeks(&db).await;
        assert!(!references.is_empty(), "events and baseline items point to the weeks");
        let sequence = scalar_i64(
            &db,
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'weekly_schedules'), 0)",
            vec![],
        )
        .await;

        crate::migrations::Migrator::up(&db, None).await.expect("migrations");

        assert_eq!(
            scalar_i64(
                &db,
                "SELECT COUNT(*) FROM weekly_schedules WHERE id = ? AND frozen_at IS NULL AND status = 'published'",
                vec![automatic.rowid.into()],
            )
            .await,
            1,
            "an automatic freeze keeps no stored time"
        );
        exec(
            &db,
            "UPDATE weekly_schedules SET frozen_at = '2026-11-15T00:00:00Z' WHERE id = ?",
            vec![automatic.rowid.into()],
        )
        .await;
        assert_eq!(
            weeks_snapshot(&db).await,
            before,
            "every week kept as it was (but that stored time)"
        );
        assert_eq!(references_to_weeks(&db).await, references, "references kept");
        assert!(
            scalar_i64(
                &db,
                "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'weekly_schedules'), 0)",
                vec![],
            )
            .await
                >= sequence,
            "the id counter is kept"
        );
        // Frozen by nobody: no time stored.
        exec(
            &db,
            "UPDATE weekly_schedules SET frozen_at = NULL, frozen_by_id = NULL WHERE id = ?",
            vec![week.rowid.into()],
        )
        .await;
        // A person's freeze still records when it was made.
        let admin = scalar_i64(&db, "SELECT id FROM user_accounts ORDER BY id LIMIT 1", vec![]).await;
        let refused = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE weekly_schedules SET frozen_by_id = ? WHERE id = ?",
                [admin.into(), week.rowid.into()],
            ))
            .await;
        assert!(refused.is_err(), "a person's freeze without its time is refused");
    }
}
