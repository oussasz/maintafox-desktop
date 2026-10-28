//! Units of measure under reference-data governance (owner decision of the reference data
//! consolidation review: "old fixed lists → governed lists, starting with units of measure").
//!
//! Two reference domains:
//!
//! - `MEASURE.UNITS` — the plant's units, a **COMPANY** list of category C (draft → validate →
//!   publish, impact preview), structure `unit_set`. Every module links a unit by its
//!   `reference_values.id`, never by a copied text:
//!   - stock unit of an article (`articles.unit_value_id`);
//!   - price unit of a supplier price (`inventory_supplier_prices.price_unit_value_id`);
//!   - both units of a conversion (`unit_conversions.from_unit_value_id` / `to_unit_value_id`);
//!   - unit of an equipment meter (`equipment_meters.unit_value_id`), read by PM plans, inspection
//!     schedules, the asset register and the asset history;
//!   - unit of an inspection measurement (`inspection_checkpoints.measurement_unit_id`, and the
//!     checkpoint package of a published checklist version).
//!
//!   A value carries its code, its French label (`label`), its English label
//!   (`metadata_json.label_en`), its symbol (`metadata_json.symbol`), its physical dimension
//!   (`semantic_tag`, the code of a `MEASURE.DIMENSIONS` value, checked on the server) and, where
//!   one exists, its UN/CEFACT Recommendation 20 common code (`external_code`, e.g. `KGM`).
//!
//!   - **Standard units** (kg, m, L, h, bar, °C, pieces…, [`UNIT_SEEDS`]) are shipped by
//!     Maintafox with `metadata_json.origin = "system"` and protected by the reference engine's
//!     protected-value guard (`governance::assert_system_value_mutable`): they cannot be edited,
//!     deactivated or recoded, because stock and meter history depend on them. Local wording goes
//!     through aliases.
//!   - **Plant units** (a bag, a drum, a pallet…) are added by the plant. Each one names a base
//!     standard unit of the same dimension (`parent_id`, a unit of the same set) and the factor
//!     to it (`metadata_json.base_factor`: 1 bag = 25 kg), both required and checked on the
//!     server ([`checked_plant_unit`]); its dimension is the base unit's.
//!   - A unit that migration 201 created for a code or a text Maintafox did not know is a plant
//!     unit flagged `metadata_json.review_required = true` (`origin = "migrated"`), without base
//!     or factor (nothing is assumed). Draft validation reports it as a warning; the first edit
//!     of such a unit requires the base and factor like any plant unit, and clears the flag.
//!
//! **Which conversion applies** (`inventory::unit_conversions::convert_quantity`, the single
//! rule for any quantity expressed in another unit — a supplier price unit or purchase unit
//! towards the article's stock unit, the stock unit towards an issue unit):
//! 1. the article's own conversion (`unit_conversions` row with `article_id`), when one exists;
//! 2. otherwise the general factors of plant units (each unit brought to its base standard unit,
//!    then a general conversion between the two base units if they differ);
//! 3. otherwise a general conversion between the two units (`unit_conversions` row without
//!    article, between standard units).
//!
//! A conversion that needs a plant unit without base or factor is refused
//! ([`GATE_UNIT_FACTOR_MISSING`]); a factor is never taken as 1. Movements kept in the article's
//! own stock unit need no conversion and are never affected.
//!
//! - `MEASURE.DIMENSIONS` — the physical dimensions, a **SYSTEM** list (hidden from Données de
//!   référence, read-only). A dimension is an internal classification the conversion engine
//!   depends on: two units convert only when they measure the same thing
//!   ([`dimensions_compatible`]); a dimension added by a plant would have no conversion rule. The
//!   codes are fixed by [`UnitDimension`]; the list exists so the FR/EN labels the editor shows
//!   come from reference data, not from a frontend copy.
//!
//! The units replace the fixed lookup `inventory.unit_of_measure` (8 values created at start-up)
//! and the free texts of meter units and checkpoint units (migration 201, [`retire_legacy_unit_lookup`]).

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::reference::audience::DomainAudience;
use crate::reference::system_catalog_integrity::{ensure_domain, ensure_published_set};

pub const UNITS_DOMAIN: &str = "MEASURE.UNITS";
pub const DIMENSIONS_DOMAIN: &str = "MEASURE.DIMENSIONS";

/// The retired fixed lookup of stock units (`lookup_domains.domain_key`).
pub const LEGACY_UNIT_LOOKUP_DOMAIN: &str = "inventory.unit_of_measure";

/// A unit that is not an active unit of the published list.
pub const GATE_UNIT_NOT_ACTIVE: &str = "GATE_UNIT_NOT_ACTIVE";
/// A conversion between units that measure different things (mass and length, …).
pub const GATE_UNIT_DIMENSION_MISMATCH: &str = "GATE_UNIT_DIMENSION_MISMATCH";
/// A merge of two units that do not stand for the same quantity.
pub const GATE_UNIT_MERGE_CHANGES_QUANTITIES: &str = "GATE_UNIT_MERGE_CHANGES_QUANTITIES";
/// A quantity entered in a unit that does not give a whole number of the article's stock unit
/// when the article moves by whole units (serial-tracked).
pub const GATE_UNIT_WHOLE_UNITS: &str = "GATE_UNIT_WHOLE_UNITS";
/// A conversion with a unit whose dimension is not set yet.
pub const GATE_UNIT_DIMENSION_MISSING: &str = "GATE_UNIT_DIMENSION_MISSING";
/// A unit whose dimension is not one of `MEASURE.DIMENSIONS`.
pub const GATE_UNIT_DIMENSION_INVALID: &str = "GATE_UNIT_DIMENSION_INVALID";
/// A plant unit without its base standard unit or its factor.
pub const GATE_UNIT_BASE_REQUIRED: &str = "GATE_UNIT_BASE_REQUIRED";
/// A plant unit whose base is not an active standard unit of the same dimension, or whose factor
/// is not a positive number.
pub const GATE_UNIT_BASE_INVALID: &str = "GATE_UNIT_BASE_INVALID";
/// A conversion that needs the factor of a plant unit that has none (migrated, to review).
pub const GATE_UNIT_FACTOR_MISSING: &str = "GATE_UNIT_FACTOR_MISSING";
/// No conversion is defined between two units.
pub const GATE_UNIT_NO_CONVERSION: &str = "GATE_UNIT_NO_CONVERSION";
/// A general conversion row for a plant unit: its general factor is set on the unit itself.
pub const GATE_UNIT_GENERAL_FACTOR_ON_UNIT: &str = "GATE_UNIT_GENERAL_FACTOR_ON_UNIT";

/// Physical dimension of a unit (internal classification, see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitDimension {
    /// Pieces, pairs, cycles: dimensionless counts.
    Count,
    /// Packaging (box, roll…): its content depends on the article, so it converts to any unit.
    Packaging,
    Mass,
    Length,
    Area,
    Volume,
    Time,
    Energy,
    Power,
    Pressure,
    Temperature,
    VolumeFlow,
    Speed,
    /// Frequency and rotational speed (Hz, rev/min).
    Frequency,
    ElectricCurrent,
    Voltage,
}

impl UnitDimension {
    pub const ALL: [UnitDimension; 16] = [
        Self::Count,
        Self::Packaging,
        Self::Mass,
        Self::Length,
        Self::Area,
        Self::Volume,
        Self::Time,
        Self::Energy,
        Self::Power,
        Self::Pressure,
        Self::Temperature,
        Self::VolumeFlow,
        Self::Speed,
        Self::Frequency,
        Self::ElectricCurrent,
        Self::Voltage,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Packaging => "packaging",
            Self::Mass => "mass",
            Self::Length => "length",
            Self::Area => "area",
            Self::Volume => "volume",
            Self::Time => "time",
            Self::Energy => "energy",
            Self::Power => "power",
            Self::Pressure => "pressure",
            Self::Temperature => "temperature",
            Self::VolumeFlow => "volume_flow",
            Self::Speed => "speed",
            Self::Frequency => "frequency",
            Self::ElectricCurrent => "electric_current",
            Self::Voltage => "voltage",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        Self::ALL.into_iter().find(|d| d.as_str().eq_ignore_ascii_case(raw))
    }
}

/// Whether a quantity in a unit of dimension `a` may be converted to a unit of dimension `b`:
/// same dimension, or one side is a packaging unit (a box or a roll holds a quantity of any
/// dimension, defined by the conversion).
pub fn dimensions_compatible(a: UnitDimension, b: UnitDimension) -> bool {
    a == b || a == UnitDimension::Packaging || b == UnitDimension::Packaging
}

struct DimensionSeed {
    dimension: UnitDimension,
    label_fr: &'static str,
    label_en: &'static str,
}

const fn dim(dimension: UnitDimension, label_fr: &'static str, label_en: &'static str) -> DimensionSeed {
    DimensionSeed {
        dimension,
        label_fr,
        label_en,
    }
}

