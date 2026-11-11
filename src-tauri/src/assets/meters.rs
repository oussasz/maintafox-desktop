//! Asset meter and reading governance service.
//!
//! Phase 2 - Sub-phase 02 - File 02 - Sprint S2.
//!
//! Provides governed meter definitions and append-only time-series readings
//! on top of the existing `equipment_meters` table (migration 005, extended
//! by migration 011 with `meter_code` and `rollover_value` columns) and the
//! new `asset_meter_readings` table (migration 011).
//!
//! Column reconciliation (roadmap → DB):
//!   roadmap field        → DB column
//!   ─────────────────────────────────────────
//!   asset_id             → equipment_meters.equipment_id
//!   meter_code           → equipment_meters.meter_code         (migration 011)
//!   meter_type           → equipment_meters.meter_type
//!   unit                 → equipment_meters.unit_value_id → MEASURE.UNITS (migration 201;
//!                          the read model carries the unit's symbol in `unit`)
//!   current_reading      → equipment_meters.current_reading
//!   last_read_at         → equipment_meters.last_read_at
//!   expected_rate_per_day→ equipment_meters.expected_rate_per_day
//!   rollover_value       → equipment_meters.rollover_value     (migration 011)
//!   is_primary           → equipment_meters.is_primary
//!   is_active            → equipment_meters.is_active
//!
//!   meter_id             → asset_meter_readings.meter_id
//!   reading_value        → asset_meter_readings.reading_value
//!   reading_at           → asset_meter_readings.reading_at
//!   source_type          → asset_meter_readings.source_type
//!   source_reference     → asset_meter_readings.source_reference
//!   quality_flag         → asset_meter_readings.quality_flag
//!   created_by_id        → asset_meter_readings.created_by_id
//!   created_at           → asset_meter_readings.created_at
//!
//! Meter readings are append-only. They are never updated or deleted.
//! A wrong reading is cancelled by a new traced row (`cancel_meter_reading`, migration 270): the
//! cancelled reading stays visible in the history with its reason and author, and is no longer
//! counted. A reading far above the expected use needs a confirmation (`assess_meter_reading`).

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Read-side meter definition record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeter {
    pub id: i64,
    pub entity_sync_id: String,
    pub asset_id: i64,
    pub name: String,
    pub meter_code: Option<String>,
    pub meter_type: String,
    /// Unit of the meter (`reference_values.id` of `MEASURE.UNITS`).
    pub unit_value_id: Option<i64>,
    /// Symbol of that unit (read from the reference data, never stored here).
    pub unit: Option<String>,
    pub current_reading: f64,
    pub last_read_at: Option<String>,
    pub expected_rate_per_day: Option<f64>,
    pub rollover_value: Option<f64>,
    pub is_primary: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Alarm thresholds (migration 289): a valid reading that crosses one raises an intervention
    /// request (`raise_threshold_request`). NULL = no threshold.
    pub alert_threshold: Option<f64>,
    pub critical_threshold: Option<f64>,
    /// `above` (the value must stay below the thresholds) or `below`; NULL when no threshold.
    pub threshold_direction: Option<String>,
}

/// Payload for creating a new meter definition on an asset.
#[derive(Debug, Deserialize)]
pub struct CreateAssetMeterPayload {
    pub asset_id: i64,
    pub name: String,
    pub meter_code: Option<String>,
    pub meter_type: String,
    /// Unit of the meter: an active unit of `MEASURE.UNITS`.
    pub unit_value_id: Option<i64>,
    pub initial_reading: Option<f64>,
    pub expected_rate_per_day: Option<f64>,
    pub rollover_value: Option<f64>,
    pub is_primary: Option<bool>,
}

/// Read-side meter reading record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterReading {
    pub id: i64,
    pub meter_id: i64,
    pub reading_value: f64,
    pub reading_at: String,
    pub source_type: String,
    pub source_reference: Option<String>,
    pub quality_flag: String,
    pub created_by_id: Option<i64>,
    /// Name of the author of the reading ("Relevé par").
    pub created_by_name: Option<String>,
    pub created_at: String,
    /// Replication identity of the reading.
    pub entity_sync_id: String,
    /// The reading was cancelled: it stays in the history but is no longer counted.
    pub is_cancelled: bool,
    pub cancel_reason: Option<String>,
    pub cancelled_by_name: Option<String>,
    pub cancelled_at: Option<String>,
    /// The intervention request this reading raised by crossing a threshold, if any.
    pub raised_request_id: Option<i64>,
    pub raised_request_code: Option<String>,
    /// Level the reading crossed (`alert` / `critical`): the raised request's level in the
    /// history, the level just crossed when the reading is recorded.
    pub threshold_level: Option<String>,
    /// Recording only: the threshold was crossed but a request for this meter and level is still
    /// open, so none was raised; its code.
    #[serde(default)]
    pub open_request_code: Option<String>,
    /// Recording only: the threshold was crossed but the request could not be raised (why).
    #[serde(default)]
    pub threshold_problem: Option<String>,
}

/// How a reading continues the meter's lifetime count (migration 183).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeterReadingKind {
    /// Same physical counter, value at or above the previous one.
    Normal,
    /// The counter passed its `rollover_value` and restarted from zero.
    Rollover,
    /// A new physical meter was fitted; the reading is the new meter's value.
    Replacement,
}

impl MeterReadingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Rollover => "rollover",
            Self::Replacement => "replacement",
        }
    }
}

/// Offset to add to the raw reading so the lifetime value never goes back, and the kind of the
/// reading. `requested` is what the user stated (`None` = let the meter decide: a lower value on
/// a meter with a `rollover_value` is a rollover, otherwise it is refused).
pub fn next_meter_offset(
    previous_raw: Option<f64>,
    current_offset: f64,
    reading: f64,
    rollover_value: Option<f64>,
    requested: Option<MeterReadingKind>,
) -> AppResult<(MeterReadingKind, f64)> {
    let Some(prev) = previous_raw else {
        return Ok((MeterReadingKind::Normal, current_offset));
    };
    match requested {
        Some(MeterReadingKind::Replacement) => Ok((MeterReadingKind::Replacement, current_offset + prev - reading)),
        Some(MeterReadingKind::Rollover) => {
            let Some(rv) = rollover_value else {
                return Err(AppError::ValidationFailed(vec![
                    "GATE_ASSET_METER_NO_ROLLOVER: Ce compteur n'a pas de valeur de retournement (rollover_value) : un retour \
                     a zero ne peut pas etre enregistre."
                        .into(),
                ]));
            };
            if reading >= prev {
                return Err(AppError::ValidationFailed(vec![format!(
                    "GATE_ASSET_METER_ROLLOVER_NOT_LOWER: Un retournement suppose un releve ({reading}) inferieur au precedent ({prev})."
                )]));
            }
            Ok((MeterReadingKind::Rollover, current_offset + rv))
        }
        Some(MeterReadingKind::Normal) | None if reading >= prev => Ok((MeterReadingKind::Normal, current_offset)),
        None if rollover_value.is_some() => Ok((
            MeterReadingKind::Rollover,
            current_offset + rollover_value.unwrap_or_default(),
        )),
        _ => Err(AppError::ValidationFailed(vec![format!(
            "GATE_ASSET_METER_READING_LOWER: Le releve ({reading}) est inferieur au precedent ({prev}). Indiquez s'il s'agit d'un \
             retour a zero du compteur (valeur de retournement configuree) ou d'un remplacement \
             du compteur."
        )])),
    }
}

