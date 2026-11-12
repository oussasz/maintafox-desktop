//! Technical characteristics of an equipment, per family (L2-07).
//!
//! The characteristics are defined in Données de référence, in the COMPANY list
//! `EQUIPMENT.CHARACTERISTICS`: each value is one characteristic of one family (`parent_id` → a
//! value of `EQUIPMENT.FAMILY`), its label is the field's label and its `metadata_json` says what
//! it holds:
//!
//! - `data_type`: `number`, `text` or `boolean` (required);
//! - `unit_code`: code of a unit of `MEASURE.UNITS` (numbers only, optional);
//! - `required`: whether an equipment of the family must fill it in.
//!
//! The values of an equipment are stored in `equipment.characteristics_json`
//! (`{"<characteristic code>": "<value>"}`, migration 289). Nothing about a characteristic is
//! written here: the asset form shows what the list of its family says. A value whose
//! characteristic was removed from the list is kept and shown as such (never lost silently).

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

/// COMPANY list of the characteristics of each family (Données de référence).
pub const DOMAIN: &str = "EQUIPMENT.CHARACTERISTICS";

/// A characteristic of the list whose metadata does not say what it holds.
pub const GATE_ASSET_CHARACTERISTIC_DEFINITION_INVALID: &str = "GATE_ASSET_CHARACTERISTIC_DEFINITION_INVALID";
/// A required characteristic of the family is left empty.
pub const GATE_ASSET_CHARACTERISTIC_REQUIRED: &str = "GATE_ASSET_CHARACTERISTIC_REQUIRED";
/// A numeric characteristic holds something that is not a number.
pub const GATE_ASSET_CHARACTERISTIC_NOT_NUMBER: &str = "GATE_ASSET_CHARACTERISTIC_NOT_NUMBER";
/// A yes / no characteristic holds something else.
pub const GATE_ASSET_CHARACTERISTIC_NOT_BOOLEAN: &str = "GATE_ASSET_CHARACTERISTIC_NOT_BOOLEAN";
/// A value names a characteristic that is not one of the equipment's family.
pub const GATE_ASSET_CHARACTERISTIC_UNKNOWN: &str = "GATE_ASSET_CHARACTERISTIC_UNKNOWN";
/// A value is longer than [`VALUE_MAX_CHARS`].
pub const GATE_ASSET_CHARACTERISTIC_TOO_LONG: &str = "GATE_ASSET_CHARACTERISTIC_TOO_LONG";

/// Longest value kept for one characteristic.
pub const VALUE_MAX_CHARS: usize = 500;

/// Shipped defaults the plant may change (not locked like `origin: system` rows).
const SHIPPED_ORIGIN: &str = "shipped";

pub fn is_characteristics_domain(domain_code: &str) -> bool {
    domain_code.trim().eq_ignore_ascii_case(DOMAIN)
}

/// What a characteristic holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacteristicType {
    Number,
    Text,
    Boolean,
}

impl CharacteristicType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Text => "text",
            Self::Boolean => "boolean",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "number" => Some(Self::Number),
            "text" => Some(Self::Text),
            "boolean" => Some(Self::Boolean),
            _ => None,
        }
    }
}

/// The definition part of a characteristic's `metadata_json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionMeta {
    pub data_type: CharacteristicType,
    pub unit_code: Option<String>,
    pub required: bool,
}

/// Reads a characteristic's `metadata_json`; the problem in French when it does not read.
pub fn parse_definition_meta(metadata_json: Option<&str>) -> Result<DefinitionMeta, String> {
    let raw = metadata_json.map(str::trim).filter(|s| !s.is_empty());
    let Some(raw) = raw else {
        return Err("le type de valeur (nombre, texte ou oui / non) n'est pas indiqué".into());
    };
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "les métadonnées ne sont pas un objet JSON valide".to_string())?;
    let Some(obj) = value.as_object() else {
        return Err("les métadonnées ne sont pas un objet JSON".into());
    };
    let data_type = obj
        .get("data_type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "le type de valeur (nombre, texte ou oui / non) n'est pas indiqué".to_string())?;
    let data_type = CharacteristicType::parse(data_type)
        .ok_or_else(|| format!("le type de valeur '{data_type}' n'est pas nombre, texte ou oui / non"))?;
    let unit_code = match obj.get("unit_code") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) if s.trim().is_empty() => None,
        Some(serde_json::Value::String(s)) => Some(s.trim().to_string()),
        Some(_) => return Err("l'unité doit être le code d'une unité de mesure".into()),
    };
    if unit_code.is_some() && data_type != CharacteristicType::Number {
        return Err("seule une caractéristique numérique a une unité".into());
    }
    let required = match obj.get("required") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(_) => return Err("« obligatoire » doit valoir oui ou non".into()),
    };
    Ok(DefinitionMeta {
        data_type,
        unit_code,
        required,
    })
}

