//! The top level made at activation was labelled "Root Organization" (English) on every PC
//! (`db::tenant_bootstrap`). It now reads "Entreprise" (UX fix list L5-20). Only the untouched
//! default label changes: a level the plant renamed keeps its name. Every PC runs this itself,
//! so the change is not sent to the others; a row with the old label received later (from a PC
//! that has not upgraded, or pulled by a new PC) is renamed on arrival the same way
//! (`sync::apply_hooks::after_upsert`).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261102_000268_org_root_level_label"
    }
}

const OLD_LABEL: &str = "Root Organization";

/// Renames the untouched English default of the top level: every such row, or only row `id`.
/// The caller decides whether the change is captured for sync (it never should be: each PC
/// renames its own copy).
pub(crate) async fn rename_default_root_label(conn: &impl ConnectionTrait, id: Option<i64>) -> Result<(), DbErr> {
    let label = crate::db::tenant_bootstrap::ROOT_LEVEL_LABEL;
    let stmt = match id {
        Some(id) => Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE org_node_types SET label = ? WHERE id = ? AND code = 'ROOT' AND label = ?",
            [label.into(), id.into(), OLD_LABEL.into()],
        ),
        None => Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE org_node_types SET label = ? WHERE code = 'ROOT' AND label = ?",
            [label.into(), OLD_LABEL.into()],
        ),
    };
    conn.execute(stmt).await?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sea-orm-migration does not open a transaction for SQLite: one here.
        let txn = manager.get_connection().begin().await?;
        let capture = txn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT 1 AS ok FROM sqlite_master WHERE type = 'table' AND name = 'sync_capture_suppression'"
                    .to_string(),
            ))
            .await?
            .is_some();
        if capture {
            crate::sync::capture::begin_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        rename_default_root_label(&txn, None).await?;
        if capture {
            crate::sync::capture::end_suppression(&txn)
                .await
                .map_err(|e| DbErr::Custom(e.to_string()))?;
        }
        txn.commit().await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // A label only: going back keeps the French one.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
    use sea_orm_migration::prelude::{MigrationTrait, SchemaManager};
    use sea_orm_migration::MigratorTrait;

    use super::{rename_default_root_label, Migration};

    async fn add_model(db: &impl ConnectionTrait, id: i64, status: &str) {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, \
             updated_at) VALUES (?, ?, ?, ?, 'x', 'x')",
            [id.into(), format!("model-{id}").into(), id.into(), status.into()],
        ))
        .await
        .expect("insert model");
    }

    async fn add_level(db: &impl ConnectionTrait, model_id: i64, code: &str, label: &str) -> i64 {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO org_node_types (entity_sync_id, structure_model_id, code, label, is_root_type, \
             is_active, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 1, 'x', 'x')",
            [
                format!("level-{model_id}-{code}").into(),
                model_id.into(),
                code.into(),
                label.into(),
                i32::from(code == "ROOT").into(),
            ],
        ))
        .await
        .expect("insert level");
        db.query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "id")
        .expect("id")
    }

    async fn label_of(db: &impl ConnectionTrait, id: i64) -> String {
        db.query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT label FROM org_node_types WHERE id = ?",
            [id.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "label")
        .expect("label")
    }

    async fn journal_rows(db: &impl ConnectionTrait) -> i64 {
        db.query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS n FROM sync_capture_journal WHERE entity_type = 'org_node_types'".to_string(),
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "n")
        .expect("n")
    }

    #[tokio::test]
    async fn renames_only_the_untouched_english_default_without_sending_it() {
        let db = Database::connect("sqlite::memory:").await.expect("db");
        crate::migrations::Migrator::up(&db, None).await.expect("migrate");
        add_model(&db, 901, "active").await;
        add_model(&db, 902, "draft").await;
        add_model(&db, 903, "archived").await;
        // Capture triggers are installed at start-up, not by the migrations: install them so the
        // rows below are journalled as on a real PC.
        crate::sync::capture::install_capture(&db)
            .await
            .expect("install capture");
        let active_root = add_level(&db, 901, "ROOT", "Root Organization").await;
        let draft_root = add_level(&db, 902, "ROOT", "Root Organization").await;
        let renamed_root = add_level(&db, 903, "ROOT", "Groupe Tellmétal").await;
        let other_level = add_level(&db, 901, "SITE", "Root Organization").await;
        let journal_before = journal_rows(&db).await;
        assert!(journal_before > 0, "capture is live: the inserts above were journalled");

        Migration.up(&SchemaManager::new(&db)).await.expect("run migration");

        assert_eq!(label_of(&db, active_root).await, "Entreprise");
        assert_eq!(label_of(&db, draft_root).await, "Entreprise");
        assert_eq!(label_of(&db, renamed_root).await, "Groupe Tellmétal");
        assert_eq!(label_of(&db, other_level).await, "Root Organization");
        assert_eq!(
            journal_rows(&db).await,
            journal_before,
            "the rename is never sent to other PCs"
        );
    }

    #[tokio::test]
    async fn a_single_received_row_is_renamed_alone() {
        let db = Database::connect("sqlite::memory:").await.expect("db");
        crate::migrations::Migrator::up(&db, None).await.expect("migrate");
        add_model(&db, 901, "active").await;
        add_model(&db, 902, "draft").await;
        let received = add_level(&db, 901, "ROOT", "Root Organization").await;
        let untouched = add_level(&db, 902, "ROOT", "Root Organization").await;

        rename_default_root_label(&db, Some(received)).await.expect("rename");

        assert_eq!(label_of(&db, received).await, "Entreprise");
        assert_eq!(label_of(&db, untouched).await, "Root Organization");
    }
}
