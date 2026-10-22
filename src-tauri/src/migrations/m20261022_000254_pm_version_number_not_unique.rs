//! Migration 254 — two versions of a PM plan may carry the same stored number (0.9.0 test finding
//! 4, rule 1, CMMS Advisor decision 2026-10-22: only the display number changes).
//!
//! A version is numbered on the PC that creates it (`MAX + 1` of the plan there). Two PCs that each
//! add a version to a plan before receiving the other's give both the same number; the unique
//! `(pm_plan_id, version_no)` of migration 048 refused the second one on every other PC. The stored
//! number now stays exactly as its PC set it, and two versions may share it; the number shown is
//! computed the same way on every PC from the versions of the plan (`pm::version_clash`).
//!
//! SQLite cannot drop a table constraint: the table is rebuilt (new table without that constraint,
//! every row copied as it is, ids included, old table dropped, new one renamed), then its indexes
//! and triggers are created again from their own SQL, and a plain index on `(pm_plan_id,
//! version_no)` replaces the unique one. No value of any row is changed.
//!
//! The view `pm_plan_version_numbers` gives each version its display number: the versions of a
//! plan taken by (stored number, sync id), each shown with its stored number, or one more than the
//! version before it when that number is already shown. Without two equal numbers every version
//! shows its stored number (gaps left by deleted drafts included); with them, every PC holding the
//! same versions shows the same distinct numbers, nothing being moved or sent
//! (`d(i) = i + max over j <= i of (stored(j) - j)`, i the position in that order). References to the table
//! from other tables keep their name (`legacy_alter_table` on during the rename) and still point to
//! the same ids; foreign keys are off during the rebuild only. A table already without the
//! constraint is left alone.

use regex::Regex;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261022_000254_pm_version_number_not_unique"
    }
}

const TABLE: &str = "pm_plan_versions";

/// Display numbers of the PM plan versions (see the module documentation).
pub(crate) const DISPLAY_NUMBER_VIEW: &str = "CREATE VIEW IF NOT EXISTS pm_plan_version_numbers AS
     SELECT id, pm_plan_id, version_no,
            rn + MAX(version_no - rn) OVER (PARTITION BY pm_plan_id ORDER BY version_no, entity_sync_id
                                            ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS display_no
     FROM (SELECT id, pm_plan_id, version_no, entity_sync_id,
                  ROW_NUMBER() OVER (PARTITION BY pm_plan_id ORDER BY version_no, entity_sync_id) AS rn
           FROM pm_plan_versions)";

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
        let unique = Regex::new(r"(?i),\s*UNIQUE\s*\(\s*pm_plan_id\s*,\s*version_no\s*\)").expect("regex");
        if !unique.is_match(&create_sql) {
            db.execute_unprepared(DISPLAY_NUMBER_VIEW).await?;
            return Ok(());
        }
        let name =
            Regex::new(r#"(?i)^\s*CREATE\s+TABLE\s+(IF\s+NOT\s+EXISTS\s+)?"?pm_plan_versions"?"#).expect("regex");
        let without_unique = unique.replace(&create_sql, "").into_owned();
        let rebuilt_sql = name
            .replace(&without_unique, "CREATE TABLE pm_plan_versions_rebuilt")
            .into_owned();
        // Indexes and triggers of the table (the automatic index of the constraint has no SQL).
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
            txn.execute_unprepared(&rebuilt_sql).await?;
            txn.execute_unprepared("INSERT INTO pm_plan_versions_rebuilt SELECT * FROM pm_plan_versions")
                .await?;
            txn.execute_unprepared("DROP TABLE pm_plan_versions").await?;
            txn.execute_unprepared("ALTER TABLE pm_plan_versions_rebuilt RENAME TO pm_plan_versions")
                .await?;
            for sql in &kept {
                txn.execute_unprepared(sql).await?;
            }
            txn.execute_unprepared(
                "CREATE INDEX IF NOT EXISTS idx_pm_versions_plan_number ON pm_plan_versions(pm_plan_id, version_no)",
            )
            .await?;
            txn.execute_unprepared(DISPLAY_NUMBER_VIEW).await?;
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