/// Checks a characteristic's `metadata_json` when the list is edited and returns it written
/// canonically (`data_type`, `unit_code`, `required` always present; other keys kept).
pub fn checked_definition_metadata(code: &str, metadata_json: Option<&str>) -> AppResult<String> {
    let meta = parse_definition_meta(metadata_json).map_err(|problem| {
        AppError::ValidationFailed(vec![format!(
            "{GATE_ASSET_CHARACTERISTIC_DEFINITION_INVALID}:code={code}: Caractéristique '{code}' : {problem}."
        )])
    })?;
    let mut obj = metadata_json
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    obj.insert("data_type".into(), meta.data_type.as_str().into());
    obj.insert(
        "unit_code".into(),
        meta.unit_code
            .map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    obj.insert("required".into(), meta.required.into());
    Ok(serde_json::Value::Object(obj).to_string())
}

// ─── Shipped characteristics ─────────────────────────────────────────────────────

/// `(code, family code, label, data type, unit code, required)`.
type ShippedCharacteristic = (
    &'static str,
    &'static str,
    &'static str,
    CharacteristicType,
    Option<&'static str>,
    bool,
);

/// The characteristics shipped for the shipped families (`assets::taxonomy_reference`). A plant
/// adds, changes or deactivates its own in Données de référence; these are only written when
/// missing (never brought back once the plant changed them).
const SHIPPED: &[ShippedCharacteristic] = &[
    (
        "PUMP_FLOW",
        "PUMP_FAMILY",
        "Débit nominal",
        CharacteristicType::Number,
        Some("M3H"),
        false,
    ),
    (
        "PUMP_HEAD",
        "PUMP_FAMILY",
        "Hauteur manométrique",
        CharacteristicType::Number,
        Some("M"),
        false,
    ),
    (
        "PUMP_POWER",
        "PUMP_FAMILY",
        "Puissance absorbée",
        CharacteristicType::Number,
        Some("KW"),
        false,
    ),
    (
        "MOTOR_POWER",
        "MOTOR_FAMILY",
        "Puissance nominale",
        CharacteristicType::Number,
        Some("KW"),
        true,
    ),
    (
        "MOTOR_VOLTAGE",
        "MOTOR_FAMILY",
        "Tension nominale",
        CharacteristicType::Number,
        Some("V"),
        false,
    ),
    (
        "MOTOR_CURRENT",
        "MOTOR_FAMILY",
        "Intensité nominale",
        CharacteristicType::Number,
        Some("A"),
        false,
    ),
    (
        "MOTOR_SPEED",
        "MOTOR_FAMILY",
        "Vitesse nominale",
        CharacteristicType::Number,
        Some("RPM"),
        false,
    ),
    (
        "VALVE_DN",
        "VALVE_FAMILY",
        "Diamètre nominal (DN)",
        CharacteristicType::Number,
        Some("MM"),
        false,
    ),
    (
        "VALVE_PN",
        "VALVE_FAMILY",
        "Pression nominale (PN)",
        CharacteristicType::Number,
        Some("BAR"),
        false,
    ),
    (
        "HX_PRESSURE",
        "HEAT_EXCHANGER_FAMILY",
        "Pression de service",
        CharacteristicType::Number,
        Some("BAR"),
        false,
    ),
    (
        "HX_TEMPERATURE",
        "HEAT_EXCHANGER_FAMILY",
        "Température de service",
        CharacteristicType::Number,
        Some("DEGC"),
        false,
    ),
    (
        "COMPRESSOR_POWER",
        "COMPRESSOR_FAMILY",
        "Puissance nominale",
        CharacteristicType::Number,
        Some("KW"),
        false,
    ),
    (
        "COMPRESSOR_PRESSURE",
        "COMPRESSOR_FAMILY",
        "Pression de refoulement",
        CharacteristicType::Number,
        Some("BAR"),
        false,
    ),
    (
        "COMPRESSOR_FLOW",
        "COMPRESSOR_FAMILY",
        "Débit",
        CharacteristicType::Number,
        Some("M3H"),
        false,
    ),
    (
        "CONVEYOR_LENGTH",
        "CONVEYOR_FAMILY",
        "Longueur",
        CharacteristicType::Number,
        Some("M"),
        false,
    ),
    (
        "CONVEYOR_POWER",
        "CONVEYOR_FAMILY",
        "Puissance d'entraînement",
        CharacteristicType::Number,
        Some("KW"),
        false,
    ),
    (
        "INSTRUMENT_RANGE",
        "INSTRUMENTATION_FAMILY",
        "Plage de mesure",
        CharacteristicType::Text,
        None,
        false,
    ),
    (
        "INSTRUMENT_SIGNAL",
        "INSTRUMENTATION_FAMILY",
        "Signal de sortie",
        CharacteristicType::Text,
        None,
        false,
    ),
    (
        "VESSEL_VOLUME",
        "VESSEL_FAMILY",
        "Volume",
        CharacteristicType::Number,
        Some("M3"),
        false,
    ),
    (
        "VESSEL_MAX_PRESSURE",
        "VESSEL_FAMILY",
        "Pression maximale admissible (PS)",
        CharacteristicType::Number,
        Some("BAR"),
        true,
    ),
    (
        "VESSEL_MAX_TEMPERATURE",
        "VESSEL_FAMILY",
        "Température maximale admissible",
        CharacteristicType::Number,
        Some("DEGC"),
        false,
    ),
];

/// The list (COMPANY) with its published set, and the shipped characteristics when missing.
/// Idempotent (integrity pass at start-up); runs after the families are ensured.
pub async fn ensure_characteristic_catalog(db: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        db,
        DOMAIN,
        "Caractéristiques techniques par famille",
        "flat",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(db, DOMAIN).await?;
    for (sort, &(code, family, label, data_type, unit, required)) in SHIPPED.iter().enumerate() {
        let meta = serde_json::json!({
            "origin": SHIPPED_ORIGIN,
            "data_type": data_type.as_str(),
            "unit_code": unit,
            "required": required,
        })
        .to_string();
        let sync_id = crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{DOMAIN}:{code}"));
        // Written once into the latest published set, under the family of the same code; skipped
        // when that family is gone or the characteristic (same identity or code) already exists.
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) \
             SELECT ?, rs.id, fam.id, ?, ?, NULL, ?, NULL, NULL, 'equipment_characteristic', NULL, 1, ? \
               FROM reference_domains d \
               JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
               JOIN reference_values fam ON fam.code = ? AND fam.is_active = 1 \
               JOIN reference_sets fs ON fs.id = fam.set_id AND fs.status = 'published' \
               JOIN reference_domains fd ON fd.id = fs.domain_id AND fd.code = 'EQUIPMENT.FAMILY' \
              WHERE d.code = ? \
                AND rs.version_no = (SELECT MAX(x.version_no) FROM reference_sets x WHERE x.domain_id = d.id AND x.status = 'published') \
                AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.entity_sync_id = ?) \
                AND NOT EXISTS (SELECT 1 FROM reference_values rv WHERE rv.set_id = rs.id AND rv.code = ?)",
            [
                sync_id.clone().into(),
                code.into(),
                label.into(),
                i64::try_from(sort + 1).unwrap_or(i64::MAX).into(),
                meta.into(),
                family.into(),
                DOMAIN.into(),
                sync_id.into(),
                code.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

// ─── Read model ──────────────────────────────────────────────────────────────────

/// One characteristic of the equipment's family, with the equipment's value.
#[derive(Debug, Clone, Serialize)]
pub struct AssetCharacteristic {
    pub code: String,
    pub label: String,
    /// `number`, `text` or `boolean`; NULL when the list does not say (shown as text, flagged).
    pub data_type: Option<String>,
    pub unit_code: Option<String>,
    /// Symbol of that unit, read from `MEASURE.UNITS` (the code when the unit is unknown).
    pub unit_symbol: Option<String>,
    pub required: bool,
    /// Why the definition does not read, if so (fix it in Données de référence).
    pub definition_problem: Option<String>,
    pub value: Option<String>,
}

/// A stored value whose characteristic is no longer in the family's list.
#[derive(Debug, Clone, Serialize)]
pub struct UnlistedCharacteristicValue {
    pub code: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetCharacteristicSheet {
    pub asset_id: i64,
    pub family_code: Option<String>,
    pub family_label: Option<String>,
    pub items: Vec<AssetCharacteristic>,
    pub unlisted: Vec<UnlistedCharacteristicValue>,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "characteristic row decode failed for column '{column}': {e}"
    ))
}

/// Stored values of an equipment (`characteristics_json`); an unreadable column reads as empty.
fn stored_values(raw: Option<&str>) -> BTreeMap<String, String> {
    raw.and_then(|s| serde_json::from_str::<BTreeMap<String, serde_json::Value>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k, s)),
            serde_json::Value::Number(n) => Some((k, n.to_string())),
            serde_json::Value::Bool(b) => Some((k, b.to_string())),
            _ => None,
        })
        .collect()
}

