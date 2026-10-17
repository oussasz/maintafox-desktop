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
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::domain::WoStatus;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WoIntervener {
    pub id: i64,
    pub work_order_id: i64,
    /// Login of the person, when they have one.
    pub intervener_id: Option<i64>,
    /// The person (personnel register) — set for everyone booked since migration 190.
    #[serde(default)]
    pub personnel_id: Option<i64>,
    /// Paid at the overtime rate.
    #[serde(default)]
    pub is_overtime: bool,
    pub skill_id: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub hours_worked: Option<f64>,
    pub hourly_rate: Option<f64>,
    pub notes: Option<String>,
    /// The person's name (personnel register), else the login's display name.
    #[serde(default)]
    pub intervener_display_name: Option<String>,
    /// Skill label from reference_values when skill_id is set.
    #[serde(default)]
    pub skill_label: Option<String>,
    /// How `hourly_rate` was obtained: `provided` | `rate_card` | `missing`.
    #[serde(default = "default_rate_source")]
    pub rate_source: String,
}

fn default_rate_source() -> String {
    "missing".to_string()
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
    AppError::Internal(anyhow::anyhow!("WoIntervener row decode error for '{field}': {e}"))
}

fn map_intervener(row: &sea_orm::QueryResult) -> AppResult<WoIntervener> {
    let hourly_rate = row
        .try_get::<Option<f64>>("", "hourly_rate")
        .map_err(|e| decode_err("hourly_rate", e))?;
    Ok(WoIntervener {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        work_order_id: row
            .try_get::<i64>("", "work_order_id")
            .map_err(|e| decode_err("work_order_id", e))?,
        intervener_id: row
            .try_get::<Option<i64>>("", "intervener_id")
            .map_err(|e| decode_err("intervener_id", e))?,
        personnel_id: row
            .try_get::<Option<i64>>("", "personnel_id")
            .map_err(|e| decode_err("personnel_id", e))?,
        is_overtime: row
            .try_get::<i64>("", "is_overtime")
            .map_err(|e| decode_err("is_overtime", e))?
            != 0,
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
        hourly_rate,
        notes: row
            .try_get::<Option<String>>("", "notes")
            .map_err(|e| decode_err("notes", e))?,
        intervener_display_name: row
            .try_get::<Option<String>>("", "intervener_display_name")
            .unwrap_or(None),
        skill_label: row.try_get::<Option<String>>("", "skill_label").unwrap_or(None),
        // Persisted snapshot — treat non-null as rate_card (or prior override).
        rate_source: if hourly_rate.is_some() {
            "rate_card".to_string()
        } else {
            "missing".to_string()
        },
    })
}

const LABOR_SELECT: &str = "\
    SELECT woi.id, woi.work_order_id, woi.intervener_id, woi.personnel_id, woi.is_overtime, woi.skill_id, \
    woi.started_at, woi.ended_at, woi.hours_worked, woi.hourly_rate, woi.notes, \
    COALESCE(p.full_name, ua.display_name, ua.username) AS intervener_display_name, \
    rv.label AS skill_label \
 FROM work_order_interveners woi \
 LEFT JOIN user_accounts ua ON ua.id = woi.intervener_id \
 LEFT JOIN personnel p ON p.id = COALESCE(woi.personnel_id, ua.personnel_id) \
 LEFT JOIN reference_values rv ON rv.id = woi.skill_id";

