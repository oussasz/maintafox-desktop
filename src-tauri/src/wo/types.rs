//! Work-order types and their maintenance nature (EN 13306).
//!
//! Two SYSTEM reference domains (`reference::audience`), maintained by Maintafox:
//!
//! - `WORK.MAINTENANCE_NATURES` — the protected nature of maintenance, after the EN 13306
//!   classification of maintenance types:
//!   - corrective maintenance, *immediate* (carried out without delay after a fault is detected)
//!     or *deferred* (delayed according to given maintenance rules);
//!   - preventive maintenance, *predetermined* (systematic, by time or usage), *condition-based*
//!     (on the monitored condition, including inspection) or *predictive* (forecast from the
//!     analysis of the degradation);
//!   - *improvement / modification* (actions that improve the intrinsic dependability or change
//!     the function of the item).
//! - `WORK.ORDER_TYPES` — the OT types a planner picks. Each type carries exactly one nature in
//!   `reference_values.semantic_tag` (the nature code); `metadata_json.nature_default` marks the
//!   type the program uses when it must create an OT of a nature (DI conversion, PM generation).
//!
//! Every rule of the program that depends on the kind of work (failure coding at close-out, RAMS
//! failure events, SMRP planned / reactive / emergency work, asset failure history, PM renewals in
//! reliability studies, spare-part demand source) tests the **nature**, never a type code, so a
//! type added later under an existing nature follows the same rules.
//!
//! Labels: `reference_values.label` is French, `metadata_json.label_en` English.
//!
//! Company sub-types (`WORK.ORDER_SUBTYPES`, owner decision 2026-10-15): a plant's own types, each
//! under one protected nature; an OT may carry one, the rules keep reading the nature.

use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const NATURES_DOMAIN: &str = "WORK.MAINTENANCE_NATURES";
pub const WORK_ORDER_TYPES_DOMAIN: &str = "WORK.ORDER_TYPES";

/// Protected maintenance nature (EN 13306).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceNature {
    CorrectiveImmediate,
    CorrectiveDeferred,
    PreventivePredetermined,
    PreventiveConditionBased,
    PreventivePredictive,
    Improvement,
}

impl MaintenanceNature {
    pub const ALL: [MaintenanceNature; 6] = [
        Self::CorrectiveImmediate,
        Self::CorrectiveDeferred,
        Self::PreventivePredetermined,
        Self::PreventiveConditionBased,
        Self::PreventivePredictive,
        Self::Improvement,
    ];

    /// Corrective natures: the work follows a failure or a fault.
    pub const CORRECTIVE: [MaintenanceNature; 2] = [Self::CorrectiveImmediate, Self::CorrectiveDeferred];

    /// Preventive natures.
    pub const PREVENTIVE: [MaintenanceNature; 3] = [
        Self::PreventivePredetermined,
        Self::PreventiveConditionBased,
        Self::PreventivePredictive,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CorrectiveImmediate => "corrective_immediate",
            Self::CorrectiveDeferred => "corrective_deferred",
            Self::PreventivePredetermined => "preventive_predetermined",
            Self::PreventiveConditionBased => "preventive_condition_based",
            Self::PreventivePredictive => "preventive_predictive",
            Self::Improvement => "improvement",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|n| n.as_str() == raw.trim())
    }

    /// The work follows a failure or a fault (failure coding and RAMS apply).
    pub const fn is_corrective(self) -> bool {
        matches!(self, Self::CorrectiveImmediate | Self::CorrectiveDeferred)
    }

    pub const fn is_preventive(self) -> bool {
        matches!(
            self,
            Self::PreventivePredetermined | Self::PreventiveConditionBased | Self::PreventivePredictive
        )
    }

    /// Planned before execution (SMRP "planned work"): preventive natures and improvement.
    pub const fn is_planned(self) -> bool {
        self.is_preventive() || matches!(self, Self::Improvement)
    }

    /// SMRP "emergency work": corrective work done without delay.
    pub const fn is_emergency(self) -> bool {
        matches!(self, Self::CorrectiveImmediate)
    }
}

