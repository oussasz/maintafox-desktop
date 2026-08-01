//! WO Plan vs Actual Phase 1 (+ Phase 2 schema hooks).
//!
//! - work_order_parts: origin, consumption_status, not_used_reason_id, not_used_comment
//! - WORK.PART_UNUSED_REASON Category B seed
//! - work_order_tasks: origin (planned|execution_added|generated reserved)
//! - work_orders.planned_downtime_hours
//! - work_order_execution_events append-only timeline
//! - work_order_tools (Phase 2)
//! - work_order_attachments.phase (before|during|after|evidence)
//! - work_order_downtime_segments.classification_code (Phase 2 deferred taxonomy)

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260728_000132_wo_plan_vs_actual"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── WORK.PART_UNUSED_REASON (Category B) ───────────────────────────
        db.execute_unprepared(
            r#"
            INSERT INTO reference_domains (
              code, name, structure_type, governance_level, governance_category,
              is_extendable, validation_rules_json, created_at, updated_at
            )
            SELECT
              'WORK.PART_UNUSED_REASON',
              'Part unused reasons',
              'flat',
              'tenant_managed',
              'operational_dictionary',
              1,
              NULL,
              strftime('%Y-%m-%dT%H:%M:%SZ','now'),
              strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE NOT EXISTS (
              SELECT 1 FROM reference_domains
              WHERE UPPER(TRIM(code)) = 'WORK.PART_UNUSED_REASON'
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            INSERT INTO reference_sets (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at)
            SELECT rd.id, 1, 'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL,
                   strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')
            FROM reference_domains rd
            WHERE UPPER(TRIM(rd.code)) = 'WORK.PART_UNUSED_REASON'
              AND NOT EXISTS (
                SELECT 1 FROM reference_sets rs
                WHERE rs.domain_id = rd.id AND rs.status = 'published'
              );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            INSERT OR IGNORE INTO reference_values
              (set_id, parent_id, code, label, description, sort_order,
               color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json)
            SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order,
                   NULL, NULL, 'part_unused_reason', NULL, 1, '{"origin":"system"}'
              FROM (
                SELECT 'inspection_ok' AS code, 'Inspection OK' AS label, 1 AS sort_order
                UNION ALL SELECT 'wrong_diagnosis', 'Wrong diagnosis', 2
                UNION ALL SELECT 'part_unavailable', 'Part unavailable', 3
                UNION ALL SELECT 'already_replaced', 'Already replaced', 4
                UNION ALL SELECT 'other', 'Other', 5
              ) seed
              JOIN reference_domains d ON UPPER(TRIM(d.code)) = 'WORK.PART_UNUSED_REASON'
              JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published';
            "#,
        )
        .await?;

        // ── Parts plan-vs-actual columns ───────────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE work_order_parts ADD COLUMN origin TEXT NOT NULL DEFAULT 'planned';",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_order_parts ADD COLUMN consumption_status TEXT NOT NULL DEFAULT 'pending';",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_order_parts ADD COLUMN not_used_reason_id INTEGER NULL REFERENCES reference_values(id);",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE work_order_parts ADD COLUMN not_used_comment TEXT NULL;",
        )
        .await?;

        // Backfill consumption_status from quantity_used
        db.execute_unprepared(
            r#"
            UPDATE work_order_parts
               SET consumption_status = 'used'
             WHERE quantity_used IS NOT NULL AND quantity_used > 0;
            "#,
        )
        .await?;

        // ── Tasks origin ───────────────────────────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE work_order_tasks ADD COLUMN origin TEXT NOT NULL DEFAULT 'planned';",
        )
        .await?;

        // ── Planned downtime on WO ─────────────────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE work_orders ADD COLUMN planned_downtime_hours REAL NULL;",
        )
        .await?;

        // ── Execution Log (not Audit) ──────────────────────────────────────
        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS work_order_execution_events (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id INTEGER NOT NULL REFERENCES work_orders(id),
                occurred_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                event_type    TEXT    NOT NULL,
                summary_key   TEXT    NOT NULL,
                summary_params_json TEXT NULL,
                entity_kind   TEXT    NULL,
                entity_id     INTEGER NULL,
                actor_id      INTEGER NULL REFERENCES user_accounts(id)
            );
            "#,
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_wo_exec_events_wo ON work_order_execution_events(work_order_id, occurred_at);",
        )
        .await?;

        // ── Phase 2: tools ─────────────────────────────────────────────────
        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS work_order_tools (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id INTEGER NOT NULL REFERENCES work_orders(id),
                origin        TEXT    NOT NULL DEFAULT 'planned',
                tool_code     TEXT    NULL,
                tool_label    TEXT    NOT NULL,
                usage_status  TEXT    NOT NULL DEFAULT 'planned',
                notes         TEXT    NULL,
                created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );
            "#,
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_wo_tools_wo ON work_order_tools(work_order_id);",
        )
        .await?;

        // ── Phase 2: attachment phase ──────────────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE work_order_attachments ADD COLUMN phase TEXT NULL;",
        )
        .await?;

        // ── Phase 2: downtime classification (nullable until taxonomy ships) ─
        db.execute_unprepared(
            "ALTER TABLE work_order_downtime_segments ADD COLUMN classification_code TEXT NULL;",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
