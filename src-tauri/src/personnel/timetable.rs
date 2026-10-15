//! Timetables (Classes horaires) behind capacity — SAP PM work-centre shift sequences, Maximo
//! shift patterns.
//!
//! - A Classe horaire (ORG.SCHEDULE_CLASS reference value) has dated versions
//!   (`schedule_class_versions`): a cycle (a week, or an N-day rotation from a start date), each
//!   cycle day's shift and break, and the utilisation factor — the share of shift time available
//!   for work orders. A version in force is never edited: a change is a new version from a date.
//! - A team works to one Classe horaire from a date (`team_schedule_assignments`); its members
//!   inherit it.
//! - A person may have dated exceptions (`personnel_schedule_exceptions`): another Classe horaire
//!   and/or a part-time percentage. A person's own home Classe horaire (set on their record) also
//!   takes precedence over the team's.
//!
//! Rows flagged `to_review` were created by the upgrade (migration 194) and stay flagged until a
//! planner confirms them or replaces them.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{org_validation_failed, AppError, AppResult};

/// Longest rotation (eight weeks).
pub const MAX_CYCLE_DAYS: i64 = 56;
/// Days of a weekly cycle (cycle day 1 = Monday … 7 = Sunday).
pub const WEEK_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleCycleDay {
    pub cycle_day: i64,
    pub shift_start: String,
    pub shift_end: String,
    #[serde(default)]
    pub break_minutes: i64,
    pub is_rest_day: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleClassVersion {
    pub id: i64,
    pub entity_sync_id: String,
    pub reference_value_id: i64,
    pub class_code: String,
    pub class_label: String,
    pub is_continuous: bool,
    pub effective_from: String,
    pub cycle_length_days: i64,
    pub rotation_start_date: Option<String>,
    pub days: Vec<ScheduleCycleDay>,
    pub utilisation_percent: f64,
    pub review_status: Option<String>,
    pub note: Option<String>,
    pub row_version: i64,
    /// Working days and net hours (shift minus break) over one cycle.
    pub working_days_per_cycle: i64,
    pub net_hours_per_cycle: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleClassVersionInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub reference_value_id: i64,
    pub effective_from: String,
    pub cycle_length_days: i64,
    pub rotation_start_date: Option<String>,
    pub days: Vec<ScheduleCycleDay>,
    /// Continuous operation (3×8, 24/7): works through the site's rest days and holidays.
    #[serde(default)]
    pub is_continuous: bool,
    pub utilisation_percent: f64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamScheduleAssignment {
    pub id: i64,
    pub entity_sync_id: String,
    pub team_id: i64,
    pub schedule_reference_value_id: Option<i64>,
    pub schedule_code: Option<String>,
    pub schedule_label: Option<String>,
    pub effective_from: String,
    pub review_status: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamScheduleAssignmentInput {
    pub team_id: i64,
    /// `None`: the team stops working to a Classe horaire from that date.
    pub schedule_reference_value_id: Option<i64>,
    pub effective_from: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonnelScheduleException {
    pub id: i64,
    pub entity_sync_id: String,
    pub personnel_id: i64,
    pub schedule_reference_value_id: Option<i64>,
    pub schedule_code: Option<String>,
    pub schedule_label: Option<String>,
    pub work_percent: Option<f64>,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub reason: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonnelScheduleExceptionInput {
    pub id: Option<i64>,
    pub expected_row_version: Option<i64>,
    pub personnel_id: i64,
    pub schedule_reference_value_id: Option<i64>,
    pub work_percent: Option<f64>,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub reason: Option<String>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("timetable decode '{field}': {err}"))
}

fn issue(code: &str, message: &str) -> AppError {
    org_validation_failed(code, message, HashMap::new())
}

fn issue_with(code: &str, message: &str, params: &[(&str, String)]) -> AppError {
    org_validation_failed(
        code,
        message,
        params.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
    )
}

fn today() -> NaiveDate {
    Utc::now().date_naive()
}

fn now_text() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse_date(raw: &str, code: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| issue(code, "Enter the date as YYYY-MM-DD."))
}

/// A change takes effect today or later: past capacity and plans are never rewritten.
fn require_not_past(day: NaiveDate) -> AppResult<()> {
    if day < today() {
        return Err(issue_with(
            "TIMETABLE_DATE_IN_PAST",
            "A timetable change applies from today or a later date; past capacity is never rewritten.",
            &[("today", today().to_string())],
        ));
    }
    Ok(())
}

fn parse_hhmm(raw: &str) -> Option<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(raw.trim(), "%H:%M")
        .ok()
        .or_else(|| chrono::NaiveTime::parse_from_str(raw.trim(), "%H:%M:%S").ok())
}

/// Shift length in minutes; a shift ending at or before its start runs past midnight.
pub fn gross_minutes(start: &str, end: &str) -> Option<i64> {
    let s = parse_hhmm(start)?;
    let e = parse_hhmm(end)?;
    let mut minutes = (e - s).num_minutes();
    if minutes <= 0 {
        minutes += 24 * 60;
    }
    Some(minutes)
}

/// Net minutes of a cycle day (shift minus break), 0 for a rest day.
pub fn net_minutes(day: &ScheduleCycleDay) -> i64 {
    if day.is_rest_day {
        return 0;
    }
    gross_minutes(&day.shift_start, &day.shift_end).map_or(0, |g| (g - day.break_minutes).max(0))
}

/// Cycle day (1-based) of `day`: the ISO weekday for a weekly cycle without a rotation start,
/// otherwise the position in the rotation counted from its start date.
pub fn cycle_day_of(day: NaiveDate, cycle_length_days: i64, rotation_start: Option<NaiveDate>) -> i64 {
    match rotation_start {
        Some(start) => (day - start).num_days().rem_euclid(cycle_length_days.max(1)) + 1,
        None => i64::from(day.weekday().number_from_monday()),
    }
}

fn validate_version(input: &ScheduleClassVersionInput) -> AppResult<(NaiveDate, Option<NaiveDate>)> {
    let effective_from = parse_date(&input.effective_from, "TIMETABLE_DATE_INVALID")?;
    if !(1.0..=100.0).contains(&input.utilisation_percent) || !input.utilisation_percent.is_finite() {
        return Err(issue(
            "TIMETABLE_UTILISATION_RANGE",
            "Enter the utilisation factor as a percentage between 1 and 100.",
        ));
    }
    let n = input.cycle_length_days;
    if !(1..=MAX_CYCLE_DAYS).contains(&n) {
        return Err(issue_with(
            "TIMETABLE_CYCLE_LENGTH",
            "A rotation lasts between 1 and 56 days.",
            &[("max", MAX_CYCLE_DAYS.to_string())],
        ));
    }
    let rotation_start = match input
        .rotation_start_date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(raw) => Some(parse_date(raw, "TIMETABLE_DATE_INVALID")?),
        None if n == WEEK_DAYS => None,
        None => {
            return Err(issue(
                "TIMETABLE_ROTATION_START_REQUIRED",
                "Give the date on which day 1 of the rotation falls.",
            ))
        }
    };
    let mut seen = HashSet::new();
    for d in &input.days {
        if !(1..=n).contains(&d.cycle_day) || !seen.insert(d.cycle_day) {
            return Err(issue(
                "TIMETABLE_DAYS_INCOMPLETE",
                "Describe each day of the cycle exactly once.",
            ));
        }
        if d.is_rest_day {
            continue;
        }
        let gross = gross_minutes(&d.shift_start, &d.shift_end).ok_or_else(|| {
            issue_with(
                "TIMETABLE_TIME_INVALID",
                "Enter shift times as HH:MM.",
                &[("day", d.cycle_day.to_string())],
            )
        })?;
        if d.break_minutes < 0 || d.break_minutes >= gross {
            return Err(issue_with(
                "TIMETABLE_BREAK_TOO_LONG",
                "The break must be shorter than the shift.",
                &[("day", d.cycle_day.to_string())],
            ));
        }
    }
    if i64::try_from(seen.len()).unwrap_or(0) != n {
        return Err(issue(
            "TIMETABLE_DAYS_INCOMPLETE",
            "Describe each day of the cycle exactly once.",
        ));
    }
    if !input.days.iter().any(|d| !d.is_rest_day) {
        return Err(issue(
            "TIMETABLE_NO_WORKING_DAY",
            "A timetable needs at least one working day.",
        ));
    }
    Ok((effective_from, rotation_start))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Classe horaire versions
// ═══════════════════════════════════════════════════════════════════════════════

const VERSION_SELECT: &str = "SELECT v.id, v.entity_sync_id, v.reference_value_id, rv.code AS class_code, \
        rv.label AS class_label, v.is_continuous, v.effective_from, v.cycle_length_days, v.rotation_start_date, \
        v.days_json, v.utilisation_percent, v.review_status, v.note, v.row_version \
     FROM schedule_class_versions v JOIN reference_values rv ON rv.id = v.reference_value_id";

/// `is_continuous` flag of a Classe horaire's reference value metadata (its weekly editor's
/// setting), carried into its first version by the upgrade.
pub fn metadata_is_continuous(metadata_json: Option<&str>) -> bool {
    metadata_json
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("is_continuous").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

fn parse_days(raw: &str) -> AppResult<Vec<ScheduleCycleDay>> {
    let mut days: Vec<ScheduleCycleDay> = serde_json::from_str(raw).map_err(|e| decode_err("days_json", e))?;
    days.sort_by_key(|d| d.cycle_day);
    Ok(days)
}

fn map_version(r: &sea_orm::QueryResult) -> AppResult<ScheduleClassVersion> {
    let days = parse_days(
        &r.try_get::<String>("", "days_json")
            .map_err(|e| decode_err("days_json", e))?,
    )?;
    let working_days_per_cycle = i64::try_from(days.iter().filter(|d| !d.is_rest_day).count()).unwrap_or(0);
    let net_hours_per_cycle = days.iter().map(net_minutes).sum::<i64>() as f64 / 60.0;
    Ok(ScheduleClassVersion {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        reference_value_id: r
            .try_get("", "reference_value_id")
            .map_err(|e| decode_err("reference_value_id", e))?,
        class_code: r.try_get("", "class_code").map_err(|e| decode_err("class_code", e))?,
        class_label: r.try_get("", "class_label").map_err(|e| decode_err("class_label", e))?,
        is_continuous: r
            .try_get::<i64>("", "is_continuous")
            .map_err(|e| decode_err("is_continuous", e))?
            != 0,
        effective_from: r
            .try_get("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?,
        cycle_length_days: r
            .try_get("", "cycle_length_days")
            .map_err(|e| decode_err("cycle_length_days", e))?,
        rotation_start_date: r.try_get("", "rotation_start_date").ok().flatten(),
        days,
        utilisation_percent: r
            .try_get("", "utilisation_percent")
            .map_err(|e| decode_err("utilisation_percent", e))?,
        review_status: r.try_get("", "review_status").ok().flatten(),
        note: r.try_get("", "note").ok().flatten(),
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
        working_days_per_cycle,
        net_hours_per_cycle: (net_hours_per_cycle * 100.0).round() / 100.0,
    })
}

/// Versions of one Classe horaire, oldest first (`None`: every Classe horaire).
pub async fn list_class_versions(
    db: &impl ConnectionTrait,
    reference_value_id: Option<i64>,
) -> AppResult<Vec<ScheduleClassVersion>> {
    let (sql, values): (String, Vec<sea_orm::Value>) = match reference_value_id {
        Some(id) => (
            format!("{VERSION_SELECT} WHERE v.reference_value_id = ? ORDER BY v.effective_from, v.id"),
            vec![id.into()],
        ),
        None => (
            format!("{VERSION_SELECT} ORDER BY rv.label, v.effective_from, v.id"),
            Vec::new(),
        ),
    };
    db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .iter()
        .map(map_version)
        .collect()
}

async fn get_version(db: &impl ConnectionTrait, id: i64) -> AppResult<ScheduleClassVersion> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{VERSION_SELECT} WHERE v.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleClassVersion".into(),
            id: id.to_string(),
        })?;
    map_version(&row)
}

/// Creates a version from a date (today or later), or edits one not yet in force. Saving a
/// version settles the review of the Classe horaire's earlier versions.
pub async fn upsert_class_version(
    db: &DatabaseConnection,
    input: ScheduleClassVersionInput,
    actor_id: i64,
) -> AppResult<ScheduleClassVersion> {
    let (effective_from, rotation_start) = validate_version(&input)?;
    require_not_past(effective_from)?;
    crate::reference::schedule_patterns::assert_schedule_reference_value_active(db, input.reference_value_id).await?;
    let mut days = input.days.clone();
    days.sort_by_key(|d| d.cycle_day);
    for d in &mut days {
        d.shift_start = d.shift_start.trim().to_string();
        d.shift_end = d.shift_end.trim().to_string();
    }
    let days_json = serde_json::to_string(&days).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    let note = input
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let txn = db.begin().await?;
    if txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM schedule_class_versions WHERE reference_value_id = ? AND effective_from = ? AND id <> ?",
            [
                input.reference_value_id.into(),
                effective_from.to_string().into(),
                input.id.unwrap_or(0).into(),
            ],
        ))
        .await?
        .is_some()
    {
        return Err(issue(
            "TIMETABLE_VERSION_DUPLICATE",
            "This Classe horaire already has a version starting that day; edit it instead.",
        ));
    }
    let now = now_text();
    let id = if let Some(id) = input.id {
        let current = get_version(&txn, id).await?;
        if current.reference_value_id != input.reference_value_id {
            return Err(issue(
                "TIMETABLE_VERSION_MISMATCH",
                "This version belongs to another Classe horaire.",
            ));
        }
        if parse_date(&current.effective_from, "TIMETABLE_DATE_INVALID")? <= today() {
            return Err(issue(
                "TIMETABLE_VERSION_IN_FORCE",
                "This version is already in force: add a new version from a date instead.",
            ));
        }
        let res = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE schedule_class_versions SET effective_from = ?, cycle_length_days = ?, rotation_start_date = ?, \
                 days_json = ?, is_continuous = ?, utilisation_percent = ?, note = ?, review_status = NULL, \
                 row_version = row_version + 1, updated_at = ? WHERE id = ? AND row_version = ?",
                [
                    effective_from.to_string().into(),
                    input.cycle_length_days.into(),
                    rotation_start.map(|d| d.to_string()).into(),
                    days_json.into(),
                    i64::from(input.is_continuous).into(),
                    input.utilisation_percent.into(),
                    note.into(),
                    now.clone().into(),
                    id.into(),
                    input.expected_row_version.unwrap_or(current.row_version).into(),
                ],
            ))
            .await?;
        if res.rows_affected() == 0 {
            return Err(issue(
                "RECORD_CHANGED_ELSEWHERE",
                "This record was changed by someone else; reload and try again.",
            ));
        }
        id
    } else {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO schedule_class_versions \
             (entity_sync_id, reference_value_id, effective_from, cycle_length_days, rotation_start_date, days_json, \
              is_continuous, utilisation_percent, review_status, note, row_version, created_by_id, created_at, \
              updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, 1, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                input.reference_value_id.into(),
                effective_from.to_string().into(),
                input.cycle_length_days.into(),
                rotation_start.map(|d| d.to_string()).into(),
                days_json.into(),
                i64::from(input.is_continuous).into(),
                input.utilisation_percent.into(),
                note.into(),
                actor_id.into(),
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
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("no id after insert")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?
    };
    settle_class_review(&txn, input.reference_value_id, &now).await?;
    if input.cycle_length_days == WEEK_DAYS && rotation_start.is_none() && effective_from <= today() {
        mirror_weekly_rows(&txn, input.reference_value_id, &days).await?;
    }
    let saved = get_version(&txn, id).await?;
    txn.commit().await?;
    Ok(saved)
}

