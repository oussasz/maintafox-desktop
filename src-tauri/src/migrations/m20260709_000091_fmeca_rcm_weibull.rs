//! Migration 091 — FMECA, RCM, Weibull fit storage (PRD §6.10.4–6.10.6).

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260709_000091_fmeca_rcm_weibull"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS weibull_fit_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                period_start TEXT NULL,
                period_end TEXT NULL,
                n_points INTEGER NOT NULL,
                inter_arrival_hours_json TEXT NOT NULL DEFAULT '[]',
                beta REAL NULL,
                eta REAL NULL,
                beta_ci_low REAL NULL,
                beta_ci_high REAL NULL,
                eta_ci_low REAL NULL,
                eta_ci_high REAL NULL,
                adequate_sample INTEGER NOT NULL DEFAULT 0,
                message TEXT NOT NULL DEFAULT '',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_weibull_fit_equipment ON weibull_fit_results(equipment_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS fmeca_analyses (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                title TEXT NOT NULL,
                boundary_definition TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'draft',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_fmeca_analyses_equipment ON fmeca_analyses(equipment_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS fmeca_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                analysis_id INTEGER NOT NULL REFERENCES fmeca_analyses(id) ON DELETE CASCADE,
                component_id INTEGER NULL,
                functional_failure TEXT NOT NULL DEFAULT '',
                failure_mode_id INTEGER NULL REFERENCES failure_codes(id),
                failure_effect TEXT NOT NULL DEFAULT '',
                severity INTEGER NOT NULL,
                occurrence INTEGER NOT NULL,
                detectability INTEGER NOT NULL,
                rpn INTEGER NOT NULL,
                recommended_action TEXT NOT NULL DEFAULT '',
                current_control TEXT NOT NULL DEFAULT '',
                linked_pm_plan_id INTEGER NULL REFERENCES pm_plans(id),
                linked_work_order_id INTEGER NULL REFERENCES work_orders(id),
                revised_rpn INTEGER NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_fmeca_items_analysis ON fmeca_items(analysis_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS rcm_studies (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                equipment_id INTEGER NOT NULL REFERENCES equipment(id),
                title TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                row_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                created_by_id INTEGER NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_rcm_studies_equipment ON rcm_studies(equipment_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS rcm_decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                study_id INTEGER NOT NULL REFERENCES rcm_studies(id) ON DELETE CASCADE,
                function_description TEXT NOT NULL DEFAULT '',
                functional_failure TEXT NOT NULL DEFAULT '',
                failure_mode_id INTEGER NULL REFERENCES failure_codes(id),
                consequence_category TEXT NOT NULL DEFAULT '',
                selected_tactic TEXT NOT NULL,
                justification TEXT NOT NULL DEFAULT '',
                review_due_at TEXT NULL,
                linked_pm_plan_id INTEGER NULL REFERENCES pm_plans(id),
                row_version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_rcm_decisions_study ON rcm_decisions(study_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS rcm_decisions").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS rcm_studies").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS fmeca_items").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS fmeca_analyses").await?;
        db.execute_unprepared("DROP TABLE IF EXISTS weibull_fit_results").await?;
        Ok(())
    }
}
