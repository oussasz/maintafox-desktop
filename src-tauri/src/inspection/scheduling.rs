//! Recurring inspection rounds (PRD §6.25, Phase 6.3): schedules, round generation and the
//! missed-round sweep. The calendar and meter rules are the PM ones (`pm::schedule`,
//! `pm::generation`), so a "every 1 month" round and a "every 1 month" PM fall on the same days.
//!
//! - A schedule walks one checklist template: its ordered, equipment-bound checkpoints are the
//!   route. Rounds use the template's current published version at generation time.
//! - `calendar`: dates `anchor_at + k × interval` (months follow the calendar, clamped to the
//!   month end), generated up to `horizon_days` ahead. Each date is one round, identified in its
//!   schedule by `calendar:<due>`; generating again never duplicates it.
//! - `meter`: when the lifetime reading of the meter reaches `meter_start_reading + k ×
//!   meter_interval`, one round is due now (`meter:<meter>:<threshold>`); a new one only once the
//!   previous meter round is no longer open.
//! - Every generated round can be done until `due + grace_hours` (`window_end_at`). A round
//!   nobody started by then becomes `missed` at the next generation run. Dates whose window
//!   closed since the last run (the application was not running) are recorded as missed too:
//!   compliance counts them. On the very first run of a schedule, dates already closed are not
//!   back-filled (the schedule did not exist then).
//! - Saving a schedule removes its future rounds that nobody started yet (no result recorded),
//!   then generates again with the new rules.
//! - Generated rounds take a deterministic identity (schedule, schedule version, due basis), so
//!   two devices generating the same schedule converge on the same rows.

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::pm::schedule::{nth_date, parse_recurrence, Recurrence};

use super::domain::{
    GenerateInspectionRoundsInput, GenerateInspectionRoundsResult, InspectionSchedule, InspectionScheduleIssue,
    InspectionScheduleSummary, SaveInspectionScheduleInput,
};

pub const TRIGGER_CALENDAR: &str = "calendar";
pub const TRIGGER_METER: &str = "meter";
pub const INTERVAL_UNITS: &[&str] = &["hour", "day", "week", "month", "year"];
/// Longest look-ahead of a calendar schedule.
pub const MAX_HORIZON_DAYS: i64 = 366;
/// Longest time a round stays doable after its due time (31 days).
pub const MAX_GRACE_HOURS: i64 = 744;
/// Most rounds one schedule creates in one run; beyond, the run reports it (shorten the horizon
/// or lengthen the interval) instead of creating thousands of rows.
pub const MAX_ROUNDS_PER_RUN: usize = 1000;

/// Round statuses that still wait for the work.
const OPEN_STATUSES_SQL: &str = "'scheduled', 'released', 'in_progress'";

const SCHEDULE_COLUMNS: &str = "s.id, s.entity_sync_id, s.code, s.name, s.template_id, s.trigger_kind, \
     s.interval_unit, s.interval_value, s.anchor_at, s.meter_id, s.meter_interval, s.meter_start_reading, \
     s.assigned_to_id, s.assigned_team_id, s.grace_hours, s.horizon_days, s.is_active, s.last_generated_at, \
     s.row_version, s.created_at, s.updated_at";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("inspection schedule decode '{field}': {err}"))
}

fn gate(code: &str, detail: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {detail}")])
}

