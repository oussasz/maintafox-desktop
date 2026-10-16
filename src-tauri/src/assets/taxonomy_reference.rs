//! Equipment taxonomy (`EQUIPMENT.CLASS`, `EQUIPMENT.CRITICALITY`, `EQUIPMENT.STATUS`, …) —
//! published reference sets + idempotent integrity repair (same idea as `wo::statuses`).

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

pub const DOMAIN_CLASS: &str = "EQUIPMENT.CLASS";
pub const DOMAIN_CRITICALITY: &str = "EQUIPMENT.CRITICALITY";
pub const DOMAIN_STATUS: &str = "EQUIPMENT.STATUS";
pub const DOMAIN_FAMILY: &str = "EQUIPMENT.FAMILY";
pub const DOMAIN_SUBFAMILY: &str = "EQUIPMENT.SUBFAMILY";

const SYSTEM_META: &str = r#"{"origin":"system"}"#;

/// Availability state a status puts the equipment in (`reference_values.semantic_tag` of the
/// system `EQUIPMENT.STATUS` rows, EN 13306 § 6): in service (`up`), unable to perform its function
/// (`down`: out of service or under maintenance), not installed (stock, spare) or retired. Readers
/// (home page "critical assets down") test the tag, never a status code.
pub const STATUS_TAG_UP: &str = "up";
pub const STATUS_TAG_DOWN: &str = "down";
pub const STATUS_TAG_NOT_INSTALLED: &str = "not_installed";
pub const STATUS_TAG_RETIRED: &str = "retired";

/// Canonical rows: (code, label, sort_order, color_hex, availability tag)
const REQUIRED_STATUS: &[(&str, &str, i64, &str, &str)] = &[
    ("ACTIVE_IN_SERVICE", "En service", 1, "#198754", STATUS_TAG_UP),
    ("IN_STOCK", "En stock", 2, "#0dcaf0", STATUS_TAG_NOT_INSTALLED),
    ("OUT_OF_SERVICE", "Hors service", 3, "#6c757d", STATUS_TAG_DOWN),
    ("UNDER_MAINTENANCE", "En maintenance", 4, "#ffc107", STATUS_TAG_DOWN),
    ("DECOMMISSIONED", "Mis hors service", 5, "#6c757d", STATUS_TAG_RETIRED),
    ("SCRAPPED", "Mis au rebut", 6, "#dc3545", STATUS_TAG_RETIRED),
    ("SPARE", "Pièce de rechange", 7, "#6c757d", STATUS_TAG_NOT_INSTALLED),
];

/// Active codes of a domain's published set (sub-select, then `$tail`: a filter or an ordering).
macro_rules! published_codes_sql {
    ($domain:literal, $tail:literal) => {
        concat!(
            "SELECT rv.code FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE d.code = '",
            $domain,
            "' AND rv.is_active = 1",
            $tail
        )
    };
}

/// SQL predicate (alias `e`, no bind): the equipment's status carries availability tag `$tag` in
/// the published `EQUIPMENT.STATUS` list. Never NULL: an equipment without a status is false.
macro_rules! status_tag_sql {
    ($tag:literal) => {
        concat!(
            "COALESCE((SELECT st.code FROM reference_values st WHERE st.id = e.equipment_status_ref_id), '') IN (",
            published_codes_sql!("EQUIPMENT.STATUS", " AND rv.semantic_tag = '"),
            $tag,
            "')"
        )
    };
}

/// SQL predicate (alias `e`, no bind): the equipment's status is `up` (in service).
pub const STATUS_UP_SQL: &str = status_tag_sql!("up");
/// SQL predicate (alias `e`, no bind): the equipment's status is `down` (out of service or under
/// maintenance — unable to perform its function).
pub const STATUS_DOWN_SQL: &str = status_tag_sql!("down");
/// SQL predicate (alias `e`, no bind): the equipment is retired (decommissioned, scrapped).
pub const STATUS_RETIRED_SQL: &str = status_tag_sql!("retired");