/// SQL list literal of nature codes (`'a', 'b'`) for `semantic_tag IN (…)`. The codes are
/// compile-time constants, never user input.
pub fn nature_sql_list(natures: &[MaintenanceNature]) -> String {
    natures
        .iter()
        .map(|n| format!("'{}'", n.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

struct NatureSeed {
    nature: MaintenanceNature,
    label_fr: &'static str,
    label_en: &'static str,
    description: &'static str,
}

const NATURE_SEEDS: &[NatureSeed] = &[
    NatureSeed {
        nature: MaintenanceNature::CorrectiveImmediate,
        label_fr: "Corrective immédiate",
        label_en: "Immediate corrective",
        description: "Maintenance corrective exécutée sans délai après la détection d'une panne (EN 13306).",
    },
    NatureSeed {
        nature: MaintenanceNature::CorrectiveDeferred,
        label_fr: "Corrective différée",
        label_en: "Deferred corrective",
        description: "Maintenance corrective différée selon des règles de maintenance données (EN 13306).",
    },
    NatureSeed {
        nature: MaintenanceNature::PreventivePredetermined,
        label_fr: "Préventive systématique",
        label_en: "Predetermined preventive",
        description: "Maintenance préventive exécutée selon un échéancier établi en temps ou en usage (EN 13306).",
    },
    NatureSeed {
        nature: MaintenanceNature::PreventiveConditionBased,
        label_fr: "Préventive conditionnelle",
        label_en: "Condition-based",
        description:
            "Maintenance préventive déclenchée par l'évaluation de l'état du bien, inspection comprise (EN 13306).",
    },
    NatureSeed {
        nature: MaintenanceNature::PreventivePredictive,
        label_fr: "Préventive prévisionnelle",
        label_en: "Predictive",
        description:
            "Maintenance conditionnelle exécutée suivant une prévision tirée de l'analyse de la dégradation (EN 13306).",
    },
    NatureSeed {
        nature: MaintenanceNature::Improvement,
        label_fr: "Amélioration / modification",
        label_en: "Improvement / modification",
        description:
            "Actions qui améliorent la sûreté de fonctionnement du bien ou en modifient la fonction (EN 13306).",
    },
];

struct TypeSeed {
    code: &'static str,
    label_fr: &'static str,
    label_en: &'static str,
    nature: MaintenanceNature,
    /// Type used when the program creates an OT of this nature.
    nature_default: bool,
}

/// The OT types shipped by Maintafox. Codes before migration 198 are kept (`emergency`,
/// `corrective`, `preventive`, `overhaul`, `condition_based`, `inspection`, `improvement`);
/// `predictive` is new, so that every nature has a type.
const TYPE_SEEDS: &[TypeSeed] = &[
    TypeSeed {
        code: "emergency",
        label_fr: "Dépannage d'urgence",
        label_en: "Emergency repair",
        nature: MaintenanceNature::CorrectiveImmediate,
        nature_default: true,
    },
    TypeSeed {
        code: "corrective",
        label_fr: "Corrective",
        label_en: "Corrective",
        nature: MaintenanceNature::CorrectiveDeferred,
        nature_default: true,
    },
    TypeSeed {
        code: "preventive",
        label_fr: "Préventive systématique",
        label_en: "Preventive (time-based)",
        nature: MaintenanceNature::PreventivePredetermined,
        nature_default: true,
    },
    TypeSeed {
        code: "overhaul",
        label_fr: "Révision générale",
        label_en: "Overhaul",
        nature: MaintenanceNature::PreventivePredetermined,
        nature_default: false,
    },
    TypeSeed {
        code: "condition_based",
        label_fr: "Préventive conditionnelle",
        label_en: "Condition-based",
        nature: MaintenanceNature::PreventiveConditionBased,
        nature_default: true,
    },
    TypeSeed {
        code: "inspection",
        label_fr: "Inspection",
        label_en: "Inspection",
        nature: MaintenanceNature::PreventiveConditionBased,
        nature_default: false,
    },
    TypeSeed {
        code: "predictive",
        label_fr: "Préventive prévisionnelle",
        label_en: "Predictive",
        nature: MaintenanceNature::PreventivePredictive,
        nature_default: true,
    },
    TypeSeed {
        code: "improvement",
        label_fr: "Amélioration / modification",
        label_en: "Improvement / modification",
        nature: MaintenanceNature::Improvement,
        nature_default: true,
    },
];

/// Codes of the types Maintafox ships (for the migration of `work_order_types`).
pub fn shipped_type_codes() -> impl Iterator<Item = &'static str> {
    TYPE_SEEDS.iter().map(|t| t.code)
}

fn seeded_sync_id(domain: &str, code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain}:{code}"))
}

#[allow(clippy::too_many_arguments)]
async fn insert_value(
    conn: &impl ConnectionTrait,
    domain: &str,
    code: &str,
    label: &str,
    description: Option<&str>,
    sort_order: i64,
    semantic_tag: Option<&str>,
    metadata_json: &str,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
         (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
          semantic_tag, external_code, is_active, metadata_json) \
         SELECT ?, rs.id, NULL, ?, ?, ?, ?, NULL, NULL, ?, NULL, 1, ? \
           FROM reference_domains d \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
          WHERE UPPER(TRIM(d.code)) = ? \
            AND NOT EXISTS ( \
                SELECT 1 FROM reference_values rv JOIN reference_sets s2 ON s2.id = rv.set_id \
                 WHERE s2.domain_id = d.id AND LOWER(TRIM(rv.code)) = LOWER(TRIM(?))) \
          ORDER BY rs.version_no DESC LIMIT 1",
        [
            seeded_sync_id(domain, code).into(),
            code.into(),
            label.into(),
            description.map(str::to_string).into(),
            sort_order.into(),
            semantic_tag.map(str::to_string).into(),
            metadata_json.into(),
            domain.into(),
            code.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Ensures both SYSTEM domains and their shipped values. Existing values are never rewritten
/// except the nature tag and the default flag of the shipped types, which belong to the product.
pub async fn ensure_work_order_type_catalog(conn: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        conn,
        NATURES_DOMAIN,
        "Natures de maintenance (EN 13306)",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    ensure_published_set(conn, NATURES_DOMAIN).await?;
    for (i, seed) in NATURE_SEEDS.iter().enumerate() {
        let meta = serde_json::json!({ "origin": "system", "label_en": seed.label_en, "standard": "EN 13306" });
        insert_value(
            conn,
            NATURES_DOMAIN,
            seed.nature.as_str(),
            seed.label_fr,
            Some(seed.description),
            (i as i64) + 1,
            Some("maintenance_nature"),
            &meta.to_string(),
        )
        .await?;
    }

    ensure_domain(
        conn,
        WORK_ORDER_TYPES_DOMAIN,
        "Types d'OT",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    ensure_published_set(conn, WORK_ORDER_TYPES_DOMAIN).await?;
    ensure_subtype_catalog(conn).await?;
    for (i, seed) in TYPE_SEEDS.iter().enumerate() {
        let meta = serde_json::json!({
            "origin": "system",
            "label_en": seed.label_en,
            "nature_default": seed.nature_default,
        });
        insert_value(
            conn,
            WORK_ORDER_TYPES_DOMAIN,
            seed.code,
            seed.label_fr,
            None,
            (i as i64) + 1,
            Some(seed.nature.as_str()),
            &meta.to_string(),
        )
        .await?;
        // The nature of a shipped type is the product's decision: re-assert it.
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET semantic_tag = ?, metadata_json = ? \
              WHERE LOWER(TRIM(code)) = ? AND set_id IN ( \
                SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id \
                 WHERE UPPER(TRIM(d.code)) = ?) \
                AND (semantic_tag IS NOT ? OR metadata_json IS NOT ?)",
            [
                seed.nature.as_str().into(),
                meta.to_string().into(),
                seed.code.into(),
                WORK_ORDER_TYPES_DOMAIN.into(),
                seed.nature.as_str().into(),
                meta.to_string().into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

// ─── Reads ──────────────────────────────────────────────────────────────────────

/// One OT type with its nature, as the OT screens and the reference tree show it.
#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderTypeOption {
    /// `reference_values.id` (what `work_orders.type_id` holds).
    pub id: i64,
    pub code: String,
    pub label: String,
    pub label_en: String,
    /// Nature code, `None` only for a type migrated from a plant's own list without a nature.
    pub nature: Option<String>,
    pub nature_label: Option<String>,
    pub nature_label_en: Option<String>,
    /// Shipped by Maintafox (always true unless migrated from a plant's own list).
    pub is_system: bool,
    pub is_active: bool,
}

const TYPE_SELECT: &str = "SELECT t.id, t.code, t.label, t.semantic_tag, t.is_active, t.metadata_json, \
            n.label AS nature_label, n.metadata_json AS nature_metadata \
       FROM reference_values t \
       JOIN reference_sets rs ON rs.id = t.set_id AND rs.status = 'published' \
       JOIN reference_domains d ON d.id = rs.domain_id AND UPPER(TRIM(d.code)) = 'WORK.ORDER_TYPES' \
       LEFT JOIN reference_values n ON n.code = t.semantic_tag AND n.set_id IN ( \
            SELECT nrs.id FROM reference_sets nrs JOIN reference_domains nd ON nd.id = nrs.domain_id \
             WHERE UPPER(TRIM(nd.code)) = 'WORK.MAINTENANCE_NATURES' AND nrs.status = 'published')";

fn decode(e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("WORK.ORDER_TYPES row decode: {e}"))
}

fn meta_str(raw: Option<&str>, key: &str) -> Option<String> {
    raw.and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get(key).and_then(|x| x.as_str()).map(str::to_string))
}

fn map_type(row: &QueryResult) -> AppResult<WorkOrderTypeOption> {
    let label: String = row.try_get("", "label").map_err(decode)?;
    let meta: Option<String> = row.try_get("", "metadata_json").map_err(decode)?;
    let nature_meta: Option<String> = row.try_get("", "nature_metadata").map_err(decode)?;
    let nature_label: Option<String> = row.try_get("", "nature_label").map_err(decode)?;
    let nature: Option<String> = row
        .try_get::<Option<String>>("", "semantic_tag")
        .map_err(decode)?
        .filter(|t| MaintenanceNature::parse(t).is_some());
    Ok(WorkOrderTypeOption {
        id: row.try_get("", "id").map_err(decode)?,
        code: row.try_get("", "code").map_err(decode)?,
        label_en: meta_str(meta.as_deref(), "label_en").unwrap_or_else(|| label.clone()),
        is_system: meta_str(meta.as_deref(), "origin").as_deref() == Some("system"),
        label,
        nature_label_en: meta_str(nature_meta.as_deref(), "label_en").or_else(|| nature_label.clone()),
        nature_label: nature.as_ref().and(nature_label),
        nature,
        is_active: row.try_get::<i64>("", "is_active").map_err(decode)? != 0,
    })
}

/// Every OT type (active and inactive), in the product order.
pub async fn list_work_order_types(conn: &impl ConnectionTrait) -> AppResult<Vec<WorkOrderTypeOption>> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{TYPE_SELECT} ORDER BY t.sort_order ASC, t.code ASC"),
        ))
        .await?;
    rows.iter().map(map_type).collect()
}

async fn type_by(
    conn: &impl ConnectionTrait,
    clause: &str,
    value: sea_orm::Value,
) -> AppResult<Option<WorkOrderTypeOption>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{TYPE_SELECT} WHERE {clause} LIMIT 1"),
            [value],
        ))
        .await?;
    row.as_ref().map(map_type).transpose()
}

