//! Migration 244 — four-eyes on permit approval (owner decision 2026-10-17, CMMS Advisor
//! 2026-10-19): the person who approves a permit is neither the person who wrote it nor the person
//! who submitted it for review, unless the permit's site opted out of four-eyes (the per-site rule
//! `reliability.decisions.allow_self_approval`, `settings::site`); such an approval is flagged.
//!
//! `work_permits` gains who did what, all written by `permit::queries` with the session user:
//! - `created_by_id`: who wrote the permit (at creation);
//! - `submitted_by_id` / `submitted_at`: who sent it for review and when (draft → pending_review);
//!   cleared, with the approval, when the permit returns to draft (a change of equipment, site or a
//!   cancelled isolation line while it waited for review): a fresh submission is needed;
//! - `approved_by_id` / `approved_at`: who approved it and when (pending_review → approved);
//! - `self_approved`: 1 when the approval was made without a verified second person (the approver
//!   wrote or submitted it, or who did was not recorded), allowed only on a site that opted out of
//!   four-eyes ("auto-approbation (site sans 4 yeux)"); 0 for a four-eyes approval.
//!
//! All nullable and NULL on existing permits: nobody is invented for a permit written, submitted or
//! approved before this version (the permit shows "approbateur non enregistré"). The user columns
//! sync with the row as user references (`sync::row_spec`); a received row is judged on arrival
//! (`permit::sync_check`).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261020_000244_permit_four_eyes"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("created_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("submitted_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("submitted_at", "TEXT NULL"),
    ("approved_by_id", "INTEGER NULL REFERENCES user_accounts(id)"),
    ("approved_at", "TEXT NULL"),
    ("self_approved", "INTEGER NULL CHECK (self_approved IN (0, 1))"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        for (column, ddl) in COLUMNS {
            let exists = txn
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT 1 AS ok FROM pragma_table_info('work_permits') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!("ALTER TABLE work_permits ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The record of who wrote, submitted and approved a permit: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    const NAME: &str = "m20261020_000244_permit_four_eyes";

    /// Permits recorded before this step are not rewritten: nobody is invented as their author,
    /// submitter or approver, and nothing else of them changes (status, version, times).
    #[tokio::test]
    async fn existing_permits_are_not_rewritten() {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == NAME)
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to 243");
        let snapshot = "SELECT id || '|' || entity_sync_id || '|' || code || '|' || status || '|' || \
                        row_version || '|' || COALESCE(requested_at, '-') || '|' || COALESCE(issued_at, '-') \
                        FROM work_permits ORDER BY id";
        for (sync, code, status) in [
            ("wp-old-draft", "PTW-OLD-1", "draft"),
            ("wp-old-review", "PTW-OLD-2", "pending_review"),
            ("wp-old-approved", "PTW-OLD-3", "approved"),
            ("wp-old-active", "PTW-OLD-4", "active"),
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO work_permits (entity_sync_id, code, permit_type_id, asset_id, entity_id, status, \
                 requested_at, issued_at, row_version) VALUES (?, ?, 1, 1, 1, ?, '2026-01-01T08:00:00Z', \
                 '2026-01-01T09:00:00Z', 3)",
                [sync.into(), code.into(), status.into()],
            ))
            .await
            .expect("old permit");
        }
        let rows = |db: &sea_orm::DatabaseConnection, sql: &'static str| {
            let db = db.clone();
            async move {
                db.query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
                    .await
                    .expect("query")
                    .iter()
                    .map(|r| r.try_get_by_index::<String>(0).expect("text"))
                    .collect::<Vec<_>>()
            }
        };
        let before = rows(&db, snapshot).await;
        Migrator::up(&db, None).await.expect("244");
        assert_eq!(rows(&db, snapshot).await, before, "existing permits unchanged");
        let recorded = rows(
            &db,
            "SELECT code FROM work_permits WHERE created_by_id IS NOT NULL OR submitted_by_id IS NOT NULL \
             OR submitted_at IS NOT NULL OR approved_by_id IS NOT NULL OR approved_at IS NOT NULL \
             OR self_approved IS NOT NULL",
        )
        .await;
        assert!(recorded.is_empty(), "nobody invented: {recorded:?}");
    }
}
