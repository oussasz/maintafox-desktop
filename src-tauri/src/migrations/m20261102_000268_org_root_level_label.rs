//! The top level made at activation was labelled "Root Organization" (English) on every PC
//! (`db::tenant_bootstrap`). It now reads "Entreprise" (UX fix list L5-20). Only the untouched
//! default label changes: a level the plant renamed keeps its name. Every PC runs this itself,
//! so the change is not sent to the others.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261102_000268_org_root_level_label"
    }
}

const OLD_LABEL: &str = "Root Organization";

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
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE org_node_types SET label = ? WHERE code = 'ROOT' AND label = ?",
            [crate::db::tenant_bootstrap::ROOT_LEVEL_LABEL.into(), OLD_LABEL.into()],
        ))
        .await?;
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

    use super::Migration;

    async fn add_root_type(db: &impl ConnectionTrait, model_id: i64, label: &str) {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO org_node_types (entity_sync_id, structure_model_id, code, label, is_root_type, \
             is_active, created_at, updated_at) VALUES (?, ?, 'ROOT', ?, 1, 1, 'x', 'x')",
            [format!("root-{model_id}").into(), model_id.into(), label.into()],
        ))
        .await
        .expect("insert root type");
    }

    async fn label_of(db: &impl ConnectionTrait, model_id: i64) -> String {
        db.query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT label FROM org_node_types WHERE structure_model_id = ? AND code = 'ROOT'",
            [model_id.into()],
        ))
        .await
        .expect("query")
        .expect("row")
        .try_get("", "label")
        .expect("label")
    }

    #[tokio::test]
    async fn renames_only_the_untouched_english_default() {
        let db = Database::connect("sqlite::memory:").await.expect("db");
        crate::migrations::Migrator::up(&db, None).await.expect("migrate");
        for id in [901_i64, 902] {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, \
                 updated_at) VALUES (?, ?, ?, 'archived', 'x', 'x')",
                [id.into(), format!("model-{id}").into(), id.into()],
            ))
            .await
            .expect("insert model");
        }
        add_root_type(&db, 901, "Root Organization").await;
        add_root_type(&db, 902, "Groupe Tellmétal").await;

        Migration.up(&SchemaManager::new(&db)).await.expect("run migration");

        assert_eq!(label_of(&db, 901).await, "Entreprise");
        assert_eq!(label_of(&db, 902).await, "Groupe Tellmétal");
    }
}
