//! Closeout validation policies + work_orders closeout sync columns (gap 06 sprint 01).

use sea_orm_migration::prelude::*;
use sea_orm::{DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260703_000085_closeout_validation_policies"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE IF NOT EXISTS closeout_validation_policies (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_id INTEGER NULL REFERENCES org_nodes(id),
                policy_name TEXT NOT NULL,
                applies_when TEXT NOT NULL DEFAULT '{}',
                require_failure_mode_if_unplanned INTEGER NOT NULL DEFAULT 1,
                require_downtime_if_production_impact INTEGER NOT NULL DEFAULT 1,
                allow_close_with_cause_not_determined INTEGER NOT NULL DEFAULT 1,
                allow_close_with_cause_mode_only INTEGER NOT NULL DEFAULT 0,
                require_verification_return_to_service INTEGER NOT NULL DEFAULT 1,
                notes_min_length_when_cnd INTEGER NOT NULL DEFAULT 10,
                entity_sync_id TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            )"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_closeout_policies_entity_sync_id \
             ON closeout_validation_policies(entity_sync_id)"
            .to_string(),
        ))
        .await?;

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT INTO closeout_validation_policies \
             (entity_id, policy_name, applies_when, require_failure_mode_if_unplanned, \
              require_downtime_if_production_impact, allow_close_with_cause_not_determined, \
              allow_close_with_cause_mode_only, require_verification_return_to_service, \
              notes_min_length_when_cnd, entity_sync_id, row_version) \
             VALUES (NULL, 'default_corrective', \
              '{\"maintenance_type\":[\"corrective\",\"emergency\"]}', 1, 1, 1, 0, 1, 10, \
              'closeout_policy:1', 1)"
            .to_string(),
        ))
        .await?;

        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN entity_sync_id TEXT NULL",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE work_orders SET entity_sync_id = lower(hex(randomblob(16))) \
             WHERE entity_sync_id IS NULL",
        )
        .await?;
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_work_orders_entity_sync_id \
             ON work_orders(entity_sync_id)",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN closeout_validation_profile_id INTEGER NULL \
             REFERENCES closeout_validation_policies(id)",
        )
        .await?;
        db.execute_unprepared(
            "UPDATE work_orders SET closeout_validation_profile_id = 1 \
             WHERE closeout_validation_profile_id IS NULL",
        )
        .await?;

        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN closeout_validation_passed INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN no_downtime_attestation INTEGER NOT NULL DEFAULT 0",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN no_downtime_attestation_reason TEXT NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("UPDATE work_orders SET closeout_validation_profile_id = NULL")
            .await
            .ok();
        db.execute_unprepared("DROP INDEX IF EXISTS uq_work_orders_entity_sync_id")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS uq_closeout_policies_entity_sync_id")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS closeout_validation_policies")
            .await?;
        Ok(())
    }
}