/// A planner confirms the Classe horaire as it stands (the upgrade's 85 % and Sunday–Thursday
/// week, for instance): its versions are no longer flagged for review.
pub async fn confirm_class_review(
    db: &DatabaseConnection,
    reference_value_id: i64,
) -> AppResult<Vec<ScheduleClassVersion>> {
    let txn = db.begin().await?;
    settle_class_review(&txn, reference_value_id, &now_text()).await?;
    txn.commit().await?;
    list_class_versions(db, Some(reference_value_id)).await
}

async fn settle_class_review(conn: &impl ConnectionTrait, reference_value_id: i64, now: &str) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_class_versions SET review_status = NULL, row_version = row_version + 1, updated_at = ? \
         WHERE reference_value_id = ? AND review_status IS NOT NULL",
        [now.into(), reference_value_id.into()],
    ))
    .await?;
    Ok(())
}

/// The weekly rows (`schedule_details`) the attendance views read follow the version in force.
async fn mirror_weekly_rows(
    conn: &impl ConnectionTrait,
    reference_value_id: i64,
    days: &[ScheduleCycleDay],
) -> AppResult<()> {
    for d in days {
        let updated = conn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE schedule_details SET shift_start = ?, shift_end = ?, is_rest_day = ? \
                 WHERE reference_value_id = ? AND day_of_week = ?",
                [
                    d.shift_start.clone().into(),
                    d.shift_end.clone().into(),
                    i64::from(d.is_rest_day).into(),
                    reference_value_id.into(),
                    d.cycle_day.into(),
                ],
            ))
            .await?;
        if updated.rows_affected() == 0 {
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO schedule_details (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day) \
                 VALUES (?, ?, ?, ?, ?)",
                [
                    reference_value_id.into(),
                    d.cycle_day.into(),
                    d.shift_start.clone().into(),
                    d.shift_end.clone().into(),
                    i64::from(d.is_rest_day).into(),
                ],
            ))
            .await?;
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Team links
// ═══════════════════════════════════════════════════════════════════════════════