/// Payload for recording a meter reading.
#[derive(Debug, Deserialize)]
pub struct RecordMeterReadingPayload {
    pub meter_id: i64,
    pub reading_value: f64,
    pub reading_at: Option<String>,
    pub source_type: String,
    pub source_reference: Option<String>,
    pub quality_flag: Option<String>,
}

// ─── Row mapping ──────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("meter row decode failed for column '{column}': {e}"))
}

fn map_meter(row: &QueryResult) -> AppResult<AssetMeter> {
    Ok(AssetMeter {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get::<String>("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        asset_id: row
            .try_get::<i64>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        name: row.try_get::<String>("", "name").map_err(|e| decode_err("name", e))?,
        meter_code: row
            .try_get::<Option<String>>("", "meter_code")
            .map_err(|e| decode_err("meter_code", e))?,
        meter_type: row
            .try_get::<String>("", "meter_type")
            .map_err(|e| decode_err("meter_type", e))?,
        unit_value_id: row
            .try_get::<Option<i64>>("", "unit_value_id")
            .map_err(|e| decode_err("unit_value_id", e))?,
        unit: row
            .try_get::<Option<String>>("", "unit")
            .map_err(|e| decode_err("unit", e))?,
        current_reading: row
            .try_get::<f64>("", "current_reading")
            .map_err(|e| decode_err("current_reading", e))?,
        last_read_at: row
            .try_get::<Option<String>>("", "last_read_at")
            .map_err(|e| decode_err("last_read_at", e))?,
        expected_rate_per_day: row
            .try_get::<Option<f64>>("", "expected_rate_per_day")
            .map_err(|e| decode_err("expected_rate_per_day", e))?,
        rollover_value: row
            .try_get::<Option<f64>>("", "rollover_value")
            .map_err(|e| decode_err("rollover_value", e))?,
        is_primary: row
            .try_get::<bool>("", "is_primary")
            .map_err(|e| decode_err("is_primary", e))?,
        is_active: row
            .try_get::<bool>("", "is_active")
            .map_err(|e| decode_err("is_active", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
        alert_threshold: row
            .try_get::<Option<f64>>("", "alert_threshold")
            .map_err(|e| decode_err("alert_threshold", e))?,
        critical_threshold: row
            .try_get::<Option<f64>>("", "critical_threshold")
            .map_err(|e| decode_err("critical_threshold", e))?,
        threshold_direction: row
            .try_get::<Option<String>>("", "threshold_direction")
            .map_err(|e| decode_err("threshold_direction", e))?,
    })
}

fn map_reading(row: &QueryResult) -> AppResult<MeterReading> {
    Ok(MeterReading {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        meter_id: row
            .try_get::<i64>("", "meter_id")
            .map_err(|e| decode_err("meter_id", e))?,
        reading_value: row
            .try_get::<f64>("", "reading_value")
            .map_err(|e| decode_err("reading_value", e))?,
        reading_at: row
            .try_get::<String>("", "reading_at")
            .map_err(|e| decode_err("reading_at", e))?,
        source_type: row
            .try_get::<String>("", "source_type")
            .map_err(|e| decode_err("source_type", e))?,
        source_reference: row
            .try_get::<Option<String>>("", "source_reference")
            .map_err(|e| decode_err("source_reference", e))?,
        quality_flag: row
            .try_get::<String>("", "quality_flag")
            .map_err(|e| decode_err("quality_flag", e))?,
        created_by_id: row
            .try_get::<Option<i64>>("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        created_by_name: row
            .try_get::<Option<String>>("", "created_by_name")
            .map_err(|e| decode_err("created_by_name", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        entity_sync_id: row
            .try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?
            .unwrap_or_default(),
        is_cancelled: row
            .try_get::<i64>("", "is_cancelled")
            .map_err(|e| decode_err("is_cancelled", e))?
            != 0,
        cancel_reason: row
            .try_get::<Option<String>>("", "cancel_reason")
            .map_err(|e| decode_err("cancel_reason", e))?,
        cancelled_by_name: row
            .try_get::<Option<String>>("", "cancelled_by_name")
            .map_err(|e| decode_err("cancelled_by_name", e))?,
        cancelled_at: row
            .try_get::<Option<String>>("", "cancelled_at")
            .map_err(|e| decode_err("cancelled_at", e))?,
        raised_request_id: row
            .try_get::<Option<i64>>("", "raised_request_id")
            .map_err(|e| decode_err("raised_request_id", e))?,
        raised_request_code: row
            .try_get::<Option<String>>("", "raised_request_code")
            .map_err(|e| decode_err("raised_request_code", e))?,
        threshold_level: row
            .try_get::<Option<String>>("", "threshold_level")
            .map_err(|e| decode_err("threshold_level", e))?,
        open_request_code: None,
        threshold_problem: None,
    })
}

// ─── Validation helpers ───────────────────────────────────────────────────────

/// Validate that a code exists in a given lookup domain.
async fn validate_lookup(db: &impl ConnectionTrait, domain_key: &str, code: &str, field_label: &str) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM lookup_values lv \
             INNER JOIN lookup_domains ld ON ld.id = lv.domain_id \
             WHERE ld.domain_key = ? \
               AND lv.code = ? AND lv.is_active = 1 AND lv.deleted_at IS NULL",
            [domain_key.into(), code.into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt == 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_ASSET_REFERENCE_VALUE_UNKNOWN: Valeur '{code}' introuvable dans le domaine '{domain_key}' ({field_label})."
        )]));
    }
    Ok(())
}

/// Assert that an equipment row exists and is not soft-deleted.
async fn assert_asset_exists(db: &impl ConnectionTrait, asset_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [asset_id.into()],
        ))
        .await?;
    if row.is_none() {
        return Err(AppError::NotFound {
            entity: "equipment".into(),
            id: asset_id.to_string(),
        });
    }
    Ok(())
}

/// Assert that a meter exists and is active. Returns meter details needed
/// for reading validation: `(equipment_id, rollover_value)`.
async fn assert_meter_exists(db: &impl ConnectionTrait, meter_id: i64) -> AppResult<(i64, Option<f64>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id, rollover_value FROM equipment_meters \
             WHERE id = ? AND is_active = 1",
            [meter_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment_meters".into(),
            id: meter_id.to_string(),
        })?;
    let equipment_id: i64 = row
        .try_get("", "equipment_id")
        .map_err(|e| decode_err("equipment_id", e))?;
    let rollover_value: Option<f64> = row
        .try_get("", "rollover_value")
        .map_err(|e| decode_err("rollover_value", e))?;
    Ok((equipment_id, rollover_value))
}