const DIMENSION_SEEDS: &[DimensionSeed] = &[
    dim(UnitDimension::Count, "Dénombrement", "Count"),
    dim(UnitDimension::Packaging, "Conditionnement", "Packaging"),
    dim(UnitDimension::Mass, "Masse", "Mass"),
    dim(UnitDimension::Length, "Longueur", "Length"),
    dim(UnitDimension::Area, "Surface", "Area"),
    dim(UnitDimension::Volume, "Volume", "Volume"),
    dim(UnitDimension::Time, "Durée", "Time"),
    dim(UnitDimension::Energy, "Énergie", "Energy"),
    dim(UnitDimension::Power, "Puissance", "Power"),
    dim(UnitDimension::Pressure, "Pression", "Pressure"),
    dim(UnitDimension::Temperature, "Température", "Temperature"),
    dim(UnitDimension::VolumeFlow, "Débit volumique", "Volume flow rate"),
    dim(UnitDimension::Speed, "Vitesse", "Speed"),
    dim(
        UnitDimension::Frequency,
        "Fréquence / vitesse de rotation",
        "Frequency / rotational speed",
    ),
    dim(UnitDimension::ElectricCurrent, "Courant électrique", "Electric current"),
    dim(UnitDimension::Voltage, "Tension électrique", "Voltage"),
];

/// A starter unit. `rec20` is the UN/CEFACT Recommendation 20 common code, set only where the
/// standard has one for the unit.
pub(crate) struct UnitSeed {
    pub code: &'static str,
    pub label_fr: &'static str,
    pub label_en: &'static str,
    pub symbol: &'static str,
    pub dimension: UnitDimension,
    pub rec20: Option<&'static str>,
}

const fn unit(
    code: &'static str,
    label_fr: &'static str,
    label_en: &'static str,
    symbol: &'static str,
    dimension: UnitDimension,
    rec20: Option<&'static str>,
) -> UnitSeed {
    UnitSeed {
        code,
        label_fr,
        label_en,
        symbol,
        dimension,
        rec20,
    }
}

/// Standard units (protected). The first eight keep the codes of the retired lookup
/// `inventory.unit_of_measure` (so stored links map by code; `U` is the piece unit); the others
/// are the usual maintenance units. Box and roll have no Recommendation 20 code of their own
/// (they are package types, UN/ECE Recommendation 21), so their symbol is their code.
pub(crate) const UNIT_SEEDS: &[UnitSeed] = &[
    unit(
        "U",
        "Unité (pièce)",
        "Unit (piece)",
        "pcs",
        UnitDimension::Count,
        Some("H87"),
    ),
    unit("KG", "Kilogramme", "Kilogram", "kg", UnitDimension::Mass, Some("KGM")),
    unit("L", "Litre", "Litre", "L", UnitDimension::Volume, Some("LTR")),
    unit("M", "Mètre", "Metre", "m", UnitDimension::Length, Some("MTR")),
    unit(
        "M2",
        "Mètre carré",
        "Square metre",
        "m²",
        UnitDimension::Area,
        Some("MTK"),
    ),
    unit("BOX", "Boîte", "Box", "BOX", UnitDimension::Packaging, None),
    unit("ROUL", "Rouleau", "Roll", "ROUL", UnitDimension::Packaging, None),
    unit("PAIRE", "Paire", "Pair", "pr", UnitDimension::Count, Some("PR")),
    unit("CYCLE", "Cycle", "Cycle", "cycle", UnitDimension::Count, Some("B7")),
    unit("G", "Gramme", "Gram", "g", UnitDimension::Mass, Some("GRM")),
    unit("T", "Tonne", "Tonne", "t", UnitDimension::Mass, Some("TNE")),
    unit(
        "MM",
        "Millimètre",
        "Millimetre",
        "mm",
        UnitDimension::Length,
        Some("MMT"),
    ),
    unit("KM", "Kilomètre", "Kilometre", "km", UnitDimension::Length, Some("KMT")),
    unit(
        "M3",
        "Mètre cube",
        "Cubic metre",
        "m³",
        UnitDimension::Volume,
        Some("MTQ"),
    ),
    unit("S", "Seconde", "Second", "s", UnitDimension::Time, Some("SEC")),
    unit("MIN", "Minute", "Minute", "min", UnitDimension::Time, Some("MIN")),
    unit("H", "Heure", "Hour", "h", UnitDimension::Time, Some("HUR")),
    unit("D", "Jour", "Day", "d", UnitDimension::Time, Some("DAY")),
    unit(
        "KWH",
        "Kilowattheure",
        "Kilowatt hour",
        "kWh",
        UnitDimension::Energy,
        Some("KWH"),
    ),
    unit("KW", "Kilowatt", "Kilowatt", "kW", UnitDimension::Power, Some("KWT")),
    unit("BAR", "Bar", "Bar", "bar", UnitDimension::Pressure, Some("BAR")),
    unit(
        "DEGC",
        "Degré Celsius",
        "Degree Celsius",
        "°C",
        UnitDimension::Temperature,
        Some("CEL"),
    ),
    unit(
        "M3H",
        "Mètre cube par heure",
        "Cubic metre per hour",
        "m³/h",
        UnitDimension::VolumeFlow,
        Some("MQH"),
    ),
    unit(
        "LMIN",
        "Litre par minute",
        "Litre per minute",
        "L/min",
        UnitDimension::VolumeFlow,
        Some("L2"),
    ),
    unit(
        "KMH",
        "Kilomètre par heure",
        "Kilometre per hour",
        "km/h",
        UnitDimension::Speed,
        Some("KMH"),
    ),
    unit("HZ", "Hertz", "Hertz", "Hz", UnitDimension::Frequency, Some("HTZ")),
    unit(
        "RPM",
        "Tour par minute",
        "Revolution per minute",
        "tr/min",
        UnitDimension::Frequency,
        Some("M46"),
    ),
    unit(
        "A",
        "Ampère",
        "Ampere",
        "A",
        UnitDimension::ElectricCurrent,
        Some("AMP"),
    ),
    unit("V", "Volt", "Volt", "V", UnitDimension::Voltage, Some("VLT")),
];

fn decode(e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("MEASURE.UNITS row decode: {e}"))
}

fn seeded_sync_id(domain: &str, code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("reference_values", &format!("{domain}:{code}"))
}

struct NewValue<'a> {
    code: &'a str,
    label: &'a str,
    sort_order: i64,
    semantic_tag: Option<&'a str>,
    external_code: Option<&'a str>,
    is_active: bool,
    metadata: serde_json::Value,
}

