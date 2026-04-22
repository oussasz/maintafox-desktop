//! Equipment taxonomy (`EQUIPMENT.CLASS`, `EQUIPMENT.CRITICALITY`, `EQUIPMENT.STATUS`, …) —
//! published reference sets + idempotent integrity repair (same idea as `wo::statuses`).

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;
use uuid::Uuid;

pub const DOMAIN_CLASS: &str = "EQUIPMENT.CLASS";
pub const DOMAIN_CRITICALITY: &str = "EQUIPMENT.CRITICALITY";
pub const DOMAIN_STATUS: &str = "EQUIPMENT.STATUS";
pub const DOMAIN_FAMILY: &str = "EQUIPMENT.FAMILY";
pub const DOMAIN_SUBFAMILY: &str = "EQUIPMENT.SUBFAMILY";

const SYSTEM_META: &str = r#"{"origin":"system"}"#;

/// Canonical rows: (code, label, sort_order, color_hex)
const REQUIRED_STATUS: &[(&str, &str, i64, &str)] = &[
    ("ACTIVE_IN_SERVICE", "En service", 1, "#198754"),
    ("IN_STOCK", "En stock", 2, "#0dcaf0"),
    ("OUT_OF_SERVICE", "Hors service", 3, "#6c757d"),
    ("UNDER_MAINTENANCE", "En maintenance", 4, "#ffc107"),
    ("DECOMMISSIONED", "Mis hors service", 5, "#6c757d"),
    ("SCRAPPED", "Mis au rebut", 6, "#dc3545"),
    ("SPARE", "Pièce de rechange", 7, "#6c757d"),
];

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
    ("HEAT_EXCHANGER_FAMILY", "Famille Échangeur de chaleur", 4, "HEAT_EXCHANGER"),
    ("COMPRESSOR_FAMILY", "Famille Compresseur", 5, "COMPRESSOR"),
    ("CONVEYOR_FAMILY", "Famille Convoyeur", 6, "CONVEYOR"),
    ("INSTRUMENTATION_FAMILY", "Famille Instrumentation", 7, "INSTRUMENTATION"),
    ("VESSEL_FAMILY", "Famille Réservoir / Appareil sous pression", 8, "VESSEL"),
];

/// Canonical subfamily rows linked to family values via `parent_id`.
const REQUIRED_SUBFAMILY: &[(&str, &str, i64, &str)] = &[
    ("PUMP_STANDARD", "Pompe standard", 1, "PUMP_FAMILY"),
    ("MOTOR_STANDARD", "Moteur standard", 2, "MOTOR_FAMILY"),
    ("VALVE_STANDARD", "Vanne standard", 3, "VALVE_FAMILY"),
    ("HEAT_EXCHANGER_STANDARD", "Échangeur standard", 4, "HEAT_EXCHANGER_FAMILY"),
    ("COMPRESSOR_STANDARD", "Compresseur standard", 5, "COMPRESSOR_FAMILY"),
    ("CONVEYOR_STANDARD", "Convoyeur standard", 6, "CONVEYOR_FAMILY"),
    ("INSTRUMENTATION_STANDARD", "Instrumentation standard", 7, "INSTRUMENTATION_FAMILY"),
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
    metadata_json.as_ref().map_or(false, |s| s.contains("\"origin\":\"system\""))
}

fn map_option_row(
    row: &sea_orm::QueryResult,
) -> AppResult<EquipmentTaxonomyOption> {
    let id: i64 = row.try_get("", "id").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("taxonomy option id: {e}"))
    })?;
    let code: String = row.try_get("", "code").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("taxonomy option code: {e}"))
    })?;
    let label: String = row.try_get("", "label").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("taxonomy option label: {e}"))
    })?;
    let parent_id: Option<i64> = row.try_get("", "parent_id").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("taxonomy option parent_id: {e}"))
    })?;
    let color_hex: Option<String> = row.try_get("", "color_hex").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("taxonomy option color_hex: {e}"))
    })?;
    let metadata_json: Option<String> = row.try_get("", "metadata_json").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("taxonomy option metadata_json: {e}"))
    })?;
    Ok(EquipmentTaxonomyOption {
        id,
        code,
        label,
        parent_id,
        color_hex,
        is_system: metadata_implies_system(metadata_json),
    })
}