const TEAM_LINK_SELECT: &str = "SELECT a.id, a.entity_sync_id, a.team_id, a.schedule_reference_value_id, \
        rv.code AS schedule_code, rv.label AS schedule_label, a.effective_from, a.review_status, a.row_version \
     FROM team_schedule_assignments a LEFT JOIN reference_values rv ON rv.id = a.schedule_reference_value_id";

fn map_team_link(r: &sea_orm::QueryResult) -> AppResult<TeamScheduleAssignment> {
    Ok(TeamScheduleAssignment {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        team_id: r.try_get("", "team_id").map_err(|e| decode_err("team_id", e))?,
        schedule_reference_value_id: r.try_get("", "schedule_reference_value_id").ok().flatten(),
        schedule_code: r.try_get("", "schedule_code").ok().flatten(),
        schedule_label: r.try_get("", "schedule_label").ok().flatten(),
        effective_from: r
            .try_get("", "effective_from")
            .map_err(|e| decode_err("effective_from", e))?,
        review_status: r.try_get("", "review_status").ok().flatten(),
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

/// A team's Classe horaire history, oldest first.
pub async fn list_team_schedule(db: &impl ConnectionTrait, team_id: i64) -> AppResult<Vec<TeamScheduleAssignment>> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("{TEAM_LINK_SELECT} WHERE a.team_id = ? ORDER BY a.effective_from, a.id"),
        [team_id.into()],
    ))
    .await?
    .iter()
    .map(map_team_link)
    .collect()
}

