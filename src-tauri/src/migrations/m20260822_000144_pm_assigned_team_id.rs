//! Migration 144 — PM crew SSOT: rename assigned_group_id → assigned_team_id.
//!
//! Same pattern as WO migration 143. Null unsafe/non-team IDs — no silent org_node remap.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260822_000144_pm_assigned_team_id"
    }
}

async fn exec_step(db: &dyn ConnectionTrait, step: &str, sql: &str) -> Result<(), DbErr> {
    db.execute_unprepared(sql)
        .await
        .map_err(|e| DbErr::Custom(format!("pm_assigned_team_id step '{step}' failed: {e}")))?;
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let cols = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA table_info(pm_plans)".to_string(),
            ))
            .await
            .map_err(|e| DbErr::Custom(format!("pragma pm_plans: {e}")))?;
        let names: Vec<String> = cols
            .iter()
            .filter_map(|r| r.try_get::<String>("", "name").ok())
            .collect();
        if names.iter().any(|n| n == "assigned_team_id") {
            return Ok(());
        }
        if !names.iter().any(|n| n == "assigned_group_id") {
            return Err(DbErr::Custom(
                "pm_assigned_team_id: neither assigned_group_id nor assigned_team_id present"
                    .into(),
            ));
        }

        exec_step(
            db,
            "null_invalid_crew",
            "UPDATE pm_plans
             SET assigned_group_id = NULL
             WHERE assigned_group_id IS NOT NULL
               AND (
                 EXISTS (SELECT 1 FROM org_nodes o WHERE o.id = pm_plans.assigned_group_id)
                 OR NOT EXISTS (
                   SELECT 1 FROM teams t
                    WHERE t.id = pm_plans.assigned_group_id
                      AND t.deleted_at IS NULL
                 )
               )",
        )
        .await?;

        exec_step(
            db,
            "rename_column",
            "ALTER TABLE pm_plans RENAME COLUMN assigned_group_id TO assigned_team_id",
        )
        .await?;

        exec_step(
            db,
            "idx_assigned_team",
            "CREATE INDEX IF NOT EXISTS idx_pm_plans_assigned_team_id
             ON pm_plans(assigned_team_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let cols = db
            .query_all(Statement::from_string(
                sea_orm::DatabaseBackend::Sqlite,
                "PRAGMA table_info(pm_plans)".to_string(),
            ))
            .await
            .map_err(|e| DbErr::Custom(format!("pragma pm_plans: {e}")))?;
        let names: Vec<String> = cols
            .iter()
            .filter_map(|r| r.try_get::<String>("", "name").ok())
            .collect();
        if names.iter().any(|n| n == "assigned_group_id") {
            return Ok(());
        }
        if names.iter().any(|n| n == "assigned_team_id") {
            exec_step(
                db,
                "rename_back",
                "ALTER TABLE pm_plans RENAME COLUMN assigned_team_id TO assigned_group_id",
            )
            .await?;
        }
        Ok(())
    }
}
