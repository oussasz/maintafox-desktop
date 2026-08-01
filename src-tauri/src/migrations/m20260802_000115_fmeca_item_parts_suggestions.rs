//! Migration 115 — FMECA item part mappings + failure-mode FK alignment.

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260802_000115_fmeca_item_parts_suggestions"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Align failure_mode_id FKs to unified reference_values (WORK.FAILURE_MODES).
        db.execute_unprepared("ALTER TABLE fmeca_items RENAME TO fmeca_items__old")
            .await?;
        db.execute_unprepared(
            "CREATE TABLE fmeca_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                analysis_id INTEGER NOT NULL REFERENCES fmeca_analyses(id) ON DELETE CASCADE,
                component_id INTEGER NULL,
                functional_failure TEXT NOT NULL DEFAULT '',
                failure_mode_id INTEGER NULL REFERENCES reference_values(id),
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
                source_ram_ishikawa_diagram_id INTEGER NULL REFERENCES ram_ishikawa_diagrams(id),
                source_ishikawa_flow_node_id TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "INSERT INTO fmeca_items (
                id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                linked_pm_plan_id, linked_work_order_id, revised_rpn,
                source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                row_version, updated_at
            )
            SELECT
                id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                linked_pm_plan_id, linked_work_order_id, revised_rpn,
                source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                row_version, updated_at
            FROM fmeca_items__old",
        )
        .await?;
        db.execute_unprepared("DROP TABLE fmeca_items__old").await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_fmeca_items_analysis ON fmeca_items(analysis_id)")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_fmeca_items_source_ram_ishikawa \
             ON fmeca_items(source_ram_ishikawa_diagram_id)",
        )
        .await?;

        db.execute_unprepared("ALTER TABLE rcm_decisions RENAME TO rcm_decisions__old")
            .await?;
        db.execute_unprepared(
            "CREATE TABLE rcm_decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                entity_sync_id TEXT NOT NULL UNIQUE,
                study_id INTEGER NOT NULL REFERENCES rcm_studies(id) ON DELETE CASCADE,
                function_description TEXT NOT NULL DEFAULT '',
                functional_failure TEXT NOT NULL DEFAULT '',
                failure_mode_id INTEGER NULL REFERENCES reference_values(id),
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
            "INSERT INTO rcm_decisions (
                id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                row_version, updated_at
            )
            SELECT
                id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                row_version, updated_at
            FROM rcm_decisions__old",
        )
        .await?;
        db.execute_unprepared("DROP TABLE rcm_decisions__old").await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_rcm_decisions_study ON rcm_decisions(study_id)")
            .await?;

        // New bridge table: FMECA failure context -> spare part recommendations.
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS fmeca_item_parts (
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                fmeca_item_id INTEGER NOT NULL REFERENCES fmeca_items(id) ON DELETE CASCADE,
                article_id INTEGER NOT NULL REFERENCES articles(id),
                quantity_hint REAL NOT NULL DEFAULT 1.0,
                priority INTEGER NOT NULL DEFAULT 100,
                notes TEXT NULL,
                UNIQUE(fmeca_item_id, article_id)
            )",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_fmeca_item_parts_item ON fmeca_item_parts(fmeca_item_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_fmeca_item_parts_article ON fmeca_item_parts(article_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP INDEX IF EXISTS idx_fmeca_item_parts_article")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_fmeca_item_parts_item")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS fmeca_item_parts").await?;

        // Revert FK targets to failure_codes.
        db.execute_unprepared("ALTER TABLE rcm_decisions RENAME TO rcm_decisions__new")
            .await?;
        db.execute_unprepared(
            "CREATE TABLE rcm_decisions (
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
            "INSERT INTO rcm_decisions (
                id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                row_version, updated_at
            )
            SELECT
                id, entity_sync_id, study_id, function_description, functional_failure, failure_mode_id,
                consequence_category, selected_tactic, justification, review_due_at, linked_pm_plan_id,
                row_version, updated_at
            FROM rcm_decisions__new",
        )
        .await?;
        db.execute_unprepared("DROP TABLE rcm_decisions__new").await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_rcm_decisions_study ON rcm_decisions(study_id)")
            .await?;

        db.execute_unprepared("ALTER TABLE fmeca_items RENAME TO fmeca_items__new")
            .await?;
        db.execute_unprepared(
            "CREATE TABLE fmeca_items (
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
                source_ram_ishikawa_diagram_id INTEGER NULL REFERENCES ram_ishikawa_diagrams(id),
                source_ishikawa_flow_node_id TEXT NULL,
                row_version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        db.execute_unprepared(
            "INSERT INTO fmeca_items (
                id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                linked_pm_plan_id, linked_work_order_id, revised_rpn,
                source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                row_version, updated_at
            )
            SELECT
                id, entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id,
                failure_effect, severity, occurrence, detectability, rpn, recommended_action, current_control,
                linked_pm_plan_id, linked_work_order_id, revised_rpn,
                source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                row_version, updated_at
            FROM fmeca_items__new",
        )
        .await?;
        db.execute_unprepared("DROP TABLE fmeca_items__new").await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_fmeca_items_analysis ON fmeca_items(analysis_id)")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_fmeca_items_source_ram_ishikawa \
             ON fmeca_items(source_ram_ishikawa_diagram_id)",
        )
        .await?;

        Ok(())
    }
}