/// Links a team to a Classe horaire from a date (today or later). A link already starting that
/// day is replaced; earlier links stay as history and their review is settled.
pub async fn set_team_schedule(
    db: &DatabaseConnection,
    input: TeamScheduleAssignmentInput,
    actor_id: i64,
) -> AppResult<Vec<TeamScheduleAssignment>> {
    let from = parse_date(&input.effective_from, "TIMETABLE_DATE_INVALID")?;
    require_not_past(from)?;
    if let Some(class_id) = input.schedule_reference_value_id {
        crate::reference::schedule_patterns::assert_schedule_reference_value_active(db, class_id).await?;
    }
    let txn = db.begin().await?;
    if txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS x FROM teams WHERE id = ? AND deleted_at IS NULL",
            [input.team_id.into()],
        ))
        .await?
        .is_none()
    {
        return Err(AppError::NotFound {
            entity: "Team".into(),
            id: input.team_id.to_string(),
        });
    }
    let now = now_text();
    let updated = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE team_schedule_assignments SET schedule_reference_value_id = ?, review_status = NULL, \
             row_version = row_version + 1, updated_at = ? WHERE team_id = ? AND effective_from = ?",
            [
                input.schedule_reference_value_id.into(),
                now.clone().into(),
                input.team_id.into(),
                from.to_string().into(),
            ],
        ))
        .await?;
    if updated.rows_affected() == 0 {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO team_schedule_assignments \
             (entity_sync_id, team_id, schedule_reference_value_id, effective_from, review_status, row_version, \
              created_by_id, created_at, updated_at) VALUES (?, ?, ?, ?, NULL, 1, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                input.team_id.into(),
                input.schedule_reference_value_id.into(),
                from.to_string().into(),
                actor_id.into(),
                now.clone().into(),
                now.clone().into(),
            ],
        ))
        .await?;
    }
    settle_team_review(&txn, input.team_id, &now).await?;
    txn.commit().await?;
    list_team_schedule(db, input.team_id).await
}

