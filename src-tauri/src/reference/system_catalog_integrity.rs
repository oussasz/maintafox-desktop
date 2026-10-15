//! System reference catalog integrity.
//!
//! Migrations seed governed `reference_*` catalogs once. Tenant runtime reset
//! keeps `seaql_migrations` and deletes catalog rows, so migrations do not re-run.
//! This module re-applies the system substrate (domains + published sets + required
//! values) idempotently for every domain that must never disappear silently.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::assets::taxonomy_reference;
use crate::errors::AppResult;
use crate::reference::audience::DomainAudience;
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
    // WO close-out coding (symptoms, causes, effects, mechanisms, detection methods).
    crate::wo::closeout_reference_catalog::ensure_closeout_reference_catalogs(db).await?;
    // Certification / authorisation types and skills in the tenant's language.
    crate::qualification::catalog::ensure_qualification_catalog(db).await?;
    Ok(())
}

async fn ensure_domain(
    db: &DatabaseConnection,
    code: &str,
    name: &str,
    structure_type: &str,
    governance_level: &str,
    is_extendable: i64,
    audience: DomainAudience,
) -> AppResult<()> {
    let category = derive_category_for_persist(code, governance_level);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_domains \
         (code, name, structure_type, governance_level, governance_category, audience, \
          is_extendable, validation_rules_json, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NULL, \
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [
            code.into(),
            name.into(),
            structure_type.into(),
            governance_level.into(),
            category.as_str().into(),
            audience.as_str().into(),
            is_extendable.into(),
        ],
    ))
    .await?;
    // Keep category and audience aligned (rows created before these columns existed, or changed by
    // a replica): both are decided by the product, not by the plant.
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET governance_category = ?, audience = ? WHERE UPPER(TRIM(code)) = ?",
        [
            category.as_str().into(),
            audience.as_str().into(),
            code.to_ascii_uppercase().into(),
        ],
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

/// One system catalog row. Every replica seeds these itself, so the row's sync identity is
/// derived from `domain:code` and agrees across machines.
struct SeedValue {
    code: &'static str,
    label: &'static str,
    description: Option<&'static str>,
    sort_order: i64,
    color_hex: Option<&'static str>,
    semantic_tag: &'static str,
    metadata_json: Option<&'static str>,
}

const fn seed(code: &'static str, label: &'static str, sort_order: i64, semantic_tag: &'static str) -> SeedValue {
    SeedValue {
        code,
        label,
        description: None,
        sort_order,
        color_hex: None,
        semantic_tag,
        metadata_json: Some(SYSTEM_META),
    }
}

fn seeded_value_sync_id(domain_code: &str, value_code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain_code}:{value_code}"))
}