/// Load the WO status code for a guard check.
async fn load_wo_status_code(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<String> {
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
        chrono::DateTime::parse_from_rfc3339(s).ok().or_else(|| {
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

pub async fn add_labor_entry(db: &DatabaseConnection, input: AddLaborInput) -> AppResult<WoIntervener> {
    let personnel_id = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ua.personnel_id FROM user_accounts ua JOIN personnel p ON p.id = ua.personnel_id \
             WHERE ua.id = ? AND ua.deleted_at IS NULL",
            [input.intervener_id.into()],
        ))
        .await?
        .and_then(|row| row.try_get::<Option<i64>>("", "personnel_id").ok())
        .flatten();
    book_labor(
        db,
        LaborBooking {
            wo_id: input.wo_id,
            intervener_id: Some(input.intervener_id),
            personnel_id,
            is_overtime: false,
            skill_id: input.skill_id,
            started_at: input.started_at,
            ended_at: input.ended_at,
            hours_worked: input.hours_worked,
            hourly_rate: input.hourly_rate,
            notes: input.notes,
        },
    )
    .await
}

/// Labour for a person of the register, with or without a login (contractor technicians,
/// shop-floor staff). Their login, when they have one, is recorded too.
#[derive(Debug, Clone, Deserialize)]
pub struct AddPersonnelLaborInput {
    pub wo_id: i64,
    pub personnel_id: i64,
    #[serde(default)]
    pub is_overtime: bool,
    pub skill_id: Option<i64>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub hours_worked: Option<f64>,
    pub hourly_rate: Option<f64>,
    pub notes: Option<String>,
}

pub async fn add_personnel_labor_entry(
    db: &DatabaseConnection,
    input: AddPersonnelLaborInput,
) -> AppResult<WoIntervener> {
    let person = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.employment_status, \
                    (SELECT ua.id FROM user_accounts ua \
                      WHERE ua.personnel_id = p.id AND ua.deleted_at IS NULL AND ua.is_active = 1 \
                      ORDER BY ua.id LIMIT 1) AS user_id \
             FROM personnel p WHERE p.id = ?",
            [input.personnel_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: input.personnel_id.to_string(),
        })?;
    let status: String = person.try_get("", "employment_status").unwrap_or_default();
    if status != "active" {
        return Err(crate::errors::org_validation_failed(
            "LABOR_PERSON_INACTIVE",
            "This person is no longer active: labour cannot be booked to them.",
            std::collections::HashMap::new(),
        ));
    }
    book_labor(
        db,
        LaborBooking {
            wo_id: input.wo_id,
            intervener_id: person.try_get::<Option<i64>>("", "user_id").ok().flatten(),
            personnel_id: Some(input.personnel_id),
            is_overtime: input.is_overtime,
            skill_id: input.skill_id,
            started_at: input.started_at,
            ended_at: input.ended_at,
            hours_worked: input.hours_worked,
            hourly_rate: input.hourly_rate,
            notes: input.notes,
        },
    )
    .await
}

struct LaborBooking {
    wo_id: i64,
    intervener_id: Option<i64>,
    personnel_id: Option<i64>,
    is_overtime: bool,
    skill_id: Option<i64>,
    started_at: Option<String>,
    ended_at: Option<String>,
    hours_worked: Option<f64>,
    hourly_rate: Option<f64>,
    notes: Option<String>,
}

async fn book_labor(db: &DatabaseConnection, input: LaborBooking) -> AppResult<WoIntervener> {
    let txn = db.begin().await?;
    let status_code = load_wo_status_code(&txn, input.wo_id).await?;
    let status = WoStatus::try_from_str(&status_code).map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    if status.is_terminal() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Impossible d'ajouter une entrée de main-d'œuvre à un OT {status_code}."
        )]));
    }

    // Labour is recorded after the fact: a start or end later than now (5 min clock tolerance)
    // would book hours that have not been worked.
    let latest_allowed = chrono::Utc::now() + chrono::Duration::minutes(5);
    for (raw, field) in [(&input.started_at, "started_at"), (&input.ended_at, "ended_at")] {
        if let Some(raw) = raw.as_deref().filter(|s| !s.trim().is_empty()) {
            let at = crate::wo::time::parse_utc_timestamp(raw, field)?;
            if at > latest_allowed {
                return Err(AppError::ValidationFailed(vec![
                    "Les heures de main-d'œuvre ne peuvent pas être dans le futur.".to_string(),
                ]));
            }
        }
    }

    // The person must hold what the work order requires (or a supervisor override must have
    // been recorded beforehand — `record_qualification_override`).
    match (input.personnel_id, input.intervener_id) {
        (Some(pid), _) => {
            crate::qualification::gate::enforce_work_order_assignment(&txn, pid, input.wo_id, None).await?;
        }
        // A login without a personnel card cannot be checked: refused when anything is required.
        (None, Some(uid)) => crate::qualification::gate::enforce_user_on_work_order(&txn, uid, input.wo_id).await?,
        (None, None) => {}
    }

    // Auto-compute hours_worked when both timestamps are provided
    let computed_hours = match (&input.started_at, &input.ended_at) {
        (Some(s), Some(e)) => elapsed_hours(s, e).or(input.hours_worked),
        _ => input.hours_worked,
    };

    let (resolved_rate, rate_source) = if let Some(rate) = input.hourly_rate {
        (Some(rate), "provided".to_string())
    } else {
        // Frozen at booking: the rate card valid on the day the work was done (its start, else
        // its end, else today) — a later card change never re-prices it.
        let work_day = input
            .started_at
            .as_deref()
            .or(input.ended_at.as_deref())
            .and_then(|t| t.get(..10))
            .map(str::to_string)
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
        match input.personnel_id {
            Some(pid) => match crate::personnel::queries::resolve_rate_pair_on(&txn, pid, &work_day).await? {
                Some((normal, overtime)) => (
                    Some(if input.is_overtime { overtime } else { normal }),
                    "rate_card".to_string(),
                ),
                None => (None, "missing".to_string()),
            },
            None => (None, "missing".to_string()),
        }
    };

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_order_interveners \
         (work_order_id, intervener_id, personnel_id, is_overtime, skill_id, started_at, ended_at, hours_worked, \
          hourly_rate, notes, entity_sync_id) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            input.wo_id.into(),
            input.intervener_id.into(),
            input.personnel_id.into(),
            i64::from(input.is_overtime).into(),
            input.skill_id.into(),
            input.started_at.clone().into(),
            input.ended_at.clone().into(),
            computed_hours.into(),
            resolved_rate.into(),
            input.notes.clone().into(),
            crate::sync::identity::new_entity_sync_id().into(),
        ],
    ))
    .await?;

    // Return the inserted row
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LABOR_SELECT} WHERE woi.rowid = last_insert_rowid()"),
            [],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to re-read labor entry after insert")))?;
    let mut entry = map_intervener(&row)?;
    entry.rate_source = rate_source;
    txn.commit().await?;
    Ok(entry)
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
    let txn = db.begin().await?;

    // Load the entry to validate it's open and compute elapsed hours
    let row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LABOR_SELECT} WHERE woi.id = ?"),
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

    txn.execute(Statement::from_sql_and_values(
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
    let updated = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LABOR_SELECT} WHERE woi.id = ?"),
            [intervener_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WoIntervener".into(),
            id: intervener_id.to_string(),
        })?;
    let entry = map_intervener(&updated)?;
    txn.commit().await?;
    Ok(entry)
}