/// A planner confirms the team's Classe horaire link as migrated.
pub async fn confirm_team_schedule_review(
    db: &DatabaseConnection,
    team_id: i64,
) -> AppResult<Vec<TeamScheduleAssignment>> {
    settle_team_review(db, team_id, &now_text()).await?;
    list_team_schedule(db, team_id).await
}

async fn settle_team_review(conn: &impl ConnectionTrait, team_id: i64, now: &str) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE team_schedule_assignments SET review_status = NULL, row_version = row_version + 1, updated_at = ? \
         WHERE team_id = ? AND review_status IS NOT NULL",
        [now.into(), team_id.into()],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Person exceptions
// ═══════════════════════════════════════════════════════════════════════════════

const EXCEPTION_SELECT: &str = "SELECT x.id, x.entity_sync_id, x.personnel_id, x.schedule_reference_value_id, \
        rv.code AS schedule_code, rv.label AS schedule_label, x.work_percent, x.valid_from, x.valid_to, x.reason, \
        x.row_version \
     FROM personnel_schedule_exceptions x LEFT JOIN reference_values rv ON rv.id = x.schedule_reference_value_id";

fn map_exception(r: &sea_orm::QueryResult) -> AppResult<PersonnelScheduleException> {
    Ok(PersonnelScheduleException {
        id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: r
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        personnel_id: r
            .try_get("", "personnel_id")
            .map_err(|e| decode_err("personnel_id", e))?,
        schedule_reference_value_id: r.try_get("", "schedule_reference_value_id").ok().flatten(),
        schedule_code: r.try_get("", "schedule_code").ok().flatten(),
        schedule_label: r.try_get("", "schedule_label").ok().flatten(),
        work_percent: r.try_get("", "work_percent").ok().flatten(),
        valid_from: r.try_get("", "valid_from").map_err(|e| decode_err("valid_from", e))?,
        valid_to: r.try_get("", "valid_to").ok().flatten(),
        reason: r.try_get("", "reason").ok().flatten(),
        row_version: r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

pub async fn list_personnel_schedule_exceptions(
    db: &impl ConnectionTrait,
    personnel_id: i64,
) -> AppResult<Vec<PersonnelScheduleException>> {
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        format!("{EXCEPTION_SELECT} WHERE x.personnel_id = ? ORDER BY x.valid_from, x.id"),
        [personnel_id.into()],
    ))
    .await?
    .iter()
    .map(map_exception)
    .collect()
}

async fn get_exception(db: &impl ConnectionTrait, id: i64) -> AppResult<PersonnelScheduleException> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{EXCEPTION_SELECT} WHERE x.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PersonnelScheduleException".into(),
            id: id.to_string(),
        })?;
    map_exception(&row)
}