/// Insert missing system rows into the latest published set of `domain_code`.
///
/// Existing rows are left untouched (same `(set_id, code)`), and so is a seeded row that already
/// lives in an older set of the domain (same `entity_sync_id`).
async fn insert_seed_values(db: &DatabaseConnection, domain_code: &str, rows: &[SeedValue]) -> AppResult<()> {
    for row in rows {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT ?, rs.id, NULL, ?, ?, ?, ?, ?, NULL, ?, NULL, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
              WHERE UPPER(TRIM(d.code)) = ? \
              ORDER BY rs.version_no DESC \
              LIMIT 1",
            [
                seeded_value_sync_id(domain_code, row.code).into(),
                row.code.into(),
                row.label.into(),
                row.description.into(),
                row.sort_order.into(),
                row.color_hex.into(),
                row.semantic_tag.into(),
                row.metadata_json.into(),
                domain_code.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

const DI_SYMPTOMS: &[SeedValue] = &[
    SeedValue {
        description: Some("Niveau vibratoire anormal détecté."),
        ..seed("vibration", "Vibration anormale", 1, "di_symptom")
    },
    SeedValue {
        description: Some("Présence de fuite (huile, eau, air, etc.)."),
        ..seed("leakage", "Fuite", 2, "di_symptom")
    },
    SeedValue {
        description: Some("Montée en température anormale."),
        ..seed("overheating", "Surchauffe", 3, "di_symptom")
    },
    SeedValue {
        description: Some("Bruit ou cliquetis inhabituel."),
        ..seed("noise", "Bruit anormal", 4, "di_symptom")
    },
    SeedValue {
        description: Some("Dégradation notable des performances."),
        ..seed("performance_drop", "Perte de performance", 5, "di_symptom")
    },
];

const DI_ORIGINS: &[SeedValue] = &[
    seed("operator", "Opérateur", 1, "di_origin"),
    seed("technician", "Technicien", 2, "di_origin"),
    seed("inspection", "Inspection", 3, "di_origin"),
    seed("pm", "Maintenance préventive", 4, "di_origin"),
    seed("iot", "IoT / Capteur", 5, "di_origin"),
    seed("quality", "Qualité", 6, "di_origin"),
    seed("hse", "HSE", 7, "di_origin"),
    seed("production", "Production", 8, "di_origin"),
    seed("external", "Externe", 9, "di_origin"),
];

const DI_PRIORITIES: &[SeedValue] = &[
    SeedValue {
        color_hex: Some("#198754"),
        ..seed("low", "Basse", 1, "di_priority")
    },
    SeedValue {
        color_hex: Some("#0dcaf0"),
        ..seed("medium", "Normale", 2, "di_priority")
    },
    SeedValue {
        color_hex: Some("#ffc107"),
        ..seed("high", "Haute", 3, "di_priority")
    },
    SeedValue {
        color_hex: Some("#dc3545"),
        ..seed("critical", "Critique", 4, "di_priority")
    },
];

const DI_IMPACT_LEVELS: &[SeedValue] = &[
    seed("unknown", "Inconnu", 1, "di_impact_level"),
    seed("none", "Aucun", 2, "di_impact_level"),
    seed("minor", "Mineur", 3, "di_impact_level"),
    seed("major", "Majeur", 4, "di_impact_level"),
    seed("critical", "Critique", 5, "di_impact_level"),
];

const DI_REQUEST_TYPES: &[SeedValue] = &[
    SeedValue {
        description: Some("Intervention corrective / réparation."),
        ..seed("repair", "Réparation", 1, "di_request_type")
    },
    SeedValue {
        description: Some("Maintenance préventive planifiée."),
        ..seed("preventive", "Préventif", 2, "di_request_type")
    },
    SeedValue {
        description: Some("Contrôle / inspection."),
        ..seed("inspection", "Inspection", 3, "di_request_type")
    },
    SeedValue {
        description: Some("Mise en place / installation."),
        ..seed("installation", "Installation", 4, "di_request_type")
    },
    SeedValue {
        description: Some("Étalonnage / calibrage."),
        ..seed("calibration", "Calibrage", 5, "di_request_type")
    },
    SeedValue {
        description: Some("Amélioration / modification."),
        ..seed("improvement", "Amélioration", 6, "di_request_type")
    },
    SeedValue {
        description: Some("Information seule — observation."),
        ..seed("observation", "Observation", 7, "di_request_type")
    },
    SeedValue {
        description: Some("Autre type de demande."),
        ..seed("other", "Autre", 8, "di_request_type")
    },
];

async fn ensure_di_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "DI.SYMPTOM",
        "Symptômes DI",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_domain(
        db,
        "DI.ORIGIN",
        "Origines DI",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_domain(
        db,
        "DI.PRIORITY",
        "Priorités DI",
        "flat",
        "protected_analytical",
        0,
        DomainAudience::Company,
    )
    .await?;
    ensure_domain(
        db,
        "DI.IMPACT_LEVEL",
        "Niveaux d'impact DI",
        "flat",
        "protected_analytical",
        0,
        DomainAudience::Company,
    )
    .await?;
    ensure_domain(
        db,
        "DI.REQUEST_TYPE",
        "Types de demande DI",
        "flat",
        "protected_analytical",
        0,
        DomainAudience::Company,
    )
    .await?;

    for code in [
        "DI.SYMPTOM",
        "DI.ORIGIN",
        "DI.PRIORITY",
        "DI.IMPACT_LEVEL",
        "DI.REQUEST_TYPE",
    ] {
        ensure_published_set(db, code).await?;
    }

    insert_seed_values(db, "DI.SYMPTOM", DI_SYMPTOMS).await?;
    insert_seed_values(db, "DI.ORIGIN", DI_ORIGINS).await?;
    insert_seed_values(db, "DI.PRIORITY", DI_PRIORITIES).await?;
    insert_seed_values(db, "DI.IMPACT_LEVEL", DI_IMPACT_LEVELS).await?;
    insert_seed_values(db, "DI.REQUEST_TYPE", DI_REQUEST_TYPES).await?;

    Ok(())
}

const PERSONNEL_SKILLS: &[SeedValue] = &[
    SeedValue {
        metadata_json: None,
        ..seed("MECH_GENERAL", "General Mechanics", 1, "personnel_skill")
    },
    SeedValue {
        metadata_json: None,
        ..seed("ELEC_INDUSTRIAL", "Industrial Electrical", 2, "personnel_skill")
    },
    SeedValue {
        metadata_json: None,
        ..seed("INSTRUMENTATION", "Instrumentation", 3, "personnel_skill")
    },
    SeedValue {
        metadata_json: None,
        ..seed("WELDING", "Welding", 4, "personnel_skill")
    },
    SeedValue {
        metadata_json: None,
        ..seed("HYDRAULICS", "Hydraulics", 5, "personnel_skill")
    },
    SeedValue {
        metadata_json: None,
        ..seed("CONDITION_MONITOR", "Condition Monitoring", 6, "personnel_skill")
    },
];

async fn ensure_personnel_skills_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "PERSONNEL.SKILLS",
        "Technical Skills",
        "hierarchical",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, "PERSONNEL.SKILLS").await?;
    insert_seed_values(db, "PERSONNEL.SKILLS", PERSONNEL_SKILLS).await?;
    Ok(())
}

/// Baseline catalog independent of `failure_codes` (empty after tenant wipe; migrations do not
/// re-run). Aligns with legacy lookup `failure.mode` codes.
const WORK_FAILURE_MODES: &[SeedValue] = &[
    SeedValue {
        description: Some("Vibration"),
        ..seed("VIBRATION", "Vibration", 1, "mode")
    },
    SeedValue {
        description: Some("Corrosion"),
        ..seed("CORROSION", "Corrosion", 2, "mode")
    },
    SeedValue {
        description: Some("Abnormal noise"),
        ..seed("BRUIT", "Bruit anormal", 3, "mode")
    },
    SeedValue {
        description: Some("Leak"),
        ..seed("FUITE", "Fuite", 4, "mode")
    },
    SeedValue {
        description: Some("Overheating"),
        ..seed("SURCHAUFFE", "Surchauffe", 5, "mode")
    },
    SeedValue {
        description: Some("Electrical fault"),
        ..seed("PANNE_ELEC", "Panne électrique", 6, "mode")
    },
    SeedValue {
        description: Some("Other"),
        ..seed("AUTRE", "Autre", 99, "mode")
    },
];

async fn ensure_work_failure_modes_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "WORK.FAILURE_MODES",
        "Failure modes",
        "hierarchical",
        "system_seeded",
        0,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, "WORK.FAILURE_MODES").await?;
    insert_seed_values(db, "WORK.FAILURE_MODES", WORK_FAILURE_MODES).await?;

    // Additive merge from legacy `failure_codes` when present (pre-wipe / migration path).
    // Every replica derives these rows from its own seeded `failure_codes`, so they get seeded ids.
    let legacy_modes = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT fc.code, fc.label, fc.iso_14224_annex_ref, fc.is_active, \
                    ROW_NUMBER() OVER (ORDER BY fc.code ASC) AS sort_order \
               FROM failure_codes fc \
              WHERE fc.code_type = 'mode' \
              ORDER BY fc.code ASC"
                .to_string(),
        ))
        .await?;
    for mode in legacy_modes {
        let code: String = mode.try_get("", "code")?;
        let label: String = mode.try_get("", "label")?;
        let annex_ref: Option<String> = mode.try_get("", "iso_14224_annex_ref")?;
        let is_active: i64 = mode.try_get("", "is_active")?;
        let sort_order: i64 = mode.try_get("", "sort_order")?;
        let entity_sync_id = seeded_value_sync_id("WORK.FAILURE_MODES", &code);
        db.execute(Statement::from_sql_and_values(
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
              entity_sync_id, set_id, parent_id, code, label, description, color_hex, sort_order, is_active, metadata_json
            )
            SELECT ?, ls.set_id, NULL, ?, ?, ?, NULL, ?, ?, NULL
            FROM latest_set ls
            WHERE NOT EXISTS (
                SELECT 1
                FROM reference_values rv
                WHERE rv.set_id = ls.set_id
                  AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?))
              )
              AND NOT EXISTS (
                SELECT 1 FROM reference_values rv WHERE rv.entity_sync_id = ?
              )
            "#,
            [
                entity_sync_id.clone().into(),
                code.clone().into(),
                label.into(),
                annex_ref.into(),
                sort_order.into(),
                is_active.into(),
                code.into(),
                entity_sync_id.into(),
            ],
        ))
        .await?;
    }

    Ok(())
}