/// Check that no other active meter on the same asset is already primary.
async fn assert_single_primary(
    db: &impl ConnectionTrait,
    asset_id: i64,
    exclude_meter_id: Option<i64>,
) -> AppResult<()> {
    let (sql, params) = match exclude_meter_id {
        Some(mid) => (
            "SELECT COUNT(*) AS cnt FROM equipment_meters \
             WHERE equipment_id = ? AND is_primary = 1 AND is_active = 1 AND id != ?",
            vec![asset_id.into(), mid.into()],
        ),
        None => (
            "SELECT COUNT(*) AS cnt FROM equipment_meters \
             WHERE equipment_id = ? AND is_primary = 1 AND is_active = 1",
            vec![asset_id.into()],
        ),
    };
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, params))
        .await?
        .expect("COUNT always returns a row");
    let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
    if cnt > 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_PRIMARY_EXISTS: Un seul compteur primaire est autorise par equipement. \
             Desactivez le compteur primaire existant avant d'en definir un nouveau."
                .into(),
        ]));
    }
    Ok(())
}

// ─── Select fragments ─────────────────────────────────────────────────────────

/// Meter columns over `equipment_meters em` joined to its unit `mu` (see [`meter_from`]).
fn meter_select() -> String {
    format!(
        "em.id, em.entity_sync_id, em.equipment_id, em.name, em.meter_code, em.meter_type, em.unit_value_id, \
         {} AS unit, em.current_reading, em.last_read_at, em.expected_rate_per_day, em.rollover_value, \
         em.is_primary, em.is_active, em.created_at, em.updated_at, \
         em.alert_threshold, em.critical_threshold, em.threshold_direction",
        crate::reference::units::symbol_sql("mu")
    )
}

/// `FROM` clause matching [`meter_select`].
const METER_FROM: &str = "equipment_meters em LEFT JOIN reference_values mu ON mu.id = em.unit_value_id";

/// Reading columns over `asset_meter_readings mr` with its author, and the cancellation that points
/// at it, if any (`c`, see [`READING_FROM`]).
const READING_SELECT: &str = "mr.id, mr.meter_id, mr.reading_value, mr.reading_at, mr.source_type,      mr.source_reference, mr.quality_flag, mr.created_by_id, mr.created_at, mr.entity_sync_id,      COALESCE(au.display_name, au.username) AS created_by_name,      CASE WHEN c.id IS NULL THEN 0 ELSE 1 END AS is_cancelled,      c.cancel_reason AS cancel_reason, c.created_at AS cancelled_at,      COALESCE(cu.display_name, cu.username) AS cancelled_by_name,      (SELECT ir.id FROM intervention_requests ir WHERE ir.source_meter_reading_id = mr.id ORDER BY ir.id LIMIT 1) AS raised_request_id,      (SELECT ir.code FROM intervention_requests ir WHERE ir.source_meter_reading_id = mr.id ORDER BY ir.id LIMIT 1) AS raised_request_code,      (SELECT ir.source_threshold_level FROM intervention_requests ir WHERE ir.source_meter_reading_id = mr.id ORDER BY ir.id LIMIT 1) AS threshold_level";

/// `FROM` clause matching [`READING_SELECT`].
const READING_FROM: &str = "asset_meter_readings mr      LEFT JOIN user_accounts au ON au.id = mr.created_by_id      LEFT JOIN asset_meter_readings c ON c.id = (SELECT MIN(x.id) FROM asset_meter_readings x WHERE x.cancels_sync_id = mr.entity_sync_id)     LEFT JOIN user_accounts cu ON cu.id = c.created_by_id";

/// Condition (alias `mr`) of a reading that counts: a real reading, not corrected or rejected, and
/// not cancelled. Usage-based plans, the study clock and the meter's current value use it.
pub const VALID_READING_SQL: &str = "mr.cancels_sync_id IS NULL      AND mr.quality_flag NOT IN ('corrected', 'cancelled', 'rejected')      AND NOT EXISTS (SELECT 1 FROM asset_meter_readings x WHERE x.cancels_sync_id = mr.entity_sync_id)";

/// A new reading further than this many times the expected use since the previous valid reading
/// needs a confirmation (a 12 450 h typed instead of 1 245 h).
const ABNORMAL_JUMP_FACTOR: f64 = 5.0;

// ─── Service functions ────────────────────────────────────────────────────────

/// List all meters for an asset.
///
/// Returns active meters first, ordered by `is_primary DESC, name ASC`.
pub async fn list_asset_meters(db: &DatabaseConnection, asset_id: i64) -> AppResult<Vec<AssetMeter>> {
    let sql = format!(
        "SELECT {} FROM {METER_FROM} \
         WHERE em.equipment_id = ? \
         ORDER BY em.is_active DESC, em.is_primary DESC, em.name ASC",
        meter_select()
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [asset_id.into()],
        ))
        .await?;
    rows.iter().map(map_meter).collect()
}

/// Create a new meter definition on an asset.
///
/// Validation:
///   - `asset_id` must exist and not be soft-deleted
///   - `meter_type` must exist in `equipment.meter_type` domain
///   - if `is_primary = true`, no other active primary meter on the asset
///   - `initial_reading` must be >= 0
///   - `rollover_value` must be > 0 if provided
///   - `name` must not be empty
pub async fn create_asset_meter(
    db: &DatabaseConnection,
    payload: CreateAssetMeterPayload,
    actor_id: i32,
) -> AppResult<AssetMeter> {
    let txn = db.begin().await?;

    // ── 1. Validate asset exists ─────────────────────────────────────────
    assert_asset_exists(&txn, payload.asset_id).await?;

    // ── 2. Validate meter_type against lookup domain ─────────────────────
    validate_lookup(&txn, "equipment.meter_type", &payload.meter_type, "meter_type").await?;

    // ── 3. Validate payload constraints ──────────────────────────────────
    let mut errors: Vec<String> = Vec::new();

    let name = payload.name.trim().to_string();
    if name.is_empty() {
        errors.push("GATE_ASSET_METER_NAME_REQUIRED: Le nom du compteur est obligatoire.".into());
    }

    let initial = payload.initial_reading.unwrap_or(0.0);
    if initial < 0.0 {
        errors.push("GATE_ASSET_METER_INITIAL_NEGATIVE: Le releve initial ne peut pas etre negatif.".into());
    }

    if let Some(rv) = payload.rollover_value {
        if rv <= 0.0 {
            errors.push("GATE_ASSET_METER_ROLLOVER_POSITIVE: La valeur de retournement (rollover) doit etre strictement positive.".into());
        }
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    if let Some(unit_id) = payload.unit_value_id {
        crate::reference::units::ensure_unit(&txn, unit_id, "unit_value_id", true).await?;
    }

    // ── 4. Enforce single-primary constraint ─────────────────────────────
    let is_primary = payload.is_primary.unwrap_or(false);
    if is_primary {
        assert_single_primary(&txn, payload.asset_id, None).await?;
    }

    // ── 5. Auto-generate meter_code if not provided ──────────────────────
    let meter_code = payload
        .meter_code
        .unwrap_or_else(|| name.to_uppercase().replace(' ', "_"));

    // ── 6. Insert the meter row ──────────────────────────────────────────
    let entity_sync_id = Uuid::new_v4().to_string();
    let now = crate::clock::now().to_rfc3339();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_meters \
         (entity_sync_id, equipment_id, name, meter_code, meter_type, unit_value_id, \
          current_reading, expected_rate_per_day, rollover_value, \
          is_primary, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            entity_sync_id.clone().into(),
            payload.asset_id.into(),
            name.into(),
            meter_code.into(),
            payload.meter_type.into(),
            payload.unit_value_id.into(),
            initial.into(),
            payload.expected_rate_per_day.into(),
            payload.rollover_value.into(),
            i32::from(is_primary).into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    // ── 7. Retrieve the inserted meter ───────────────────────────────────
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {} FROM {METER_FROM} WHERE em.entity_sync_id = ?",
                meter_select()
            ),
            [entity_sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("meter created but not found after insert")))?;
    let meter = map_meter(&row)?;

    txn.commit().await?;

    tracing::info!(
        meter_id = meter.id,
        asset_id = meter.asset_id,
        meter_type = %meter.meter_type,
        "meter created (actor={})", actor_id
    );
    Ok(meter)
}