/// Records a dated exception (another Classe horaire and/or a part-time percentage). An
/// exception that has started can only be ended (its end date, today or later); one not yet
/// started can be changed freely. Exceptions of a person may not overlap.
pub async fn upsert_personnel_schedule_exception(
    db: &DatabaseConnection,
    input: PersonnelScheduleExceptionInput,
    actor_id: i64,
) -> AppResult<PersonnelScheduleException> {
    let from = parse_date(&input.valid_from, "TIMETABLE_DATE_INVALID")?;
    let to = match input.valid_to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => Some(parse_date(raw, "TIMETABLE_DATE_INVALID")?),
        None => None,
    };
    if to.is_some_and(|t| t < from) {
        return Err(issue(
            "TIMETABLE_PERIOD_INVALID",
            "The end date comes before the start date.",
        ));
    }
    if input.schedule_reference_value_id.is_none() && input.work_percent.is_none() {
        return Err(issue(
            "TIMETABLE_EXCEPTION_EMPTY",
            "Choose another Classe horaire, a part-time percentage, or both.",
        ));
    }
    if let Some(p) = input.work_percent {
        if !p.is_finite() || p <= 0.0 || p > 100.0 {
            return Err(issue(
                "TIMETABLE_WORK_PERCENT_RANGE",
                "Enter the part-time percentage between 1 and 100.",
            ));
        }
    }
    if let Some(class_id) = input.schedule_reference_value_id {
        crate::reference::schedule_patterns::assert_schedule_reference_value_active(db, class_id).await?;
    }
    let reason = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let txn = db.begin().await?;
    let current = match input.id {
        Some(id) => Some(get_exception(&txn, id).await?),
        None => None,
    };
    match &current {
        Some(c) if parse_date(&c.valid_from, "TIMETABLE_DATE_INVALID")? <= today() => {
            // Started: only its end may move, and not into the past.
            let same = c.personnel_id == input.personnel_id
                && c.schedule_reference_value_id == input.schedule_reference_value_id
                && c.work_percent == input.work_percent
                && c.valid_from == from.to_string();
            if !same {
                return Err(issue(
                    "TIMETABLE_EXCEPTION_STARTED",
                    "This exception has started: you can only set its end date; add a new one for other changes.",
                ));
            }
            if let Some(t) = to {
                if t < today() {
                    require_not_past(t)?;
                }
            }
        }
        _ => require_not_past(from)?,
    }
    let overlapping = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT valid_from FROM personnel_schedule_exceptions \
             WHERE personnel_id = ? AND id <> ? AND valid_from <= COALESCE(?, '9999-12-31') \
               AND COALESCE(valid_to, '9999-12-31') >= ? LIMIT 1",
            [
                input.personnel_id.into(),
                input.id.unwrap_or(0).into(),
                to.map(|d| d.to_string()).into(),
                from.to_string().into(),
            ],
        ))
        .await?;
    if let Some(row) = overlapping {
        let other: String = row.try_get("", "valid_from").map_err(|e| decode_err("valid_from", e))?;
        return Err(issue_with(
            "TIMETABLE_EXCEPTION_OVERLAP",
            "Another exception of this person covers part of these dates.",
            &[("from", other)],
        ));
    }
    let now = now_text();
    let id = if let Some(c) = current {
        let res = txn
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE personnel_schedule_exceptions SET schedule_reference_value_id = ?, work_percent = ?, \
                 valid_from = ?, valid_to = ?, reason = ?, row_version = row_version + 1, updated_at = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.schedule_reference_value_id.into(),
                    input.work_percent.into(),
                    from.to_string().into(),
                    to.map(|d| d.to_string()).into(),
                    reason.into(),
                    now.into(),
                    c.id.into(),
                    input.expected_row_version.unwrap_or(c.row_version).into(),
                ],
            ))
            .await?;
        if res.rows_affected() == 0 {
            return Err(issue(
                "RECORD_CHANGED_ELSEWHERE",
                "This record was changed by someone else; reload and try again.",
            ));
        }
        c.id
    } else {
        if txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS x FROM personnel WHERE id = ?",
                [input.personnel_id.into()],
            ))
            .await?
            .is_none()
        {
            return Err(AppError::NotFound {
                entity: "Personnel".into(),
                id: input.personnel_id.to_string(),
            });
        }
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO personnel_schedule_exceptions \
             (entity_sync_id, personnel_id, schedule_reference_value_id, work_percent, valid_from, valid_to, reason, \
              row_version, created_by_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
            [
                crate::sync::identity::new_entity_sync_id().into(),
                input.personnel_id.into(),
                input.schedule_reference_value_id.into(),
                input.work_percent.into(),
                from.to_string().into(),
                to.map(|d| d.to_string()).into(),
                reason.into(),
                actor_id.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await?;
        txn.query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("no id after insert")))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?
    };
    let saved = get_exception(&txn, id).await?;
    txn.commit().await?;
    Ok(saved)
}

