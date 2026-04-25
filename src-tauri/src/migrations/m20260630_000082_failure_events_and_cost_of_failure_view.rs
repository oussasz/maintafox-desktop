//! Migration 082 — `failure_events` + `v_cost_of_failure` (Gaps 04 Sprint 03).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260630_000082_failure_events_and_cost_of_failure_view"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS failure_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                source_type TEXT NOT NULL,
                source_id INTEGER NOT NULL,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                component_id INTEGER NULL,
                detected_at TEXT NULL,
                failed_at TEXT NULL,
                restored_at TEXT NULL,
                downtime_duration_hours REAL NOT NULL DEFAULT 0,
                active_repair_hours REAL NOT NULL DEFAULT 0,
                waiting_hours REAL NOT NULL DEFAULT 0,
                is_planned INTEGER NOT NULL DEFAULT 0,
                failure_class_id INTEGER NULL REFERENCES failure_codes(id),
                failure_mode_id INTEGER NULL REFERENCES failure_codes(id),
                failure_cause_id INTEGER NULL REFERENCES failure_codes(id),
                failure_effect_id INTEGER NULL REFERENCES failure_codes(id),
                failure_mechanism_id INTEGER NULL REFERENCES failure_codes(id),
                cause_not_determined INTEGER NOT NULL DEFAULT 0,
                production_impact_level INTEGER NULL,
                safety_impact_level INTEGER NULL,
                recorded_by_id INTEGER NULL,
                verification_status TEXT NOT NULL DEFAULT 'recorded',
                eligible_flags_json TEXT NOT NULL DEFAULT '{}',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE (source_type, source_id)
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_failure_events_equipment
             ON failure_events(equipment_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_failure_events_source
             ON failure_events(source_type, source_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE VIEW IF NOT EXISTS v_cost_of_failure AS
             SELECT
               fe.equipment_id AS equipment_id,
               strftime('%Y-%m', COALESCE(fe.failed_at, fe.detected_at, wo.closed_at)) AS period,
               SUM(
                 COALESCE(wo.downtime_hours, fe.downtime_duration_hours, 0) * (
                   CASE WHEN COALESCE(wo.actual_duration_hours, 0) > 0
                     THEN wo.labor_cost / wo.actual_duration_hours
                     ELSE 0 END
                 )
               ) AS total_downtime_cost,
               SUM(COALESCE(wo.total_cost, 0)) AS total_corrective_cost,
               COALESCE(
                 (SELECT bv.currency_code FROM budget_versions bv ORDER BY bv.id DESC LIMIT 1),
                 'USD'
               ) AS currency_code
             FROM failure_events fe
             LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND fe.source_id = wo.id
             WHERE fe.equipment_id IS NOT NULL
               AND strftime('%Y-%m', COALESCE(fe.failed_at, fe.detected_at, wo.closed_at)) IS NOT NULL
             GROUP BY fe.equipment_id, strftime('%Y-%m', COALESCE(fe.failed_at, fe.detected_at, wo.closed_at))",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP VIEW IF EXISTS v_cost_of_failure").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS failure_events").await?;
        Ok(())
    }
}