/// Timestamps written by this module: UTC, whole seconds, `Z`.
pub fn ts(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_ts(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn map_schedule(row: &sea_orm::QueryResult) -> AppResult<InspectionSchedule> {
    macro_rules! get {
        ($name:literal) => {
            row.try_get("", $name).map_err(|e| decode_err($name, e))?
        };
    }
    let active: i64 = get!("is_active");
    Ok(InspectionSchedule {
        id: get!("id"),
        entity_sync_id: get!("entity_sync_id"),
        code: get!("code"),
        name: get!("name"),
        template_id: get!("template_id"),
        trigger_kind: get!("trigger_kind"),
        interval_unit: get!("interval_unit"),
        interval_value: get!("interval_value"),
        anchor_at: get!("anchor_at"),
        meter_id: get!("meter_id"),
        meter_interval: get!("meter_interval"),
        meter_start_reading: get!("meter_start_reading"),
        assigned_to_id: get!("assigned_to_id"),
        assigned_team_id: get!("assigned_team_id"),
        grace_hours: get!("grace_hours"),
        horizon_days: get!("horizon_days"),
        is_active: active != 0,
        last_generated_at: get!("last_generated_at"),
        row_version: get!("row_version"),
        created_at: get!("created_at"),
        updated_at: get!("updated_at"),
    })
}

fn summary_sql(where_clause: &str) -> String {
    format!(
        "SELECT {SCHEDULE_COLUMNS}, t.code AS template_code, t.name AS template_name, \
         p.full_name AS assigned_to_name, tm.name AS assigned_team_name, \
         m.name AS meter_name, m.unit AS meter_unit, m.equipment_id AS meter_asset_id, \
         e.asset_id_code AS meter_asset_code, \
         (m.current_reading + m.reading_offset) AS meter_lifetime_reading, \
         (SELECT MIN(r.scheduled_at) FROM inspection_rounds r WHERE r.schedule_id = s.id \
             AND r.status IN ({OPEN_STATUSES_SQL})) AS next_due_at, \
         (SELECT COUNT(*) FROM inspection_rounds r WHERE r.schedule_id = s.id \
             AND r.status IN ({OPEN_STATUSES_SQL})) AS open_round_count, \
         (SELECT COUNT(*) FROM inspection_rounds r WHERE r.schedule_id = s.id \
             AND r.status IN ({OPEN_STATUSES_SQL}) AND julianday(r.scheduled_at) < julianday('now')) \
             AS overdue_round_count, \
         (SELECT COUNT(*) FROM inspection_rounds r WHERE r.schedule_id = s.id AND r.status = 'missed') \
             AS missed_round_count \
         FROM inspection_schedules s \
         JOIN inspection_templates t ON t.id = s.template_id \
         LEFT JOIN personnel p ON p.id = s.assigned_to_id \
         LEFT JOIN teams tm ON tm.id = s.assigned_team_id \
         LEFT JOIN equipment_meters m ON m.id = s.meter_id \
         LEFT JOIN equipment e ON e.id = m.equipment_id \
         {where_clause}"
    )
}

fn map_summary(row: &sea_orm::QueryResult) -> AppResult<InspectionScheduleSummary> {
    macro_rules! get {
        ($name:literal) => {
            row.try_get("", $name).map_err(|e| decode_err($name, e))?
        };
    }
    Ok(InspectionScheduleSummary {
        schedule: map_schedule(row)?,
        template_code: get!("template_code"),
        template_name: get!("template_name"),
        assigned_to_name: get!("assigned_to_name"),
        assigned_team_name: get!("assigned_team_name"),
        meter_name: get!("meter_name"),
        meter_unit: get!("meter_unit"),
        meter_asset_id: get!("meter_asset_id"),
        meter_asset_code: get!("meter_asset_code"),
        meter_lifetime_reading: get!("meter_lifetime_reading"),
        next_due_at: get!("next_due_at"),
        open_round_count: get!("open_round_count"),
        overdue_round_count: get!("overdue_round_count"),
        missed_round_count: get!("missed_round_count"),
    })
}

pub async fn list_inspection_schedules(db: &impl ConnectionTrait) -> AppResult<Vec<InspectionScheduleSummary>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{} ORDER BY s.code ASC", summary_sql("")),
        ))
        .await?;
    rows.iter().map(map_summary).collect()
}

pub async fn get_inspection_schedule_summary(
    db: &impl ConnectionTrait,
    id: i64,
) -> AppResult<InspectionScheduleSummary> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            summary_sql("WHERE s.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionSchedule".into(),
            id: id.to_string(),
        })?;
    map_summary(&row)
}

async fn load_schedule(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<InspectionSchedule>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {SCHEDULE_COLUMNS} FROM inspection_schedules s WHERE s.id = ?"),
            [id.into()],
        ))
        .await?;
    row.map(|r| map_schedule(&r)).transpose()
}

/// The recurrence of a calendar schedule, read with the PM rules.
pub fn schedule_recurrence(unit: &str, value: i64) -> AppResult<Recurrence> {
    let mut trigger = Map::new();
    trigger.insert("interval_unit".into(), Value::from(unit));
    trigger.insert("interval_value".into(), Value::from(value));
    parse_recurrence(&trigger)
}