pub async fn get_asset_characteristics(
    db: &impl ConnectionTrait,
    asset_id: i64,
) -> AppResult<AssetCharacteristicSheet> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT f.code AS family_code, f.label AS family_label, e.characteristics_json \
               FROM equipment e LEFT JOIN reference_values f ON f.id = e.equipment_family_ref_id \
              WHERE e.id = ? AND e.deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        })?;
    let family_code: Option<String> = row
        .try_get("", "family_code")
        .map_err(|e| decode_err("family_code", e))?;
    let family_label: Option<String> = row
        .try_get("", "family_label")
        .map_err(|e| decode_err("family_label", e))?;
    let raw: Option<String> = row
        .try_get("", "characteristics_json")
        .map_err(|e| decode_err("characteristics_json", e))?;
    let mut values = stored_values(raw.as_deref());

    let mut items = Vec::new();
    if let Some(family) = family_code.as_deref() {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT rv.code, rv.label, rv.metadata_json, \
                        (SELECT COALESCE(NULLIF(TRIM(CASE WHEN json_valid(u.metadata_json) THEN json_extract(u.metadata_json, '$.symbol') END), ''), u.code) \
                           FROM reference_values u \
                           JOIN reference_sets us ON us.id = u.set_id AND us.status = 'published' \
                           JOIN reference_domains ud ON ud.id = us.domain_id AND ud.code = 'MEASURE.UNITS' \
                          WHERE u.code = CASE WHEN json_valid(rv.metadata_json) THEN json_extract(rv.metadata_json, '$.unit_code') END \
                          ORDER BY us.version_no DESC LIMIT 1) AS unit_symbol \
                   FROM reference_values rv \
                   JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
                   JOIN reference_domains d ON d.id = rs.domain_id \
                   JOIN reference_values pf ON pf.id = rv.parent_id \
                  WHERE d.code = ? AND rv.is_active = 1 AND pf.code = ? \
                  ORDER BY rv.sort_order ASC, rv.code ASC",
                [DOMAIN.into(), family.into()],
            ))
            .await?;
        for r in rows {
            let code: String = r.try_get("", "code").map_err(|e| decode_err("code", e))?;
            let label: String = r.try_get("", "label").map_err(|e| decode_err("label", e))?;
            let metadata_json: Option<String> = r
                .try_get("", "metadata_json")
                .map_err(|e| decode_err("metadata_json", e))?;
            let unit_symbol: Option<String> = r.try_get("", "unit_symbol").map_err(|e| decode_err("unit_symbol", e))?;
            let value = values.remove(&code);
            match parse_definition_meta(metadata_json.as_deref()) {
                Ok(meta) => {
                    let unit_symbol = meta
                        .unit_code
                        .as_ref()
                        .map(|c| unit_symbol.clone().unwrap_or_else(|| c.clone()));
                    items.push(AssetCharacteristic {
                        code,
                        label,
                        data_type: Some(meta.data_type.as_str().to_string()),
                        unit_code: meta.unit_code,
                        unit_symbol,
                        required: meta.required,
                        definition_problem: None,
                        value,
                    });
                }
                Err(problem) => items.push(AssetCharacteristic {
                    code,
                    label,
                    data_type: None,
                    unit_code: None,
                    unit_symbol: None,
                    required: false,
                    definition_problem: Some(problem),
                    value,
                }),
            }
        }
    }
    let unlisted = values
        .into_iter()
        .map(|(code, value)| UnlistedCharacteristicValue { code, value })
        .collect();
    Ok(AssetCharacteristicSheet {
        asset_id,
        family_code,
        family_label,
        items,
        unlisted,
    })
}