/// Inserts `value` into the latest published set of `domain` unless a value with the same code
/// exists in any version of the domain (a unit the plant renamed or retired stays as it is).
async fn insert_value(conn: &impl ConnectionTrait, domain: &str, value: &NewValue<'_>) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_values \
         (entity_sync_id, set_id, parent_id, code, label, description, sort_order, color_hex, icon_name, \
          semantic_tag, external_code, is_active, metadata_json) \
         SELECT ?, rs.id, NULL, ?, ?, NULL, ?, NULL, NULL, ?, ?, ?, ? \
           FROM reference_domains d \
           JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published' \
          WHERE UPPER(TRIM(d.code)) = ? \
            AND NOT EXISTS ( \
                SELECT 1 FROM reference_values rv JOIN reference_sets s2 ON s2.id = rv.set_id \
                 WHERE s2.domain_id = d.id AND UPPER(TRIM(rv.code)) = UPPER(TRIM(?))) \
          ORDER BY rs.version_no DESC LIMIT 1",
        [
            seeded_sync_id(domain, value.code).into(),
            value.code.into(),
            value.label.into(),
            value.sort_order.into(),
            value.semantic_tag.map(str::to_string).into(),
            value.external_code.map(str::to_string).into(),
            i64::from(value.is_active).into(),
            value.metadata.to_string().into(),
            domain.into(),
            value.code.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Ensures both domains, their published sets, the dimensions and the standard units. Existing
/// values are never rewritten. Run by migration 201 and by the startup integrity pass.
pub async fn ensure_unit_catalog(conn: &impl ConnectionTrait) -> AppResult<()> {
    ensure_domain(
        conn,
        DIMENSIONS_DOMAIN,
        "Grandeurs physiques des unités",
        "flat",
        "system_seeded",
        0,
        DomainAudience::System,
    )
    .await?;
    ensure_published_set(conn, DIMENSIONS_DOMAIN).await?;
    for (i, seed) in DIMENSION_SEEDS.iter().enumerate() {
        insert_value(
            conn,
            DIMENSIONS_DOMAIN,
            &NewValue {
                code: seed.dimension.as_str(),
                label: seed.label_fr,
                sort_order: i as i64 + 1,
                semantic_tag: Some("unit_dimension"),
                external_code: None,
                is_active: true,
                metadata: serde_json::json!({ "origin": "system", "label_en": seed.label_en }),
            },
        )
        .await?;
    }

    ensure_domain(
        conn,
        UNITS_DOMAIN,
        "Unités de mesure",
        "unit_set",
        "tenant_managed",
        1,
        DomainAudience::Company,
    )
    .await?;
    ensure_published_set(conn, UNITS_DOMAIN).await?;
    for (i, seed) in UNIT_SEEDS.iter().enumerate() {
        let mut metadata = serde_json::json!({
            "origin": "system",
            "label_en": seed.label_en,
            "symbol": seed.symbol,
        });
        if seed.rec20.is_some() {
            metadata["standard"] = "UN/CEFACT Rec 20".into();
        }
        insert_value(
            conn,
            UNITS_DOMAIN,
            &NewValue {
                code: seed.code,
                label: seed.label_fr,
                sort_order: i as i64 + 1,
                semantic_tag: Some(seed.dimension.as_str()),
                external_code: seed.rec20,
                is_active: true,
                metadata,
            },
        )
        .await?;
        // A standard unit is protected: what the program depends on (active, dimension, code of
        // the standard) is re-applied if a replica changed it.
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE reference_values SET is_active = 1, semantic_tag = ?, external_code = ?               WHERE UPPER(TRIM(code)) = ?                 AND set_id IN (SELECT rs.id FROM reference_sets rs JOIN reference_domains d ON d.id = rs.domain_id                                 WHERE UPPER(TRIM(d.code)) = 'MEASURE.UNITS' AND rs.status IN ('published', 'draft', 'validated'))                 AND json_valid(metadata_json) AND json_extract(metadata_json, '$.origin') = 'system'                 AND (is_active <> 1 OR semantic_tag IS NOT ? OR external_code IS NOT ?)",
            [
                seed.dimension.as_str().into(),
                seed.rec20.map(str::to_string).into(),
                seed.code.into(),
                seed.dimension.as_str().into(),
                seed.rec20.map(str::to_string).into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

// ─── Reads ──────────────────────────────────────────────────────────────────────

/// SQL expression of the symbol of the unit row aliased `alias` (`reference_values`): the
/// `metadata_json.symbol`, else the code. NULL when the row is absent (LEFT JOIN).
pub fn symbol_sql(alias: &str) -> String {
    format!(
        "CASE WHEN {alias}.id IS NULL THEN NULL ELSE COALESCE(NULLIF(TRIM(CASE WHEN json_valid({alias}.metadata_json) \
         THEN json_extract({alias}.metadata_json, '$.symbol') END), ''), {alias}.code) END"
    )
}

/// SQL literal (usable in `const` queries): the symbol of the unit whose id is the SQL expression
/// `$id`, NULL when it is NULL. Same rule as [`symbol_sql`].
macro_rules! unit_symbol_of {
    ($id:literal) => {
        concat!(
            "(SELECT COALESCE(NULLIF(TRIM(CASE WHEN json_valid(mu_s.metadata_json) \
             THEN json_extract(mu_s.metadata_json, '$.symbol') END), ''), mu_s.code) \
             FROM reference_values mu_s WHERE mu_s.id = ",
            $id,
            ")"
        )
    };
}
pub(crate) use unit_symbol_of;

/// One unit as the modules see it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnitOfMeasure {
    /// `reference_values.id`, what the linking columns hold.
    pub id: i64,
    pub code: String,
    /// French label.
    pub label: String,
    pub label_en: Option<String>,
    pub symbol: String,
    pub dimension: Option<UnitDimension>,
    /// UN/CEFACT Recommendation 20 common code.
    pub external_code: Option<String>,
    pub is_active: bool,
    /// Created by migration 201 for a code or a text Maintafox did not know.
    pub review_required: bool,
    /// The unit belongs to the published version of the list.
    pub is_published: bool,
    /// Standard unit shipped by Maintafox (protected); otherwise a plant unit.
    pub is_standard: bool,
    /// Plant unit: its base standard unit (`reference_values.id`, same set) and the factor
    /// `1 unit = base_factor × base unit`.
    pub base_unit_id: Option<i64>,
    pub base_factor: Option<f64>,
    pub set_id: i64,
}

const UNIT_SELECT: &str = "SELECT u.id, u.code, u.label, u.semantic_tag, u.external_code, u.is_active, \
            u.metadata_json, u.parent_id, u.set_id, rs.status AS set_status \
       FROM reference_values u \
       JOIN reference_sets rs ON rs.id = u.set_id \
       JOIN reference_domains d ON d.id = rs.domain_id AND UPPER(TRIM(d.code)) = 'MEASURE.UNITS'";

fn meta_of(raw: Option<&str>) -> serde_json::Value {
    raw.and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn meta_text(meta: &serde_json::Value, key: &str) -> Option<String> {
    meta.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `metadata_json.base_factor` when it is a positive finite number.
fn base_factor_of(meta: &serde_json::Value) -> Option<f64> {
    meta.get("base_factor")
        .and_then(serde_json::Value::as_f64)
        .filter(|f| f.is_finite() && *f > 0.0)
}

fn map_unit(row: &QueryResult) -> AppResult<UnitOfMeasure> {
    let code: String = row.try_get("", "code").map_err(decode)?;
    let metadata: Option<String> = row.try_get("", "metadata_json").map_err(decode)?;
    let meta = meta_of(metadata.as_deref());
    let tag: Option<String> = row.try_get("", "semantic_tag").map_err(decode)?;
    let status: String = row.try_get("", "set_status").map_err(decode)?;
    Ok(UnitOfMeasure {
        id: row.try_get("", "id").map_err(decode)?,
        label: row.try_get("", "label").map_err(decode)?,
        label_en: meta_text(&meta, "label_en"),
        symbol: meta_text(&meta, "symbol").unwrap_or_else(|| code.clone()),
        dimension: tag.as_deref().and_then(UnitDimension::parse),
        external_code: row.try_get("", "external_code").map_err(decode)?,
        is_active: row.try_get::<i64>("", "is_active").map_err(decode)? != 0,
        review_required: meta.get("review_required").and_then(serde_json::Value::as_bool) == Some(true),
        is_published: status == "published",
        is_standard: meta.get("origin").and_then(serde_json::Value::as_str) == Some("system"),
        base_unit_id: row.try_get("", "parent_id").map_err(decode)?,
        base_factor: base_factor_of(&meta),
        set_id: row.try_get("", "set_id").map_err(decode)?,
        code,
    })
}

/// The unit `id` (any version of the list), `None` when the id is not a unit.
pub async fn unit_by_id(conn: &impl ConnectionTrait, id: i64) -> AppResult<Option<UnitOfMeasure>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("{UNIT_SELECT} WHERE u.id = ?"),
        [id.into()],
    ))
    .await?
    .as_ref()
    .map(map_unit)
    .transpose()
}

/// The active unit `code` of the published list (ids of the starter units, tests, seeders).
pub async fn unit_id_by_code(conn: &impl ConnectionTrait, code: &str) -> AppResult<i64> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!(
            "{UNIT_SELECT} WHERE rs.status = 'published' AND UPPER(TRIM(u.code)) = UPPER(TRIM(?)) \
             ORDER BY rs.version_no DESC LIMIT 1"
        ),
        [code.into()],
    ))
    .await?
    .map(|r| r.try_get::<i64>("", "id"))
    .transpose()
    .map_err(decode)?
    .ok_or_else(|| AppError::NotFound {
        entity: "unit_of_measure".into(),
        id: code.to_string(),
    })
}

/// Units of the published list still flagged "to review" (created by migration 201 for a code
/// or a text Maintafox did not know, not yet completed by an administrator), by code.
pub async fn list_units_to_review(conn: &impl ConnectionTrait) -> AppResult<Vec<UnitOfMeasure>> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "{UNIT_SELECT} WHERE rs.status = 'published' AND json_valid(u.metadata_json)                    AND json_extract(u.metadata_json, '$.review_required') = 1 ORDER BY u.code"
            ),
        ))
        .await?;
    rows.iter().map(map_unit).collect()
}

fn not_active(field: &str, detail: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{GATE_UNIT_NOT_ACTIVE}: {field} — {detail}")])
}

/// The unit a form sends in `field`. `require_active`: it must be an active unit of the
/// published list (a new link); otherwise any unit of the list is accepted (a record keeping the
/// unit it already had, even if the plant retired it since).
pub async fn ensure_unit(
    conn: &impl ConnectionTrait,
    id: i64,
    field: &str,
    require_active: bool,
) -> AppResult<UnitOfMeasure> {
    let Some(unit) = unit_by_id(conn, id).await? else {
        return Err(not_active(field, &format!("{id} n'est pas une unité de mesure.")));
    };
    if require_active && !(unit.is_active && unit.is_published) {
        return Err(not_active(
            field,
            &format!(
                "l'unité '{}' n'est plus active dans les données de référence.",
                unit.code
            ),
        ));
    }
    Ok(unit)
}

/// Refuses a conversion between two units that measure different things. A unit without a
/// dimension (migrated, to review) is refused only when `require_known` (a new conversion).
pub fn assert_convertible(from: &UnitOfMeasure, to: &UnitOfMeasure, require_known: bool) -> AppResult<()> {
    match (from.dimension, to.dimension) {
        (Some(a), Some(b)) if !dimensions_compatible(a, b) => Err(AppError::ValidationFailed(vec![format!(
            "{GATE_UNIT_DIMENSION_MISMATCH}: '{}' ({}) et '{}' ({}) ne mesurent pas la même grandeur.",
            from.code,
            a.as_str(),
            to.code,
            b.as_str()
        )])),
        (Some(_), Some(_)) => Ok(()),
        _ if require_known => {
            let missing = if from.dimension.is_none() { from } else { to };
            Err(AppError::ValidationFailed(vec![format!(
                "{GATE_UNIT_DIMENSION_MISSING}: la grandeur de l'unité '{}' n'est pas renseignée.",
                missing.code
            )]))
        }
        _ => Ok(()),
    }
}

