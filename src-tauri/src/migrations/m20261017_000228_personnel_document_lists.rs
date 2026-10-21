//! Migration 228 — personnel lists and document categories as governed reference data.
//!
//! - `PERSONNEL.EMPLOYMENT_STATUSES` (SYSTEM), `PERSONNEL.EMPLOYMENT_TYPES`,
//!   `PERSONNEL.ABSENCE_TYPES` (with a required behaviour tag) and
//!   `PERSONNEL.POSITION_CATEGORIES` (COMPANY), `personnel::reference_lists`.
//! - `DOCUMENT.CATEGORIES` (COMPANY), `library_documents`.
//!
//! Every list is seeded with the codes the program used to fix, so stored values keep their
//! meaning (seeded sync identities, the same on every device). `library_documents` carried a CHECK
//! constraint on its four categories: the table is rebuilt without it, in one transaction, with
//! every column, row id, index and trigger kept (row count asserted equal); the category is now
//! checked against the list on every write (`library_documents::checked_category`).

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261017_000228_personnel_document_lists"
    }
}

const CHECK_MARKER: &str = "CHECK (category IN (";

/// The table's CREATE statement without its category CHECK (`None` when there is none).
pub(crate) fn without_category_check(create_sql: &str) -> Result<Option<String>, DbErr> {
    let Some(start) = create_sql.find(CHECK_MARKER) else {
        return Ok(None);
    };
    // The comma that separates the constraint from the previous column definition.
    let comma = create_sql[..start]
        .rfind(',')
        .ok_or_else(|| DbErr::Custom("library_documents: no comma before the category CHECK".into()))?;
    // The CHECK ends at the parenthesis that balances its opening one.
    let open = start + "CHECK ".len();
    let mut depth = 0_i32;
    let mut end = None;
    for (i, ch) in create_sql[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.ok_or_else(|| DbErr::Custom("library_documents: unbalanced category CHECK".into()))?;
    let out = format!("{}{}", &create_sql[..comma], &create_sql[end..]);
    if out.contains(CHECK_MARKER) {
        return Err(DbErr::Custom("library_documents: more than one category CHECK".into()));
    }
    Ok(Some(out))
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str) -> Result<i64, DbErr> {
    db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?
        .ok_or_else(|| DbErr::Custom(format!("no row: {sql}")))?
        .try_get_by_index::<i64>(0)
}

/// Rebuilds `library_documents` without its category CHECK. Idempotent (no CHECK: nothing to do).
pub(crate) async fn rebuild_library_documents(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    let Some(table_sql) = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'library_documents'".to_string(),
        ))
        .await?
        .map(|r| r.try_get::<String>("", "sql"))
        .transpose()?
    else {
        return Ok(());
    };
    let Some(new_sql) = without_category_check(&table_sql)? else {
        return Ok(());
    };
    // Indexes and triggers of the table (auto-indexes of UNIQUE columns come back with the table).
    let dependents: Vec<String> = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT sql FROM sqlite_master WHERE tbl_name = 'library_documents' \
              AND type IN ('index', 'trigger') AND sql IS NOT NULL ORDER BY type, name"
                .to_string(),
        ))
        .await?
        .iter()
        .map(|r| r.try_get::<String>("", "sql"))
        .collect::<Result<_, _>>()?;
    let columns: Vec<String> = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM pragma_table_info('library_documents') ORDER BY cid".to_string(),
        ))
        .await?
        .iter()
        .map(|r| r.try_get::<String>("", "name").map(|n| format!("\"{n}\"")))
        .collect::<Result<_, _>>()?;
    let column_list = columns.join(", ");
    let rows_before = scalar_i64(db, "SELECT COUNT(*) FROM library_documents").await?;
    let seq_before: Option<i64> = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT seq FROM sqlite_sequence WHERE name = 'library_documents'".to_string(),
        ))
        .await?
        .map(|r| r.try_get_by_index::<i64>(0))
        .transpose()?;

    let staged_sql = new_sql.replacen("library_documents", "library_documents_m228", 1);
    if !staged_sql.contains("library_documents_m228") {
        return Err(DbErr::Custom(
            "library_documents: could not stage the rebuilt table".into(),
        ));
    }
    for sql in [
        staged_sql,
        format!("INSERT INTO library_documents_m228 ({column_list}) SELECT {column_list} FROM library_documents"),
        "DROP TABLE library_documents".to_string(),
        "ALTER TABLE library_documents_m228 RENAME TO library_documents".to_string(),
    ] {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql)).await?;
    }
    for sql in dependents {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql)).await?;
    }
    if let Some(seq) = seq_before {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE sqlite_sequence SET seq = MAX(seq, ?) WHERE name = 'library_documents'",
            [seq.into()],
        ))
        .await?;
    }
    let rows_after = scalar_i64(db, "SELECT COUNT(*) FROM library_documents").await?;
    if rows_after != rows_before {
        return Err(DbErr::Custom(format!(
            "library_documents rebuild: {rows_before} rows before, {rows_after} after"
        )));
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        crate::personnel::reference_lists::ensure_personnel_reference_lists(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        crate::library_documents::ensure_document_categories(&txn)
            .await
            .map_err(|e| DbErr::Custom(e.to_string()))?;
        rebuild_library_documents(&txn).await?;
        txn.commit().await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The lists may already carry the plant's own values, and documents may use them: kept.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_is_cut_out_and_nothing_else() {
        let sql = "CREATE TABLE library_documents (\n id INTEGER PRIMARY KEY AUTOINCREMENT,\n \
                   category TEXT NOT NULL,\n notes TEXT NULL,\n CHECK (category IN (\n 'a',\n 'b'\n ))\n, \
                   entity_sync_id TEXT)";
        let out = without_category_check(sql).expect("ok").expect("had a check");
        assert!(!out.contains("CHECK"));
        assert!(out.contains("notes TEXT NULL\n, entity_sync_id TEXT)"), "{out}");
        assert_eq!(without_category_check(&out).expect("ok"), None);
    }
}