/// Id of an active OT type by code (OT creation).
pub async fn resolve_work_order_type_id_by_code(conn: &impl ConnectionTrait, type_code: &str) -> AppResult<i64> {
    let normalized = type_code.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le type d'OT est obligatoire (type_code).".into(),
        ]));
    }
    let found = type_by(conn, "LOWER(TRIM(t.code)) = ?", normalized.clone().into()).await?;
    match found {
        None => Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT introuvable (type_code='{normalized}')."
        )])),
        Some(t) if !t.is_active => Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT inactif (type_code='{normalized}')."
        )])),
        Some(t) => Ok(t.id),
    }
}

/// Code of an active OT type by id.
pub async fn resolve_work_order_type_code_by_id(conn: &impl ConnectionTrait, type_id: i64) -> AppResult<String> {
    match type_by(conn, "t.id = ?", type_id.into()).await? {
        None => Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT introuvable (type_id={type_id})."
        )])),
        Some(t) if !t.is_active => Err(AppError::ValidationFailed(vec![format!(
            "Type d'OT inactif (type_id={type_id})."
        )])),
        Some(t) => Ok(t.code),
    }
}

/// The OT type the program uses when it creates an OT of `nature` (DI conversion, PM).
pub async fn default_type_for_nature(
    conn: &impl ConnectionTrait,
    nature: MaintenanceNature,
) -> AppResult<WorkOrderTypeOption> {
    let candidates = list_work_order_types(conn).await?;
    candidates
        .into_iter()
        .filter(|t| t.is_active && t.nature.as_deref() == Some(nature.as_str()))
        .max_by_key(|t| t.is_system && t.code == default_code(nature))
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "WORK.ORDER_TYPES has no active type of nature '{}'",
                nature.as_str()
            ))
        })
}