/// Checks the dimension tag of a unit value (`values::checked_semantic_tag`): empty means no
/// dimension (a migrated unit to review); otherwise an active value of `MEASURE.DIMENSIONS`.
pub(crate) async fn checked_dimension_tag(conn: &impl ConnectionTrait, tag: Option<&str>) -> AppResult<Option<String>> {
    let Some(tag) = tag.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let known = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM reference_values rv \
               JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published' \
               JOIN reference_domains d ON d.id = rs.domain_id \
              WHERE UPPER(TRIM(d.code)) = 'MEASURE.DIMENSIONS' AND rv.code = ? AND rv.is_active = 1",
            [tag.to_ascii_lowercase().into()],
        ))
        .await?
        .is_some();
    if !known || UnitDimension::parse(tag).is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "{GATE_UNIT_DIMENSION_INVALID}: '{tag}' n'est pas une grandeur physique connue."
        )]));
    }
    Ok(Some(tag.to_ascii_lowercase()))
}

fn refuse(gate: &str, detail: String) -> AppError {
    AppError::ValidationFailed(vec![format!("{gate}: {detail}")])
}

/// Checks a plant unit written by a user (create, update, import, draft validation): its base is
/// an active standard unit of the same set with a dimension, its own dimension (`tag`, optional)
/// is the base's, and `metadata_json.base_factor` is a positive number. Returns the dimension to
/// store (the base unit's).
pub(crate) async fn checked_plant_unit(
    conn: &impl ConnectionTrait,
    set_id: i64,
    code: &str,
    base_unit_id: Option<i64>,
    tag: Option<&str>,
    metadata_json: Option<&str>,
) -> AppResult<String> {
    let Some(base_id) = base_unit_id else {
        return Err(refuse(
            GATE_UNIT_BASE_REQUIRED,
            format!("l'unité '{code}' doit indiquer son unité standard de base et le facteur."),
        ));
    };
    let base = unit_by_id(conn, base_id).await?;
    let base = match base {
        Some(b) if b.set_id == set_id && b.is_standard && b.is_active => b,
        _ => {
            return Err(refuse(
                GATE_UNIT_BASE_INVALID,
                format!("la base de l'unité '{code}' doit être une unité standard active de la même version."),
            ))
        }
    };
    let Some(dimension) = base.dimension else {
        return Err(refuse(
            GATE_UNIT_BASE_INVALID,
            format!("l'unité de base '{}' n'a pas de grandeur physique.", base.code),
        ));
    };
    if let Some(tag) = tag.map(str::trim).filter(|t| !t.is_empty()) {
        if UnitDimension::parse(tag) != Some(dimension) {
            return Err(refuse(
                GATE_UNIT_BASE_INVALID,
                format!(
                    "l'unité '{code}' ({tag}) et sa base '{}' ({}) ne mesurent pas la même grandeur.",
                    base.code,
                    dimension.as_str()
                ),
            ));
        }
    }
    let meta = meta_of(metadata_json);
    match meta.get("base_factor") {
        None | Some(serde_json::Value::Null) => Err(refuse(
            GATE_UNIT_BASE_REQUIRED,
            format!("l'unité '{code}' doit indiquer le facteur vers '{}'.", base.code),
        )),
        Some(_) if base_factor_of(&meta).is_none() => Err(refuse(
            GATE_UNIT_BASE_INVALID,
            format!("le facteur de l'unité '{code}' doit être un nombre strictement positif."),
        )),
        Some(_) => Ok(dimension.as_str().to_string()),
    }
}

/// Checks the base a plant unit is moved under (`values::move_value_parent`): an active standard
/// unit of the same set, of the unit's dimension when the unit already has one.
pub(crate) async fn checked_base_move(
    conn: &impl ConnectionTrait,
    set_id: i64,
    code: &str,
    tag: Option<&str>,
    base_unit_id: Option<i64>,
) -> AppResult<()> {
    let Some(base_id) = base_unit_id else {
        return Err(refuse(
            GATE_UNIT_BASE_REQUIRED,
            format!("l'unité '{code}' doit garder une unité standard de base."),
        ));
    };
    match unit_by_id(conn, base_id).await? {
        Some(b) if b.set_id == set_id && b.is_standard && b.is_active && b.dimension.is_some() => {
            let own = tag.and_then(UnitDimension::parse);
            if own.is_some() && own != b.dimension {
                return Err(refuse(
                    GATE_UNIT_BASE_INVALID,
                    format!("l'unité '{code}' et '{}' ne mesurent pas la même grandeur.", b.code),
                ));
            }
            Ok(())
        }
        _ => Err(refuse(
            GATE_UNIT_BASE_INVALID,
            format!("la base de l'unité '{code}' doit être une unité standard active de la même version."),
        )),
    }
}

/// Metadata stored for a plant unit after a user edit: the review flag of a migrated unit is
/// cleared (its base and factor have just been checked).
pub(crate) fn reviewed_metadata(metadata_json: Option<&str>) -> Option<String> {
    let mut meta = meta_of(metadata_json);
    let removed = meta
        .as_object_mut()
        .is_some_and(|obj| obj.remove("review_required").is_some());
    if removed {
        Some(meta.to_string())
    } else {
        metadata_json.map(str::to_string)
    }
}

/// Problems of an imported unit row (CSV): a plant unit names its base standard unit
/// (`parent_code`) and its factor (`metadata_json.base_factor` > 0); the "to review" flag is set
/// by migration 201 only, never imported. The base and dimension are checked again on apply,
/// against the target version.
pub(crate) fn import_row_problems(parent_code: Option<&str>, metadata_json: Option<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    let meta = meta_of(metadata_json);
    if metadata_json.is_some_and(|m| !m.trim().is_empty()) && !meta.is_object() {
        problems.push("GATE_UNIT_IMPORT_METADATA_INVALID: metadata_json n'est pas un objet JSON.".to_string());
    }
    if meta.get("review_required").and_then(serde_json::Value::as_bool) == Some(true) {
        problems.push("GATE_UNIT_IMPORT_REVIEW_FLAG: le statut « à revoir » ne s'importe pas.".to_string());
    }
    if parent_code.map(str::trim).filter(|c| !c.is_empty()).is_none() {
        problems.push(format!(
            "{GATE_UNIT_BASE_REQUIRED}: unité standard de base manquante (colonne parent_code)."
        ));
    }
    if base_factor_of(&meta).is_none() {
        problems.push(format!(
            "{GATE_UNIT_BASE_REQUIRED}: facteur vers l'unité de base manquant ou invalide (metadata_json.base_factor > 0)."
        ));
    }
    problems
}

pub(crate) fn is_units_domain(domain_code: &str) -> bool {
    domain_code.trim().eq_ignore_ascii_case(UNITS_DOMAIN)
}

// ─── Mapping texts and legacy codes onto the list (migration 201) ───────────────────────────

/// Key used to match a text on a unit: case, accents, spaces, the superscripts ² / ³ and the
/// ordinal sign `º` written for `°` do not matter.
pub fn normalize_unit_text(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'º' { '°' } else { strip_accent(c) })
        .collect()
}