// ═══════════════════════════════════════════════════════════════════════════════
// B2) list_labor_candidates — people who can be booked on a WO
// ═══════════════════════════════════════════════════════════════════════════════

/// A person from the personnel register who has a login (labour entries reference
/// `user_accounts`, so only linked personnel can be booked).
#[derive(Debug, Clone, Serialize)]
pub struct WoLaborCandidate {
    pub user_id: i64,
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub position_name: Option<String>,
    pub availability_status: String,
}

/// Active personnel linked to an active, non-deleted login, ordered by name.
pub async fn list_labor_candidates(db: &DatabaseConnection) -> AppResult<Vec<WoLaborCandidate>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                (SELECT ua.id FROM user_accounts ua
                  WHERE ua.personnel_id = p.id AND ua.deleted_at IS NULL AND ua.is_active = 1
                  ORDER BY ua.id LIMIT 1) AS user_id,
                p.id AS personnel_id,
                p.employee_code,
                p.full_name,
                pos.name AS position_name,
                p.availability_status
             FROM personnel p
             LEFT JOIN positions pos ON pos.id = p.position_id
             WHERE p.employment_status = 'active'
               AND EXISTS (SELECT 1 FROM user_accounts ua
                            WHERE ua.personnel_id = p.id AND ua.deleted_at IS NULL AND ua.is_active = 1)
             ORDER BY p.full_name COLLATE NOCASE ASC, p.id ASC"
                .to_string(),
        ))
        .await?;

    rows.iter()
        .map(|row| {
            Ok(WoLaborCandidate {
                user_id: row
                    .try_get::<i64>("", "user_id")
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("user_id decode: {e}")))?,
                personnel_id: row
                    .try_get::<i64>("", "personnel_id")
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("personnel_id decode: {e}")))?,
                employee_code: row.try_get::<String>("", "employee_code").unwrap_or_default(),
                full_name: row.try_get::<String>("", "full_name").unwrap_or_default(),
                position_name: row.try_get::<Option<String>>("", "position_name").unwrap_or(None),
                availability_status: row
                    .try_get::<String>("", "availability_status")
                    .unwrap_or_else(|_| "available".to_string()),
            })
        })
        .collect()
}