async fn list_domain_options(
    db: &DatabaseConnection,
    domain_code: &str,
) -> AppResult<Vec<EquipmentTaxonomyOption>> {
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
    for &(code, label, sort, color) in REQUIRED_STATUS {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, NULL, ?, ?, NULL, ?, ?, NULL, NULL, NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                color_hex = excluded.color_hex, \
                metadata_json = excluded.metadata_json",
            [
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
    let codes: Vec<&str> = REQUIRED_STATUS.iter().map(|(c, _, _, _)| *c).collect();
    verify_required_codes(db, DOMAIN_STATUS, &codes).await?;
    Ok(())
}

/// Idempotent upsert of A–D criticality rows in the published `EQUIPMENT.CRITICALITY` set.
pub async fn ensure_required_equipment_criticality_values(
    db: &DatabaseConnection,
) -> AppResult<()> {
    let set_id = published_set_id(db, DOMAIN_CRITICALITY).await?;
    for &(code, label, sort, color) in REQUIRED_CRITICALITY {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, NULL, ?, ?, NULL, ?, ?, NULL, NULL, NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                color_hex = excluded.color_hex, \
                metadata_json = excluded.metadata_json",
            [
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
            "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, NULL, ?, ?, NULL, ?, NULL, NULL, 'equipment_class', NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                metadata_json = excluded.metadata_json",
            [
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

async fn resolve_published_value_id(
    db: &DatabaseConnection,
    domain_code: &str,
    value_code: &str,
) -> AppResult<i64> {
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
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "missing published value {domain_code}.{value_code}"
            ))
        })?;
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
            "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, ?, ?, ?, NULL, ?, NULL, NULL, 'equipment_family', NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                parent_id = excluded.parent_id, \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                metadata_json = excluded.metadata_json, \
                is_active = 1",
            [
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
            "INSERT INTO reference_values \
             (set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             VALUES (?, ?, ?, ?, NULL, ?, NULL, NULL, 'equipment_subfamily', NULL, 1, ?) \
             ON CONFLICT(set_id, code) DO UPDATE SET \
                parent_id = excluded.parent_id, \
                label = excluded.label, \
                sort_order = excluded.sort_order, \
                metadata_json = excluded.metadata_json, \
                is_active = 1",
            [
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

async fn verify_required_codes(
    db: &DatabaseConnection,
    domain_code: &str,
    required: &[&str],
) -> AppResult<()> {
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
        let code: String = row.try_get("", "code").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("sync equipment_classes: read code: {e}"))
        })?;
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
            let sync_id = Uuid::new_v4().to_string();
            db.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO equipment_classes \
                 (sync_id, code, name, parent_id, level, is_active, created_at, updated_at) \
                 VALUES (?, ?, ?, NULL, 'class', 1, ?, ?)",
                [
                    sync_id.into(),
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

/// Startup hook: repair drift for system-locked equipment domains.
pub async fn ensure_equipment_taxonomy_reference_integrity(db: &DatabaseConnection) -> AppResult<()> {
    ensure_required_equipment_status_values(db).await?;
    ensure_required_equipment_criticality_values(db).await?;
    ensure_required_equipment_class_values(db).await?;
    ensure_required_equipment_family_values(db).await?;
    ensure_required_equipment_subfamily_values(db).await?;
    sync_equipment_classes_from_published_reference(db).await?;
    Ok(())
}

/// Published values for asset forms and filters (includes user-extendable family/subfamily).
pub async fn list_equipment_taxonomy_catalog(
    db: &DatabaseConnection,
) -> AppResult<EquipmentTaxonomyCatalog> {
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