/// The fields of a schedule after validation (only the fields of its trigger kind are kept).
#[derive(Debug, Clone, PartialEq)]
pub struct ValidSchedule {
    pub code: String,
    pub name: String,
    pub trigger_kind: &'static str,
    pub interval_unit: Option<String>,
    pub interval_value: Option<i64>,
    pub anchor_at: Option<String>,
    pub meter_id: Option<i64>,
    pub meter_interval: Option<f64>,
    pub meter_start_reading: Option<f64>,
}

/// Rules that need no database: required fields, trigger, ranges, assignee.
pub fn validate_schedule_input(input: &SaveInspectionScheduleInput) -> AppResult<ValidSchedule> {
    let code = input.code.trim();
    if code.is_empty() {
        return Err(gate("GATE_INSP_SCHEDULE_CODE_REQUIRED", "the schedule needs a code."));
    }
    let name = input.name.trim();
    if name.is_empty() {
        return Err(gate("GATE_INSP_SCHEDULE_NAME_REQUIRED", "the schedule needs a name."));
    }
    if !(1..=MAX_HORIZON_DAYS).contains(&input.horizon_days) {
        return Err(gate(
            "GATE_INSP_SCHEDULE_HORIZON_RANGE",
            &format!("the look-ahead must be between 1 and {MAX_HORIZON_DAYS} days."),
        ));
    }
    if !(1..=MAX_GRACE_HOURS).contains(&input.grace_hours) {
        return Err(gate(
            "GATE_INSP_SCHEDULE_GRACE_RANGE",
            &format!("the time allowed after the due time must be between 1 and {MAX_GRACE_HOURS} hours."),
        ));
    }
    if input.assigned_to_id.is_none() && input.assigned_team_id.is_none() {
        return Err(gate(
            "GATE_INSP_SCHEDULE_ASSIGNEE_REQUIRED",
            "assign the rounds to a person or a team.",
        ));
    }
    match input.trigger_kind.as_str() {
        TRIGGER_CALENDAR => {
            let unit = input
                .interval_unit
                .as_deref()
                .map(str::trim)
                .filter(|u| INTERVAL_UNITS.contains(u));
            let (Some(unit), Some(value)) = (unit, input.interval_value.filter(|v| *v >= 1)) else {
                return Err(gate(
                    "GATE_INSP_SCHEDULE_INTERVAL_REQUIRED",
                    "a calendar schedule needs an interval (a number of hours, days, weeks, months or years).",
                ));
            };
            schedule_recurrence(unit, value)?;
            let Some(anchor) = input.anchor_at.as_deref().and_then(parse_ts) else {
                return Err(gate(
                    "GATE_INSP_SCHEDULE_ANCHOR_REQUIRED",
                    "a calendar schedule needs the date and time of its first round.",
                ));
            };
            Ok(ValidSchedule {
                code: code.to_string(),
                name: name.to_string(),
                trigger_kind: TRIGGER_CALENDAR,
                interval_unit: Some(unit.to_string()),
                interval_value: Some(value),
                anchor_at: Some(ts(anchor)),
                meter_id: None,
                meter_interval: None,
                meter_start_reading: None,
            })
        }
        TRIGGER_METER => {
            let Some(meter_id) = input.meter_id else {
                return Err(gate(
                    "GATE_INSP_SCHEDULE_METER_REQUIRED",
                    "a meter schedule needs the equipment meter it follows.",
                ));
            };
            let Some(interval) = input.meter_interval.filter(|v| v.is_finite() && *v > 0.0) else {
                return Err(gate(
                    "GATE_INSP_SCHEDULE_METER_INTERVAL",
                    "a meter schedule needs an interval greater than zero.",
                ));
            };
            if input.meter_start_reading.is_some_and(|v| !v.is_finite()) {
                return Err(gate(
                    "GATE_INSP_SCHEDULE_METER_INTERVAL",
                    "the start reading must be a number.",
                ));
            }
            Ok(ValidSchedule {
                code: code.to_string(),
                name: name.to_string(),
                trigger_kind: TRIGGER_METER,
                interval_unit: None,
                interval_value: None,
                anchor_at: None,
                meter_id: Some(meter_id),
                meter_interval: Some(interval),
                meter_start_reading: input.meter_start_reading,
            })
        }
        _ => Err(gate(
            "GATE_INSP_SCHEDULE_TRIGGER_INVALID",
            "the trigger must be calendar or meter.",
        )),
    }
}

