//! Migration 145 — Drop unused DI review_team_id (org_node_id is routing SSOT).

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260822_000145_di_drop_review_team_id"
    }
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
        if !names.iter().any(|n| n == "review_team_id") {
            return Ok(());
        }
        db.execute_unprepared(
            "ALTER TABLE intervention_requests DROP COLUMN review_team_id",
        )
        .await
        .map_err(|e| DbErr::Custom(format!("drop review_team_id failed: {e}")))?;
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
        if names.iter().any(|n| n == "review_team_id") {
            return Ok(());
        }
        db.execute_unprepared(
            "ALTER TABLE intervention_requests ADD COLUMN review_team_id INTEGER NULL",
        )
        .await
        .map_err(|e| DbErr::Custom(format!("add review_team_id failed: {e}")))?;
        Ok(())
    }
}