/// SQL predicate (alias `e`, no bind): the equipment is at the most critical level of the published
/// `EQUIPMENT.CRITICALITY` list (lowest `sort_order`). Never NULL.
macro_rules! most_critical_sql {
    () => {
        concat!(
            "COALESCE((SELECT c.code FROM reference_values c WHERE c.id = e.equipment_criticality_ref_id), '') \
             = COALESCE((",
            published_codes_sql!(
                "EQUIPMENT.CRITICALITY",
                " ORDER BY rv.sort_order ASC, rv.id ASC LIMIT 1"
            ),
            "), '-')"
        )
    };
}

/// SQL predicate (alias `e`, no bind): the equipment is at the most critical level of the published
/// `EQUIPMENT.CRITICALITY` list (lowest `sort_order`).
pub const MOST_CRITICAL_SQL: &str = most_critical_sql!();

/// SQL predicate (alias `e`, no bind): equipment at the most critical level of the published
/// `EQUIPMENT.CRITICALITY` list (lowest `sort_order`) whose status carries the `down`
/// availability tag in the published `EQUIPMENT.STATUS` list. Both are read from Données de
/// référence by code, so a republished set keeps working; no criticality or status code is
/// written here.
pub const CRITICAL_DOWN_SQL: &str = concat!(
    "e.deleted_at IS NULL AND ",
    most_critical_sql!(),
    " AND ",
    status_tag_sql!("down")
);

const REQUIRED_CRITICALITY: &[(&str, &str, i64, &str)] = &[
    ("A", "Criticité A", 1, "#dc3545"),
    ("B", "Criticité B", 2, "#ffc107"),
    ("C", "Criticité C", 3, "#0dcaf0"),
    ("D", "Criticité D", 4, "#198754"),
];

const REQUIRED_CLASS: &[(&str, &str, i64)] = &[
    ("PUMP", "Pompe", 1),
    ("MOTOR", "Moteur", 2),
    ("VALVE", "Vanne", 3),
    ("HEAT_EXCHANGER", "Échangeur de chaleur", 4),
    ("COMPRESSOR", "Compresseur", 5),
    ("CONVEYOR", "Convoyeur", 6),
    ("INSTRUMENTATION", "Instrumentation", 7),
    ("VESSEL", "Réservoir / Appareil sous pression", 8),
];

/// Canonical family rows linked to class values via `parent_id`.
const REQUIRED_FAMILY: &[(&str, &str, i64, &str)] = &[
    ("PUMP_FAMILY", "Famille Pompe", 1, "PUMP"),
    ("MOTOR_FAMILY", "Famille Moteur", 2, "MOTOR"),
    ("VALVE_FAMILY", "Famille Vanne", 3, "VALVE"),
    (
        "HEAT_EXCHANGER_FAMILY",
        "Famille Échangeur de chaleur",
        4,
        "HEAT_EXCHANGER",
    ),
    ("COMPRESSOR_FAMILY", "Famille Compresseur", 5, "COMPRESSOR"),
    ("CONVEYOR_FAMILY", "Famille Convoyeur", 6, "CONVEYOR"),
    (
        "INSTRUMENTATION_FAMILY",
        "Famille Instrumentation",
        7,
        "INSTRUMENTATION",
    ),
    (
        "VESSEL_FAMILY",
        "Famille Réservoir / Appareil sous pression",
        8,
        "VESSEL",
    ),
];

/// Canonical subfamily rows linked to family values via `parent_id`.
const REQUIRED_SUBFAMILY: &[(&str, &str, i64, &str)] = &[
    ("PUMP_STANDARD", "Pompe standard", 1, "PUMP_FAMILY"),
    ("MOTOR_STANDARD", "Moteur standard", 2, "MOTOR_FAMILY"),
    ("VALVE_STANDARD", "Vanne standard", 3, "VALVE_FAMILY"),
    (
        "HEAT_EXCHANGER_STANDARD",
        "Échangeur standard",
        4,
        "HEAT_EXCHANGER_FAMILY",
    ),
    ("COMPRESSOR_STANDARD", "Compresseur standard", 5, "COMPRESSOR_FAMILY"),
    ("CONVEYOR_STANDARD", "Convoyeur standard", 6, "CONVEYOR_FAMILY"),
    (
        "INSTRUMENTATION_STANDARD",
        "Instrumentation standard",
        7,
        "INSTRUMENTATION_FAMILY",
    ),
    ("VESSEL_STANDARD", "Réservoir standard", 8, "VESSEL_FAMILY"),
];