/// Lifetime reading of an active meter (`None` when the meter is unknown or inactive).
async fn meter_lifetime(db: &impl ConnectionTrait, meter_id: i64) -> AppResult<Option<(String, f64)>> {
    let mut spec = Map::new();
    spec.insert("asset_meter_id".into(), Value::from(meter_id));
    Ok(crate::pm::generation::resolve_meter(db, &spec, None)
        .await?
        .map(|m| (m.reference, m.lifetime)))
}

/// Creates or updates a schedule, then generates its rounds (an active schedule) with the new
/// rules. Future rounds nobody started are replaced.
pub async fn save_inspection_schedule(
    db: &DatabaseConnection,
    input: SaveInspectionScheduleInput,
) -> AppResult<InspectionScheduleSummary> {
    let mut valid = validate_schedule_input(&input)?;
    let template = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active, current_version_id FROM inspection_templates WHERE id = ?",
            [input.template_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionTemplate".into(),
            id: input.template_id.to_string(),
        })?;
    let active: i64 = template
        .try_get("", "is_active")
        .map_err(|e| decode_err("is_active", e))?;
    let version: Option<i64> = template
        .try_get("", "current_version_id")
        .map_err(|e| decode_err("current_version_id", e))?;
    if active == 0 || version.is_none() {
        return Err(gate(
            "GATE_INSP_SCHEDULE_TEMPLATE_NOT_PUBLISHED",
            "the checklist is inactive or has no published version.",
        ));
    }
    if let Some(meter_id) = valid.meter_id {
        let Some((_, lifetime)) = meter_lifetime(db, meter_id).await? else {
            return Err(gate(
                "GATE_INSP_SCHEDULE_METER_INACTIVE",
                "the meter is unknown or inactive.",
            ));
        };
        // No start given: count from the reading when the schedule is saved, stored on the
        // schedule so it is visible and never recomputed silently.
        if valid.meter_start_reading.is_none() {
            valid.meter_start_reading = Some(lifetime);
        }
    }
    let taken = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inspection_schedules WHERE UPPER(code) = UPPER(?) AND id IS NOT ?",
            [valid.code.clone().into(), input.id.into()],
        ))
        .await?;
    if taken.is_some() {
        return Err(gate(
            "GATE_INSP_SCHEDULE_CODE_TAKEN",
            &format!("another schedule already uses the code {}.", valid.code),
        ));
    }

    let now = ts(Utc::now());
    let txn = db.begin().await?;
    let id = match input.id {
        None => {
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO inspection_schedules (entity_sync_id, code, name, template_id, trigger_kind, \
                 interval_unit, interval_value, anchor_at, meter_id, meter_interval, meter_start_reading, \
                 assigned_to_id, assigned_team_id, grace_hours, horizon_days, is_active, row_version, \
                 created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
                [
                    Uuid::new_v4().to_string().into(),
                    valid.code.clone().into(),
                    valid.name.clone().into(),
                    input.template_id.into(),
                    valid.trigger_kind.into(),
                    valid.interval_unit.clone().into(),
                    valid.interval_value.into(),
                    valid.anchor_at.clone().into(),
                    valid.meter_id.into(),
                    valid.meter_interval.into(),
                    valid.meter_start_reading.into(),
                    input.assigned_to_id.into(),
                    input.assigned_team_id.into(),
                    input.grace_hours.into(),
                    input.horizon_days.into(),
                    i64::from(input.is_active).into(),
                    now.clone().into(),
                    now.clone().into(),
                ],
            ))
            .await?;
            txn.query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?
            .try_get("", "id")
            .map_err(|e| decode_err("id", e))?
        }
        Some(id) => {
            let current = load_schedule(&txn, id).await?.ok_or_else(|| AppError::NotFound {
                entity: "InspectionSchedule".into(),
                id: id.to_string(),
            })?;
            if input.expected_row_version != Some(current.row_version) {
                return Err(AppError::ValidationFailed(vec![
                    "row_version mismatch on inspection_schedules.".into(),
                ]));
            }
            let updated = txn
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE inspection_schedules SET code = ?, name = ?, template_id = ?, trigger_kind = ?, \
                     interval_unit = ?, interval_value = ?, anchor_at = ?, meter_id = ?, meter_interval = ?, \
                     meter_start_reading = ?, assigned_to_id = ?, assigned_team_id = ?, grace_hours = ?, \
                     horizon_days = ?, is_active = ?, updated_at = ?, row_version = row_version + 1 \
                     WHERE id = ? AND row_version = ?",
                    [
                        valid.code.clone().into(),
                        valid.name.clone().into(),
                        input.template_id.into(),
                        valid.trigger_kind.into(),
                        valid.interval_unit.clone().into(),
                        valid.interval_value.into(),
                        valid.anchor_at.clone().into(),
                        valid.meter_id.into(),
                        valid.meter_interval.into(),
                        valid.meter_start_reading.into(),
                        input.assigned_to_id.into(),
                        input.assigned_team_id.into(),
                        input.grace_hours.into(),
                        input.horizon_days.into(),
                        i64::from(input.is_active).into(),
                        now.clone().into(),
                        id.into(),
                        current.row_version.into(),
                    ],
                ))
                .await?;
            if updated.rows_affected() == 0 {
                return Err(AppError::ValidationFailed(vec![
                    "Concurrent update on inspection_schedules.".into(),
                ]));
            }
            remove_unstarted_future_rounds(&txn, id, &now).await?;
            id
        }
    };
    txn.commit().await?;
    if input.is_active {
        generate_inspection_rounds(
            db,
            GenerateInspectionRoundsInput {
                as_of: None,
                schedule_id: Some(id),
            },
        )
        .await?;
    }
    get_inspection_schedule_summary(db, id).await
}

