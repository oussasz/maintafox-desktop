//! Migration 146 — DI equipment FK rename: asset_id → equipment_id.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260823_000146_di_equipment_id"
    }
}

async fn exec_step(db: &dyn ConnectionTrait, step: &str, sql: &str) -> Result<(), DbErr> {
    db.execute_unprepared(sql)
        .await
        .map_err(|e| DbErr::Custom(format!("di_equipment_id step '{step}' failed: {e}")))?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let cols = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA table_info(intervention_requests)".to_string(),
            ))
            .await
            .map_err(|e| DbErr::Custom(format!("pragma intervention_requests: {e}")))?;
        let names: Vec<String> = cols
            .iter()
            .filter_map(|r| r.try_get::<String>("", "name").ok())
            .collect();
        if names.iter().any(|n| n == "equipment_id") {
            return Ok(());
        }
        if !names.iter().any(|n| n == "asset_id") {
            return Err(DbErr::Custom(
                "di_equipment_id: neither asset_id nor equipment_id present".into(),
            ));
        }

        exec_step(db, "drop_old_index", "DROP INDEX IF EXISTS idx_ir_asset").await?;

        exec_step(
            db,
            "rename_column",
            "ALTER TABLE intervention_requests RENAME COLUMN asset_id TO equipment_id",
        )
        .await?;

        exec_step(
            db,
            "idx_equipment",
            "CREATE INDEX IF NOT EXISTS idx_ir_equipment ON intervention_requests(equipment_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let cols = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA table_info(intervention_requests)".to_string(),
            ))
            .await
            .map_err(|e| DbErr::Custom(format!("pragma intervention_requests: {e}")))?;
        let names: Vec<String> = cols
            .iter()
            .filter_map(|r| r.try_get::<String>("", "name").ok())
            .collect();
        if names.iter().any(|n| n == "asset_id") {
            return Ok(());
        }
        if names.iter().any(|n| n == "equipment_id") {
            let _ = db.execute_unprepared("DROP INDEX IF EXISTS idx_ir_equipment").await;
            exec_step(
                db,
                "rename_back",
                "ALTER TABLE intervention_requests RENAME COLUMN equipment_id TO asset_id",
            )
            .await?;
            let _ = db
                .execute_unprepared("CREATE INDEX IF NOT EXISTS idx_ir_asset ON intervention_requests(asset_id)")
                .await;
        }
        Ok(())
    }
}