/// Removes an exception that has not started yet (a started one is ended instead).
pub async fn delete_personnel_schedule_exception(db: &DatabaseConnection, id: i64) -> AppResult<()> {
    let current = get_exception(db, id).await?;
    if parse_date(&current.valid_from, "TIMETABLE_DATE_INVALID")? <= today() {
        return Err(issue(
            "TIMETABLE_EXCEPTION_STARTED",
            "This exception has started: you can only set its end date; add a new one for other changes.",
        ));
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM personnel_schedule_exceptions WHERE id = ?",
        [id.into()],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Resolution (used by capacity)
// ═══════════════════════════════════════════════════════════════════════════════

/// Dated versions of the Classes horaires a computation needs, loaded once.
#[derive(Default)]
pub struct ClassBook {
    versions: HashMap<i64, Vec<(NaiveDate, ScheduleClassVersion)>>,
}

impl ClassBook {
    pub async fn load(db: &impl ConnectionTrait, class_ids: &HashSet<i64>) -> AppResult<Self> {
        let mut versions: HashMap<i64, Vec<(NaiveDate, ScheduleClassVersion)>> = HashMap::new();
        for id in class_ids {
            for v in list_class_versions(db, Some(*id)).await? {
                if let Ok(from) = NaiveDate::parse_from_str(&v.effective_from, "%Y-%m-%d") {
                    versions.entry(*id).or_default().push((from, v));
                }
            }
        }
        for list in versions.values_mut() {
            list.sort_by_key(|(from, v)| (*from, v.id));
        }
        Ok(Self { versions })
    }

    /// The version of `class_id` in force on `day`.
    pub fn version_on(&self, class_id: i64, day: NaiveDate) -> Option<&ScheduleClassVersion> {
        self.versions
            .get(&class_id)?
            .iter()
            .rev()
            .find(|(from, _)| *from <= day)
            .map(|(_, v)| v)
    }

    /// The cycle day of `version` on `day`.
    pub fn line_on<'a>(version: &'a ScheduleClassVersion, day: NaiveDate) -> Option<&'a ScheduleCycleDay> {
        let start = version
            .rotation_start_date
            .as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
        let n = cycle_day_of(day, version.cycle_length_days, start);
        version.days.iter().find(|d| d.cycle_day == n)
    }
}