/// Future rounds of a schedule that nobody started and that hold no result: they were planned
/// with the previous rules.
async fn remove_unstarted_future_rounds(db: &impl ConnectionTrait, schedule_id: i64, now: &str) -> AppResult<u64> {
    let res = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM inspection_rounds WHERE schedule_id = ? AND status = 'scheduled' AND started_at IS NULL \
             AND julianday(scheduled_at) > julianday(?) \
             AND NOT EXISTS (SELECT 1 FROM inspection_results r WHERE r.round_id = inspection_rounds.id)",
            [schedule_id.into(), now.into()],
        ))
        .await?;
    Ok(res.rows_affected())
}

/// Due dates of a calendar schedule to generate in this run (pure).
///
/// - `after`: the latest due date already generated for the schedule (`None` on the first run);
///   only later dates are returned, and on the first run only dates whose window is still open
///   at `as_of`.
/// - Dates run up to `horizon_end`, at most `limit` of them (`true` when the limit cut the list).
pub fn calendar_due_dates(
    anchor: DateTime<Utc>,
    recurrence: Recurrence,
    after: Option<DateTime<Utc>>,
    as_of: DateTime<Utc>,
    grace: Duration,
    horizon_end: DateTime<Utc>,
    limit: usize,
) -> (Vec<DateTime<Utc>>, bool) {
    let mut out = Vec::new();
    let mut k: u32 = 0;
    loop {
        let due = nth_date(anchor, recurrence, k);
        if due > horizon_end || due == DateTime::<Utc>::MAX_UTC {
            return (out, false);
        }
        let wanted = match after {
            Some(last) => due > last,
            None => due + grace >= as_of,
        };
        if wanted {
            if out.len() == limit {
                return (out, true);
            }
            out.push(due);
        }
        k = match k.checked_add(1) {
            Some(next) => next,
            None => return (out, false),
        };
    }
}

struct RoundPlan {
    due: DateTime<Utc>,
    basis: String,
    due_meter_value: Option<f64>,
}