fn default_code(nature: MaintenanceNature) -> &'static str {
    TYPE_SEEDS
        .iter()
        .find(|t| t.nature == nature && t.nature_default)
        .map(|t| t.code)
        .unwrap_or("")
}

/// Nature of the work order `wo_id` (`None` for a migrated type without a nature).
pub async fn work_order_nature(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<MaintenanceNature>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.semantic_tag AS nature FROM work_orders wo \
               LEFT JOIN reference_values t ON t.id = wo.type_id \
              WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let raw: Option<String> = row.try_get("", "nature").map_err(decode)?;
    Ok(raw.as_deref().and_then(MaintenanceNature::parse))
}

/// The shipped request types (`DI.REQUEST_TYPE`) that name the work of the OT they become:
/// `(request type code, nature)`. Written as the semantic tag of those values (startup seed and
/// migration 198); the conversion reads the tag, never this table, so a plant's own request type
/// follows the same rule once tagged.
pub const DI_REQUEST_TYPE_NATURES: &[(&str, MaintenanceNature)] = &[
    ("preventive", MaintenanceNature::PreventivePredetermined),
    ("calibration", MaintenanceNature::PreventivePredetermined),
    ("inspection", MaintenanceNature::PreventiveConditionBased),
    ("installation", MaintenanceNature::Improvement),
    ("improvement", MaintenanceNature::Improvement),
];