#[derive(Debug, Clone, Serialize)]
pub struct EquipmentTaxonomyOption {
    pub id: i64,
    pub code: String,
    pub label: String,
    pub parent_id: Option<i64>,
    pub color_hex: Option<String>,
    pub is_system: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquipmentTaxonomyCatalog {
    pub statuses: Vec<EquipmentTaxonomyOption>,
    pub criticalities: Vec<EquipmentTaxonomyOption>,
    pub classes: Vec<EquipmentTaxonomyOption>,
    pub families: Vec<EquipmentTaxonomyOption>,
    pub subfamilies: Vec<EquipmentTaxonomyOption>,
}

/// Every replica seeds these system rows itself, so their sync identity is derived from the
/// `domain:code` natural key and agrees across machines.
fn seeded_reference_value_sync_id(domain_code: &str, value_code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain_code}:{value_code}"))
}

async fn published_set_id(db: &DatabaseConnection, domain_code: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rs.id AS id FROM reference_sets rs \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' \
             ORDER BY rs.version_no DESC LIMIT 1",
            [domain_code.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "published reference set missing for domain {domain_code}"
            ))
        })?;
    row.try_get::<i64>("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("published_set_id: {e}")))
}

fn metadata_implies_system(metadata_json: Option<String>) -> bool {
    metadata_json
        .as_ref()
        .is_some_and(|s| s.contains("\"origin\":\"system\""))
}

fn map_option_row(row: &sea_orm::QueryResult) -> AppResult<EquipmentTaxonomyOption> {
    let id: i64 = row
        .try_get("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("taxonomy option id: {e}")))?;
    let code: String = row
        .try_get("", "code")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("taxonomy option code: {e}")))?;
    let label: String = row
        .try_get("", "label")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("taxonomy option label: {e}")))?;
    let parent_id: Option<i64> = row
        .try_get("", "parent_id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("taxonomy option parent_id: {e}")))?;
    let color_hex: Option<String> = row
        .try_get("", "color_hex")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("taxonomy option color_hex: {e}")))?;
    let metadata_json: Option<String> = row
        .try_get("", "metadata_json")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("taxonomy option metadata_json: {e}")))?;
    Ok(EquipmentTaxonomyOption {
        id,
        code,
        label,
        parent_id,
        color_hex,
        is_system: metadata_implies_system(metadata_json),
    })
}

async fn list_domain_options(db: &DatabaseConnection, domain_code: &str) -> AppResult<Vec<EquipmentTaxonomyOption>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id, rv.code, rv.label, rv.parent_id, rv.color_hex, rv.metadata_json \
             FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' AND rv.is_active = 1 \
             ORDER BY rv.sort_order ASC, rv.code ASC",
            [domain_code.into()],
        ))
        .await?;
    rows.iter().map(map_option_row).collect()
}

/// Idempotent upsert of system status rows in the published `EQUIPMENT.STATUS` set.
pub async fn ensure_required_equipment_status_values(db: &DatabaseConnection) -> AppResult<()> {
    let set_id = published_set_id(db, DOMAIN_STATUS).await?;
    for &(code, label, sort, color, tag) in REQUIRED_STATUS {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, ?, NULL, ?, ?, NULL, ?, ?, NULL, ?, NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                color_hex = excluded.color_hex, \
                semantic_tag = excluded.semantic_tag, \
                metadata_json = excluded.metadata_json",
            [
                seeded_reference_value_sync_id(DOMAIN_STATUS, code).into(),
                set_id.into(),
                code.into(),
                label.into(),
                sort.into(),
                color.into(),
                tag.into(),
                SYSTEM_META.into(),
            ],
        ))
        .await?;
    }
    let codes: Vec<&str> = REQUIRED_STATUS.iter().map(|(c, _, _, _, _)| *c).collect();
    verify_required_codes(db, DOMAIN_STATUS, &codes).await?;
    Ok(())
}