/// A person who can be booked for labour: every active person of the register, with or
/// without a login (contractor technicians included).
#[derive(Debug, Clone, Serialize)]
pub struct WoLaborPerson {
    pub personnel_id: i64,
    pub user_id: Option<i64>,
    pub employee_code: String,
    pub full_name: String,
    pub position_name: Option<String>,
    pub employment_type: String,
    pub company_name: Option<String>,
    pub availability_status: String,
}

pub async fn list_labor_people(db: &DatabaseConnection) -> AppResult<Vec<WoLaborPerson>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.id AS personnel_id, \
                    (SELECT ua.id FROM user_accounts ua \
                      WHERE ua.personnel_id = p.id AND ua.deleted_at IS NULL AND ua.is_active = 1 \
                      ORDER BY ua.id LIMIT 1) AS user_id, \
                    p.employee_code, p.full_name, pos.name AS position_name, p.employment_type, \
                    ec.name AS company_name, p.availability_status \
             FROM personnel p \
             LEFT JOIN positions pos ON pos.id = p.position_id \
             LEFT JOIN external_companies ec ON ec.id = p.external_company_id \
             WHERE p.employment_status = 'active' \
             ORDER BY p.full_name COLLATE NOCASE ASC, p.id ASC"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(WoLaborPerson {
                personnel_id: row
                    .try_get::<i64>("", "personnel_id")
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("personnel_id decode: {e}")))?,
                user_id: row.try_get::<Option<i64>>("", "user_id").unwrap_or(None),
                employee_code: row.try_get::<String>("", "employee_code").unwrap_or_default(),
                full_name: row.try_get::<String>("", "full_name").unwrap_or_default(),
                position_name: row.try_get::<Option<String>>("", "position_name").unwrap_or(None),
                employment_type: row
                    .try_get::<String>("", "employment_type")
                    .unwrap_or_else(|_| "employee".to_string()),
                company_name: row.try_get::<Option<String>>("", "company_name").unwrap_or(None),
                availability_status: row
                    .try_get::<String>("", "availability_status")
                    .unwrap_or_else(|_| "available".to_string()),
            })
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) list_labor_entries
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn list_labor_entries(db: &DatabaseConnection, wo_id: i64) -> AppResult<Vec<WoIntervener>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{LABOR_SELECT} WHERE woi.work_order_id = ? ORDER BY woi.id ASC"),
            [wo_id.into()],
        ))
        .await?;
    rows.iter().map(map_intervener).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) remove_labor_entry
// ═══════════════════════════════════════════════════════════════════════════════

pub async fn remove_labor_entry(db: &DatabaseConnection, intervener_id: i64, _actor_id: i64) -> AppResult<()> {
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

    // Guard: only allowed in draft / planning / ready
    let status_code = load_wo_status_code(db, wo_id).await?;
    let allowed = matches!(status_code.as_str(), "draft" | "planning" | "ready");
    if !allowed {
        return Err(AppError::ValidationFailed(vec![format!(
            "La suppression d'une entrée de main-d'œuvre n'est pas autorisée au statut '{status_code}'."
        )]));
    }

    // Universal Sync delete: skip staging (no outbox delete helper for interveners yet).
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM work_order_interveners WHERE id = ?",
        [intervener_id.into()],
    ))
    .await?;

    Ok(())
}