/// Nature named by the request type `code` (its semantic tag in `DI.REQUEST_TYPE`); `None` for a
/// repair, observation or other request, whose OT is corrective.
pub async fn nature_for_request_type(conn: &impl ConnectionTrait, code: &str) -> AppResult<Option<MaintenanceNature>> {
    let tag: Option<String> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT v.semantic_tag FROM reference_values v \
               JOIN reference_sets rs ON rs.id = v.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = 'DI.REQUEST_TYPE' AND UPPER(TRIM(v.code)) = UPPER(TRIM(?)) \
              ORDER BY rs.version_no DESC LIMIT 1",
            [code.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "semantic_tag"))
        .transpose()
        .map_err(decode)?
        .flatten();
    Ok(tag.as_deref().and_then(MaintenanceNature::parse))
}

// ─── Company sub-types ──────────────────────────────────────────────────────────

/// COMPANY list of the plant's own OT sub-types (owner decision 2026-10-15). Each value hangs
/// under one protected nature (`reference_values.parent_id` → a value of
/// `WORK.MAINTENANCE_NATURES`, enforced by `reference::values`). An OT may carry a sub-type
/// (`work_orders.subtype_id`) whose nature matches its type's nature; every rule keeps reading
/// the type's nature.
pub const SUBTYPES_DOMAIN: &str = "WORK.ORDER_SUBTYPES";

/// Stable gate code of an OT sub-type that does not fit the OT (translated under `errors:gateError`).
pub const GATE_WO_SUBTYPE_INVALID: &str = "GATE_WO_SUBTYPE_INVALID";

/// Ensures the (empty) COMPANY domain of the sub-types and its published set. The plant fills it.
pub async fn ensure_subtype_catalog(conn: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        conn,
        SUBTYPES_DOMAIN,
        "Sous-types d'OT",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(conn, SUBTYPES_DOMAIN).await
}