/// Record a governed meter reading.
///
/// Validation:
///   - `meter_id` must exist and be active
///   - `source_type` must exist in `equipment.reading_source_type` domain
///   - `reading_value` must be >= 0
///   - `reading_at` must be after the latest existing reading
///     (unless `quality_flag = 'corrected'`)
///   - rollover handling: if meter has `rollover_value` set, the reading
///     may be less than the current value (counter reset)
///
/// Side-effects:
///   - Updates `equipment_meters.current_reading` and `last_read_at`
///     when the new reading is the most recent.
pub async fn record_meter_reading(
    db: &DatabaseConnection,
    payload: RecordMeterReadingPayload,
    actor_id: i32,
) -> AppResult<MeterReading> {
    record_meter_reading_of_kind(db, payload, None, actor_id).await
}

/// [`record_meter_reading`] with the user's statement of a rollover or a meter replacement
/// (`kind`); the lifetime value of the meter stays continuous (migration 183).
pub async fn record_meter_reading_of_kind(
    db: &DatabaseConnection,
    payload: RecordMeterReadingPayload,
    kind: Option<MeterReadingKind>,
    actor_id: i32,
) -> AppResult<MeterReading> {
    record_reading_inner(db, payload, kind, true, actor_id).await
}

/// [`record_meter_reading`] for a person typing a value: a reading far above the expected use is
/// refused until `confirm_jump` is set (see [`assess_meter_reading`]).
pub async fn record_meter_reading_checked(
    db: &DatabaseConnection,
    payload: RecordMeterReadingPayload,
    confirm_jump: bool,
    actor_id: i32,
) -> AppResult<MeterReading> {
    record_reading_inner(db, payload, None, confirm_jump, actor_id).await
}

/// [`record_meter_reading_checked`] when the person also states a rollover or replacement.
pub async fn record_meter_reading_of_kind_checked(
    db: &DatabaseConnection,
    payload: RecordMeterReadingPayload,
    kind: Option<MeterReadingKind>,
    confirm_jump: bool,
    actor_id: i32,
) -> AppResult<MeterReading> {
    record_reading_inner(db, payload, kind, confirm_jump, actor_id).await
}

async fn record_reading_inner(
    db: &DatabaseConnection,
    payload: RecordMeterReadingPayload,
    kind: Option<MeterReadingKind>,
    confirm_jump: bool,
    actor_id: i32,
) -> AppResult<MeterReading> {
    let txn = db.begin().await?;

    // ── 1. Validate meter exists and is active ───────────────────────────
    let (equipment_id, rollover_value) = assert_meter_exists(&txn, payload.meter_id).await?;

    // ── 2. Validate source_type against lookup domain ────────────────────
    validate_lookup(
        &txn,
        "equipment.reading_source_type",
        &payload.source_type,
        "source_type",
    )
    .await?;

    // ── 3. Validate reading_value ────────────────────────────────────────
    if payload.reading_value < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_NEGATIVE: La valeur du releve ne peut pas etre negative.".into(),
        ]));
    }

    // ── 4. Quality flag: default to 'accepted' ──────────────────────────
    let quality_flag = payload.quality_flag.as_deref().unwrap_or("accepted").to_string();
    if quality_flag == "cancelled" {
        // Reserved for the cancellation marker rows written by `cancel_meter_reading`.
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_QUALITY_RESERVED: Cette qualite de releve est reservee aux annulations.".into(),
        ]));
    }
    // A corrected or rejected reading is kept for the record but never becomes the meter's value.
    let counts = quality_flag != "corrected" && quality_flag != "rejected";

    let now = crate::clock::now().to_rfc3339();
    let reading_at = payload.reading_at.unwrap_or_else(|| now.clone());

    let current_offset: f64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reading_offset FROM equipment_meters WHERE id = ?",
            [payload.meter_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<f64>("", "reading_offset"))
        .transpose()
        .map_err(|e| decode_err("reading_offset", e))?
        .unwrap_or(0.0);
    let mut reading_kind = MeterReadingKind::Normal;
    let mut new_offset = current_offset;

    // ── 5. Monotonic timestamp check (skip for corrections) ──────────────
    if counts {
        let latest = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT mr.reading_at, mr.reading_value FROM asset_meter_readings mr \
                     WHERE mr.meter_id = ? AND {VALID_READING_SQL} \
                     ORDER BY mr.reading_at DESC, mr.entity_sync_id DESC LIMIT 1"
                ),
                [payload.meter_id.into()],
            ))
            .await?;

        if let Some(prev_row) = latest {
            let prev_at: String = prev_row
                .try_get("", "reading_at")
                .map_err(|e| decode_err("reading_at", e))?;
            if reading_at <= prev_at {
                return Err(AppError::ValidationFailed(vec![format!(
                    "GATE_ASSET_METER_TIME_ORDER: L'horodatage du releve ({reading_at}) doit etre posterieur \
                     au dernier releve ({prev_at})."
                )]));
            }

            // A lower value is a rollover or a meter replacement (see `next_meter_offset`).
            let prev_val: f64 = prev_row
                .try_get("", "reading_value")
                .map_err(|e| decode_err("reading_value", e))?;
            (reading_kind, new_offset) = next_meter_offset(
                Some(prev_val),
                current_offset,
                payload.reading_value,
                rollover_value,
                kind,
            )?;
            if reading_kind == MeterReadingKind::Normal && !confirm_jump {
                if let Some(jump) = find_abnormal_jump(
                    &txn,
                    payload.meter_id,
                    &prev_at,
                    prev_val,
                    &reading_at,
                    payload.reading_value,
                )
                .await?
                {
                    return Err(jump.into_error());
                }
            }
        } else if kind == Some(MeterReadingKind::Rollover) {
            return Err(AppError::ValidationFailed(vec![
                "GATE_ASSET_METER_ROLLOVER_NEEDS_PREVIOUS: Un retournement suppose un releve precedent.".into(),
            ]));
        }
    }
    let lifetime_value = payload.reading_value + new_offset;
    let reading_sync_id = Uuid::new_v4().to_string();

    // ── 6. Insert the reading row ────────────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO asset_meter_readings \
         (meter_id, reading_value, reading_at, source_type, \
          source_reference, quality_flag, created_by_id, created_at, \
          reading_kind, lifetime_value, entity_sync_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            payload.meter_id.into(),
            payload.reading_value.into(),
            reading_at.clone().into(),
            payload.source_type.into(),
            payload.source_reference.into(),
            quality_flag.clone().into(),
            (actor_id as i64).into(),
            now.clone().into(),
            reading_kind.as_str().into(),
            lifetime_value.into(),
            reading_sync_id.clone().into(),
        ],
    ))
    .await?;

    // ── 7. Update equipment_meters current value ─────────────────────────
    // Only update if the new reading is the most recent accepted reading.
    if counts {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_meters SET current_reading = ?, reading_offset = ?, \
             last_read_at = ?, updated_at = ? \
             WHERE id = ?",
            [
                payload.reading_value.into(),
                new_offset.into(),
                reading_at.into(),
                now.clone().into(),
                payload.meter_id.into(),
            ],
        ))
        .await?;
    }

    // ── 8. Retrieve the inserted reading ─────────────────────────────────
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {READING_SELECT} FROM {READING_FROM} WHERE mr.entity_sync_id = ?"),
            [reading_sync_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("meter reading created but not found after insert")))?;
    let mut reading = map_reading(&row)?;

    // ── 9. Alarm thresholds (migration 289) ──────────────────────────────
    // A counted reading that crosses a threshold raises an intervention request, in a savepoint:
    // a request that cannot be raised never loses the reading (the reason is returned).
    if counts {
        apply_threshold_outcome(&txn, &mut reading, equipment_id, actor_id).await?;
    }

    txn.commit().await?;

    // Best-effort PM regen for equipment-scoped meter plans (never fail the reading).
    if counts {
        crate::pm::generation_control::try_generate_pm_for_equipment(db, equipment_id).await;
    }

    tracing::info!(
        reading_id = reading.id,
        meter_id = reading.meter_id,
        value = reading.reading_value,
        quality = %reading.quality_flag,
        "meter reading recorded (actor={})", actor_id
    );
    Ok(reading)
}

