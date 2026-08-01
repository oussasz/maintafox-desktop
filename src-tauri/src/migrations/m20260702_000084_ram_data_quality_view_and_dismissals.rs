//! Migration 084 — `v_ram_data_quality_issues` view + optional `user_dismissals` (Sprint 04).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260702_000084_ram_data_quality_view_and_dismissals"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS user_dismissals (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                user_id INTEGER NOT NULL REFERENCES user_accounts(id),
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                issue_code TEXT NOT NULL,
                scope_key TEXT NOT NULL,
                dismissed_at TEXT NOT NULL,
                row_version INTEGER NOT NULL DEFAULT 1
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_user_dismissals_user_scope
             ON user_dismissals(user_id, scope_key)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_user_dismissals_equipment
             ON user_dismissals(equipment_id)",
        )
        .await?;

        db.execute_unprepared("DROP VIEW IF EXISTS v_ram_data_quality_issues")
            .await?;

        db.execute_unprepared(
            "CREATE VIEW v_ram_data_quality_issues AS
             SELECT DISTINCT wo.equipment_id AS equipment_id,
                    'MISSING_FAILURE_MODE' AS issue_code,
                    'blocking' AS severity,
                    'maintafox://ram/equipment/' || wo.equipment_id || '/failure-mode' AS remediation_url
             FROM work_orders wo
             INNER JOIN work_order_failure_details wfd ON wfd.work_order_id = wo.id
             INNER JOIN work_order_types wot ON wot.id = wo.type_id
             WHERE wo.equipment_id IS NOT NULL
               AND wo.closed_at IS NOT NULL
               AND wfd.failure_mode_id IS NULL
               AND wot.code IN ('corrective', 'emergency')

             UNION ALL

             SELECT e.id AS equipment_id,
                    'MISSING_EXPOSURE' AS issue_code,
                    'blocking' AS severity,
                    'maintafox://ram/equipment/' || e.id || '/exposure' AS remediation_url
             FROM equipment e
             WHERE NOT EXISTS (
               SELECT 1 FROM runtime_exposure_logs rel
               WHERE rel.equipment_id = e.id
                 AND rel.recorded_at >= datetime('now', '-90 days')
             )

             UNION ALL

             SELECT DISTINCT k.equipment_id AS equipment_id,
                    'LOW_SAMPLE' AS issue_code,
                    'warning' AS severity,
                    'maintafox://ram/equipment/' || k.equipment_id || '/kpi' AS remediation_url
             FROM reliability_kpi_snapshots k
             WHERE k.equipment_id IS NOT NULL
               AND (k.data_quality_score < 0.85 OR k.event_count < 5)

             UNION ALL

             SELECT DISTINCT s.equipment_id AS equipment_id,
                    'MISSING_INSPECTION_COVERAGE' AS issue_code,
                    'warning' AS severity,
                    'maintafox://ram/equipment/' || s.equipment_id || '/inspection' AS remediation_url
             FROM inspection_reliability_signals s
             WHERE s.checkpoint_coverage_ratio < 0.85",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP VIEW IF EXISTS v_ram_data_quality_issues")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS user_dismissals")
            .await?;
        Ok(())
    }
}