/// Spellings found in free unit fields, FR and EN, and the standard unit they name (its code).
/// Matched after [`normalize_unit_text`] (so `Heures`, `HEURES` and `heures ` are one entry).
/// A text that is not listed here and is not the code, symbol or label of a unit (`tr`, `pc/h`…)
/// is not guessed: it becomes a unit to review.
pub const UNIT_SYNONYMS: &[(&str, &str)] = &[
    // time
    ("h", "H"),
    ("hr", "H"),
    ("hrs", "H"),
    ("heure", "H"),
    ("heures", "H"),
    ("hour", "H"),
    ("hours", "H"),
    ("min", "MIN"),
    ("mn", "MIN"),
    ("mins", "MIN"),
    ("minute", "MIN"),
    ("minutes", "MIN"),
    ("s", "S"),
    ("sec", "S"),
    ("seconde", "S"),
    ("secondes", "S"),
    ("second", "S"),
    ("seconds", "S"),
    ("j", "D"),
    ("jour", "D"),
    ("jours", "D"),
    ("day", "D"),
    ("days", "D"),
    // rotational speed, frequency
    ("tr/min", "RPM"),
    ("trs/min", "RPM"),
    ("t/min", "RPM"),
    ("tour/min", "RPM"),
    ("tours/min", "RPM"),
    ("rpm", "RPM"),
    ("rev/min", "RPM"),
    ("hz", "HZ"),
    ("hertz", "HZ"),
    // temperature
    ("°c", "DEGC"),
    ("degc", "DEGC"),
    ("deg.c", "DEGC"),
    ("degrec", "DEGC"),
    ("degrescelsius", "DEGC"),
    ("degrecelsius", "DEGC"),
    ("celsius", "DEGC"),
    // volume, flow
    ("l", "L"),
    ("lt", "L"),
    ("ltr", "L"),
    ("litre", "L"),
    ("litres", "L"),
    ("liter", "L"),
    ("liters", "L"),
    ("m3", "M3"),
    ("metrecube", "M3"),
    ("metrescubes", "M3"),
    ("cubicmetre", "M3"),
    ("cubicmeter", "M3"),
    ("m3/h", "M3H"),
    ("l/min", "LMIN"),
    ("lpm", "LMIN"),
    // length, area, speed
    ("m", "M"),
    ("metre", "M"),
    ("metres", "M"),
    ("meter", "M"),
    ("meters", "M"),
    ("mm", "MM"),
    ("millimetre", "MM"),
    ("millimetres", "MM"),
    ("millimeter", "MM"),
    ("millimeters", "MM"),
    ("km", "KM"),
    ("kilometre", "KM"),
    ("kilometres", "KM"),
    ("kilometer", "KM"),
    ("kilometers", "KM"),
    ("m2", "M2"),
    ("metrecarre", "M2"),
    ("metrescarres", "M2"),
    ("squaremetre", "M2"),
    ("squaremeter", "M2"),
    ("km/h", "KMH"),
    ("kmh", "KMH"),
    // mass
    ("kg", "KG"),
    ("kgs", "KG"),
    ("kilo", "KG"),
    ("kilos", "KG"),
    ("kilogramme", "KG"),
    ("kilogrammes", "KG"),
    ("kilogram", "KG"),
    ("kilograms", "KG"),
    ("g", "G"),
    ("gr", "G"),
    ("gramme", "G"),
    ("grammes", "G"),
    ("gram", "G"),
    ("grams", "G"),
    ("t", "T"),
    ("tonne", "T"),
    ("tonnes", "T"),
    // energy, power, pressure, electricity
    ("kwh", "KWH"),
    ("kw.h", "KWH"),
    ("kilowattheure", "KWH"),
    ("kilowattheures", "KWH"),
    ("kw", "KW"),
    ("kilowatt", "KW"),
    ("kilowatts", "KW"),
    ("bar", "BAR"),
    ("bars", "BAR"),
    ("amp", "A"),
    ("amps", "A"),
    ("ampere", "A"),
    ("amperes", "A"),
    ("v", "V"),
    ("volt", "V"),
    ("volts", "V"),
    // counts
    ("u", "U"),
    ("pc", "U"),
    ("pcs", "U"),
    ("pce", "U"),
    ("piece", "U"),
    ("pieces", "U"),
    ("unite", "U"),
    ("unites", "U"),
    ("unit", "U"),
    ("units", "U"),
    ("ea", "U"),
    ("paire", "PAIRE"),
    ("paires", "PAIRE"),
    ("pair", "PAIRE"),
    ("pairs", "PAIRE"),
    ("cycle", "CYCLE"),
    ("cycles", "CYCLE"),
    ("cyc", "CYCLE"),
];

/// The units of the published list, by every key a text may use (code, symbol, FR / EN label).
struct UnitMatcher {
    by_key: HashMap<String, i64>,
}

impl UnitMatcher {
    async fn load(conn: &impl ConnectionTrait) -> AppResult<Self> {
        let rows = conn
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                format!("{UNIT_SELECT} WHERE rs.status = 'published' ORDER BY rs.version_no DESC, u.sort_order, u.id"),
            ))
            .await?;
        let mut by_key = HashMap::new();
        // The code wins over a symbol or a label of another unit: codes first.
        let units: Vec<UnitOfMeasure> = rows.iter().map(map_unit).collect::<AppResult<_>>()?;
        for u in &units {
            by_key.entry(normalize_unit_text(&u.code)).or_insert(u.id);
        }
        for u in &units {
            for key in [Some(&u.symbol), Some(&u.label), u.label_en.as_ref()]
                .into_iter()
                .flatten()
            {
                by_key.entry(normalize_unit_text(key)).or_insert(u.id);
            }
        }
        // Documented spellings of the standard units.
        for (spelling, code) in UNIT_SYNONYMS {
            if let Some(u) = units.iter().find(|u| u.is_standard && u.code == *code) {
                by_key.entry(normalize_unit_text(spelling)).or_insert(u.id);
            }
        }
        Ok(Self { by_key })
    }

    /// The unit a text names (code, symbol, label or documented spelling); `None` otherwise.
    fn find(&self, text: &str) -> Option<i64> {
        let key = normalize_unit_text(text);
        if key.is_empty() {
            return None;
        }
        self.by_key.get(&key).copied()
    }

    fn remember(&mut self, text: &str, id: i64) {
        self.by_key.entry(normalize_unit_text(text)).or_insert(id);
    }
}

fn strip_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        '²' => '2',
        '³' => '3',
        other => other,
    }
}

/// A value code for a text (`m3/h` → `M3_H`, `°C` → `C`, `heures` → `HEURES`): uppercase ASCII
/// letters, digits and underscores, starting with a letter, at most 60 characters.
pub fn code_for_text(text: &str) -> String {
    let mut code = String::new();
    for c in text.trim().chars().flat_map(char::to_lowercase).map(strip_accent) {
        if c.is_ascii_alphanumeric() {
            code.push(c.to_ascii_uppercase());
        } else if !code.ends_with('_') && !code.is_empty() {
            code.push('_');
        }
    }
    let mut code = code.trim_end_matches('_').to_string();
    if !code.starts_with(|c: char| c.is_ascii_uppercase()) {
        code.insert_str(0, "U_");
    }
    code.truncate(60);
    code.trim_end_matches('_').to_string()
}

async fn code_taken(conn: &impl ConnectionTrait, code: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{UNIT_SELECT} WHERE UPPER(TRIM(u.code)) = UPPER(TRIM(?)) LIMIT 1"),
            [code.into()],
        ))
        .await?
        .is_some())
}

/// A unit for a code or a text the list does not know, flagged for review. Its code is derived
/// from `code_hint` (made unique with `_2`, `_3`…), so every device running the migration on the
/// same data creates the same unit with the same identity.
#[derive(Debug, Clone)]
pub(crate) struct ReviewUnit<'a> {
    pub code_hint: &'a str,
    pub label: &'a str,
    pub label_en: Option<&'a str>,
    pub symbol: &'a str,
    pub is_active: bool,
    /// Where the unit came from (`lookup:inventory.unit_of_measure`, `equipment_meters.unit`…).
    pub source: &'a str,
    /// Code of the retired lookup value the unit replaces (its sync identity derives from it,
    /// so a row of an older device naming that value is linked to this unit).
    pub legacy_code: Option<&'a str>,
}

pub(crate) async fn create_review_unit(conn: &impl ConnectionTrait, unit: &ReviewUnit<'_>) -> AppResult<i64> {
    let base = code_for_text(unit.code_hint);
    let mut code = base.clone();
    let mut n = 2;
    while code_taken(conn, &code).await? {
        code = format!("{base}_{n}");
        n += 1;
    }
    let sort_order: i64 = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(u.sort_order), 0) + 1 AS next FROM reference_values u \
               JOIN reference_sets rs ON rs.id = u.set_id \
               JOIN reference_domains d ON d.id = rs.domain_id AND UPPER(TRIM(d.code)) = 'MEASURE.UNITS'"
                .to_string(),
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "next"))
        .transpose()
        .map_err(decode)?
        .unwrap_or(1);
    let mut metadata = serde_json::json!({
        "origin": "migrated",
        "label_en": unit.label_en.unwrap_or(unit.label),
        "symbol": unit.symbol.trim(),
        "review_required": true,
        "source": unit.source,
    });
    if let Some(legacy) = unit.legacy_code {
        metadata["legacy_code"] = legacy.into();
    }
    insert_value(
        conn,
        UNITS_DOMAIN,
        &NewValue {
            code: &code,
            label: unit.label.trim(),
            sort_order,
            semantic_tag: None,
            external_code: None,
            is_active: unit.is_active,
            metadata,
        },
    )
    .await?;
    let id = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{UNIT_SELECT} WHERE u.code = ? ORDER BY rs.version_no DESC LIMIT 1"),
            [code.clone().into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "id"))
        .transpose()
        .map_err(decode)?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("MEASURE.UNITS value '{code}' missing after insert")))?;
    tracing::warn!(code = %code, source = unit.source, "unit created for an unknown unit, to review in reference data");
    Ok(id)
}