// ─── Abnormal jump ────────────────────────────────────────────────────────────

/// A reading far above the use the meter normally sees between two readings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterReadingWarning {
    /// Rise since the previous valid reading.
    pub delta: f64,
    /// Rise expected over the same time at the usual rate.
    pub expected_delta: f64,
    /// Usual use per day (the meter's expected rate, or the observed average).
    pub rate_per_day: f64,
    pub days: f64,
    pub previous_value: f64,
    pub previous_at: String,
}

impl MeterReadingWarning {
    fn into_error(self) -> AppError {
        AppError::ValidationFailed(vec![format!(
            "GATE_ASSET_METER_JUMP_UNCONFIRMED: Le releve augmente de {:.1} depuis le precedent ({:.1}) alors que \
             l'usage habituel donne environ {:.1} sur la meme periode. Verifiez la valeur saisie, puis confirmez \
             si elle est exacte.",
            self.delta, self.previous_value, self.expected_delta
        )])
    }
}

fn parse_instant(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// Usual use per day of a meter: its configured expected rate, else the average between its first
/// and last valid readings (needs two readings over a positive time).
async fn usual_rate_per_day(db: &impl ConnectionTrait, meter_id: i64) -> AppResult<Option<f64>> {
    let configured: Option<f64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT expected_rate_per_day FROM equipment_meters WHERE id = ?",
            [meter_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<f64>>("", "expected_rate_per_day"))
        .transpose()
        .map_err(|e| decode_err("expected_rate_per_day", e))?
        .flatten();
    if let Some(rate) = configured.filter(|r| *r > 0.0) {
        return Ok(Some(rate));
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT mr.reading_at, COALESCE(mr.lifetime_value, mr.reading_value) AS v \
                 FROM asset_meter_readings mr WHERE mr.meter_id = ? AND {VALID_READING_SQL} \
                 ORDER BY mr.reading_at ASC, mr.entity_sync_id ASC"
            ),
            [meter_id.into()],
        ))
        .await?;
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Ok(None);
    };
    if rows.len() < 2 {
        return Ok(None);
    }
    let at = |r: &QueryResult| {
        r.try_get::<String>("", "reading_at")
            .ok()
            .and_then(|s| parse_instant(&s))
    };
    let (Some(t0), Some(t1)) = (at(first), at(last)) else {
        return Ok(None);
    };
    let days = (t1 - t0).num_seconds() as f64 / 86_400.0;
    let v0: f64 = first.try_get("", "v").unwrap_or(0.0);
    let v1: f64 = last.try_get("", "v").unwrap_or(0.0);
    Ok((days > 0.0 && v1 > v0).then(|| (v1 - v0) / days))
}

/// The jump warning of a reading of `value` at `at`, when it rises by more than
/// [`ABNORMAL_JUMP_FACTOR`] times the usual use since the previous valid reading.
async fn find_abnormal_jump(
    db: &impl ConnectionTrait,
    meter_id: i64,
    prev_at: &str,
    prev_value: f64,
    at: &str,
    value: f64,
) -> AppResult<Option<MeterReadingWarning>> {
    let Some(rate) = usual_rate_per_day(db, meter_id).await? else {
        return Ok(None);
    };
    let (Some(t0), Some(t1)) = (parse_instant(prev_at), parse_instant(at)) else {
        return Ok(None);
    };
    // Readings a few minutes apart still count as a day of use: no false alarm on quick re-reads.
    let days = ((t1 - t0).num_seconds() as f64 / 86_400.0).max(1.0);
    let delta = value - prev_value;
    let expected_delta = rate * days;
    Ok(
        (delta > ABNORMAL_JUMP_FACTOR * expected_delta).then_some(MeterReadingWarning {
            delta,
            expected_delta,
            rate_per_day: rate,
            days,
            previous_value: prev_value,
            previous_at: prev_at.to_string(),
        }),
    )
}