/// A team's links, newest last, for picking the Classe horaire in force on a day.
pub async fn team_links(db: &impl ConnectionTrait, team_id: i64) -> AppResult<Vec<(NaiveDate, Option<i64>)>> {
    Ok(list_team_schedule(db, team_id)
        .await?
        .into_iter()
        .filter_map(|l| {
            NaiveDate::parse_from_str(&l.effective_from, "%Y-%m-%d")
                .ok()
                .map(|d| (d, l.schedule_reference_value_id))
        })
        .collect())
}

/// The link in force on `day` (links sorted oldest first).
pub fn link_on(links: &[(NaiveDate, Option<i64>)], day: NaiveDate) -> Option<i64> {
    links.iter().rev().find(|(from, _)| *from <= day).and_then(|(_, c)| *c)
}

/// A person's exceptions as date ranges.
pub struct ExceptionSpan {
    pub from: NaiveDate,
    pub to: Option<NaiveDate>,
    pub class_id: Option<i64>,
    pub work_percent: Option<f64>,
}

pub async fn exception_spans(db: &impl ConnectionTrait, personnel_id: i64) -> AppResult<Vec<ExceptionSpan>> {
    Ok(list_personnel_schedule_exceptions(db, personnel_id)
        .await?
        .into_iter()
        .filter_map(|x| {
            Some(ExceptionSpan {
                from: NaiveDate::parse_from_str(&x.valid_from, "%Y-%m-%d").ok()?,
                to: x
                    .valid_to
                    .as_deref()
                    .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok()),
                class_id: x.schedule_reference_value_id,
                work_percent: x.work_percent,
            })
        })
        .collect())
}

pub fn exception_on(spans: &[ExceptionSpan], day: NaiveDate) -> Option<&ExceptionSpan> {
    spans
        .iter()
        .rev()
        .find(|s| s.from <= day && s.to.is_none_or(|t| t >= day))
}
