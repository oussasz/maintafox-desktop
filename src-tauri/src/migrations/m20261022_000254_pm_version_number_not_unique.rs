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
//! every row copied as it is, ids included, old table dropped, new one renamed, its AUTOINCREMENT
//! counter kept), then its indexes and triggers are created again from their own SQL, and a plain index on `(pm_plan_id,
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

/// The plain index that replaces the unique one.
const NUMBER_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS idx_pm_versions_plan_number ON pm_plan_versions(pm_plan_id, version_no)";

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
            db.execute_unprepared(NUMBER_INDEX).await?;
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
            // The AUTOINCREMENT counter goes with the dropped table: kept, so the ids of deleted
            // versions are never given again (history rows still name them).
            let sequence: Option<i64> = txn
                .query_one(Statement::from_string(
                    DatabaseBackend::Sqlite,
                    "SELECT seq FROM sqlite_sequence WHERE name = 'pm_plan_versions'".to_string(),
                ))
                .await?
                .map(|r| r.try_get_by_index::<i64>(0))
                .transpose()?;
            txn.execute_unprepared(&rebuilt_sql).await?;
            txn.execute_unprepared("INSERT INTO pm_plan_versions_rebuilt SELECT * FROM pm_plan_versions")
                .await?;
            txn.execute_unprepared("DROP TABLE pm_plan_versions").await?;
            txn.execute_unprepared("ALTER TABLE pm_plan_versions_rebuilt RENAME TO pm_plan_versions")
                .await?;
            for sql in &kept {
                txn.execute_unprepared(sql).await?;
            }
            if let Some(seq) = sequence {
                txn.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sqlite_sequence SET seq = MAX(seq, ?) WHERE name = 'pm_plan_versions'",
                    [seq.into()],
                ))
                .await?;
                txn.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "INSERT INTO sqlite_sequence (name, seq) SELECT 'pm_plan_versions', ?
                     WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'pm_plan_versions')",
                    [seq.into()],
                ))
                .await?;
            }
            txn.execute_unprepared(NUMBER_INDEX).await?;
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

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::sync::test_support::{exec, scalar_i64, started_db_before_last_migrations, RowFactory};

    async fn strings(db: &DatabaseConnection, sql: &str) -> Vec<String> {
        db.query_all(Statement::from_string(DatabaseBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .iter()
            .map(|r| r.try_get_by_index::<String>(0).expect("text"))
            .collect()
    }

    /// Every row of the table, every column, in id order.
    async fn versions_snapshot(db: &DatabaseConnection) -> Vec<String> {
        let columns = strings(
            db,
            "SELECT name FROM pragma_table_info('pm_plan_versions') ORDER BY cid",
        )
        .await;
        let quoted: Vec<String> = columns.iter().map(|c| format!("quote(\"{c}\")")).collect();
        strings(
            db,
            &format!(
                "SELECT {} FROM pm_plan_versions ORDER BY id",
                quoted.join(" || '|' || ")
            ),
        )
        .await
    }

    /// `(table, column)` of every foreign key that points to `pm_plan_versions`.
    async fn references_to_versions(db: &DatabaseConnection) -> Vec<(String, String)> {
        let tables = strings(db, "SELECT name FROM sqlite_master WHERE type = 'table'").await;
        let mut out = Vec::new();
        for table in tables {
            for column in strings(
                db,
                &format!(
                    "SELECT \"from\" FROM pragma_foreign_key_list('{table}') WHERE \"table\" = 'pm_plan_versions'"
                ),
            )
            .await
            {
                out.push((table.clone(), column));
            }
        }
        out
    }

    async fn sequence(db: &DatabaseConnection) -> i64 {
        scalar_i64(
            db,
            "SELECT seq FROM sqlite_sequence WHERE name = 'pm_plan_versions'",
            vec![],
        )
        .await
    }

    #[tokio::test]
    async fn an_upgraded_database_keeps_every_version_its_ids_its_links_and_its_guards() {
        let db = started_db_before_last_migrations(1).await;
        assert_eq!(
            strings(&db, "SELECT sql FROM sqlite_master WHERE name = 'pm_plan_versions'")
                .await
                .first()
                .map(|s| s.contains("UNIQUE(pm_plan_id, version_no)")),
            Some(true),
            "the earlier schema"
        );
        let mut factory = RowFactory::new();
        for entity_type in [
            "pm_occurrences",
            "pm_trigger_events",
            "rcm_decisions",
            "reliability_study_decisions",
        ] {
            factory.create(&db, entity_type).await.expect(entity_type);
        }
        // A draft deleted after a later version: its id is a gap the counter must keep.
        exec(
            &db,
            "INSERT INTO pm_plan_versions (entity_sync_id, pm_plan_id, version_no, status, effective_from,
                                           trigger_definition_json)
             SELECT 'deleted-draft', pm_plan_id, version_no + 100, 'draft', '2026-11-01', '{}'
             FROM pm_plan_versions ORDER BY id LIMIT 1",
            vec![],
        )
        .await;
        exec(
            &db,
            "DELETE FROM pm_plan_versions WHERE entity_sync_id = 'deleted-draft'",
            vec![],
        )
        .await;

        let rows_before = versions_snapshot(&db).await;
        let sequence_before = sequence(&db).await;
        let triggers_before = strings(
            &db,
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'pm_plan_versions' ORDER BY name",
        )
        .await;
        let references = references_to_versions(&db).await;
        assert!(!references.is_empty());
        assert!(sequence_before > scalar_i64(&db, "SELECT MAX(id) FROM pm_plan_versions", vec![]).await);

        crate::migrations::Migrator::up(&db, None).await.expect("migration 254");

        assert_eq!(
            versions_snapshot(&db).await,
            rows_before,
            "every row, id and value kept"
        );
        assert_eq!(sequence(&db).await, sequence_before, "the id counter kept");
        let triggers_after = strings(
            &db,
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'pm_plan_versions' ORDER BY name",
        )
        .await;
        assert_eq!(triggers_after, triggers_before);
        for name in [
            "trg_pm_plan_versions_crew_size_insert",
            "trg_pm_plan_versions_crew_size_update",
        ] {
            assert!(triggers_after.iter().any(|t| t == name), "{name}: {triggers_after:?}");
        }
        for name in [
            "sync_cap_pm_plan_versions_ai",
            "sync_cap_pm_plan_versions_au",
            "sync_cap_pm_plan_versions_ad",
        ] {
            assert!(triggers_after.iter().any(|t| t == name), "{name}: {triggers_after:?}");
        }
        assert!(
            strings(&db, "PRAGMA foreign_key_check").await.is_empty(),
            "no broken foreign key"
        );
        let mut linked = 0;
        for (table, column) in &references {
            linked += scalar_i64(
                &db,
                &format!("SELECT COUNT(*) FROM \"{table}\" WHERE \"{column}\" IS NOT NULL"),
                vec![],
            )
            .await;
            assert_eq!(
                scalar_i64(
                    &db,
                    &format!(
                        "SELECT COUNT(*) FROM \"{table}\" t WHERE t.\"{column}\" IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM pm_plan_versions v WHERE v.id = t.\"{column}\")"
                    ),
                    vec![],
                )
                .await,
                0,
                "{table}.{column} resolves"
            );
        }
        assert!(
            linked >= 4,
            "occurrence, trigger event and decisions point to versions: {linked}"
        );
        // The unique number is gone; a new id continues after the deleted draft.
        exec(
            &db,
            "INSERT INTO pm_plan_versions (entity_sync_id, pm_plan_id, version_no, status, effective_from,
                                           trigger_definition_json)
             SELECT 'same-number', pm_plan_id, version_no, 'draft', '2026-11-01', '{}'
             FROM pm_plan_versions ORDER BY id LIMIT 1",
            vec![],
        )
        .await;
        assert_eq!(
            scalar_i64(
                &db,
                "SELECT id FROM pm_plan_versions WHERE entity_sync_id = 'same-number'",
                vec![]
            )
            .await,
            sequence_before + 1
        );
    }
}