/// Warning (if any) a reading would raise, so the screen can ask for a confirmation before saving.
pub async fn assess_meter_reading(
    db: &DatabaseConnection,
    meter_id: i64,
    reading_value: f64,
    reading_at: Option<String>,
) -> AppResult<Option<MeterReadingWarning>> {
    assert_meter_exists(db, meter_id).await?;
    let Some(prev) = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT mr.reading_at, mr.reading_value FROM asset_meter_readings mr \
                 WHERE mr.meter_id = ? AND {VALID_READING_SQL} \
                 ORDER BY mr.reading_at DESC, mr.entity_sync_id DESC LIMIT 1"
            ),
            [meter_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let prev_at: String = prev
        .try_get("", "reading_at")
        .map_err(|e| decode_err("reading_at", e))?;
    let prev_val: f64 = prev
        .try_get("", "reading_value")
        .map_err(|e| decode_err("reading_value", e))?;
    if reading_value < prev_val {
        // A lower value is a rollover or a replacement: handled when recording.
        return Ok(None);
    }
    let at = reading_at.unwrap_or_else(|| crate::clock::now().to_rfc3339());
    find_abnormal_jump(db, meter_id, &prev_at, prev_val, &at, reading_value).await
}

// ─── Cancellation ─────────────────────────────────────────────────────────────

/// Cancel a wrong reading, with a reason. Only the most recent valid reading of a meter can be
/// cancelled (older ones first cancel the newer ones): the meter goes back to the reading before
/// it. The reading stays in the history, marked cancelled with the reason and the author, through
/// a new append-only row; an audit event records the correction. Record the right value afterwards.
pub async fn cancel_meter_reading(
    db: &DatabaseConnection,
    reading_id: i64,
    reason: &str,
    actor_id: i32,
) -> AppResult<MeterReading> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_CANCEL_REASON_REQUIRED: Indiquez le motif de l'annulation du releve.".into(),
        ]));
    }
    let txn = db.begin().await?;
    let target = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {READING_SELECT}, mr.reading_kind AS reading_kind \
                 FROM {READING_FROM} WHERE mr.id = ?"
            ),
            [reading_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "asset_meter_readings".into(),
            id: reading_id.to_string(),
        })?;
    let target_reading = map_reading(&target)?;
    let target_kind: String = target
        .try_get("", "reading_kind")
        .map_err(|e| decode_err("reading_kind", e))?;
    if target_kind == "cancellation" || target_reading.quality_flag == "cancelled" {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_CANCEL_MARKER: Cette ligne est deja une annulation.".into(),
        ]));
    }
    if target_reading.is_cancelled {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_ALREADY_CANCELLED: Ce releve est deja annule.".into(),
        ]));
    }

    let valid = txn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT mr.id, mr.reading_at, mr.reading_value, \
                        COALESCE(mr.lifetime_value, mr.reading_value) AS lifetime \
                 FROM asset_meter_readings mr WHERE mr.meter_id = ? AND {VALID_READING_SQL} \
                 ORDER BY mr.reading_at DESC, mr.entity_sync_id DESC LIMIT 2"
            ),
            [target_reading.meter_id.into()],
        ))
        .await?;
    let latest_id: Option<i64> = valid.first().and_then(|r| r.try_get("", "id").ok());
    if latest_id != Some(reading_id) {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_CANCEL_NOT_LATEST: Seul le dernier releve du compteur peut etre annule. Annulez d'abord \
             les releves plus recents."
                .into(),
        ]));
    }

    // The meter goes back to the reading before the cancelled one. The value given when the meter
    // was created is not kept as a reading, so a meter's only reading cannot be cancelled: the
    // meter would fall back to zero instead of its real starting value.
    let Some(prev) = valid.get(1) else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_CANCEL_ONLY_READING: C'est le premier releve du compteur : il ne peut pas etre annule \
             car la valeur de depart n'est pas conservee. Saisissez un releve plus recent pour le remplacer."
                .into(),
        ]));
    };
    let (restored, restored_at, restored_offset) = {
        let value: f64 = prev
            .try_get("", "reading_value")
            .map_err(|e| decode_err("reading_value", e))?;
        let lifetime: f64 = prev.try_get("", "lifetime").map_err(|e| decode_err("lifetime", e))?;
        let at: String = prev
            .try_get("", "reading_at")
            .map_err(|e| decode_err("reading_at", e))?;
        (value, Some(at), lifetime - value)
    };
    if target_reading.entity_sync_id.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_ASSET_METER_CANCEL_NO_SYNC_ID: Ce releve n'a pas d'identifiant de synchronisation.".into(),
        ]));
    }

    let now = crate::clock::now().to_rfc3339();
    let marker_sync_id = Uuid::new_v4().to_string();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO asset_meter_readings \
         (meter_id, reading_value, reading_at, source_type, source_reference, quality_flag, created_by_id, \
          created_at, reading_kind, lifetime_value, entity_sync_id, cancels_sync_id, cancel_reason) \
         VALUES (?, ?, ?, ?, NULL, 'cancelled', ?, ?, 'cancellation', NULL, ?, ?, ?)",
        [
            target_reading.meter_id.into(),
            target_reading.reading_value.into(),
            now.clone().into(),
            target_reading.source_type.clone().into(),
            (actor_id as i64).into(),
            now.clone().into(),
            marker_sync_id.into(),
            target_reading.entity_sync_id.clone().into(),
            reason.to_string().into(),
        ],
    ))
    .await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment_meters SET current_reading = ?, reading_offset = ?, last_read_at = ?, updated_at = ? \
         WHERE id = ?",
        [
            restored.into(),
            restored_offset.into(),
            restored_at.into(),
            now.into(),
            target_reading.meter_id.into(),
        ],
    ))
    .await?;

    let target_id = reading_id.to_string();
    crate::audit::emit_in(
        &txn,
        crate::audit::AuditEvent {
            event_type: "asset.meter_reading_cancelled",
            actor_id: Some(actor_id),
            entity_type: Some("asset_meter_readings"),
            entity_id: Some(&target_id),
            summary: "Meter reading cancelled",
            detail_json: Some(
                serde_json::json!({
                    "meter_id": target_reading.meter_id,
                    "reading_value": target_reading.reading_value,
                    "reading_at": target_reading.reading_at,
                    "reason": reason,
                    "restored_value": restored,
                })
                .to_string(),
            ),
            ..Default::default()
        },
    )
    .await?;

    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {READING_SELECT} FROM {READING_FROM} WHERE mr.id = ?"),
            [reading_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("cancelled reading not found")))?;
    let cancelled = map_reading(&row)?;
    txn.commit().await?;
    tracing::info!(reading_id, actor_id, "meter reading cancelled");
    Ok(cancelled)
}

