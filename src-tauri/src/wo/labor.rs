//! WO labor tracking — intervener time entries and hours accumulation.
//!
//! Phase 2 - Sub-phase 05 - File 02 - Sprint S1.
//!
//! `work_order_interveners` records per-person labor segments: start time, end time,
//! computed or manually entered hours, and hourly rate. The aggregate
//! `active_labor_hours` on `work_orders` is written back at mechanical completion.
//!
//! Business rules:
//!   - Entries may be added when WO is not yet closed/cancelled.
//!   - If both `started_at` and `ended_at` are provided at insert, `hours_worked` is
//!     auto-computed from the elapsed duration.
//!   - `remove_labor_entry` is only allowed while the WO is in draft, planned, or assigned.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use super::domain::WoStatus;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoIntervener {
    pub id: i64,
    pub work_order_id: i64,
    pub intervener_id: i64,
    pub skill_id: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub hours_worked: Option<f64>,
    pub hourly_rate: Option<f64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddLaborInput {
    pub wo_id: i64,
    pub intervener_id: i64,
    pub skill_id: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub hours_worked: Option<f64>,
    pub hourly_rate: Option<f64>,
    pub notes: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn decode_err(field: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "WoIntervener row decode error for '{field}': {e}"
    ))
}

fn map_intervener(row: &sea_orm::QueryResult) -> AppResult<WoIntervener> {
    Ok(WoIntervener {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        intervener_id: row
            .try_get::<i64>("", "intervener_id")
            .map_err(|e| decode_err("intervener_id", e))?,
        skill_id: row
            .try_get::<Option<i64>>("", "skill_id")
            .map_err(|e| decode_err("skill_id", e))?,
        started_at: row
            .try_get::<Option<String>>("", "started_at")
            .map_err(|e| decode_err("started_at", e))?,
        ended_at: row
            .try_get::<Option<String>>("", "ended_at")
            .map_err(|e| decode_err("ended_at", e))?,
        hours_worked: row
            .try_get::<Option<f64>>("", "hours_worked")
            .map_err(|e| decode_err("hours_worked", e))?,
        hourly_rate: row
            .try_get::<Option<f64>>("", "hourly_rate")
            .map_err(|e| decode_err("hourly_rate", e))?,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
    })
}

/// Load the WO status code for a guard check.
async fn load_wo_status_code(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code \
             FROM work_orders wo \
             JOIN work_order_statuses wos ON wos.id = wo.status_id \
             WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    row.try_get::<String>("", "status_code")
        .map_err(|e| decode_err("status_code", e))
}

/// Compute elapsed hours between two ISO timestamp strings.
/// Returns `None` if either string is not parseable.
fn elapsed_hours(start: &str, end: &str) -> Option<f64> {
    let parse = |s: &str| -> Option<chrono::DateTime<chrono::FixedOffset>> {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .or_else(|| {
                chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
                    .ok()
                    .map(|dt| dt.and_utc().fixed_offset())
            })
    };
    let s = parse(start)?;
    let e = parse(end)?;
    let diff = e.signed_duration_since(s);
    if diff.num_seconds() <= 0 {
        return None;
    }
    Some(diff.num_seconds() as f64 / 3600.0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) add_labor_entry
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn add_labor_entry(
    db: &DatabaseConnection,
    input: AddLaborInput,
) -> AppResult<WoIntervener> {
    let status_code = load_wo_status_code(db, input.wo_id).await?;
    let status = WoStatus::try_from_str(&status_code)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    if status.is_terminal() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible d'ajouter une entrée de main-d'œuvre à un OT {status_code}."
        )]));
    }

    // Auto-compute hours_worked when both timestamps are provided
    let computed_hours = match (&input.started_at, &input.ended_at) {
        (Some(s), Some(e)) => elapsed_hours(s, e).or(input.hours_worked),
        _ => input.hours_worked,
    };

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_interveners \
         (work_order_id, intervener_id, skill_id, started_at, ended_at, hours_worked, hourly_rate, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            input.intervener_id.into(),
            input
                .skill_id
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<i64>)),
            input
                .started_at
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            input
                .ended_at
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
            computed_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<f64>)),
            input
                .hourly_rate
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<f64>)),
            input
                .notes
                .clone()
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<String>)),
        ],
    ))
    .await?;

    // Return the inserted row
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, intervener_id, skill_id, started_at, ended_at, \
                    hours_worked, hourly_rate, notes \
             FROM work_order_interveners \
             WHERE rowid = last_insert_rowid()",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("Failed to re-read labor entry after insert"))
        })?;
    map_intervener(&row)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) close_labor_entry
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn close_labor_entry(
    db: &DatabaseConnection,
    intervener_id: i64,
    ended_at: String,
    _actor_id: i64,
) -> AppResult<WoIntervener> {
    // Load the entry to validate it's open and compute elapsed hours
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, intervener_id, skill_id, started_at, ended_at, \
                    hours_worked, hourly_rate, notes \
             FROM work_order_interveners WHERE id = ?",
            [intervener_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoIntervener".into(),
            id: intervener_id.to_string(),
        })?;

    let entry = map_intervener(&row)?;

    if entry.ended_at.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Cette entrée de main-d'œuvre est déjà clôturée.".to_string(),
        ]));
    }

    // Compute elapsed hours if started_at is available
    let computed_hours = entry
        .started_at
        .as_deref()
        .and_then(|s| elapsed_hours(s, &ended_at))
        .or(entry.hours_worked);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_order_interveners SET ended_at = ?, hours_worked = ? WHERE id = ?",
        [
            ended_at.clone().into(),
            computed_hours
                .map(sea_orm::Value::from)
                .unwrap_or(sea_orm::Value::from(None::<f64>)),
            intervener_id.into(),
        ],
    ))
    .await?;

    // Return updated row
    let updated = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, intervener_id, skill_id, started_at, ended_at, \
                    hours_worked, hourly_rate, notes \
             FROM work_order_interveners WHERE id = ?",
            [intervener_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoIntervener".into(),
            id: intervener_id.to_string(),
        })?;
    map_intervener(&updated)
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) list_labor_entries
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_labor_entries(
    db: &DatabaseConnection,
    wo_id: i64,
) -> AppResult<Vec<WoIntervener>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, work_order_id, intervener_id, skill_id, started_at, ended_at, \
                    hours_worked, hourly_rate, notes \
             FROM work_order_interveners \
             WHERE work_order_id = ? \
             ORDER BY id ASC",
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_intervener).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) remove_labor_entry
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn remove_labor_entry(
    db: &DatabaseConnection,
    intervener_id: i64,
    _actor_id: i64,
) -> AppResult<()> {
    // Load the entry to get wo_id
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT work_order_id FROM work_order_interveners WHERE id = ?",
            [intervener_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoIntervener".into(),
            id: intervener_id.to_string(),
        })?;
    let wo_id: i64 = row
        .try_get::<i64>("", "work_order_id")
        .map_err(|e| decode_err("work_order_id", e))?;

    // Guard: only allowed in draft, planned, or assigned
    let status_code = load_wo_status_code(db, wo_id).await?;
    let allowed = matches!(
        status_code.as_str(),
        "draft" | "planned" | "ready_to_schedule" | "assigned"
    );
    if !allowed {
        return Err(AppError::ValidationFailed(vec![format!(
            "La suppression d'une entrée de main-d'œuvre n'est pas autorisée au statut '{status_code}'."
        )]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM work_order_interveners WHERE id = ?",
        [intervener_id.into()],
    ))
    .await?;

    Ok(())
}
