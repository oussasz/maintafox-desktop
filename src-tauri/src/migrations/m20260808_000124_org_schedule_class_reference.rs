//! Migrate schedule_classes into ORG.SCHEDULE_CLASS reference domain (Category B).
//!
//! - Creates ORG.SCHEDULE_CLASS domain + published set
//! - Migrates each schedule_classes row → reference_values
//! - Repoints schedule_details to reference_value_id
//! - Adds consumer FKs on personnel / equipment
//! - Drops schedule_classes after nulling legacy FKs
//! - Retires orphan ORG.SCHEDULES demo placeholder

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260808_000124_org_schedule_class_reference"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── 1. Domain + published set ────────────────────────────────────────
        db.execute_unprepared(
            r#"
            INSERT INTO reference_domains (
              code, name, structure_type, governance_level, governance_category,
              is_extendable, validation_rules_json, created_at, updated_at
            )
            SELECT
              'ORG.SCHEDULE_CLASS',
              'Classes horaires',
              'flat',
              'tenant_managed',
              'operational_dictionary',
              1,
              NULL,
              strftime('%Y-%m-%dT%H:%M:%SZ','now'),
              strftime('%Y-%m-%dT%H:%M:%SZ','now')
            WHERE NOT EXISTS (
              SELECT 1 FROM reference_domains
              WHERE UPPER(TRIM(code)) = 'ORG.SCHEDULE_CLASS'
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            INSERT INTO reference_sets (
              domain_id, version_no, status, effective_from, created_by_id, created_at, published_at
            )
            SELECT d.id, 1, 'published',
                   strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL,
                   strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                   strftime('%Y-%m-%dT%H:%M:%SZ','now')
            FROM reference_domains d
            WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS'
              AND NOT EXISTS (
                SELECT 1 FROM reference_sets rs
                WHERE rs.domain_id = d.id AND rs.status = 'published'
              );
            "#,
        )
        .await?;

        // ── 2. Migrate schedule_classes → reference_values ───────────────────
        // Use a temp mapping table (old id → new reference_value id).
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS _tmp_schedule_class_map (
                old_id INTEGER PRIMARY KEY,
                new_id INTEGER NOT NULL
            )",
        )
        .await?;

        // Insert one reference_value per schedule_class (code = shift_pattern_code).
        // If code already exists in the published set, reuse it for the map.
        let classes = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id, name, shift_pattern_code, is_continuous, nominal_hours_per_day, is_active \
                 FROM schedule_classes ORDER BY id ASC"
                    .to_string(),
            ))
            .await?;

        for (sort_idx, row) in classes.iter().enumerate() {
            let old_id: i64 = row.try_get("", "id").map_err(|e| {
                DbErr::Custom(format!("schedule_classes.id decode: {e}"))
            })?;
            let name: String = row.try_get("", "name").map_err(|e| {
                DbErr::Custom(format!("schedule_classes.name decode: {e}"))
            })?;
            let code: String = row.try_get("", "shift_pattern_code").map_err(|e| {
                DbErr::Custom(format!("schedule_classes.shift_pattern_code decode: {e}"))
            })?;
            let is_continuous: i64 = row.try_get("", "is_continuous").unwrap_or(0);
            let nominal: f64 = row.try_get("", "nominal_hours_per_day").unwrap_or(8.0);
            let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
            // Codes are alphanumeric (e.g. DAY_SHIFT); escape quotes defensively.
            let safe_code = code.replace('\\', "\\\\").replace('"', "\\\"");
            let metadata = format!(
                r#"{{"shift_pattern_code":"{safe_code}","is_continuous":{},"nominal_hours_per_day":{nominal}}}"#,
                if is_continuous != 0 { "true" } else { "false" },
            );

            // Reuse existing value with same code if present.
            let existing = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT rv.id AS id FROM reference_values rv \
                     JOIN reference_sets rs ON rs.id = rv.set_id \
                     JOIN reference_domains d ON d.id = rs.domain_id \
                     WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
                       AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) \
                     LIMIT 1",
                    [code.clone().into()],
                ))
                .await?;

            let new_id: i64 = if let Some(ex) = existing {
                let id: i64 = ex.try_get("", "id").map_err(|e| {
                    DbErr::Custom(format!("existing rv.id decode: {e}"))
                })?;
                // Refresh metadata / active / label from operational row.
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE reference_values SET label = ?, is_active = ?, metadata_json = ? WHERE id = ?",
                    [
                        name.clone().into(),
                        is_active.into(),
                        metadata.clone().into(),
                        id.into(),
                    ],
                ))
                .await?;
                id
            } else {
                db.execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO reference_values \
                     (set_id, parent_id, code, label, description, sort_order, \
                      color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
                     SELECT rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, 'schedule_class', NULL, ?, ? \
                       FROM reference_domains d \
                       JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
                      WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
                      ORDER BY rs.version_no DESC LIMIT 1",
                    [
                        code.clone().into(),
                        name.clone().into(),
                        ((sort_idx as i64) + 1).into(),
                        is_active.into(),
                        metadata.clone().into(),
                    ],
                ))
                .await?;

                let inserted = db
                    .query_one(Statement::from_sql_and_values(
                        DbBackend::Sqlite,
                        "SELECT rv.id AS id FROM reference_values rv \
                         JOIN reference_sets rs ON rs.id = rv.set_id \
                         JOIN reference_domains d ON d.id = rs.domain_id \
                         WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' \
                           AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?)) \
                         LIMIT 1",
                        [code.clone().into()],
                    ))
                    .await?
                    .ok_or_else(|| {
                        DbErr::Custom(format!(
                            "reference_value missing after insert for schedule code {code}"
                        ))
                    })?;
                inserted
                    .try_get("", "id")
                    .map_err(|e| DbErr::Custom(format!("new rv.id decode: {e}")))?
            };

            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR REPLACE INTO _tmp_schedule_class_map (old_id, new_id) VALUES (?, ?)",
                [old_id.into(), new_id.into()],
            ))
            .await?;
        }

        // If no schedule_classes existed, seed DAY_SHIFT baseline.
        let mapped_count: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c FROM _tmp_schedule_class_map".to_string(),
            ))
            .await?
            .and_then(|r| r.try_get::<i64>("", "c").ok())
            .unwrap_or(0);

        if mapped_count == 0 {
            db.execute_unprepared(
                r#"
                INSERT INTO reference_values
                  (set_id, parent_id, code, label, description, sort_order,
                   color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json)
                SELECT rs.id, NULL, 'DAY_SHIFT', 'Journée normale', NULL, 1,
                       NULL, NULL, 'schedule_class', NULL, 1,
                       '{"shift_pattern_code":"DAY_SHIFT","is_continuous":false,"nominal_hours_per_day":8.0}'
                  FROM reference_domains d
                  JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'
                 WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS'
                   AND NOT EXISTS (
                     SELECT 1 FROM reference_values rv
                     WHERE rv.set_id = rs.id AND UPPER(TRIM(rv.code)) = 'DAY_SHIFT'
                   )
                 ORDER BY rs.version_no DESC LIMIT 1;
                "#,
            )
            .await?;
        }

        // ── 3. Rebuild schedule_details keyed by reference_value_id ──────────
        db.execute_unprepared("ALTER TABLE schedule_details RENAME TO schedule_details__legacy")
            .await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE schedule_details (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                reference_value_id  INTEGER NOT NULL REFERENCES reference_values(id),
                day_of_week         INTEGER NOT NULL,
                shift_start         TEXT    NOT NULL,
                shift_end           TEXT    NOT NULL,
                is_rest_day         INTEGER NOT NULL DEFAULT 0,
                UNIQUE(reference_value_id, day_of_week)
            )
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            INSERT INTO schedule_details
              (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day)
            SELECT m.new_id, sd.day_of_week, sd.shift_start, sd.shift_end, sd.is_rest_day
              FROM schedule_details__legacy sd
              JOIN _tmp_schedule_class_map m ON m.old_id = sd.schedule_class_id
            "#,
        )
        .await?;

        // Seed default weekday rows for any ORG.SCHEDULE_CLASS value missing details.
        db.execute_unprepared(
            r#"
            INSERT OR IGNORE INTO schedule_details
              (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day)
            SELECT rv.id, d.day, '08:00', '16:00', d.rest
              FROM reference_values rv
              JOIN reference_sets rs ON rs.id = rv.set_id
              JOIN reference_domains dm ON dm.id = rs.domain_id
              CROSS JOIN (
                  SELECT 1 AS day, 0 AS rest UNION ALL SELECT 2, 0 UNION ALL SELECT 3, 0
                  UNION ALL SELECT 4, 0 UNION ALL SELECT 5, 0 UNION ALL SELECT 6, 1 UNION ALL SELECT 7, 1
              ) d
             WHERE UPPER(TRIM(dm.code)) = 'ORG.SCHEDULE_CLASS'
               AND NOT EXISTS (
                 SELECT 1 FROM schedule_details sd WHERE sd.reference_value_id = rv.id
               );
            "#,
        )
        .await?;

        db.execute_unprepared("DROP TABLE IF EXISTS schedule_details__legacy")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_schedule_details_ref ON schedule_details(reference_value_id)",
        )
        .await?;

        // ── 4. Consumer FK columns ───────────────────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE personnel ADD COLUMN home_schedule_reference_value_id INTEGER REFERENCES reference_values(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE equipment ADD COLUMN rams_schedule_reference_value_id INTEGER REFERENCES reference_values(id)",
        )
        .await?;

        db.execute_unprepared(
            r#"
            UPDATE personnel
               SET home_schedule_reference_value_id = (
                     SELECT m.new_id FROM _tmp_schedule_class_map m
                      WHERE m.old_id = personnel.home_schedule_id
                   )
             WHERE home_schedule_id IS NOT NULL
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            UPDATE equipment
               SET rams_schedule_reference_value_id = (
                     SELECT m.new_id FROM _tmp_schedule_class_map m
                      WHERE m.old_id = equipment.rams_schedule_class_id
                   )
             WHERE rams_schedule_class_id IS NOT NULL
            "#,
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_personnel_home_schedule_ref ON personnel(home_schedule_reference_value_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_rams_schedule_ref ON equipment(rams_schedule_reference_value_id)",
        )
        .await?;

        // Drop legacy FK columns before removing schedule_classes (SQLite rejects
        // writes when a column REFERENCES a missing table).
        db.execute_unprepared("DROP INDEX IF EXISTS idx_equipment_rams_schedule_class")
            .await?;
        db.execute_unprepared("ALTER TABLE personnel DROP COLUMN home_schedule_id")
            .await?;
        db.execute_unprepared("ALTER TABLE equipment DROP COLUMN rams_schedule_class_id")
            .await?;

        // ── 5. Drop schedule_classes ─────────────────────────────────────────
        db.execute_unprepared("DROP TABLE IF EXISTS schedule_classes")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS _tmp_schedule_class_map")
            .await?;

        // ── 6. Retire ORG.SCHEDULES placeholder ──────────────────────────────
        db.execute_unprepared(
            r#"
            DELETE FROM reference_aliases
             WHERE reference_value_id IN (
               SELECT rv.id FROM reference_values rv
               JOIN reference_sets rs ON rs.id = rv.set_id
               JOIN reference_domains d ON d.id = rs.domain_id
               WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULES'
             );
            "#,
        )
        .await?;
        db.execute_unprepared(
            r#"
            DELETE FROM reference_values
             WHERE set_id IN (
               SELECT rs.id FROM reference_sets rs
               JOIN reference_domains d ON d.id = rs.domain_id
               WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULES'
             );
            "#,
        )
        .await?;
        db.execute_unprepared(
            r#"
            DELETE FROM reference_sets
             WHERE domain_id IN (
               SELECT id FROM reference_domains WHERE UPPER(TRIM(code)) = 'ORG.SCHEDULES'
             );
            "#,
        )
        .await?;
        db.execute_unprepared(
            "DELETE FROM reference_domains WHERE UPPER(TRIM(code)) = 'ORG.SCHEDULES'",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