/// Get the latest reading for a meter.
///
/// Returns the most recent accepted (non-corrected) reading, or `None`
/// if the meter has no readings yet.
pub async fn get_latest_meter_value(db: &DatabaseConnection, meter_id: i64) -> AppResult<Option<MeterReading>> {
    let sql = format!(
        "SELECT {READING_SELECT} FROM {READING_FROM} \
         WHERE mr.meter_id = ? AND {VALID_READING_SQL} \
         ORDER BY mr.reading_at DESC, mr.entity_sync_id DESC LIMIT 1"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [meter_id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_reading(&r)?)),
        None => Ok(None),
    }
}

/// List readings for a meter, ordered newest-first.
///
/// # Arguments
/// - `meter_id` — the meter id
/// - `limit` — max rows (capped at 1000)
pub async fn list_meter_readings(
    db: &DatabaseConnection,
    meter_id: i64,
    limit: Option<u64>,
) -> AppResult<Vec<MeterReading>> {
    let row_limit = limit.unwrap_or(100).min(1000);
    // Cancelled readings stay in the list (with their reason); the cancellation rows themselves do not.
    let sql = format!(
        "SELECT {READING_SELECT} FROM {READING_FROM} \
         WHERE mr.meter_id = ? AND mr.cancels_sync_id IS NULL \
         ORDER BY mr.reading_at DESC, mr.entity_sync_id DESC \
         LIMIT {row_limit}"
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [meter_id.into()],
        ))
        .await?;
    rows.iter().map(map_reading).collect()
}

// ─── Alarm thresholds (L2-08, migration 289) ─────────────────────────────────────

/// Stable gate codes of the threshold settings and of a request that could not be raised.
pub const GATE_ASSET_METER_THRESHOLD_DIRECTION_REQUIRED: &str = "GATE_ASSET_METER_THRESHOLD_DIRECTION_REQUIRED";
pub const GATE_ASSET_METER_THRESHOLD_ORDER: &str = "GATE_ASSET_METER_THRESHOLD_ORDER";
pub const GATE_ASSET_METER_THRESHOLD_NOT_FINITE: &str = "GATE_ASSET_METER_THRESHOLD_NOT_FINITE";
pub const GATE_ASSET_METER_THRESHOLD_NO_LOCATION: &str = "GATE_ASSET_METER_THRESHOLD_NO_LOCATION";

/// Which side of a threshold is abnormal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdDirection {
    /// A value at or above the threshold crosses it (temperature, vibration…).
    Above,
    /// A value at or below the threshold crosses it (oil level, flow…).
    Below,
}

impl ThresholdDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Below => "below",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "above" => Some(Self::Above),
            "below" => Some(Self::Below),
            _ => None,
        }
    }
}

/// The threshold a reading crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdLevel {
    Alert,
    Critical,
}

impl ThresholdLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Critical => "critical",
        }
    }

    /// Urgency of the request raised at this level (shared DI / OT scale `WORK.URGENCY`).
    pub const fn urgency(self) -> crate::reference::urgency_scale::UrgencyCode {
        match self {
            Self::Alert => crate::reference::urgency_scale::UrgencyCode::High,
            Self::Critical => crate::reference::urgency_scale::UrgencyCode::Urgent,
        }
    }

    const fn label_fr(self) -> &'static str {
        match self {
            Self::Alert => "d'alerte",
            Self::Critical => "critique",
        }
    }
}

/// The highest threshold `value` crosses (critical wins over alert), `None` when it crosses none
/// or the meter has no threshold.
pub fn crossed_level(
    value: f64,
    alert: Option<f64>,
    critical: Option<f64>,
    direction: Option<ThresholdDirection>,
) -> Option<ThresholdLevel> {
    let direction = direction?;
    let crosses = |t: f64| match direction {
        ThresholdDirection::Above => value >= t,
        ThresholdDirection::Below => value <= t,
    };
    if critical.is_some_and(crosses) {
        return Some(ThresholdLevel::Critical);
    }
    if alert.is_some_and(crosses) {
        return Some(ThresholdLevel::Alert);
    }
    None
}

/// New alarm thresholds of a meter. Both empty clears them (and the direction).
#[derive(Debug, Deserialize)]
pub struct SetMeterThresholdsPayload {
    pub meter_id: i64,
    pub alert_threshold: Option<f64>,
    pub critical_threshold: Option<f64>,
    pub threshold_direction: Option<String>,
}

/// Checks the thresholds; returns what to store `(alert, critical, direction)`.
pub fn checked_thresholds(
    alert: Option<f64>,
    critical: Option<f64>,
    direction: Option<&str>,
) -> AppResult<(Option<f64>, Option<f64>, Option<ThresholdDirection>)> {
    if alert.is_none() && critical.is_none() {
        return Ok((None, None, None));
    }
    if alert.is_some_and(|v| !v.is_finite()) || critical.is_some_and(|v| !v.is_finite()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "{GATE_ASSET_METER_THRESHOLD_NOT_FINITE}: Un seuil doit être un nombre."
        )]));
    }
    let direction = direction.and_then(ThresholdDirection::parse).ok_or_else(|| {
        AppError::ValidationFailed(vec![format!(
            "{GATE_ASSET_METER_THRESHOLD_DIRECTION_REQUIRED}: Indiquez si le seuil est franchi par le haut ou par le bas."
        )])
    })?;
    if let (Some(a), Some(c)) = (alert, critical) {
        let ordered = match direction {
            ThresholdDirection::Above => a < c,
            ThresholdDirection::Below => a > c,
        };
        if !ordered {
            return Err(AppError::ValidationFailed(vec![format!(
                "{GATE_ASSET_METER_THRESHOLD_ORDER}:alert={a}:critical={c}: Le seuil d'alerte ({a}) doit être atteint avant le seuil critique ({c})."
            )]));
        }
    }
    Ok((alert, critical, Some(direction)))
}

/// Set (or clear) the alarm thresholds of an active meter.
pub async fn set_meter_thresholds(
    db: &DatabaseConnection,
    payload: SetMeterThresholdsPayload,
    actor_id: i32,
) -> AppResult<AssetMeter> {
    let (alert, critical, direction) = checked_thresholds(
        payload.alert_threshold,
        payload.critical_threshold,
        payload.threshold_direction.as_deref(),
    )?;
    let txn = db.begin().await?;
    assert_meter_exists(&txn, payload.meter_id).await?;
    let now = crate::clock::now().to_rfc3339();
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE equipment_meters SET alert_threshold = ?, critical_threshold = ?, threshold_direction = ?, updated_at = ? WHERE id = ?",
        [
            alert.into(),
            critical.into(),
            direction.map(|d| d.as_str().to_string()).into(),
            now.into(),
            payload.meter_id.into(),
        ],
    ))
    .await?;
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {} FROM {METER_FROM} WHERE em.id = ?", meter_select()),
            [payload.meter_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("meter updated but not found")))?;
    let meter = map_meter(&row)?;
    txn.commit().await?;
    tracing::info!(meter_id = meter.id, "meter thresholds set (actor={})", actor_id);
    Ok(meter)
}