// ─── Write ───────────────────────────────────────────────────────────────────────

/// New values of an equipment's characteristics: `code → value` (empty or null clears it).
/// Characteristics not named keep their value.
#[derive(Debug, Deserialize)]
pub struct SaveAssetCharacteristicsPayload {
    pub asset_id: i64,
    pub values: BTreeMap<String, Option<String>>,
}

/// The value as stored for its type, or the refusal (gate code first).
pub fn normalized_value(code: &str, data_type: CharacteristicType, raw: &str) -> Result<String, String> {
    let v = raw.trim();
    if v.chars().count() > VALUE_MAX_CHARS {
        return Err(format!(
            "{GATE_ASSET_CHARACTERISTIC_TOO_LONG}:code={code}:max={VALUE_MAX_CHARS}: La valeur de '{code}' dépasse {VALUE_MAX_CHARS} caractères."
        ));
    }
    match data_type {
        CharacteristicType::Text => Ok(v.to_string()),
        CharacteristicType::Number => {
            let n: f64 = v
                .replace(' ', "")
                .replace(',', ".")
                .parse()
                .ok()
                .filter(|n: &f64| n.is_finite())
                .ok_or_else(|| {
                    format!("{GATE_ASSET_CHARACTERISTIC_NOT_NUMBER}:code={code}: La valeur de '{code}' doit être un nombre (reçu « {v} »).")
                })?;
            Ok(n.to_string())
        }
        CharacteristicType::Boolean => match v {
            "true" | "false" => Ok(v.to_string()),
            _ => Err(format!(
                "{GATE_ASSET_CHARACTERISTIC_NOT_BOOLEAN}:code={code}: La valeur de '{code}' doit être oui ou non."
            )),
        },
    }
}