/// Keeps a text exactly as it was typed as a `legacy` alias of the unit it was mapped onto
/// (nothing of the original spelling is lost when the text column is dropped). Skipped when the
/// text already is the code, symbol or label of the unit.
async fn keep_text_as_alias(conn: &impl ConnectionTrait, unit_id: i64, text: &str) -> AppResult<()> {
    let text = text.trim();
    let Some(unit) = unit_by_id(conn, unit_id).await? else {
        return Ok(());
    };
    if [unit.code.as_str(), unit.symbol.as_str(), unit.label.as_str()].contains(&text)
        || unit.label_en.as_deref() == Some(text)
    {
        return Ok(());
    }
    let sync_id = crate::sync::identity::seeded_entity_sync_id(
        "reference_aliases",
        &format!("{UNITS_DOMAIN}:{}:legacy:fr:{text}", unit.code),
    );
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR IGNORE INTO reference_aliases \
         (entity_sync_id, reference_value_id, alias_label, locale, alias_type, is_preferred, created_at) \
         SELECT ?, ?, ?, 'fr', 'legacy', 0, strftime('%Y-%m-%dT%H:%M:%SZ','now') \
          WHERE NOT EXISTS (SELECT 1 FROM reference_aliases \
                             WHERE reference_value_id = ? AND locale = 'fr' AND alias_type = 'legacy' AND alias_label = ?)",
        [
            sync_id.into(),
            unit_id.into(),
            text.into(),
            unit_id.into(),
            text.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Maps texts typed in a free unit field onto the list, creating a unit flagged for review for a
/// text no unit matches. Returns `text → unit id` for every non-empty text.
pub(crate) async fn map_unit_texts(
    conn: &impl ConnectionTrait,
    texts: &[String],
    source: &str,
) -> AppResult<HashMap<String, i64>> {
    let mut matcher = UnitMatcher::load(conn).await?;
    let mut out = HashMap::new();
    for text in texts {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let id = match matcher.find(trimmed) {
            Some(id) => id,
            None => {
                let id = create_review_unit(
                    conn,
                    &ReviewUnit {
                        code_hint: trimmed,
                        label: trimmed,
                        label_en: None,
                        symbol: trimmed,
                        is_active: true,
                        source,
                        legacy_code: None,
                    },
                )
                .await?;
                matcher.remember(trimmed, id);
                id
            }
        };
        keep_text_as_alias(conn, id, trimmed).await?;
        out.insert(text.clone(), id);
    }
    Ok(out)
}

/// Moves the links of `columns` from the `old` to the `new` unit of each pair. One `UPDATE` per
/// table sets all its unit columns together, so a row is moved once even when old and new ids
/// overlap, and a table rule over two of its columns (`unit_conversions`: from ≠ to) is checked on
/// the moved row, never on a half-moved one. `skip_current_units`: a value that already is a unit
/// of the list is left alone (the startup pass, where the old ids come from a replayed lookup).
async fn remap_unit_columns(
    conn: &impl ConnectionTrait,
    columns: &[(&str, &str)],
    pairs: &[(i64, i64)],
    skip_current_units: bool,
) -> AppResult<()> {
    if pairs.is_empty() || columns.is_empty() {
        return Ok(());
    }
    conn.execute_unprepared(
        "CREATE TEMP TABLE IF NOT EXISTS unit_remap (old_id INTEGER PRIMARY KEY, new_id INTEGER NOT NULL)",
    )
    .await?;
    conn.execute_unprepared("DELETE FROM unit_remap").await?;
    for (old_id, new_id) in pairs {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR REPLACE INTO unit_remap (old_id, new_id) VALUES (?, ?)",
            [(*old_id).into(), (*new_id).into()],
        ))
        .await?;
    }
    let current_units = "SELECT u.id FROM reference_values u JOIN reference_sets rs ON rs.id = u.set_id \
         JOIN reference_domains d ON d.id = rs.domain_id WHERE UPPER(TRIM(d.code)) = 'MEASURE.UNITS'";
    let mut tables: Vec<&str> = columns.iter().map(|(t, _)| *t).collect();
    tables.sort_unstable();
    tables.dedup();
    for table in tables {
        let t = quote(table);
        let cols: Vec<String> = columns
            .iter()
            .filter(|(tb, _)| *tb == table)
            .map(|(_, c)| quote(c))
            .collect();
        let movable = |c: &str| {
            if skip_current_units {
                format!("({c} IN (SELECT old_id FROM unit_remap) AND {c} NOT IN ({current_units}))")
            } else {
                format!("{c} IN (SELECT old_id FROM unit_remap)")
            }
        };
        let sets: Vec<String> = cols
            .iter()
            .map(|c| {
                format!(
                    "{c} = CASE WHEN {m} THEN (SELECT r.new_id FROM unit_remap r WHERE r.old_id = {t}.{c}) ELSE {c} END",
                    m = movable(c)
                )
            })
            .collect();
        let any: Vec<String> = cols.iter().map(|c| movable(c)).collect();
        conn.execute_unprepared(&format!(
            "UPDATE {t} SET {} WHERE {}",
            sets.join(", "),
            any.join(" OR ")
        ))
        .await?;
    }
    conn.execute_unprepared("DROP TABLE IF EXISTS unit_remap").await?;
    Ok(())
}

/// Moves the unit ids inside the checkpoint packages of the checklist versions
/// (`[*].measurement_unit_id`) from the `old` to the `new` unit of each pair.
pub(crate) async fn remap_package_unit_ids(conn: &impl ConnectionTrait, pairs: &[(i64, i64)]) -> AppResult<u64> {
    if pairs.is_empty() || !table_has_column(conn, "inspection_template_versions", "checkpoint_package_json").await? {
        return Ok(0);
    }
    let map: HashMap<i64, i64> = pairs.iter().copied().collect();
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, checkpoint_package_json FROM inspection_template_versions \
              WHERE checkpoint_package_json LIKE '%measurement_unit_id%' ORDER BY id"
                .to_string(),
        ))
        .await?;
    let mut changed = 0;
    for r in rows {
        let id: i64 = r.try_get("", "id").map_err(decode)?;
        let raw: String = r.try_get("", "checkpoint_package_json").map_err(decode)?;
        let Ok(mut package) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let mut touched = false;
        if let Some(entries) = package.as_array_mut() {
            for entry in entries.iter_mut() {
                let Some(old) = entry.get("measurement_unit_id").and_then(serde_json::Value::as_i64) else {
                    continue;
                };
                if let Some(new) = map.get(&old) {
                    entry["measurement_unit_id"] = (*new).into();
                    touched = true;
                }
            }
        }
        if touched {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE inspection_template_versions SET checkpoint_package_json = ? WHERE id = ?",
                [package.to_string().into(), id.into()],
            ))
            .await?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// Columns of current records that link a unit of `MEASURE.UNITS`. History (stock movements,
/// receipt lines) keeps the unit it was entered in and is not listed: a merged unit stays as an
/// inactive value, so those links still resolve.
pub const UNIT_RECORD_COLUMNS: &[(&str, &str)] = &[
    ("articles", "unit_value_id"),
    ("inventory_supplier_prices", "price_unit_value_id"),
    ("unit_conversions", "from_unit_value_id"),
    ("unit_conversions", "to_unit_value_id"),
    ("equipment_meters", "unit_value_id"),
    ("inspection_checkpoints", "measurement_unit_id"),
];

/// A unit merged into another (`reference::migrations`, merge / usage migration) must mean the same
/// quantity: a unit to review (no factor yet) may be merged into any unit — that is how its meaning
/// is stated; a unit with a factor only into a unit with the same base and factor. Otherwise
/// stock and readings would silently change their meaning.
pub(crate) async fn assert_merge_keeps_quantities(
    conn: &impl ConnectionTrait,
    from_id: i64,
    to_id: i64,
) -> AppResult<()> {
    let (Some(from), Some(to)) = (unit_by_id(conn, from_id).await?, unit_by_id(conn, to_id).await?) else {
        return Ok(());
    };
    // `(base unit, factor)`: a standard unit is its own base.
    let reduce = |u: &UnitOfMeasure| {
        if u.is_standard {
            Some((u.id, 1.0))
        } else {
            u.base_unit_id.zip(u.base_factor)
        }
    };
    let Some((from_base, f_from)) = reduce(&from) else {
        // A unit to review: the merge states what it means.
        return Ok(());
    };
    let same = reduce(&to)
        .is_some_and(|(to_base, f_to)| to_base == from_base && (f_to - f_from).abs() <= 1e-9 * f_from.abs().max(1.0));
    if same {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "{GATE_UNIT_MERGE_CHANGES_QUANTITIES}: '{}' et '{}' ne représentent pas la même quantité ;              la fusion changerait les stocks et les relevés qui les utilisent.",
            from.code, to.code
        )]))
    }
}

