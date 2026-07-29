use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            INSERT INTO reference_domains (
              code, name, structure_type, governance_level, is_extendable, validation_rules_json, created_at, updated_at
            )
            SELECT
              'WORK.FAILURE_MODES',
              'Failure modes',
              'hierarchical',
              'system_seeded',
              0,
              NULL,
              datetime('now'),
              datetime('now')
            WHERE NOT EXISTS (
              SELECT 1 FROM reference_domains WHERE UPPER(TRIM(code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            INSERT INTO reference_sets (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at)
            SELECT rd.id, 1, 'published', datetime('now'), NULL, datetime('now'), datetime('now')
            FROM reference_domains rd
            WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
              AND NOT EXISTS (
                SELECT 1 FROM reference_sets rs WHERE rs.domain_id = rd.id
              );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            WITH latest_set AS (
              SELECT rs.id AS set_id
              FROM reference_sets rs
              INNER JOIN reference_domains rd ON rd.id = rs.domain_id
              WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
              ORDER BY CASE rs.status
                WHEN 'draft' THEN 0
                WHEN 'validated' THEN 1
                WHEN 'published' THEN 2
                ELSE 3
              END, rs.version_no DESC
              LIMIT 1
            )
            INSERT INTO reference_values (
              set_id, parent_id, code, label, description, color_hex, sort_order, is_active, metadata_json
            )
            SELECT
              ls.set_id,
              NULL,
              fc.code,
              fc.label,
              fc.iso_14224_annex_ref,
              NULL,
              ROW_NUMBER() OVER (ORDER BY fc.code ASC),
              fc.is_active,
              NULL
            FROM failure_codes fc
            CROSS JOIN latest_set ls
            WHERE fc.code_type = 'mode'
              AND NOT EXISTS (
                SELECT 1
                FROM reference_values rv
                WHERE rv.set_id = ls.set_id
                  AND UPPER(TRIM(rv.code)) = UPPER(TRIM(fc.code))
              );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            WITH code_map AS (
              SELECT
                fc.id AS old_id,
                rv.id AS new_id
              FROM failure_codes fc
              INNER JOIN reference_values rv
                ON UPPER(TRIM(rv.code)) = UPPER(TRIM(fc.code))
              INNER JOIN reference_sets rs ON rs.id = rv.set_id
              INNER JOIN reference_domains rd ON rd.id = rs.domain_id
              WHERE fc.code_type = 'mode'
                AND UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
            )
            UPDATE work_order_failure_details
            SET failure_mode_id = (
              SELECT cm.new_id FROM code_map cm WHERE cm.old_id = work_order_failure_details.failure_mode_id
            )
            WHERE failure_mode_id IN (SELECT old_id FROM code_map);
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            WITH code_map AS (
              SELECT
                fc.id AS old_id,
                rv.id AS new_id
              FROM failure_codes fc
              INNER JOIN reference_values rv
                ON UPPER(TRIM(rv.code)) = UPPER(TRIM(fc.code))
              INNER JOIN reference_sets rs ON rs.id = rv.set_id
              INNER JOIN reference_domains rd ON rd.id = rs.domain_id
              WHERE fc.code_type = 'mode'
                AND UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
            )
            UPDATE failure_events
            SET failure_mode_id = (
              SELECT cm.new_id FROM code_map cm WHERE cm.old_id = failure_events.failure_mode_id
            )
            WHERE failure_mode_id IN (SELECT old_id FROM code_map);
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            WITH code_map AS (
              SELECT
                fc.id AS old_id,
                rv.id AS new_id
              FROM failure_codes fc
              INNER JOIN reference_values rv
                ON UPPER(TRIM(rv.code)) = UPPER(TRIM(fc.code))
              INNER JOIN reference_sets rs ON rs.id = rv.set_id
              INNER JOIN reference_domains rd ON rd.id = rs.domain_id
              WHERE fc.code_type = 'mode'
                AND UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
            )
            UPDATE fmeca_items
            SET failure_mode_id = (
              SELECT cm.new_id FROM code_map cm WHERE cm.old_id = fmeca_items.failure_mode_id
            )
            WHERE failure_mode_id IN (SELECT old_id FROM code_map);
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            WITH code_map AS (
              SELECT
                fc.id AS old_id,
                rv.id AS new_id
              FROM failure_codes fc
              INNER JOIN reference_values rv
                ON UPPER(TRIM(rv.code)) = UPPER(TRIM(fc.code))
              INNER JOIN reference_sets rs ON rs.id = rv.set_id
              INNER JOIN reference_domains rd ON rd.id = rs.domain_id
              WHERE fc.code_type = 'mode'
                AND UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
            )
            UPDATE rcm_decisions
            SET failure_mode_id = (
              SELECT cm.new_id FROM code_map cm WHERE cm.old_id = rcm_decisions.failure_mode_id
            )
            WHERE failure_mode_id IN (SELECT old_id FROM code_map);
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