/// One sub-type with the nature it belongs to.
#[derive(Debug, Clone, Serialize)]
pub struct WorkOrderSubtypeOption {
    pub id: i64,
    pub code: String,
    pub label: String,
    pub nature: Option<String>,
    pub is_active: bool,
}

const SUBTYPE_SELECT: &str = "SELECT st.id, st.code, st.label, st.is_active, n.code AS nature \
       FROM reference_values st \
       JOIN reference_sets rs ON rs.id = st.set_id AND rs.status = 'published' \
       JOIN reference_domains d ON d.id = rs.domain_id AND UPPER(TRIM(d.code)) = 'WORK.ORDER_SUBTYPES' \
       LEFT JOIN reference_values n ON n.id = st.parent_id";

fn map_subtype(row: &QueryResult) -> AppResult<WorkOrderSubtypeOption> {
    Ok(WorkOrderSubtypeOption {
        id: row.try_get("", "id").map_err(decode)?,
        code: row.try_get("", "code").map_err(decode)?,
        label: row.try_get("", "label").map_err(decode)?,
        nature: row
            .try_get::<Option<String>>("", "nature")
            .map_err(decode)?
            .filter(|n| MaintenanceNature::parse(n).is_some()),
        is_active: row.try_get::<i64>("", "is_active").map_err(decode)? != 0,
    })
}

/// Every sub-type of the plant (active and inactive), by label.
pub async fn list_work_order_subtypes(conn: &impl ConnectionTrait) -> AppResult<Vec<WorkOrderSubtypeOption>> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{SUBTYPE_SELECT} ORDER BY st.sort_order ASC, st.label ASC"),
        ))
        .await?;
    rows.iter().map(map_subtype).collect()
}

/// Id of the active sub-type `code` when its nature is the nature of the OT type `type_id`.
pub async fn resolve_subtype_for_type(conn: &impl ConnectionTrait, code: &str, type_id: i64) -> AppResult<i64> {
    let refuse = |msg: String| AppError::ValidationFailed(vec![format!("{GATE_WO_SUBTYPE_INVALID}: {msg}")]);
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SUBTYPE_SELECT} WHERE UPPER(TRIM(st.code)) = UPPER(TRIM(?)) LIMIT 1"),
            [code.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(refuse(format!("sous-type d'OT introuvable (code='{code}').")));
    };
    let subtype = map_subtype(&row)?;
    if !subtype.is_active {
        return Err(refuse(format!("sous-type d'OT inactif (code='{code}').")));
    }
    let type_nature = type_by(conn, "t.id = ?", type_id.into()).await?.and_then(|t| t.nature);
    if subtype.nature.is_none() || subtype.nature != type_nature {
        return Err(refuse(format!(
            "le sous-type '{}' relève d'une autre nature de maintenance que le type de l'OT.",
            subtype.label
        )));
    }
    Ok(subtype.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_nature_has_exactly_one_default_type() {
        for nature in MaintenanceNature::ALL {
            let defaults = TYPE_SEEDS
                .iter()
                .filter(|t| t.nature == nature && t.nature_default)
                .count();
            assert_eq!(defaults, 1, "nature {}", nature.as_str());
        }
    }

    #[test]
    fn families_partition_the_natures() {
        for n in MaintenanceNature::ALL {
            let families = [
                n.is_corrective(),
                n.is_preventive(),
                n == MaintenanceNature::Improvement,
            ];
            assert_eq!(families.iter().filter(|f| **f).count(), 1, "{}", n.as_str());
            assert_eq!(MaintenanceNature::parse(n.as_str()), Some(n));
        }
        assert!(MaintenanceNature::Improvement.is_planned());
        assert!(!MaintenanceNature::CorrectiveDeferred.is_planned());
        assert!(MaintenanceNature::CorrectiveImmediate.is_emergency());
    }

    #[test]
    fn nature_sql_list_quotes_codes() {
        assert_eq!(
            nature_sql_list(&MaintenanceNature::CORRECTIVE),
            "'corrective_immediate', 'corrective_deferred'"
        );
    }
}