const WORK_DELAY_REASONS: &[SeedValue] = &[
    seed("no_parts", "Awaiting Spare Parts", 1, "parts"),
    seed("backordered", "Parts Backordered", 2, "parts"),
    seed("no_permit", "Permit Not Issued", 3, "permit"),
    seed("permit_expired", "Permit Expired / Revoked", 4, "permit"),
    seed("no_shutdown", "Shutdown Window Unavailable", 5, "shutdown"),
    seed("vendor_delay", "Vendor / Contractor Delay", 6, "vendor"),
    seed("no_labor", "Insufficient Labor", 7, "labor"),
    seed("no_access", "Access to Equipment Denied", 8, "access"),
    seed("diagnosis", "Awaiting Diagnosis Result", 9, "diagnosis"),
    seed("other", "Other (see notes)", 10, "other"),
];

async fn ensure_work_delay_reasons_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "WORK.DELAY_REASONS",
        "Delay reasons",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, "WORK.DELAY_REASONS").await?;
    insert_seed_values(db, "WORK.DELAY_REASONS", WORK_DELAY_REASONS).await?;
    Ok(())
}

const WORK_PART_UNUSED_REASONS: &[SeedValue] = &[
    seed("inspection_ok", "Inspection OK", 1, "part_unused_reason"),
    seed("wrong_diagnosis", "Wrong diagnosis", 2, "part_unused_reason"),
    seed("part_unavailable", "Part unavailable", 3, "part_unused_reason"),
    seed("already_replaced", "Already replaced", 4, "part_unused_reason"),
    seed("other", "Other", 5, "part_unused_reason"),
];

async fn ensure_work_part_unused_reason_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "WORK.PART_UNUSED_REASON",
        "Part unused reasons",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, "WORK.PART_UNUSED_REASON").await?;
    insert_seed_values(db, "WORK.PART_UNUSED_REASON", WORK_PART_UNUSED_REASONS).await?;
    Ok(())
}

const ORG_SCHEDULE_CLASSES: &[SeedValue] = &[SeedValue {
    metadata_json: Some(r#"{"shift_pattern_code":"DAY_SHIFT","is_continuous":false}"#),
    ..seed("DAY_SHIFT", "Journée normale", 1, "schedule_class")
}];

async fn ensure_org_schedule_class_reference_catalog(db: &DatabaseConnection) -> AppResult<()> {
    ensure_domain(
        db,
        "ORG.SCHEDULE_CLASS",
        "Classes horaires",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, "ORG.SCHEDULE_CLASS").await?;
    insert_seed_values(db, "ORG.SCHEDULE_CLASS", ORG_SCHEDULE_CLASSES).await?;

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
