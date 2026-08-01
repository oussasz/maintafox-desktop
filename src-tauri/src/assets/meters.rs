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
//!   unit                 → equipment_meters.unit
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
//! Corrections are recorded as a new reading with `quality_flag = 'corrected'`.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Types ────────────────────────────────────────────────────────────────────

/// Read-side meter definition record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMeter {
    pub id: i64,
    pub sync_id: String,
    pub asset_id: i64,
    pub name: String,
    pub meter_code: Option<String>,
    pub meter_type: String,
    pub unit: Option<String>,
    pub current_reading: f64,
    pub last_read_at: Option<String>,
    pub expected_rate_per_day: Option<f64>,
    pub rollover_value: Option<f64>,
    pub is_primary: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Payload for creating a new meter definition on an asset.
#[derive(Debug, Deserialize)]
pub struct CreateAssetMeterPayload {
    pub asset_id: i64,
    pub name: String,
    pub meter_code: Option<String>,
    pub meter_type: String,
    pub unit: Option<String>,
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
    pub created_at: String,
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
    AppError::Internal(anyhow::anyhow!(
        "meter row decode failed for column '{column}': {e}"
    ))
}

fn map_meter(row: &QueryResult) -> AppResult<AssetMeter> {
    Ok(AssetMeter {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
        sync_id: row
            .try_get::<String>("", "sync_id")
            .map_err(|e| decode_err("sync_id", e))?,
        asset_id: row
            .try_get::<i64>("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        name: row
            .try_get::<String>("", "name")
            .map_err(|e| decode_err("name", e))?,
        meter_code: row
            .try_get::<Option<String>>("", "meter_code")
            .map_err(|e| decode_err("meter_code", e))?,
        meter_type: row
            .try_get::<String>("", "meter_type")
            .map_err(|e| decode_err("meter_type", e))?,
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
    })
}

fn map_reading(row: &QueryResult) -> AppResult<MeterReading> {
    Ok(MeterReading {
        id: row
            .try_get::<i64>("", "id")
            .map_err(|e| decode_err("id", e))?,
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
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

// ─── Validation helpers ───────────────────────────────────────────────────────

/// Validate that a code exists in a given lookup domain.
async fn validate_lookup(
    db: &impl ConnectionTrait,
    domain_key: &str,
    code: &str,
    field_label: &str,
) -> AppResult<()> {
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
            "Valeur '{code}' introuvable dans le domaine '{domain_key}' ({field_label})."
        )]));
    }
    Ok(())
}

/// Assert that an equipment row exists and is not soft-deleted.
async fn assert_asset_exists(
    db: &impl ConnectionTrait,
    asset_id: i64,
) -> AppResult<()> {
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
async fn assert_meter_exists(
    db: &impl ConnectionTrait,
    meter_id: i64,
) -> AppResult<(i64, Option<f64>)> {
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
            "Un seul compteur primaire est autorise par equipement. \
             Desactivez le compteur primaire existant avant d'en definir un nouveau."
                .into(),
        ]));
    }
    Ok(())
}

// ─── Select fragments ─────────────────────────────────────────────────────────

const METER_SELECT: &str = r"
    id, sync_id, equipment_id, name, meter_code, meter_type, unit,
    current_reading, last_read_at, expected_rate_per_day, rollover_value,
    is_primary, is_active, created_at, updated_at
";

const READING_SELECT: &str = r"
    id, meter_id, reading_value, reading_at, source_type,
    source_reference, quality_flag, created_by_id, created_at
";

// ─── Service functions ────────────────────────────────────────────────────────

