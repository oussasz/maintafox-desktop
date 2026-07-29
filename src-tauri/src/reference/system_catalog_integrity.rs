//! System reference catalog integrity.
//!
//! Migrations seed governed `reference_*` catalogs once. Tenant runtime reset
//! keeps `seaql_migrations` and deletes catalog rows, so migrations do not re-run.
//! This module re-applies the system substrate (domains + published sets + required
//! values) idempotently for every domain that must never disappear silently.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::assets::taxonomy_reference;
use crate::errors::AppResult;
use crate::reference::governance::derive_category_for_persist;

const SYSTEM_META: &str = r#"{"origin":"system"}"#;

/// Ensure all system-governed reference catalogs exist after wipe/drift.
pub async fn ensure_system_reference_catalog_integrity(db: &DatabaseConnection) -> AppResult<()> {
    taxonomy_reference::ensure_equipment_taxonomy_reference_integrity(db).await?;
    ensure_di_reference_catalog(db).await?;
    ensure_personnel_skills_reference_catalog(db).await?;
    ensure_work_failure_modes_reference_catalog(db).await?;
    ensure_work_delay_reasons_reference_catalog(db).await?;
    ensure_work_part_unused_reason_reference_catalog(db).await?;
    ensure_org_schedule_class_reference_catalog(db).await?;
    Ok(())
}

async fn ensure_domain(
    db: &DatabaseConnection,
    code: &str,
    name: &str,
    structure_type: &str,
    governance_level: &str,
    is_extendable: i64,
) -> AppResult<()> {
    let category = derive_category_for_persist(code, governance_level);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_domains \
         (code, name, structure_type, governance_level, governance_category, \
          is_extendable, validation_rules_json, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, NULL, \
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [
            code.into(),
            name.into(),
            structure_type.into(),
            governance_level.into(),
            category.as_str().into(),
            is_extendable.into(),
        ],
    ))
    .await?;
    // Keep category aligned for rows created before governance_category existed.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET governance_category = ? WHERE UPPER(TRIM(code)) = ?",
        [category.as_str().into(), code.to_ascii_uppercase().into()],
    ))
    .await?;
    Ok(())
}

async fn ensure_published_set(db: &DatabaseConnection, domain_code: &str) -> AppResult<()> {
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
        [domain_code.into()],
    ))
    .await?;
    Ok(())
}

async fn ensure_di_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(db, "DI.SYMPTOM", "Symptômes DI", "flat", "tenant_managed", 1).await?;
    ensure_domain(db, "DI.ORIGIN", "Origines DI", "flat", "tenant_managed", 1).await?;
    ensure_domain(
        db,
        "DI.PRIORITY",
        "Priorités DI",
        "flat",
        "protected_analytical",
        0,
    )
    .await?;
    ensure_domain(
        db,
        "DI.IMPACT_LEVEL",
        "Niveaux d'impact DI",
        "flat",
        "protected_analytical",
        0,
    )
    .await?;
    ensure_domain(
        db,
        "DI.REQUEST_TYPE",
        "Types de demande DI",
        "flat",
        "protected_analytical",
        0,
    )
    .await?;

    for code in ["DI.SYMPTOM", "DI.ORIGIN", "DI.PRIORITY", "DI.IMPACT_LEVEL", "DI.REQUEST_TYPE"] {
        ensure_published_set(db, code).await?;
    }

    let symptom_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, seed.description, seed.sort_order, NULL, NULL, 'di_symptom', NULL, 1, '{SYSTEM_META}' \
           FROM ( \
                SELECT 'vibration' AS code, 'Vibration anormale' AS label, 'Niveau vibratoire anormal détecté.' AS description, 1 AS sort_order \
                UNION ALL SELECT 'leakage', 'Fuite', 'Présence de fuite (huile, eau, air, etc.).', 2 \
                UNION ALL SELECT 'overheating', 'Surchauffe', 'Montée en température anormale.', 3 \
                UNION ALL SELECT 'noise', 'Bruit anormal', 'Bruit ou cliquetis inhabituel.', 4 \
                UNION ALL SELECT 'performance_drop', 'Perte de performance', 'Dégradation notable des performances.', 5 \
           ) seed \
           JOIN reference_domains d ON d.code = 'DI.SYMPTOM' \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, symptom_sql))
        .await?;

    let origin_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'di_origin', NULL, 1, '{SYSTEM_META}' \
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
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, origin_sql))
        .await?;

    let priority_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, seed.color_hex, NULL, 'di_priority', NULL, 1, '{SYSTEM_META}' \
           FROM ( \
                SELECT 'low' AS code, 'Basse' AS label, 1 AS sort_order, '#198754' AS color_hex \
                UNION ALL SELECT 'medium', 'Normale', 2, '#0dcaf0' \
                UNION ALL SELECT 'high', 'Haute', 3, '#ffc107' \
                UNION ALL SELECT 'critical', 'Critique', 4, '#dc3545' \
           ) seed \
           JOIN reference_domains d ON d.code = 'DI.PRIORITY' \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, priority_sql))
        .await?;

    let impact_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'di_impact_level', NULL, 1, '{SYSTEM_META}' \
           FROM ( \
                SELECT 'unknown' AS code, 'Inconnu' AS label, 1 AS sort_order \
                UNION ALL SELECT 'none', 'Aucun', 2 \
                UNION ALL SELECT 'minor', 'Mineur', 3 \
                UNION ALL SELECT 'major', 'Majeur', 4 \
                UNION ALL SELECT 'critical', 'Critique', 5 \
           ) seed \
           JOIN reference_domains d ON d.code = 'DI.IMPACT_LEVEL' \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, impact_sql))
        .await?;

    let request_type_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, seed.description, seed.sort_order, NULL, NULL, 'di_request_type', NULL, 1, '{SYSTEM_META}' \
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
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, request_type_sql))
        .await?;

    Ok(())
}

