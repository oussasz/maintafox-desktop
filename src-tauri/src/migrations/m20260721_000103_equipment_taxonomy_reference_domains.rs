//! Migration 103 — Equipment taxonomy in `reference_domains` / `reference_sets` / `reference_values`
//! plus FK columns on `equipment` for status and criticality (published reference values).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260721_000103_equipment_taxonomy_reference_domains"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── Reference domains (system-locked vs user-extendable) ─────────────
        db.execute_unprepared(
            "INSERT OR IGNORE INTO reference_domains \
             (code, name, structure_type, governance_level, is_extendable, validation_rules_json, created_at, updated_at) \
             VALUES \
             ('EQUIPMENT.CLASS', 'Classes d''équipement', 'flat', 'system_seeded', 0, NULL, \
              strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
             ('EQUIPMENT.CRITICALITY', 'Criticités d''équipement', 'flat', 'system_seeded', 0, NULL, \
              strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
             ('EQUIPMENT.STATUS', 'Statuts d''équipement', 'flat', 'system_seeded', 0, NULL, \
              strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
             ('EQUIPMENT.FAMILY', 'Familles d''équipement', 'flat', 'tenant_managed', 1, NULL, \
              strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
             ('EQUIPMENT.SUBFAMILY', 'Sous-familles d''équipement', 'flat', 'tenant_managed', 1, NULL, \
              strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .await?;

        // Published v1 sets when missing
        for code in [
            "EQUIPMENT.CLASS",
            "EQUIPMENT.CRITICALITY",
            "EQUIPMENT.STATUS",
            "EQUIPMENT.FAMILY",
            "EQUIPMENT.SUBFAMILY",
        ] {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO reference_sets \
                 (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at) \
                 SELECT d.id, 1, 'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL, \
                        strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                   FROM reference_domains d \
                  WHERE d.code = ? \
                    AND NOT EXISTS ( \
                        SELECT 1 FROM reference_sets rs \
                         WHERE rs.domain_id = d.id AND rs.status = 'published' \
                    )",
                [code.into()],
            ))
            .await?;
        }

        // ── Seed values (system metadata for locked domains) ────────────────
        let sys_meta = r#"{"origin":"system"}"#;

        // STATUS — aligned with existing lifecycle codes + OUT_OF_SERVICE
        let status_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, seed.color_hex, NULL, NULL, NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'ACTIVE_IN_SERVICE' AS code, 'En service' AS label, 1 AS sort_order, '#198754' AS color_hex \
                    UNION ALL SELECT 'IN_STOCK', 'En stock', 2, '#0dcaf0' \
                    UNION ALL SELECT 'OUT_OF_SERVICE', 'Hors service', 3, '#6c757d' \
                    UNION ALL SELECT 'UNDER_MAINTENANCE', 'En maintenance', 4, '#ffc107' \
                    UNION ALL SELECT 'DECOMMISSIONED', 'Mis hors service', 5, '#6c757d' \
                    UNION ALL SELECT 'SCRAPPED', 'Mis au rebut', 6, '#dc3545' \
                    UNION ALL SELECT 'SPARE', 'Pièce de rechange', 7, '#6c757d' \
               ) seed \
               JOIN reference_domains d ON d.code = 'EQUIPMENT.STATUS' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, status_sql))
            .await?;

        // CRITICALITY — A/B/C/D (maps from legacy lookup codes via application layer)
        let crit_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, seed.color_hex, NULL, NULL, NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'A' AS code, 'Criticité A' AS label, 1 AS sort_order, '#dc3545' AS color_hex \
                    UNION ALL SELECT 'B', 'Criticité B', 2, '#ffc107' \
                    UNION ALL SELECT 'C', 'Criticité C', 3, '#0dcaf0' \
                    UNION ALL SELECT 'D', 'Criticité D', 4, '#198754' \
               ) seed \
               JOIN reference_domains d ON d.code = 'EQUIPMENT.CRITICALITY' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, crit_sql))
            .await?;

        // CLASS — includes PUMP (tests / common seed); industrial breadth
        let class_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'equipment_class', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'PUMP' AS code, 'Pompe' AS label, 1 AS sort_order \
                    UNION ALL SELECT 'MOTOR', 'Moteur', 2 \
                    UNION ALL SELECT 'VALVE', 'Vanne', 3 \
                    UNION ALL SELECT 'HEAT_EXCHANGER', 'Échangeur de chaleur', 4 \
                    UNION ALL SELECT 'COMPRESSOR', 'Compresseur', 5 \
                    UNION ALL SELECT 'CONVEYOR', 'Convoyeur', 6 \
                    UNION ALL SELECT 'INSTRUMENTATION', 'Instrumentation', 7 \
                    UNION ALL SELECT 'VESSEL', 'Réservoir / Appareil sous pression', 8 \
               ) seed \
               JOIN reference_domains d ON d.code = 'EQUIPMENT.CLASS' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, class_sql))
            .await?;

        // FAMILY — user-extendable; seed baseline system rows
        let fam_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'equipment_family', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'GENERAL' AS code, 'Général' AS label, 1 AS sort_order \
                    UNION ALL SELECT 'PROCESS', 'Procédé', 2 \
                    UNION ALL SELECT 'UTILITIES', 'Utilités', 3 \
               ) seed \
               JOIN reference_domains d ON d.code = 'EQUIPMENT.FAMILY' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, fam_sql))
            .await?;

        // SUBFAMILY — metadata links to family code (JSON literals; not format-interpolated)
        db.execute_unprepared(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'equipment_subfamily', NULL, 1, seed.metadata_json \
               FROM ( \
                    SELECT 'GENERAL_OTHER' AS code, 'Autre (général)' AS label, 1 AS sort_order, \
                           '{\"family_code\":\"GENERAL\",\"origin\":\"system\"}' AS metadata_json \
                    UNION ALL SELECT 'PROCESS_ROTATING', 'Équipement rotatif', 2, \
                           '{\"family_code\":\"PROCESS\",\"origin\":\"system\"}' \
                    UNION ALL SELECT 'PROCESS_STATIC', 'Équipement statique', 3, \
                           '{\"family_code\":\"PROCESS\",\"origin\":\"system\"}' \
                    UNION ALL SELECT 'UTIL_ELECTRICAL', 'Distribution électrique', 4, \
                           '{\"family_code\":\"UTILITIES\",\"origin\":\"system\"}' \
               ) seed \
               JOIN reference_domains d ON d.code = 'EQUIPMENT.SUBFAMILY' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
        )
        .await?;

        // ── Equipment FK columns ────────────────────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE equipment ADD COLUMN equipment_status_ref_id INTEGER REFERENCES reference_values(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE equipment ADD COLUMN equipment_criticality_ref_id INTEGER REFERENCES reference_values(id)",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE equipment ADD COLUMN equipment_class_ref_id INTEGER REFERENCES reference_values(id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_status_ref ON equipment(equipment_status_ref_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_crit_ref ON equipment(equipment_criticality_ref_id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_equipment_class_ref ON equipment(equipment_class_ref_id)",
        )
        .await?;

        // Backfill status ref from lifecycle_status (case-insensitive)
        db.execute_unprepared(
            "UPDATE equipment SET equipment_status_ref_id = ( \
                SELECT rv.id FROM reference_values rv \
                INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                WHERE rd.code = 'EQUIPMENT.STATUS' AND rs.status = 'published' \
                  AND UPPER(TRIM(rv.code)) = UPPER(TRIM(equipment.lifecycle_status)) \
                LIMIT 1 \
            ) WHERE equipment_status_ref_id IS NULL",
        )
        .await?;

        // Map legacy plain-language defaults to canonical codes
        db.execute_unprepared(
            "UPDATE equipment SET equipment_status_ref_id = ( \
                SELECT rv.id FROM reference_values rv \
                INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                WHERE rd.code = 'EQUIPMENT.STATUS' AND rs.status = 'published' AND rv.code = 'ACTIVE_IN_SERVICE' LIMIT 1 \
            ), lifecycle_status = 'ACTIVE_IN_SERVICE' \
            WHERE equipment_status_ref_id IS NULL \
              AND LOWER(TRIM(lifecycle_status)) IN ('active', 'active_in_service', 'operational', 'in_service')",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE equipment SET equipment_status_ref_id = ( \
                SELECT rv.id FROM reference_values rv \
                INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                WHERE rd.code = 'EQUIPMENT.STATUS' AND rs.status = 'published' AND rv.code = 'ACTIVE_IN_SERVICE' LIMIT 1 \
            ), lifecycle_status = 'ACTIVE_IN_SERVICE' \
            WHERE equipment_status_ref_id IS NULL",
        )
        .await?;

        // Criticality ref from lookup_values → A/B/C/D
        db.execute_unprepared(
            "UPDATE equipment SET equipment_criticality_ref_id = ( \
                SELECT rv.id FROM reference_values rv \
                INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                INNER JOIN lookup_values lv ON lv.id = equipment.criticality_value_id \
                WHERE rd.code = 'EQUIPMENT.CRITICALITY' AND rs.status = 'published' \
                  AND rv.code = CASE UPPER(TRIM(lv.code)) \
                    WHEN 'CRITIQUE' THEN 'A' \
                    WHEN 'IMPORTANT' THEN 'B' \
                    WHEN 'STANDARD' THEN 'C' \
                    WHEN 'NON_CRITIQUE' THEN 'D' \
                    ELSE 'C' END \
                LIMIT 1 \
            ) WHERE criticality_value_id IS NOT NULL AND equipment_criticality_ref_id IS NULL",
        )
        .await?;

        db.execute_unprepared(
            "UPDATE equipment SET equipment_criticality_ref_id = ( \
                SELECT rv.id FROM reference_values rv \
                INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                WHERE rd.code = 'EQUIPMENT.CRITICALITY' AND rs.status = 'published' AND rv.code = 'C' LIMIT 1 \
            ) WHERE equipment_criticality_ref_id IS NULL",
        )
        .await?;

        // Class ref: match equipment_classes.code to reference class code
        db.execute_unprepared(
            "UPDATE equipment SET equipment_class_ref_id = ( \
                SELECT rv.id FROM reference_values rv \
                INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                INNER JOIN reference_domains rd ON rd.id = rs.domain_id \
                INNER JOIN equipment_classes ec ON ec.id = equipment.class_id \
                WHERE rd.code = 'EQUIPMENT.CLASS' AND rs.status = 'published' \
                  AND UPPER(TRIM(rv.code)) = UPPER(TRIM(ec.code)) \
                LIMIT 1 \
            ) WHERE class_id IS NOT NULL AND equipment_class_ref_id IS NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