/// List all meters for an asset.
///
/// Returns active meters first, ordered by `is_primary DESC, name ASC`.
pub async fn list_asset_meters(
    db: &DatabaseConnection,
    asset_id: i64,
) -> AppResult<Vec<AssetMeter>> {
    let sql = format!(
        "SELECT {METER_SELECT} \
         FROM equipment_meters \
         WHERE equipment_id = ? \
         ORDER BY is_active DESC, is_primary DESC, name ASC"
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
        errors.push("Le nom du compteur est obligatoire.".into());
    }

    let initial = payload.initial_reading.unwrap_or(0.0);
    if initial < 0.0 {
        errors.push("Le releve initial ne peut pas etre negatif.".into());
    }

    if let Some(rv) = payload.rollover_value {
        if rv <= 0.0 {
            errors.push(
                "La valeur de retournement (rollover) doit etre strictement positive.".into(),
            );
        }
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
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
    let sync_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO equipment_meters \
         (sync_id, equipment_id, name, meter_code, meter_type, unit, \
          current_reading, expected_rate_per_day, rollover_value, \
          is_primary, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        [
            sync_id.clone().into(),
            payload.asset_id.into(),
            name.into(),
            meter_code.into(),
            payload.meter_type.into(),
            payload.unit.into(),
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
            &format!(
                "SELECT {METER_SELECT} FROM equipment_meters WHERE sync_id = ?"
            ),
            [sync_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "meter created but not found after insert"
            ))
        })?;
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
    let txn = db.begin().await?;

    // ── 1. Validate meter exists and is active ───────────────────────────
    let (_equipment_id, rollover_value) = assert_meter_exists(&txn, payload.meter_id).await?;

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
            "La valeur du releve ne peut pas etre negative.".into(),
        ]));
    }

    // ── 4. Quality flag: default to 'accepted' ──────────────────────────
    let quality_flag = payload
        .quality_flag
        .as_deref()
        .unwrap_or("accepted")
        .to_string();

    let now = Utc::now().to_rfc3339();
    let reading_at = payload.reading_at.unwrap_or_else(|| now.clone());

    // ── 5. Monotonic timestamp check (skip for corrections) ──────────────
    if quality_flag != "corrected" {
        let latest = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT reading_at, reading_value FROM asset_meter_readings \
                 WHERE meter_id = ? AND quality_flag != 'corrected' \
                 ORDER BY reading_at DESC LIMIT 1",
                [payload.meter_id.into()],
            ))
            .await?;

        if let Some(prev_row) = latest {
            let prev_at: String = prev_row
                .try_get("", "reading_at")
                .map_err(|e| decode_err("reading_at", e))?;
            if reading_at <= prev_at {
                return Err(AppError::ValidationFailed(vec![format!(
                    "L'horodatage du releve ({reading_at}) doit etre posterieur \
                     au dernier releve ({prev_at})."
                )]));
            }

            // Validate monotonic value unless rollover is configured
            let prev_val: f64 = prev_row
                .try_get("", "reading_value")
                .map_err(|e| decode_err("reading_value", e))?;
            if payload.reading_value < prev_val && rollover_value.is_none() {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Le releve ({}) est inferieur au precedent ({prev_val}). \
                     Si le compteur se remet a zero, configurez une valeur \
                     de retournement (rollover_value).",
                    payload.reading_value
                )]));
            }
        }
    }

    // ── 6. Insert the reading row ────────────────────────────────────────
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO asset_meter_readings \
         (meter_id, reading_value, reading_at, source_type, \
          source_reference, quality_flag, created_by_id, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            payload.meter_id.into(),
            payload.reading_value.into(),
            reading_at.clone().into(),
            payload.source_type.into(),
            payload.source_reference.into(),
            quality_flag.clone().into(),
            (actor_id as i64).into(),
            now.clone().into(),
        ],
    ))
    .await?;

    // ── 7. Update equipment_meters current value ─────────────────────────
    // Only update if the new reading is the most recent accepted reading.
    if quality_flag != "corrected" {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment_meters SET current_reading = ?, \
             last_read_at = ?, updated_at = ? \
             WHERE id = ?",
            [
                payload.reading_value.into(),
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
            &format!(
                "SELECT {READING_SELECT} FROM asset_meter_readings \
                 WHERE meter_id = ? ORDER BY id DESC LIMIT 1"
            ),
            [payload.meter_id.into()],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "meter reading created but not found after insert"
            ))
        })?;
    let reading = map_reading(&row)?;

    txn.commit().await?;

    tracing::info!(
        reading_id = reading.id,
        meter_id = reading.meter_id,
        value = reading.reading_value,
        quality = %reading.quality_flag,
        "meter reading recorded (actor={})", actor_id
    );
    Ok(reading)
}

/// Get the latest reading for a meter.
///
/// Returns the most recent accepted (non-corrected) reading, or `None`
/// if the meter has no readings yet.
pub async fn get_latest_meter_value(
    db: &DatabaseConnection,
    meter_id: i64,
) -> AppResult<Option<MeterReading>> {
    let sql = format!(
        "SELECT {READING_SELECT} FROM asset_meter_readings \
         WHERE meter_id = ? AND quality_flag != 'corrected' \
         ORDER BY reading_at DESC LIMIT 1"
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
    let sql = format!(
        "SELECT {READING_SELECT} FROM asset_meter_readings \
         WHERE meter_id = ? \
         ORDER BY reading_at DESC, id DESC \
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