#[cfg(test)]
mod rebuild_tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    async fn exec(db: &sea_orm::DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn strings(db: &sea_orm::DatabaseConnection, sql: &str) -> Vec<String> {
        db.query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect(sql)
            .iter()
            .map(|r| r.try_get_by_index::<String>(0).expect("text"))
            .collect()
    }

    const ROWS: &str = "SELECT id || '|' || category || '|' || entity_sync_id || '|' || relative_path \
                        FROM library_documents ORDER BY id";

    /// The table as migration 099 created it (CHECK on the four categories), later given a sync id,
    /// a unique index and a capture trigger; documents in all four categories, one row removed.
    #[tokio::test]
    async fn the_rebuild_keeps_rows_ids_indexes_and_triggers_and_drops_only_the_check() {
        let db = crate::db::test_db::empty_memory_db().await;
        exec(&db, "CREATE TABLE equipment (id INTEGER PRIMARY KEY)").await;
        exec(&db, "CREATE TABLE user_accounts (id INTEGER PRIMARY KEY)").await;
        exec(&db, "CREATE TABLE capture_log (doc_id INTEGER)").await;
        exec(
            &db,
            r"CREATE TABLE IF NOT EXISTS library_documents (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                category         TEXT    NOT NULL,
                equipment_id     INTEGER NULL REFERENCES equipment(id),
                title            TEXT    NOT NULL DEFAULT '',
                file_name        TEXT    NOT NULL,
                relative_path    TEXT    NOT NULL UNIQUE,
                mime_type        TEXT    NOT NULL DEFAULT 'application/octet-stream',
                size_bytes       INTEGER NOT NULL DEFAULT 0,
                uploaded_by_id   INTEGER NULL REFERENCES user_accounts(id),
                uploaded_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                notes            TEXT    NULL,
                CHECK (category IN (
                    'technical_manuals',
                    'sops',
                    'safety_protocols',
                    'compliance_certificates'
                ))
            )",
        )
        .await;
        exec(
            &db,
            "CREATE INDEX idx_library_documents_category ON library_documents(category)",
        )
        .await;
        exec(&db, "ALTER TABLE library_documents ADD COLUMN entity_sync_id TEXT").await;
        exec(
            &db,
            "CREATE UNIQUE INDEX idx_library_documents_sync ON library_documents(entity_sync_id)",
        )
        .await;
        exec(
            &db,
            "CREATE TRIGGER trg_library_documents_capture AFTER INSERT ON library_documents \
             BEGIN INSERT INTO capture_log (doc_id) VALUES (NEW.id); END",
        )
        .await;
        exec(&db, "INSERT INTO equipment (id) VALUES (7)").await;
        let cats = [
            "technical_manuals",
            "sops",
            "safety_protocols",
            "compliance_certificates",
            "sops",
        ];
        for (i, cat) in cats.iter().enumerate() {
            exec(
                &db,
                &format!(
                    "INSERT INTO library_documents (category, equipment_id, title, file_name, relative_path, \
                     entity_sync_id) VALUES ('{cat}', 7, 'Doc {i}', 'f{i}.pdf', \
                     'library_documents/{cat}/f{i}.pdf', 'sync-{i}')"
                ),
            )
            .await;
        }
        // The last document is removed, so the AUTOINCREMENT sequence is ahead of MAX(id).
        exec(&db, "DELETE FROM library_documents WHERE entity_sync_id = 'sync-4'").await;
        let before = strings(&db, ROWS).await;
        assert!(
            db.execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT INTO library_documents (category, file_name, relative_path) VALUES ('PLANT_OWN', 'x', 'x')"
                    .to_string(),
            ))
            .await
            .is_err(),
            "the CHECK is there before the rebuild"
        );

        super::rebuild_library_documents(&db).await.expect("rebuild");

        let after = strings(&db, ROWS).await;
        assert_eq!(before, after, "every row, id and column kept");
        assert_eq!(before.len(), 4, "documents in all four categories");
        let names = strings(
            &db,
            "SELECT name FROM sqlite_master WHERE tbl_name = 'library_documents' AND type IN ('index','trigger') \
             AND sql IS NOT NULL ORDER BY name",
        )
        .await;
        assert_eq!(
            names,
            [
                "idx_library_documents_category",
                "idx_library_documents_sync",
                "trg_library_documents_capture"
            ]
        );
        // A plant category is accepted by the table (the list is checked on write), the sequence
        // did not reuse the removed id, and the trigger still fires.
        exec(
            &db,
            "INSERT INTO library_documents (category, file_name, relative_path, entity_sync_id) \
             VALUES ('PLANT_OWN', 'x', 'library_documents/PLANT_OWN/x', 'sync-new')",
        )
        .await;
        assert_eq!(
            strings(
                &db,
                "SELECT CAST(id AS TEXT) FROM library_documents WHERE entity_sync_id = 'sync-new'"
            )
            .await,
            ["6"]
        );
        assert_eq!(
            strings(&db, "SELECT CAST(COUNT(*) AS TEXT) FROM capture_log WHERE doc_id = 6").await,
            ["1"]
        );
        // Foreign keys kept: an unknown equipment is refused.
        assert!(db
            .execute(Statement::from_string(
                DbBackend::Sqlite,
                "INSERT INTO library_documents (category, equipment_id, file_name, relative_path) \
                 VALUES ('sops', 999, 'y', 'y')"
                    .to_string(),
            ))
            .await
            .is_err());
        // Idempotent.
        super::rebuild_library_documents(&db)
            .await
            .expect("second run is a no-op");
    }
}
