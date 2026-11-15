//! Migration 288 — versions of a library document (L7-17, field test): a new revision of a
//! procedure or manual replaces the file without losing the earlier ones.
//!
//! The versions of a document form a chain inside `library_documents` (no new table, so the sync
//! allowlist does not change):
//! - `previous_version_id`: the version this row replaces (NULL for a first version);
//! - `superseded_at`: when a newer version replaced it (NULL for the current version).
//!
//! The library lists current versions only; the earlier ones stay readable in the document's
//! history. Existing documents are first and current versions: both columns stay NULL.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261111_000288_library_document_versions"
    }
}

const COLUMNS: &[(&str, &str)] = &[
    ("previous_version_id", "INTEGER NULL REFERENCES library_documents(id)"),
    ("superseded_at", "TEXT NULL"),
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
            "CREATE INDEX IF NOT EXISTS idx_library_documents_previous_version \
             ON library_documents(previous_version_id)",
        )
        .await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The document history: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::migrations::Migrator;

    const NAME: &str = "m20261111_000288_library_document_versions";

    /// Documents added before this step stay current first versions.
    #[tokio::test]
    async fn existing_documents_stay_current_first_versions() {
        let db = crate::db::test_db::empty_memory_db().await;
        let pos = Migrator::migrations()
            .iter()
            .position(|m| m.name() == NAME)
            .expect("registered");
        Migrator::up(&db, Some(u32::try_from(pos).expect("position")))
            .await
            .expect("up to the previous step");
        db.execute_unprepared(
            "INSERT INTO library_documents (category, title, file_name, relative_path, entity_sync_id, revision) \
             VALUES ('sops', 'Consignation', 'loto.pdf', 'library_documents/sops/u-loto.pdf', 'ld-old-288', 'A')",
        )
        .await
        .expect("old document");
        Migrator::up(&db, None).await.expect("288");
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT revision, previous_version_id IS NULL AND superseded_at IS NULL AS current \
                 FROM library_documents WHERE entity_sync_id = 'ld-old-288'"
                    .to_string(),
            ))
            .await
            .expect("query")
            .expect("row");
        assert_eq!(row.try_get::<String>("", "revision").expect("revision"), "A");
        assert_eq!(row.try_get::<i64>("", "current").expect("current"), 1);
    }
}
