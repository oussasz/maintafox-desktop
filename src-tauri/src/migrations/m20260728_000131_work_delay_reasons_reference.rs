//! Migrate WO delay reasons to WORK.DELAY_REASONS (Category B reference domain).
//!
//! - Creates WORK.DELAY_REASONS domain + published set + baseline values
//! - Remaps work_order_delay_segments.delay_reason_id by code
//! - Retargets FK from delay_reason_codes → reference_values
//! - Leaves legacy delay_reason_codes table in place (unused by pause_wo)

use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260728_000131_work_delay_reasons_reference"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            INSERT INTO reference_domains (
              code, name, structure_type, governance_level, governance_category,
              is_extendable, validation_rules_json, created_at, updated_at
            )
            SELECT
              'WORK.DELAY_REASONS',
              'Delay reasons',
              'flat',
              'tenant_managed',
              'operational_dictionary',
              1,
              NULL,
              strftime('%Y-%m-%dT%H:%M:%SZ','now'),
              strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE NOT EXISTS (
              SELECT 1 FROM reference_domains
              WHERE UPPER(TRIM(code)) = 'WORK.DELAY_REASONS'
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
            WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS'
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
                   NULL, NULL, seed.tag, NULL, 1, '{"origin":"system"}'
              FROM (
                SELECT 'no_parts' AS code, 'Awaiting Spare Parts' AS label, 1 AS sort_order, 'parts' AS tag
                UNION ALL SELECT 'backordered', 'Parts Backordered', 2, 'parts'
                UNION ALL SELECT 'no_permit', 'Permit Not Issued', 3, 'permit'
                UNION ALL SELECT 'permit_expired', 'Permit Expired / Revoked', 4, 'permit'
                UNION ALL SELECT 'no_shutdown', 'Shutdown Window Unavailable', 5, 'shutdown'
                UNION ALL SELECT 'vendor_delay', 'Vendor / Contractor Delay', 6, 'vendor'
                UNION ALL SELECT 'no_labor', 'Insufficient Labor', 7, 'labor'
                UNION ALL SELECT 'no_access', 'Access to Equipment Denied', 8, 'access'
                UNION ALL SELECT 'diagnosis', 'Awaiting Diagnosis Result', 9, 'diagnosis'
                UNION ALL SELECT 'other', 'Other (see notes)', 10, 'other'
              ) seed
              JOIN reference_domains d ON UPPER(TRIM(d.code)) = 'WORK.DELAY_REASONS'
              JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published';
            "#,
        )
        .await?;

        // Prefer labels from legacy table when present (idempotent upsert by code).
        db.execute_unprepared(
            r#"
            INSERT OR IGNORE INTO reference_values
              (set_id, parent_id, code, label, description, sort_order,
               color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json)
            SELECT rs.id, NULL, drc.code, drc.label, NULL,
                   ROW_NUMBER() OVER (ORDER BY drc.id),
                   NULL, NULL, COALESCE(drc.category, 'other'), NULL, 1, '{"origin":"delay_reason_codes"}'
              FROM delay_reason_codes drc
              JOIN reference_domains d ON UPPER(TRIM(d.code)) = 'WORK.DELAY_REASONS'
              JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'
             WHERE NOT EXISTS (
               SELECT 1 FROM reference_values rv
                WHERE rv.set_id = rs.id AND UPPER(TRIM(rv.code)) = UPPER(TRIM(drc.code))
             );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS work_order_delay_segments_new (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                work_order_id   INTEGER NOT NULL REFERENCES work_orders(id),
                started_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                ended_at        TEXT    NULL,
                delay_reason_id INTEGER NOT NULL REFERENCES reference_values(id),
                comment         TEXT    NULL,
                entered_by_id   INTEGER NULL REFERENCES user_accounts(id)
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            INSERT INTO work_order_delay_segments_new
              (id, work_order_id, started_at, ended_at, delay_reason_id, comment, entered_by_id)
            SELECT
              s.id,
              s.work_order_id,
              s.started_at,
              s.ended_at,
              COALESCE(mapped.new_id, already_ref.id, fallback.id),
              s.comment,
              s.entered_by_id
            FROM work_order_delay_segments s
            LEFT JOIN (
              SELECT drc.id AS old_id, rv.id AS new_id
                FROM delay_reason_codes drc
                INNER JOIN reference_values rv
                  ON UPPER(TRIM(rv.code)) = UPPER(TRIM(drc.code))
                INNER JOIN reference_sets rs ON rs.id = rv.set_id
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id
               WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS'
            ) mapped ON mapped.old_id = s.delay_reason_id
            LEFT JOIN (
              SELECT rv.id
                FROM reference_values rv
                INNER JOIN reference_sets rs ON rs.id = rv.set_id
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id
               WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS'
                 AND rv.id = rv.id
            ) already_ref ON already_ref.id = s.delay_reason_id
            CROSS JOIN (
              SELECT rv.id AS id
                FROM reference_values rv
                INNER JOIN reference_sets rs ON rs.id = rv.set_id
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id
               WHERE UPPER(TRIM(rd.code)) = 'WORK.DELAY_REASONS'
                 AND UPPER(TRIM(rv.code)) = 'OTHER'
               LIMIT 1
            ) fallback
            WHERE COALESCE(mapped.new_id, already_ref.id, fallback.id) IS NOT NULL;
            "#,
        )
        .await?;

        db.execute_unprepared("DROP TABLE IF EXISTS work_order_delay_segments;")
            .await?;
        db.execute_unprepared("ALTER TABLE work_order_delay_segments_new RENAME TO work_order_delay_segments;")
            .await?;
        db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_wods_wo_id ON work_order_delay_segments(work_order_id);")
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
