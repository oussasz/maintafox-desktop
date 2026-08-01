//! Migration 128 — DI request type (Category A catalog + column).
//!
//! Adds:
//! - `DI.REQUEST_TYPE` reference domain (system catalog, locked)
//! - seeded values: repair (default), preventive, inspection, installation,
//!   calibration, improvement, observation, other
//! - `intervention_requests.request_type` NOT NULL DEFAULT 'repair'

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260812_000128_di_request_type"
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
             ('DI.REQUEST_TYPE', 'Types de demande DI', 'flat', 'protected_analytical', 0, NULL, \
              strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        )
        .await?;

        // Backfill governance_category when the column exists (migration 123+).
        let _ = db
            .execute_unprepared(
                "UPDATE reference_domains SET governance_category = 'A' \
                 WHERE code = 'DI.REQUEST_TYPE' AND (governance_category IS NULL OR governance_category = '')",
            )
            .await;

        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_sets \
             (domain_id, version_no, status, effective_from, created_by_id, created_at, published_at) \
             SELECT d.id, 1, 'published', strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL, \
                    strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now') \
               FROM reference_domains d \
              WHERE d.code = 'DI.REQUEST_TYPE' \
                AND NOT EXISTS ( \
                    SELECT 1 FROM reference_sets rs \
                     WHERE rs.domain_id = d.id AND rs.status = 'published' \
                )",
            [],
        ))
        .await?;

        let sys_meta = r#"{"origin":"system"}"#;
        let values_sql = format!(
            "INSERT OR IGNORE INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT rs.id, NULL, seed.code, seed.label, seed.description, seed.sort_order, NULL, NULL, 'di_request_type', NULL, 1, '{sys_meta}' \
               FROM ( \
                    SELECT 'repair' AS code, 'Réparation' AS label, 'Intervention corrective / réparation.' AS description, 1 AS sort_order \
                    UNION ALL SELECT 'preventive', 'Préventif', 'Maintenance préventive planifiée.', 2 \
                    UNION ALL SELECT 'inspection', 'Inspection', 'Contrôle / inspection.', 3 \
                    UNION ALL SELECT 'installation', 'Installation', 'Mise en place / installation.', 4 \
                    UNION ALL SELECT 'calibration', 'Calibrage', 'Étalonnage / calibrage.', 5 \
                    UNION ALL SELECT 'improvement', 'Amélioration', 'Amélioration / modification.', 6 \
                    UNION ALL SELECT 'observation', 'Observation', 'Information seule — observation.', 7 \
                    UNION ALL SELECT 'other', 'Autre', 'Autre type de demande.', 8 \
               ) seed \
               JOIN reference_domains d ON d.code = 'DI.REQUEST_TYPE' \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'",
            sys_meta = sys_meta
        );
        db.execute(Statement::from_string(DbBackend::Sqlite, values_sql))
            .await?;

        // Add column if missing (SQLite ALTER ADD is idempotent via pragma check).
        let cols = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "PRAGMA table_info(intervention_requests)".to_string(),
            ))
            .await?;
        let has_request_type = cols.iter().any(|r| {
            r.try_get::<String>("", "name")
                .map(|n| n == "request_type")
                .unwrap_or(false)
        });
        if !has_request_type {
            db.execute_unprepared(
                "ALTER TABLE intervention_requests ADD COLUMN request_type TEXT NOT NULL DEFAULT 'repair'",
            )
            .await?;
        }

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_intervention_requests_request_type \
             ON intervention_requests(request_type)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        // SQLite cannot DROP COLUMN safely on older builds — leave column; remove catalog values.
        db.execute_unprepared(
            "DELETE FROM reference_values WHERE set_id IN ( \
                SELECT rs.id FROM reference_sets rs \
                JOIN reference_domains d ON d.id = rs.domain_id \
                WHERE d.code = 'DI.REQUEST_TYPE' \
             )",
        )
        .await?;
        db.execute_unprepared(
            "DELETE FROM reference_sets WHERE domain_id IN ( \
                SELECT id FROM reference_domains WHERE code = 'DI.REQUEST_TYPE' \
             )",
        )
        .await?;
        db.execute_unprepared("DELETE FROM reference_domains WHERE code = 'DI.REQUEST_TYPE'")
            .await?;
        Ok(())
    }
}