async fn ensure_personnel_skills_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "PERSONNEL.SKILLS",
        "Technical Skills",
        "hierarchical",
        "tenant_managed",
        1,
    )
    .await?;
    ensure_published_set(db, "PERSONNEL.SKILLS").await?;

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'personnel_skill', NULL, 1, NULL \
           FROM ( \
                SELECT 'MECH_GENERAL' AS code, 'General Mechanics' AS label, 1 AS sort_order \
                UNION ALL SELECT 'ELEC_INDUSTRIAL', 'Industrial Electrical', 2 \
                UNION ALL SELECT 'INSTRUMENTATION', 'Instrumentation', 3 \
                UNION ALL SELECT 'WELDING', 'Welding', 4 \
                UNION ALL SELECT 'HYDRAULICS', 'Hydraulics', 5 \
                UNION ALL SELECT 'CONDITION_MONITOR', 'Condition Monitoring', 6 \
           ) seed \
           JOIN reference_domains d ON d.code = 'PERSONNEL.SKILLS' \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
            .to_string(),
    ))
    .await?;

    Ok(())
}

async fn ensure_work_failure_modes_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "WORK.FAILURE_MODES",
        "Failure modes",
        "hierarchical",
        "system_seeded",
        0,
    )
    .await?;
    ensure_published_set(db, "WORK.FAILURE_MODES").await?;

    // Baseline catalog independent of `failure_codes` (empty after tenant wipe;
    // migrations do not re-run). Aligns with legacy lookup `failure.mode` codes.
    let baseline_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, seed.description, seed.sort_order, NULL, NULL, 'mode', NULL, 1, '{SYSTEM_META}' \
           FROM ( \
                SELECT 'VIBRATION' AS code, 'Vibration' AS label, 'Vibration' AS description, 1 AS sort_order \
                UNION ALL SELECT 'CORROSION', 'Corrosion', 'Corrosion', 2 \
                UNION ALL SELECT 'BRUIT', 'Bruit anormal', 'Abnormal noise', 3 \
                UNION ALL SELECT 'FUITE', 'Fuite', 'Leak', 4 \
                UNION ALL SELECT 'SURCHAUFFE', 'Surchauffe', 'Overheating', 5 \
                UNION ALL SELECT 'PANNE_ELEC', 'Panne électrique', 'Electrical fault', 6 \
                UNION ALL SELECT 'AUTRE', 'Autre', 'Other', 99 \
           ) seed \
           JOIN reference_domains d ON UPPER(TRIM(d.code)) = UPPER(TRIM('WORK.FAILURE_MODES')) \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, baseline_sql))
        .await?;

    // Additive merge from legacy `failure_codes` when present (pre-wipe / migration path).
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        r#"
            WITH latest_set AS (
              SELECT rs.id AS set_id
              FROM reference_sets rs
              INNER JOIN reference_domains rd ON rd.id = rs.domain_id
              WHERE UPPER(TRIM(rd.code)) = UPPER(TRIM('WORK.FAILURE_MODES'))
                AND rs.status = 'published'
              ORDER BY rs.version_no DESC
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
              )
            "#
        .to_string(),
    ))
    .await?;

    Ok(())
}