/// Idempotent upsert of A–D criticality rows in the published `EQUIPMENT.CRITICALITY` set.
pub async fn ensure_required_equipment_criticality_values(db: &DatabaseConnection) -> AppResult<()> {
    let set_id = published_set_id(db, DOMAIN_CRITICALITY).await?;
    for &(code, label, sort, color) in REQUIRED_CRITICALITY {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, ?, NULL, ?, ?, NULL, ?, ?, NULL, NULL, NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                color_hex = excluded.color_hex, \
                metadata_json = excluded.metadata_json",
            [
                seeded_reference_value_sync_id(DOMAIN_CRITICALITY, code).into(),
                set_id.into(),
                code.into(),
                label.into(),
                sort.into(),
                color.into(),
                SYSTEM_META.into(),
            ],
        ))
        .await?;
    }
    let codes: Vec<&str> = REQUIRED_CRITICALITY.iter().map(|(c, _, _, _)| *c).collect();
    verify_required_codes(db, DOMAIN_CRITICALITY, &codes).await?;
    Ok(())
}

/// Idempotent upsert of industrial class codes in the published `EQUIPMENT.CLASS` set.
pub async fn ensure_required_equipment_class_values(db: &DatabaseConnection) -> AppResult<()> {
    let set_id = published_set_id(db, DOMAIN_CLASS).await?;
    for &(code, label, sort) in REQUIRED_CLASS {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, ?, NULL, ?, ?, NULL, ?, NULL, NULL, 'equipment_class', NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                metadata_json = excluded.metadata_json",
            [
                seeded_reference_value_sync_id(DOMAIN_CLASS, code).into(),
                set_id.into(),
                code.into(),
                label.into(),
                sort.into(),
                SYSTEM_META.into(),
            ],
        ))
        .await?;
    }
    let codes: Vec<&str> = REQUIRED_CLASS.iter().map(|(c, _, _)| *c).collect();
    verify_required_codes(db, DOMAIN_CLASS, &codes).await?;
    Ok(())
}

async fn resolve_published_value_id(db: &DatabaseConnection, domain_code: &str, value_code: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.id FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' \
               AND rv.code = ? AND rv.is_active = 1 LIMIT 1",
            [domain_code.into(), value_code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("missing published value {domain_code}.{value_code}")))?;
    row.try_get::<i64>("", "id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("resolve published value id: {e}")))
}