/// Moves every current record from unit `from_id` to unit `to_id` (merge of a unit, usage
/// migration). A conversion that would convert the unit into itself is deactivated first.
/// Returns the number of rows moved.
pub(crate) async fn remap_unit_value(conn: &impl ConnectionTrait, from_id: i64, to_id: i64) -> AppResult<i64> {
    let mut columns: Vec<(&str, &str)> = Vec::new();
    for (table, column) in UNIT_RECORD_COLUMNS {
        if table_has_column(conn, table, column).await? {
            columns.push((*table, *column));
        }
    }
    let mut moved: i64 = 0;
    for (table, column) in &columns {
        moved += conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT COUNT(*) AS n FROM {} WHERE {} = ?", quote(table), quote(column)),
                [from_id.into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "n"))
            .transpose()
            .map_err(decode)?
            .unwrap_or(0);
    }
    if table_has_column(conn, "unit_conversions", "from_unit_value_id").await? {
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE unit_conversions SET is_active = 0, row_version = row_version + 1, updated_at = ? \
              WHERE is_active = 1 AND ((from_unit_value_id = ? AND to_unit_value_id = ?) \
                 OR (from_unit_value_id = ? AND to_unit_value_id = ?))",
            [
                chrono::Utc::now().to_rfc3339().into(),
                from_id.into(),
                to_id.into(),
                to_id.into(),
                from_id.into(),
            ],
        ))
        .await?;
        // A conversion between the two units cannot be moved onto one unit (from ≠ to): it stays
        // on the retired unit, deactivated above.
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE OR IGNORE unit_conversions \
                SET from_unit_value_id = CASE WHEN from_unit_value_id = ? THEN ? ELSE from_unit_value_id END, \
                    to_unit_value_id = CASE WHEN to_unit_value_id = ? THEN ? ELSE to_unit_value_id END \
              WHERE (from_unit_value_id = ? OR to_unit_value_id = ?) \
                AND NOT ((from_unit_value_id = ? AND to_unit_value_id = ?) OR (from_unit_value_id = ? AND to_unit_value_id = ?))",
            [
                from_id.into(),
                to_id.into(),
                from_id.into(),
                to_id.into(),
                from_id.into(),
                from_id.into(),
                from_id.into(),
                to_id.into(),
                to_id.into(),
                from_id.into(),
            ],
        ))
        .await?;
    }
    let others: Vec<(&str, &str)> = columns.into_iter().filter(|(t, _)| *t != "unit_conversions").collect();
    remap_unit_columns(conn, &others, &[(from_id, to_id)], false).await?;
    remap_package_unit_ids(conn, &[(from_id, to_id)]).await?;
    Ok(moved)
}

// ─── Rows of older devices (sync) ──────────────────────────────────────────────────────────

/// Columns an inbound row may fill with a unit: the record columns, plus the units entered on
/// receipts and stock movements.
pub fn is_unit_ref_column(table: &str, column: &str) -> bool {
    UNIT_RECORD_COLUMNS
        .iter()
        .chain(
            [
                ("goods_receipt_lines", "entered_unit_value_id"),
                ("inventory_transactions", "entered_unit_value_id"),
            ]
            .iter(),
        )
        .any(|(t, c)| *t == table && *c == column)
}

/// Sync identity a value of the retired lookup had (seeded on every device from its code,
/// `db::seeder::seed_value`).
pub fn legacy_lookup_sync_id(code: &str) -> String {
    crate::sync::identity::seeded_entity_sync_id("lookup_values", &format!("{LEGACY_UNIT_LOOKUP_DOMAIN}:{code}"))
}

/// The unit a reference of an older device names when it is the sync id of a value of the retired
/// lookup: the unit with that code (or that `legacy_code`), else, when the lookup row itself came
/// back with the history, the unit its code maps to (created to review when unknown).
pub async fn unit_for_legacy_sync_id(conn: &impl ConnectionTrait, sync_id: &str) -> AppResult<Option<i64>> {
    let rows = conn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{UNIT_SELECT} WHERE rs.status = 'published' ORDER BY rs.version_no DESC, u.id"),
        ))
        .await?;
    for r in &rows {
        let id: i64 = r.try_get("", "id").map_err(decode)?;
        let code: String = r.try_get("", "code").map_err(decode)?;
        let meta = meta_of(
            r.try_get::<Option<String>>("", "metadata_json")
                .map_err(decode)?
                .as_deref(),
        );
        let legacy = meta_text(&meta, "legacy_code");
        if legacy_lookup_sync_id(&code) == sync_id || legacy.is_some_and(|l| legacy_lookup_sync_id(&l) == sync_id) {
            return Ok(Some(id));
        }
    }
    let replayed = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.code, lv.label, lv.fr_label, lv.en_label FROM lookup_values lv \
               JOIN lookup_domains ld ON ld.id = lv.domain_id \
              WHERE ld.domain_key = ? AND lv.entity_sync_id = ?",
            [LEGACY_UNIT_LOOKUP_DOMAIN.into(), sync_id.into()],
        ))
        .await?;
    let Some(row) = replayed else {
        return Ok(None);
    };
    let code: String = row.try_get("", "code").map_err(decode)?;
    if let Ok(id) = unit_id_by_code(conn, &code).await {
        return Ok(Some(id));
    }
    let label: String = row.try_get("", "label").map_err(decode)?;
    let fr: Option<String> = row.try_get("", "fr_label").map_err(decode)?;
    let en: Option<String> = row.try_get("", "en_label").map_err(decode)?;
    let fr = fr.filter(|s| !s.trim().is_empty()).unwrap_or(label.clone());
    let id = create_review_unit(
        conn,
        &ReviewUnit {
            code_hint: &code,
            label: &fr,
            label_en: en.as_deref().filter(|s| !s.trim().is_empty()),
            symbol: &label,
            is_active: true,
            source: "sync:lookup:inventory.unit_of_measure",
            legacy_code: Some(&code),
        },
    )
    .await?;
    Ok(Some(id))
}

/// Unit text columns of rows sent by a device older than migration 201, with the link that
/// replaces them.
pub const LEGACY_UNIT_TEXT_COLUMNS: &[(&str, &str, &str)] = &[
    ("equipment_meters", "unit", "unit_value_id"),
    ("inspection_checkpoints", "measurement_unit", "measurement_unit_id"),
];

async fn unit_sync_id(conn: &impl ConnectionTrait, id: i64) -> AppResult<Option<String>> {
    conn.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "SELECT entity_sync_id FROM reference_values WHERE id = ?",
        [id.into()],
    ))
    .await?
    .map(|r| r.try_get::<Option<String>>("", "entity_sync_id"))
    .transpose()
    .map_err(decode)
    .map(Option::flatten)
}

/// Rewrites an inbound payload of a device older than migration 201: a unit text (meter unit,
/// checkpoint unit, unit inside a checkpoint package) is mapped onto the list with the same rule
/// as the migration (synonym table; an unmatched text becomes a unit to review) and sent on as
/// the link. A text never overrides a link the payload already carries.
pub async fn upgrade_legacy_unit_payload(
    conn: &impl ConnectionTrait,
    table: &str,
    payload: &mut serde_json::Map<String, serde_json::Value>,
) -> AppResult<()> {
    for (t, text_col, link_col) in LEGACY_UNIT_TEXT_COLUMNS {
        if *t != table {
            continue;
        }
        let Some(text) = payload.remove(*text_col) else {
            continue;
        };
        let has_link = payload.get(*link_col).is_some_and(|v| !v.is_null());
        let Some(text) = text.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        if has_link {
            continue;
        }
        let map = map_unit_texts(conn, &[text.to_string()], &format!("sync:{table}.{text_col}")).await?;
        if let Some(id) = map.get(text) {
            if let Some(sid) = unit_sync_id(conn, *id).await? {
                payload.insert((*link_col).to_string(), sid.into());
            }
        }
    }
    if table == "inspection_template_versions" {
        let Some(raw) = payload
            .get("checkpoint_package_json")
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(());
        };
        if !raw.contains("\"measurement_unit\"") {
            return Ok(());
        }
        let Ok(mut package) = serde_json::from_str::<serde_json::Value>(raw) else {
            return Ok(());
        };
        if let Some(entries) = package.as_array_mut() {
            for entry in entries.iter_mut() {
                let Some(obj) = entry.as_object_mut() else {
                    continue;
                };
                let Some(old) = obj.remove("measurement_unit") else {
                    continue;
                };
                if obj.get("measurement_unit_id").is_some_and(|v| !v.is_null()) {
                    continue;
                }
                let mut link = serde_json::Value::Null;
                if let Some(text) = old.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    let map = map_unit_texts(
                        conn,
                        &[text.to_string()],
                        "sync:inspection_template_versions.checkpoint_package_json",
                    )
                    .await?;
                    if let Some(id) = map.get(text) {
                        // Inbound packages carry sync ids (`sync::row_spec` json ref).
                        if let Some(sid) = unit_sync_id(conn, *id).await? {
                            link = sid.into();
                        }
                    }
                }
                obj.insert("measurement_unit_id".into(), link);
            }
        }
        payload.insert("checkpoint_package_json".into(), package.to_string().into());
    }
    Ok(())
}

// ─── Retiring the fixed lookup `inventory.unit_of_measure` ──────────────────────────────────

/// Columns that linked a unit of the retired lookup (`lookup_values.id`) and now link a unit of
/// `MEASURE.UNITS` (`reference_values.id`). Migration 201 retargets their foreign keys.
pub const UNIT_LINK_COLUMNS: &[(&str, &str)] = &[
    ("articles", "unit_value_id"),
    ("inventory_supplier_prices", "price_unit_value_id"),
    ("unit_conversions", "from_unit_value_id"),
    ("unit_conversions", "to_unit_value_id"),
];

/// How the stored links are moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyLinkMode {
    /// Migration 201: every value of the linking columns is a lookup id.
    Migration,
    /// Startup retirement pass (the lookup came back with a replayed sync history): a value that
    /// already is a unit of the list is left alone, only lookup ids are moved.
    Replay,
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

