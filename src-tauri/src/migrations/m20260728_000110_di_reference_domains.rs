//! Migration 110 — DI reference domain standardization.
//!
//! Registers DI intake controlled vocabularies in governed reference data:
//! - `DI.SYMPTOM`      (tenant-managed, extendable)
//! - `DI.ORIGIN`       (tenant-managed, extendable)
//! - `DI.PRIORITY`     (protected analytical, locked)
//! - `DI.IMPACT_LEVEL` (protected analytical, locked)
//!
//! Also guarantees `intervention_requests.symptom_code_id` exists for legacy
//! databases and adds an index for filtering/perf.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260728_000110_di_reference_domains"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "INSERT OR IGNORE INTO reference_domains \
             (code, name, structure_type, governance_level, is_extendable, validation_rules_json, created_at, updated_at) \
             VALUES \
             ('DI.SYMPTOM', 'Symptômes DI', 'flat', 'tenant_managed', 1, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
             ('DI.ORIGIN', 'Origines DI', 'flat', 'tenant_managed', 1, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
             ('DI.PRIORITY', 'Priorités DI', 'flat', 'protected_analytical', 0, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
             ('DI.IMPACT_LEVEL', 'Niveaux d''impact DI', 'flat', 'protected_analytical', 0, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .await?;

        for code in ["DI.SYMPTOM", "DI.ORIGIN", "DI.PRIORITY", "DI.IMPACT_LEVEL"] {
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

        let sys_meta = r#"{"origin":"system"}"#;

        let symptom_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, seed.description, seed.sort_order, NULL, NULL, 'di_symptom', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'vibration' AS code, 'Vibration anormale' AS label, 'Niveau vibratoire anormal détecté.' AS description, 1 AS sort_order \
                    UNION ALL SELECT 'leakage', 'Fuite', 'Présence de fuite (huile, eau, air, etc.).', 2 \
                    UNION ALL SELECT 'overheating', 'Surchauffe', 'Montée en température anormale.', 3 \
                    UNION ALL SELECT 'noise', 'Bruit anormal', 'Bruit ou cliquetis inhabituel.', 4 \
                    UNION ALL SELECT 'performance_drop', 'Perte de performance', 'Dégradation notable des performances.', 5 \
               ) seed \
               JOIN reference_domains d ON d.code = 'DI.SYMPTOM' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, symptom_sql))
            .await?;

        let origin_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'di_origin', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'operator' AS code, 'Opérateur' AS label, 1 AS sort_order \
                    UNION ALL SELECT 'technician', 'Technicien', 2 \
                    UNION ALL SELECT 'inspection', 'Inspection', 3 \
                    UNION ALL SELECT 'pm', 'Maintenance préventive', 4 \
                    UNION ALL SELECT 'iot', 'IoT / Capteur', 5 \
                    UNION ALL SELECT 'quality', 'Qualité', 6 \
                    UNION ALL SELECT 'hse', 'HSE', 7 \
                    UNION ALL SELECT 'production', 'Production', 8 \
                    UNION ALL SELECT 'external', 'Externe', 9 \
               ) seed \
               JOIN reference_domains d ON d.code = 'DI.ORIGIN' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, origin_sql))
            .await?;

        let priority_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, seed.color_hex, NULL, 'di_priority', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'low' AS code, 'Basse' AS label, 1 AS sort_order, '#198754' AS color_hex \
                    UNION ALL SELECT 'medium', 'Normale', 2, '#0dcaf0' \
                    UNION ALL SELECT 'high', 'Haute', 3, '#ffc107' \
                    UNION ALL SELECT 'critical', 'Critique', 4, '#dc3545' \
               ) seed \
               JOIN reference_domains d ON d.code = 'DI.PRIORITY' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, priority_sql))
            .await?;

        let impact_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'di_impact_level', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'unknown' AS code, 'Inconnu' AS label, 1 AS sort_order \
                    UNION ALL SELECT 'none', 'Aucun', 2 \
                    UNION ALL SELECT 'minor', 'Mineur', 3 \
                    UNION ALL SELECT 'major', 'Majeur', 4 \
                    UNION ALL SELECT 'critical', 'Critique', 5 \
               ) seed \
               JOIN reference_domains d ON d.code = 'DI.IMPACT_LEVEL' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, impact_sql))
            .await?;

        // Ensure symptom FK column exists for legacy DBs.
        let cols = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info('intervention_requests')",
            ))
            .await?;

        let has_symptom_col = cols.iter().any(|r| {
            r.try_get::<String>("", "name")
                .map(|n| n.eq_ignore_ascii_case("symptom_code_id"))
                .unwrap_or(false)
        });

        if !has_symptom_col {
            db.execute_unprepared(
                "ALTER TABLE intervention_requests ADD COLUMN symptom_code_id INTEGER REFERENCES reference_values(id)",
            )
            .await?;
        }

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_ir_symptom_code_id ON intervention_requests(symptom_code_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