/// Idempotent upsert of family rows in published `EQUIPMENT.FAMILY`, linked to class parents.
pub async fn ensure_required_equipment_family_values(db: &DatabaseConnection) -> AppResult<()> {
    let set_id = published_set_id(db, DOMAIN_FAMILY).await?;
    for &(code, label, sort, class_code) in REQUIRED_FAMILY {
        let class_parent_id = resolve_published_value_id(db, DOMAIN_CLASS, class_code).await?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, ?, ?, ?, ?, NULL, ?, NULL, NULL, 'equipment_family', NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                parent_id = excluded.parent_id, \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                metadata_json = excluded.metadata_json, \
                is_active = 1",
            [
                seeded_reference_value_sync_id(DOMAIN_FAMILY, code).into(),
                set_id.into(),
                class_parent_id.into(),
                code.into(),
                label.into(),
                sort.into(),
                SYSTEM_META.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// Idempotent upsert of subfamily rows in published `EQUIPMENT.SUBFAMILY`, linked to family parents.
pub async fn ensure_required_equipment_subfamily_values(db: &DatabaseConnection) -> AppResult<()> {
    let set_id = published_set_id(db, DOMAIN_SUBFAMILY).await?;
    for &(code, label, sort, family_code) in REQUIRED_SUBFAMILY {
        let family_parent_id = resolve_published_value_id(db, DOMAIN_FAMILY, family_code).await?;
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, ?, ?, ?, ?, NULL, ?, NULL, NULL, 'equipment_subfamily', NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                parent_id = excluded.parent_id, \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                metadata_json = excluded.metadata_json, \
                is_active = 1",
            [
                seeded_reference_value_sync_id(DOMAIN_SUBFAMILY, code).into(),
                set_id.into(),
                family_parent_id.into(),
                code.into(),
                label.into(),
                sort.into(),
                SYSTEM_META.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

async fn verify_required_codes(db: &DatabaseConnection, domain_code: &str, required: &[&str]) -> AppResult<()> {
    let mut missing: Vec<String> = Vec::new();
    for code in required {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT rv.id FROM reference_values rv \
                 INNER JOIN reference_sets rs ON rs.id = rv.set_id \
                 INNER JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE d.code = ? AND rs.status = 'published' AND rv.code = ? LIMIT 1",
                [domain_code.into(), (*code).into()],
            ))
            .await?;
        if row.is_none() {
            missing.push((*code).to_string());
        }
    }
    if !missing.is_empty() {
        return Err(AppError::Internal(anyhow::anyhow!(
            "reference integrity failed for {domain_code}; missing codes: {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

/// Keeps `equipment_classes` aligned with published `EQUIPMENT.CLASS` codes.
///
/// Asset create/update resolves `class_code` against `equipment_classes` (legacy FK `class_id`).
/// Reference Manager seeds canonical codes such as `PUMP`, while tenant bootstrap may only insert
/// unrelated rows (e.g. `GEN-CLASS-PUMP`). Without this sync, choosing "PUMP" from the catalog fails
/// validation even though the reference value exists.
pub async fn sync_equipment_classes_from_published_reference(db: &DatabaseConnection) -> AppResult<()> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT rv.code, rv.label \
             FROM reference_values rv \
             INNER JOIN reference_sets rs ON rs.id = rv.set_id \
             INNER JOIN reference_domains d ON d.id = rs.domain_id \
             WHERE d.code = ? AND rs.status = 'published' AND rv.is_active = 1 \
             ORDER BY rv.sort_order ASC, rv.code ASC",
            [DOMAIN_CLASS.into()],
        ))
        .await?;

    let now = Utc::now().to_rfc3339();

    for row in rows {
        let code: String = row
            .try_get("", "code")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("sync equipment_classes: read code: {e}")))?;
        let label: String = row.try_get("", "label").unwrap_or_default();
        let name = {
            let t = label.trim();
            if t.is_empty() {
                code.clone()
            } else {
                t.to_string()
            }
        };

        let existing = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM equipment_classes WHERE code = ?",
                [code.clone().into()],
            ))
            .await?;

        if existing.is_some() {
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE equipment_classes \
                 SET name = ?, is_active = 1, deleted_at = NULL, updated_at = ? \
                 WHERE code = ?",
                [name.into(), now.clone().into(), code.into()],
            ))
            .await?;
        } else {
            // Derived from the published catalog on every device: identity from the class code
            // (`sync::seeded_identity`), so all devices agree.
            let entity_sync_id = crate::sync::identity::seeded_entity_sync_id("equipment_classes", &code);
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO equipment_classes \
                 (entity_sync_id, code, name, parent_id, level, is_active, created_at, updated_at) \
                 VALUES (?, ?, ?, NULL, 'class', 1, ?, ?)",
                [
                    entity_sync_id.into(),
                    code.into(),
                    name.into(),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;
        }
    }

    Ok(())
}

/// Idempotent recreate of EQUIPMENT.* domains + published v1 sets when missing
/// (same substrate as migration 103). Required when migration history is present
/// but catalog rows were wiped.
async fn ensure_equipment_taxonomy_domains_and_published_sets(db: &DatabaseConnection) -> AppResult<()> {
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_domains \
         (code, name, structure_type, governance_level, governance_category, audience, \
          is_extendable, validation_rules_json, created_at, updated_at) \
         VALUES \
         ('EQUIPMENT.CLASS', 'Classes d''équipement', 'flat', 'system_seeded', 'system_catalog', 'company', 0, NULL, \
          strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
         ('EQUIPMENT.CRITICALITY', 'Criticités d''équipement', 'flat', 'system_seeded', 'system_catalog', 'system', 0, NULL, \
          strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
         ('EQUIPMENT.STATUS', 'Statuts d''équipement', 'flat', 'system_seeded', 'system_catalog', 'system', 0, NULL, \
          strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
         ('EQUIPMENT.FAMILY', 'Familles d''équipement', 'flat', 'tenant_managed', 'operational_dictionary', 'company', 1, NULL, \
          strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')), \
         ('EQUIPMENT.SUBFAMILY', 'Sous-familles d''équipement', 'flat', 'tenant_managed', 'operational_dictionary', 'company', 1, NULL, \
          strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))"
            .to_string(),
    ))
    .await?;

    // Audience is decided by the product: the criticality scale and the status codes (they drive
    // decommissioning) are Maintafox lists; classes and families are the plant's own.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE reference_domains \
            SET audience = CASE WHEN UPPER(TRIM(code)) IN ('EQUIPMENT.CRITICALITY','EQUIPMENT.STATUS') \
                                THEN 'system' ELSE 'company' END \
          WHERE UPPER(TRIM(code)) IN ('EQUIPMENT.CLASS','EQUIPMENT.CRITICALITY','EQUIPMENT.STATUS', \
                                      'EQUIPMENT.FAMILY','EQUIPMENT.SUBFAMILY')"
            .to_string(),
    ))
    .await?;

    // Align category for rows inserted before governance_category existed.
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET governance_category = 'system_catalog' \
         WHERE UPPER(TRIM(code)) IN ('EQUIPMENT.CLASS','EQUIPMENT.CRITICALITY','EQUIPMENT.STATUS')"
            .to_string(),
    ))
    .await?;
    db.execute(Statement::from_string(
        DbBackend::Sqlite,
        "UPDATE reference_domains SET governance_category = 'operational_dictionary' \
         WHERE UPPER(TRIM(code)) IN ('EQUIPMENT.FAMILY','EQUIPMENT.SUBFAMILY')"
            .to_string(),
    ))
    .await?;

    for code in [
        DOMAIN_CLASS,
        DOMAIN_CRITICALITY,
        DOMAIN_STATUS,
        DOMAIN_FAMILY,
        DOMAIN_SUBFAMILY,
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

    Ok(())
}