async fn table_has_column(conn: &impl ConnectionTrait, table: &str, column: &str) -> AppResult<bool> {
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM pragma_table_info(?) WHERE name = ?",
            [table.into(), column.into()],
        ))
        .await?
        .is_some())
}

/// Moves every link from the units of the fixed lookup `inventory.unit_of_measure` to the units
/// of `MEASURE.UNITS`, by code, then deletes the lookup (its values, their aliases and the
/// domain). A code the list does not know becomes a unit of the plant flagged for review (never
/// dropped). Two lookup codes that only differ by case keep two units. A link from any other
/// column to a retired lookup value stops the step with the table and column named. Returns
/// `false` when the lookup no longer exists (nothing to do), so it can run again safely.
pub async fn retire_legacy_unit_lookup(conn: &impl ConnectionTrait, mode: LegacyLinkMode) -> AppResult<bool> {
    let Some(domain) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM lookup_domains WHERE domain_key = ?",
            [LEGACY_UNIT_LOOKUP_DOMAIN.into()],
        ))
        .await?
    else {
        return Ok(false);
    };
    let domain_id: i64 = domain.try_get("", "id").map_err(decode)?;
    ensure_unit_catalog(conn).await?;

    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, label, fr_label, en_label, is_active, deleted_at FROM lookup_values \
              WHERE domain_id = ? ORDER BY id",
            [domain_id.into()],
        ))
        .await?;

    let columns: Vec<(&str, &str)> = {
        let mut present = Vec::new();
        for (table, column) in UNIT_LINK_COLUMNS {
            if table_has_column(conn, table, column).await? {
                present.push((*table, *column));
            }
        }
        present
    };

    let mut pairs: Vec<(i64, i64)> = Vec::new();
    let mut taken: Vec<i64> = Vec::new();
    let mut retired: Vec<i64> = Vec::new();
    for r in &rows {
        let old_id: i64 = r.try_get("", "id").map_err(decode)?;
        retired.push(old_id);
        let code: String = r.try_get("", "code").map_err(decode)?;
        let deleted: Option<String> = r.try_get("", "deleted_at").map_err(decode)?;
        if deleted.is_some() && !is_linked(conn, &columns, old_id).await? {
            // A unit the plant deleted that nothing uses: not carried over.
            continue;
        }
        let same_code = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "{UNIT_SELECT} WHERE rs.status = 'published' AND UPPER(TRIM(u.code)) = UPPER(TRIM(?)) \
                     ORDER BY rs.version_no DESC LIMIT 1"
                ),
                [code.clone().into()],
            ))
            .await?
            .map(|r| r.try_get::<i64>("", "id"))
            .transpose()
            .map_err(decode)?
            .filter(|id| !taken.contains(id));
        let new_id = match same_code {
            Some(id) => id,
            None => {
                let label: String = r.try_get("", "label").map_err(decode)?;
                let fr: Option<String> = r.try_get("", "fr_label").map_err(decode)?;
                let en: Option<String> = r.try_get("", "en_label").map_err(decode)?;
                let is_active: i64 = r.try_get("", "is_active").map_err(decode)?;
                let fr = fr.filter(|s| !s.trim().is_empty()).unwrap_or(label.clone());
                create_review_unit(
                    conn,
                    &ReviewUnit {
                        code_hint: &code,
                        label: &fr,
                        label_en: en.as_deref().filter(|s| !s.trim().is_empty()),
                        symbol: &label,
                        is_active: is_active != 0 && deleted.is_none(),
                        source: "lookup:inventory.unit_of_measure",
                        legacy_code: Some(&code),
                    },
                )
                .await?
            }
        };
        taken.push(new_id);
        pairs.push((old_id, new_id));
    }

    remap_unit_columns(conn, &columns, &pairs, mode == LegacyLinkMode::Replay).await?;

    // Aliases of the lookup values become legacy aliases of their unit.
    for (old_id, new_id) in &pairs {
        let aliases = conn
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT alias_code FROM lookup_value_aliases WHERE value_id = ? ORDER BY id",
                [(*old_id).into()],
            ))
            .await?;
        for a in aliases {
            let alias: String = a.try_get("", "alias_code").map_err(decode)?;
            keep_text_as_alias(conn, *new_id, &alias).await?;
        }
    }

    delete_lookup_values(conn, domain_id, &retired).await?;
    tracing::info!(
        units = pairs.len(),
        "retired the fixed unit lookup inventory.unit_of_measure: links moved to MEASURE.UNITS"
    );
    Ok(true)
}

async fn is_linked(conn: &impl ConnectionTrait, columns: &[(&str, &str)], id: i64) -> AppResult<bool> {
    for (table, column) in columns {
        let found = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT 1 AS ok FROM {} WHERE {} = ? LIMIT 1",
                    quote(table),
                    quote(column)
                ),
                [id.into()],
            ))
            .await?
            .is_some();
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Deletes the retired lookup values and their domain. Any other column still pointing at one of
/// them stops the step (nothing is lost silently).
async fn delete_lookup_values(conn: &impl ConnectionTrait, domain_id: i64, ids: &[i64]) -> AppResult<()> {
    if !ids.is_empty() {
        let placeholders = vec!["?"; ids.len()].join(", ");
        let values = || ids.iter().map(|id| sea_orm::Value::from(*id)).collect::<Vec<_>>();
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("DELETE FROM lookup_value_aliases WHERE value_id IN ({placeholders})"),
            values(),
        ))
        .await?;
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("UPDATE lookup_values SET parent_value_id = NULL WHERE parent_value_id IN ({placeholders})"),
            values(),
        ))
        .await?;
        for (table, column) in crate::reference::retirement::referencing_columns(conn, "lookup_values").await? {
            if table.starts_with("lookup_") {
                continue;
            }
            let left = conn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT COUNT(*) AS n FROM {} WHERE {} IN ({placeholders})",
                        quote(&table),
                        quote(&column)
                    ),
                    values(),
                ))
                .await?
                .map(|r| r.try_get::<i64>("", "n"))
                .transpose()
                .map_err(decode)?
                .unwrap_or(0);
            if left > 0 {
                return Err(AppError::Internal(anyhow::anyhow!(
                    "{left} row(s) of {table}.{column} still point at a unit of the retired lookup \
                     inventory.unit_of_measure; the lookup cannot be retired without losing these links"
                )));
            }
        }
        conn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("DELETE FROM lookup_values WHERE id IN ({placeholders})"),
            values(),
        ))
        .await?;
    }
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM lookup_domains WHERE id = ?",
        [domain_id.into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_dimension_has_one_seed_and_round_trips() {
        for d in UnitDimension::ALL {
            assert_eq!(UnitDimension::parse(d.as_str()), Some(d));
            assert_eq!(DIMENSION_SEEDS.iter().filter(|s| s.dimension == d).count(), 1, "{d:?}");
        }
        assert_eq!(UnitDimension::parse("Mass"), Some(UnitDimension::Mass));
        assert_eq!(UnitDimension::parse("weight"), None);
    }

    #[test]
    fn starter_codes_are_unique_valid_codes() {
        let mut seen = std::collections::HashSet::new();
        for u in UNIT_SEEDS {
            assert!(seen.insert(u.code), "duplicate {}", u.code);
            assert_eq!(code_for_text(u.code), u.code, "{} is a valid value code", u.code);
            assert!(!u.symbol.trim().is_empty());
        }
    }

    #[test]
    fn packaging_converts_to_anything_other_dimensions_only_to_themselves() {
        use UnitDimension::*;
        assert!(dimensions_compatible(Mass, Mass));
        assert!(!dimensions_compatible(Mass, Length));
        assert!(dimensions_compatible(Packaging, Length));
        assert!(dimensions_compatible(Count, Packaging));
        assert!(!dimensions_compatible(Count, Mass));
    }

    #[test]
    fn every_synonym_names_a_standard_unit_once() {
        let mut seen = std::collections::HashSet::new();
        for (spelling, code) in UNIT_SYNONYMS {
            assert!(UNIT_SEEDS.iter().any(|u| u.code == *code), "{spelling} -> {code}");
            assert!(
                seen.insert(normalize_unit_text(spelling)),
                "duplicate spelling {spelling}"
            );
        }
        assert!(!seen.contains("tr"), "'tr' is ambiguous and must stay a unit to review");
    }

    #[test]
    fn texts_normalise_and_become_codes() {
        assert_eq!(normalize_unit_text(" M ³ /h "), "m3/h");
        assert_eq!(normalize_unit_text("Mètres"), "metres");
        assert_eq!(normalize_unit_text("ºC"), "°c");
        assert_eq!(normalize_unit_text("KWh"), "kwh");
        assert_eq!(code_for_text("m3/h"), "M3_H");
        assert_eq!(code_for_text("°C"), "C");
        assert_eq!(code_for_text("heures"), "HEURES");
        assert_eq!(code_for_text("1/min"), "U_1_MIN");
        assert_eq!(code_for_text("Pièce"), "PIECE");
    }
}