async fn ensure_work_delay_reasons_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "WORK.DELAY_REASONS",
        "Delay reasons",
        "flat",
        "tenant_managed",
        1,
    )
    .await?;
    ensure_published_set(db, "WORK.DELAY_REASONS").await?;

    let seed_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, seed.tag, NULL, 1, '{SYSTEM_META}' \
           FROM ( \
                SELECT 'no_parts' AS code, 'Awaiting Spare Parts' AS label, 1 AS sort_order, 'parts' AS tag \
                UNION ALL SELECT 'backordered', 'Parts Backordered', 2, 'parts' \
                UNION ALL SELECT 'no_permit', 'Permit Not Issued', 3, 'permit' \
                UNION ALL SELECT 'permit_expired', 'Permit Expired / Revoked', 4, 'permit' \
                UNION ALL SELECT 'no_shutdown', 'Shutdown Window Unavailable', 5, 'shutdown' \
                UNION ALL SELECT 'vendor_delay', 'Vendor / Contractor Delay', 6, 'vendor' \
                UNION ALL SELECT 'no_labor', 'Insufficient Labor', 7, 'labor' \
                UNION ALL SELECT 'no_access', 'Access to Equipment Denied', 8, 'access' \
                UNION ALL SELECT 'diagnosis', 'Awaiting Diagnosis Result', 9, 'diagnosis' \
                UNION ALL SELECT 'other', 'Other (see notes)', 10, 'other' \
           ) seed \
           JOIN reference_domains d ON d.code = 'WORK.DELAY_REASONS' \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, seed_sql))
        .await?;

    Ok(())
}

async fn ensure_work_part_unused_reason_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "WORK.PART_UNUSED_REASON",
        "Part unused reasons",
        "flat",
        "tenant_managed",
        1,
    )
    .await?;
    ensure_published_set(db, "WORK.PART_UNUSED_REASON").await?;

    let seed_sql = format!(
        "INSERT OR IGNORE INTO reference_values \
         (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
         SELECT rs.id, NULL, seed.code, seed.label, NULL, seed.sort_order, NULL, NULL, 'part_unused_reason', NULL, 1, '{SYSTEM_META}' \
           FROM ( \
                SELECT 'inspection_ok' AS code, 'Inspection OK' AS label, 1 AS sort_order \
                UNION ALL SELECT 'wrong_diagnosis', 'Wrong diagnosis', 2 \
                UNION ALL SELECT 'part_unavailable', 'Part unavailable', 3 \
                UNION ALL SELECT 'already_replaced', 'Already replaced', 4 \
                UNION ALL SELECT 'other', 'Other', 5 \
           ) seed \
           JOIN reference_domains d ON d.code = 'WORK.PART_UNUSED_REASON' \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'"
    );
    db.execute(Statement::from_string(DbBackend::Sqlite, seed_sql))
        .await?;

    Ok(())
}

async fn ensure_org_schedule_class_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "ORG.SCHEDULE_CLASS",
        "Classes horaires",
        "flat",
        "tenant_managed",
        1,
    )
    .await?;
    ensure_published_set(db, "ORG.SCHEDULE_CLASS").await?;

    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        r#"
        INSERT OR IGNORE INTO reference_values
          (set_id, parent_id, code, label, description, sort_order,
           color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json)
        SELECT rs.id, NULL, 'DAY_SHIFT', 'Journée normale', NULL, 1,
               NULL, NULL, 'schedule_class', NULL, 1,
               '{"shift_pattern_code":"DAY_SHIFT","is_continuous":false,"nominal_hours_per_day":8.0}'
          FROM reference_domains d
          JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'
         WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS'
         ORDER BY rs.version_no DESC
         LIMIT 1
        "#
        .to_string(),
    ))
    .await?;

    // Seed default weekday rows for any schedule class value missing details.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
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
           )
        "#
        .to_string(),
    ))
    .await?;

    Ok(())
}
