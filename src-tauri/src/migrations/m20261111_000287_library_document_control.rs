//! Migration 287 — document control fields of the library (L7-18, field test): a document now
//! carries its reference number, its revision and the date until which it is valid, so the plant
//! sees which procedures, certificates and manuals have expired or expire soon.
//!
//! `library_documents` gains three nullable columns:
//! - `document_number`: the plant's reference of the document (e.g. "PR-ELEC-012");
//! - `revision`: its revision label as printed on the document (e.g. "B", "3");
//! - `valid_until`: the last day it is valid (`YYYY-MM-DD`), NULL when it never expires.
//!
//! NULL on existing rows: nothing is invented for a document added before this version. The
//! columns sync with the document row (the generic engine sends every column of the row; the
//! server allowlist is per entity type, so it does not change).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261111_000287_library_document_control"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("document_number", "TEXT NULL"),
    ("revision", "TEXT NULL"),
    ("valid_until", "TEXT NULL"),
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
                    "SELECT 1 AS ok FROM pragma_table_info('library_documents') WHERE name = ?",
                    [(*column).into()],
                ))
                .await?
                .is_some();
            if !exists {
                txn.execute_unprepared(&format!("ALTER TABLE library_documents ADD COLUMN {column} {ddl}"))
                    .await?;
            }
        }
        txn.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_library_documents_valid_until \
             ON library_documents(valid_until)",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The plant's document references: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    const NAME: &str = "m20261111_000287_library_document_control";

    /// Documents added before this step keep every value; the new fields stay empty.
    #[tokio::test]
    async fn existing_documents_are_kept_with_empty_control_fields() {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == NAME)
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to the previous step");
        db.execute_unprepared(
            "INSERT INTO library_documents (category, title, file_name, relative_path, entity_sync_id) \
             VALUES ('sops', 'Consignation', 'loto.pdf', 'library_documents/sops/u-loto.pdf', 'ld-old-287')",
        )
        .await
        .expect("old document");
        Migrator::up(&db, None).await.expect("287");
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT title, document_number IS NULL AND revision IS NULL AND valid_until IS NULL AS empty \
                 FROM library_documents WHERE entity_sync_id = 'ld-old-287'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.try_get::<String>("", "title").expect("title"), "Consignation");
        assert_eq!(row.try_get::<i64>("", "empty").expect("empty"), 1, "nothing invented");
    }
}