/// Inserts one generated round unless its occurrence already has one. `true` when created.
async fn insert_generated_round(
    db: &impl ConnectionTrait,
    schedule: &InspectionSchedule,
    version_id: i64,
    plan: &RoundPlan,
    as_of: DateTime<Utc>,
    created_ids: &mut Vec<i64>,
) -> AppResult<bool> {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inspection_rounds WHERE schedule_id = ? AND due_basis = ? AND status <> 'cancelled'",
            [schedule.id.into(), plan.basis.clone().into()],
        ))
        .await?
        .is_some();
    if exists {
        return Ok(false);
    }
    let window_end = plan.due + Duration::hours(schedule.grace_hours);
    // A window that closed before this run: the round was due and nobody could do it.
    let missed = window_end < as_of;
    let sync_id = crate::sync::identity::seeded_entity_sync_id(
        "inspection_rounds",
        &format!("{}|{}|{}", schedule.entity_sync_id, schedule.row_version, plan.basis),
    );
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inspection_rounds (entity_sync_id, template_id, template_version_id, scheduled_at, \
         assigned_to_id, assigned_team_id, status, schedule_id, due_basis, window_end_at, due_meter_value, \
         missed_at, row_version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            schedule.template_id.into(),
            version_id.into(),
            ts(plan.due).into(),
            schedule.assigned_to_id.into(),
            schedule.assigned_team_id.into(),
            (if missed { "missed" } else { "scheduled" }).into(),
            schedule.id.into(),
            plan.basis.clone().into(),
            ts(window_end).into(),
            plan.due_meter_value.into(),
            (if missed { Some(ts(window_end)) } else { None }).into(),
        ],
    ))
    .await?;
    let id: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;
    created_ids.push(id);
    Ok(true)
}

/// Plans of the rounds a schedule needs at `as_of`, or the reason it cannot generate.
async fn plan_rounds(
    db: &impl ConnectionTrait,
    schedule: &InspectionSchedule,
    as_of: DateTime<Utc>,
) -> AppResult<Result<(Vec<RoundPlan>, bool), String>> {
    match schedule.trigger_kind.as_str() {
        TRIGGER_CALENDAR => {
            let (Some(unit), Some(value), Some(anchor)) = (
                schedule.interval_unit.as_deref(),
                schedule.interval_value,
                schedule.anchor_at.as_deref().and_then(parse_ts),
            ) else {
                return Ok(Err(
                    "GATE_INSP_SCHEDULE_INCOMPLETE: calendar schedule without interval or first date".into(),
                ));
            };
            let recurrence = match schedule_recurrence(unit, value) {
                Ok(r) => r,
                Err(e) => return Ok(Err(format!("GATE_INSP_SCHEDULE_INCOMPLETE: {e}"))),
            };
            let last: Option<String> = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT MAX(scheduled_at) AS last FROM inspection_rounds \
                     WHERE schedule_id = ? AND status <> 'cancelled' AND due_basis LIKE 'calendar:%'",
                    [schedule.id.into()],
                ))
                .await?
                .map(|r| r.try_get("", "last"))
                .transpose()
                .map_err(|e| decode_err("last", e))?
                .flatten();
            let (dates, truncated) = calendar_due_dates(
                anchor,
                recurrence,
                last.as_deref().and_then(parse_ts),
                as_of,
                Duration::hours(schedule.grace_hours),
                as_of + Duration::days(schedule.horizon_days),
                MAX_ROUNDS_PER_RUN,
            );
            Ok(Ok((
                dates
                    .into_iter()
                    .map(|due| RoundPlan {
                        due,
                        basis: format!("calendar:{}", ts(due)),
                        due_meter_value: None,
                    })
                    .collect(),
                truncated,
            )))
        }
        TRIGGER_METER => {
            let (Some(meter_id), Some(interval), Some(start)) =
                (schedule.meter_id, schedule.meter_interval, schedule.meter_start_reading)
            else {
                return Ok(Err(
                    "GATE_INSP_SCHEDULE_INCOMPLETE: meter schedule without meter, interval or start reading".into(),
                ));
            };
            let Some((reference, lifetime)) = meter_lifetime(db, meter_id).await? else {
                return Ok(Err(
                    "GATE_INSP_SCHEDULE_METER_INACTIVE: the meter is unknown or inactive.".into(),
                ));
            };
            let open = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    format!(
                        "SELECT id FROM inspection_rounds WHERE schedule_id = ? AND status IN ({OPEN_STATUSES_SQL})"
                    ),
                    [schedule.id.into()],
                ))
                .await?;
            if open.is_some() {
                return Ok(Ok((Vec::new(), false)));
            }
            let last: Option<f64> = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT MAX(due_meter_value) AS last FROM inspection_rounds \
                     WHERE schedule_id = ? AND status <> 'cancelled' AND due_meter_value IS NOT NULL",
                    [schedule.id.into()],
                ))
                .await?
                .map(|r| r.try_get("", "last"))
                .transpose()
                .map_err(|e| decode_err("last", e))?
                .flatten();
            let threshold = crate::pm::queries::next_meter_threshold(start, interval, lifetime, last);
            if lifetime + f64::EPSILON < threshold {
                return Ok(Ok((Vec::new(), false)));
            }
            Ok(Ok((
                vec![RoundPlan {
                    due: as_of,
                    basis: format!("meter:{reference}:{threshold:.4}"),
                    due_meter_value: Some(threshold),
                }],
                false,
            )))
        }
        other => Ok(Err(format!(
            "GATE_INSP_SCHEDULE_TRIGGER_INVALID: unknown trigger '{other}'."
        ))),
    }
}

