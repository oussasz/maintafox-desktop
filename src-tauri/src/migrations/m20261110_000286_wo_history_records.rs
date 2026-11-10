//! Migration 286 — imported maintenance history (L7-30, CMMS Advisor option C, 2026-11-10).
//!
//! One row = one past intervention loaded from the plant's former system ("OT historiques"
//! sheet of the import wizard). It is not a work order: it never enters the OT workflow, has no
//! permits, stock movements or budget postings, and is read-only once imported, except its
//! failure coding, which may be completed later (audited). A failure (`is_failure = 1`) also has
//! one `failure_events` row (`source_type = 'history_record'`), so RAMS counts it with the live
//! work orders; cost per asset reads its historical amounts.
//!
//! `import_ref` is the n° OT of the former system (upper case, unique): a re-run skips it.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20261110_000286_wo_history_records"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS wo_history_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                import_ref TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                type_id INTEGER NOT NULL REFERENCES reference_values(id),
                is_failure INTEGER NOT NULL DEFAULT 0 CHECK (is_failure IN (0, 1)),
                started_at TEXT NOT NULL,
                ended_at TEXT NOT NULL,
                detected_at TEXT NULL,
                stopped_at TEXT NULL,
                restored_at TEXT NULL,
                downtime_hours REAL NULL CHECK (downtime_hours IS NULL OR downtime_hours >= 0),
                failure_mode_id INTEGER NULL REFERENCES reference_values(id),
                failure_cause_id INTEGER NULL REFERENCES reference_values(id),
                failure_mechanism_id INTEGER NULL REFERENCES reference_values(id),
                detection_method_id INTEGER NULL REFERENCES reference_values(id),
                failure_effect_id INTEGER NULL REFERENCES reference_values(id),
                description TEXT NULL,
                work_done TEXT NULL,
                labor_hours REAL NULL CHECK (labor_hours IS NULL OR labor_hours >= 0),
                labor_cost REAL NULL CHECK (labor_cost IS NULL OR labor_cost >= 0),
                parts_cost REAL NULL CHECK (parts_cost IS NULL OR parts_cost >= 0),
                service_cost REAL NULL CHECK (service_cost IS NULL OR service_cost >= 0),
                interveners_text TEXT NULL,
                imported_by_id INTEGER NULL REFERENCES user_accounts(id),
                coded_by_id INTEGER NULL REFERENCES user_accounts(id),
                coded_at TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_wo_history_records_equipment \
             ON wo_history_records(equipment_id, started_at)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS wo_history_records")
            .await?;
        Ok(())
    }
}