/// Startup hook: repair drift for system-locked equipment domains.
pub async fn ensure_equipment_taxonomy_reference_integrity(db: &DatabaseConnection) -> AppResult<()> {
    ensure_equipment_taxonomy_domains_and_published_sets(db).await?;
    ensure_required_equipment_status_values(db).await?;
    ensure_required_equipment_criticality_values(db).await?;
    ensure_required_equipment_class_values(db).await?;
    ensure_required_equipment_family_values(db).await?;
    ensure_required_equipment_subfamily_values(db).await?;
    sync_equipment_classes_from_published_reference(db).await?;
    Ok(())
}

/// Published values for asset forms and filters (includes user-extendable family/subfamily).
pub async fn list_equipment_taxonomy_catalog(db: &DatabaseConnection) -> AppResult<EquipmentTaxonomyCatalog> {
    ensure_equipment_taxonomy_reference_integrity(db).await?;
    let statuses = list_domain_options(db, DOMAIN_STATUS).await?;
    let criticalities = list_domain_options(db, DOMAIN_CRITICALITY).await?;
    let classes = list_domain_options(db, DOMAIN_CLASS).await?;
    let families = list_domain_options(db, DOMAIN_FAMILY).await?;
    let subfamilies = list_domain_options(db, DOMAIN_SUBFAMILY).await?;
    Ok(EquipmentTaxonomyCatalog {
        statuses,
        criticalities,
        classes,
        families,
        subfamilies,
    })
}