/// Rounds generated and still unstarted whose window closed before `as_of` become `missed`.
pub async fn mark_missed_rounds(db: &impl ConnectionTrait, as_of: DateTime<Utc>) -> AppResult<i64> {
    let res = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_rounds SET status = 'missed', missed_at = window_end_at, \
             row_version = row_version + 1 \
             WHERE status IN ('scheduled', 'released') AND started_at IS NULL AND window_end_at IS NOT NULL \
             AND julianday(window_end_at) < julianday(?)",
            [ts(as_of).into()],
        ))
        .await?;
    Ok(i64::try_from(res.rows_affected()).unwrap_or(i64::MAX))
}

/// Generates the rounds of every active schedule (or of one), then marks missed rounds. A
/// schedule that cannot generate is reported and does not stop the others.
pub async fn generate_inspection_rounds(
    db: &DatabaseConnection,
    input: GenerateInspectionRoundsInput,
) -> AppResult<GenerateInspectionRoundsResult> {
    let as_of = match input.as_of.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => parse_ts(raw)
            .ok_or_else(|| AppError::ValidationFailed(vec!["as_of must be an RFC 3339 timestamp.".into()]))?,
        None => Utc::now(),
    };
    let txn = db.begin().await?;
    let mut sql = format!("SELECT {SCHEDULE_COLUMNS} FROM inspection_schedules s WHERE s.is_active = 1");
    let mut binds: Vec<sea_orm::Value> = Vec::new();
    if let Some(id) = input.schedule_id {
        sql.push_str(" AND s.id = ?");
        binds.push(id.into());
    }
    sql.push_str(" ORDER BY s.code");
    let schedules = txn
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?
        .iter()
        .map(map_schedule)
        .collect::<AppResult<Vec<_>>>()?;

    let mut out = GenerateInspectionRoundsResult::default();
    for schedule in &schedules {
        let version_id: Option<i64> = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT current_version_id FROM inspection_templates WHERE id = ? AND is_active = 1",
                [schedule.template_id.into()],
            ))
            .await?
            .map(|r| r.try_get("", "current_version_id"))
            .transpose()
            .map_err(|e| decode_err("current_version_id", e))?
            .flatten();
        let Some(version_id) = version_id else {
            out.issues.push(InspectionScheduleIssue {
                schedule_code: schedule.code.clone(),
                message: "GATE_INSP_SCHEDULE_TEMPLATE_NOT_PUBLISHED: the checklist is inactive or has no published \
                          version."
                    .into(),
            });
            continue;
        };
        let (plans, truncated) = match plan_rounds(&txn, schedule, as_of).await? {
            Ok(p) => p,
            Err(message) => {
                out.issues.push(InspectionScheduleIssue {
                    schedule_code: schedule.code.clone(),
                    message,
                });
                continue;
            }
        };
        for plan in &plans {
            if insert_generated_round(&txn, schedule, version_id, plan, as_of, &mut out.round_ids).await? {
                out.generated += 1;
            } else {
                out.skipped += 1;
            }
        }
        if truncated {
            out.issues.push(InspectionScheduleIssue {
                schedule_code: schedule.code.clone(),
                message: format!(
                    "GATE_INSP_SCHEDULE_TOO_MANY_ROUNDS: stopped after {MAX_ROUNDS_PER_RUN} rounds; shorten the \
                     look-ahead or lengthen the interval."
                ),
            });
        }
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inspection_schedules SET last_generated_at = ? WHERE id = ?",
            [ts(as_of).into(), schedule.id.into()],
        ))
        .await?;
    }
    // Back-filled rounds created as missed above are already counted in `generated`.
    out.missed = mark_missed_rounds(&txn, as_of).await?;
    txn.commit().await?;
    Ok(out)
}