/// What crossing a threshold led to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThresholdOutcome {
    /// A request was raised: `(id, code)`.
    Raised(i64, String),
    /// A request raised by this meter at this level is still open: its code.
    AlreadyOpen(String),
}

/// Code of a request raised by `meter_id` at `level` that is still open: not closed and not
/// converted, or converted to a work order that is not closed yet.
pub async fn open_threshold_request(
    db: &impl ConnectionTrait,
    meter_id: i64,
    level: ThresholdLevel,
) -> AppResult<Option<String>> {
    let sql = format!(
        "SELECT ir.code FROM intervention_requests ir \
           JOIN asset_meter_readings r ON r.id = ir.source_meter_reading_id \
          WHERE r.meter_id = ? AND ir.source_threshold_level = ? \
            AND ({open} OR EXISTS (SELECT 1 FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                                    WHERE wo.source_di_id = ir.id AND {not_closed})) \
          ORDER BY ir.id DESC LIMIT 1",
        open = crate::di::stats::OPEN_SQL,
        not_closed = crate::wo::backlog::NOT_CLOSED_SQL,
    );
    db.query_one(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [meter_id.into(), level.as_str().into()],
    ))
    .await?
    .map(|r| r.try_get::<String>("", "code"))
    .transpose()
    .map_err(|e| decode_err("code", e))
}

/// Raises the intervention request of a reading that crossed `level` (or names the open one).
/// The request is filed by the person who recorded the reading, on the equipment's location, as an
/// inspection request (condition-based work, EN 13306) of origin `iot` (measure / sensor), with the
/// urgency of the level; it records the reading that raised it.
pub async fn raise_threshold_request(
    db: &impl ConnectionTrait,
    reading: &MeterReading,
    equipment_id: i64,
    level: ThresholdLevel,
    actor_id: i32,
) -> AppResult<ThresholdOutcome> {
    if let Some(code) = open_threshold_request(db, reading.meter_id, level).await? {
        return Ok(ThresholdOutcome::AlreadyOpen(code));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {}, e.installed_at_node_id AS node_id, e.asset_id_code AS asset_code \
                   FROM {METER_FROM} JOIN equipment e ON e.id = em.equipment_id WHERE em.id = ?",
                meter_select()
            ),
            [reading.meter_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment_meters".into(),
            id: reading.meter_id.to_string(),
        })?;
    let node_id: Option<i64> = row.try_get("", "node_id").map_err(|e| decode_err("node_id", e))?;
    let asset_code: String = row.try_get("", "asset_code").map_err(|e| decode_err("asset_code", e))?;
    let meter = map_meter(&row)?;
    let Some(org_node_id) = node_id else {
        return Err(AppError::ValidationFailed(vec![format!(
            "{GATE_ASSET_METER_THRESHOLD_NO_LOCATION}:asset={asset_code}: L'équipement {asset_code} n'a pas d'emplacement : \
             aucune demande d'intervention ne peut être créée pour le seuil franchi."
        )]));
    };
    let threshold = match level {
        ThresholdLevel::Alert => meter.alert_threshold,
        ThresholdLevel::Critical => meter.critical_threshold,
    }
    .unwrap_or_default();
    let unit = meter.unit.clone().map(|u| format!(" {u}")).unwrap_or_default();
    let side = match meter.threshold_direction.as_deref().and_then(ThresholdDirection::parse) {
        Some(ThresholdDirection::Below) => "inférieur ou égal",
        _ => "supérieur ou égal",
    };
    let symptom = crate::di::reference_catalog::resolve_system_di_symptom_id(db, &meter.meter_type).await?;
    let input = crate::di::queries::DiCreateInput {
        equipment_id,
        org_node_id,
        title: format!(
            "Seuil {} franchi — {} : {}{unit}",
            level.label_fr(),
            meter.name,
            reading.reading_value
        ),
        description: format!(
            "Relevé du {} sur le compteur « {} » : {}{unit}, {side} au seuil {} ({threshold}{unit}). \
             Demande créée automatiquement par le relevé.",
            reading.reading_at,
            meter.name,
            reading.reading_value,
            level.label_fr()
        ),
        origin_type: "iot".into(),
        request_type: "inspection".into(),
        symptom_code_id: Some(symptom),
        // A reading tells nothing about production impact: the reviewer assesses it.
        impact_level: "unknown".into(),
        production_impact: false,
        safety_flag: false,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: level.urgency().as_str().to_string(),
        observed_at: Some(reading.reading_at.clone()),
        submitter_id: i64::from(actor_id),
        source_inspection_anomaly_id: None,
    };
    let di = crate::di::queries::create_intervention_request_in(db, input).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE intervention_requests SET source_meter_reading_id = ?, source_threshold_level = ? WHERE id = ?",
        [reading.id.into(), level.as_str().into(), di.id.into()],
    ))
    .await?;
    Ok(ThresholdOutcome::Raised(di.id, di.code))
}

/// Evaluates the thresholds of the reading's meter and writes the outcome on `reading`. The
/// request is raised in a savepoint of `txn`: if it cannot be raised, nothing of it is kept and the
/// reason is returned on the reading, which is recorded anyway.
async fn apply_threshold_outcome(
    txn: &sea_orm::DatabaseTransaction,
    reading: &mut MeterReading,
    equipment_id: i64,
    actor_id: i32,
) -> AppResult<()> {
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT alert_threshold, critical_threshold, threshold_direction FROM equipment_meters WHERE id = ?",
            [reading.meter_id.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(()) };
    let alert: Option<f64> = row
        .try_get("", "alert_threshold")
        .map_err(|e| decode_err("alert_threshold", e))?;
    let critical: Option<f64> = row
        .try_get("", "critical_threshold")
        .map_err(|e| decode_err("critical_threshold", e))?;
    let direction: Option<String> = row
        .try_get("", "threshold_direction")
        .map_err(|e| decode_err("threshold_direction", e))?;
    let direction = direction.as_deref().and_then(ThresholdDirection::parse);
    let Some(level) = crossed_level(reading.reading_value, alert, critical, direction) else {
        return Ok(());
    };
    reading.threshold_level = Some(level.as_str().to_string());
    let savepoint = txn.begin().await?;
    match raise_threshold_request(&savepoint, reading, equipment_id, level, actor_id).await {
        Ok(outcome) => {
            savepoint.commit().await?;
            match outcome {
                ThresholdOutcome::Raised(id, code) => {
                    reading.raised_request_id = Some(id);
                    reading.raised_request_code = Some(code);
                }
                ThresholdOutcome::AlreadyOpen(code) => reading.open_request_code = Some(code),
            }
        }
        Err(e) => {
            savepoint.rollback().await?;
            tracing::warn!(
                meter_id = reading.meter_id,
                "threshold crossed, request not raised: {e}"
            );
            reading.threshold_problem = Some(match e {
                AppError::ValidationFailed(msgs) => msgs.join(" "),
                other => other.to_string(),
            });
        }
    }
    Ok(())
}