pub async fn save_asset_characteristics(
    db: &DatabaseConnection,
    payload: SaveAssetCharacteristicsPayload,
    actor_id: i32,
) -> AppResult<AssetCharacteristicSheet> {
    let txn = db.begin().await?;
    let sheet = get_asset_characteristics(&txn, payload.asset_id).await?;
    let raw: Option<String> = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT characteristics_json FROM equipment WHERE id = ?",
            [payload.asset_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "characteristics_json"))
        .transpose()
        .map_err(|e| decode_err("characteristics_json", e))?
        .flatten();
    let mut merged = stored_values(raw.as_deref());

    let mut errors = Vec::new();
    for (code, value) in &payload.values {
        let Some(def) = sheet.items.iter().find(|i| &i.code == code) else {
            errors.push(format!(
                "{GATE_ASSET_CHARACTERISTIC_UNKNOWN}:code={code}: '{code}' n'est pas une caractéristique de la famille de cet équipement."
            ));
            continue;
        };
        let value = value.as_deref().map(str::trim).filter(|v| !v.is_empty());
        let Some(value) = value else {
            merged.remove(code);
            continue;
        };
        // A characteristic whose definition does not read keeps what is typed, as text.
        let data_type = def
            .data_type
            .as_deref()
            .and_then(CharacteristicType::parse)
            .unwrap_or(CharacteristicType::Text);
        match normalized_value(code, data_type, value) {
            Ok(v) => {
                merged.insert(code.clone(), v);
            }
            Err(e) => errors.push(e),
        }
    }
    for def in sheet.items.iter().filter(|i| i.required) {
        if !merged.contains_key(&def.code) {
            errors.push(format!(
                "{GATE_ASSET_CHARACTERISTIC_REQUIRED}:code={}: La caractéristique « {} » est obligatoire pour cette famille.",
                def.code, def.label
            ));
        }
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    let json = if merged.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&merged)?)
    };
    let now = crate::clock::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment SET characteristics_json = ?, updated_at = ?, row_version = row_version + 1 WHERE id = ?",
        [json.into(), now.into(), payload.asset_id.into()],
    ))
    .await?;
    let sheet = get_asset_characteristics(&txn, payload.asset_id).await?;
    txn.commit().await?;
    tracing::info!(
        asset_id = payload.asset_id,
        "asset characteristics saved (actor={})",
        actor_id
    );
    Ok(sheet)
}
