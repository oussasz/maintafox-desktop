//! The shift a work order is planned on — a window of its team's Classes horaires on the planned
//! day (SAP PM: operations scheduled inside the work centre's shift sequence; Maximo: labour
//! assigned within the crew's shift pattern).
//!
//! Options come from `personnel::capacity`, the source planning books against: each member's own
//! Classe horaire (exception, else personal, else the team's default), the version in force on the
//! day and its shift window. A work order without a team or a planned day, or a day on which no
//! member has a shift, offers nothing: the shift stays empty, never assumed.
//!
//! The plan keeps a link to the version and the window it was planned with (migration 202); the
//! old fixed codes are refused (`domain::validate_wo_shift`).

use std::collections::BTreeMap;

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::personnel::capacity;

pub const GATE_WO_SHIFT_NO_TEAM: &str = "GATE_WO_SHIFT_NO_TEAM";
pub const GATE_WO_SHIFT_NO_DAY: &str = "GATE_WO_SHIFT_NO_DAY";
pub const GATE_WO_SHIFT_NOT_OFFERED: &str = "GATE_WO_SHIFT_NOT_OFFERED";
pub const GATE_WO_SHIFT_LEGACY_REPLAN: &str = "GATE_WO_SHIFT_LEGACY_REPLAN";

/// One shift window the team works on the day.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WoShiftOption {
    pub class_version_id: i64,
    pub schedule_class_id: i64,
    pub class_label: String,
    /// `HH:MM`; a window ending at or before its start runs past midnight.
    pub shift_start: String,
    pub shift_end: String,
    pub break_minutes: i64,
    /// Shift minus break.
    pub net_hours: f64,
    /// Members whose Classe horaire puts them on this window that day.
    pub members: i64,
    /// Of those, members not fully absent.
    pub members_available: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WoShiftOptions {
    pub team_id: Option<i64>,
    pub day: Option<String>,
    pub options: Vec<WoShiftOption>,
    /// Why there is nothing to offer: `no_team` | `no_day` | `no_shift`.
    pub reason: Option<String>,
}

/// The window stored on the work order.
#[derive(Debug, Clone, PartialEq)]
pub struct ChosenShift {
    pub class_version_id: i64,
    pub shift_start: String,
    pub shift_end: String,
}

fn refuse(code: &str, message: &str) -> AppError {
    AppError::ValidationFailed(vec![format!("{code}: {message}")])
}

/// Calendar day of a planned start (`YYYY-MM-DD…`).
pub fn planned_day(planned_start: Option<&str>) -> Option<NaiveDate> {
    let s = planned_start?.trim();
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

fn window_minutes(start: &str, end: &str) -> i64 {
    let parse = |s: &str| chrono::NaiveTime::parse_from_str(s, "%H:%M").ok();
    match (parse(start), parse(end)) {
        (Some(s), Some(e)) if e > s => (e - s).num_minutes(),
        (Some(s), Some(e)) => (e - s).num_minutes() + 24 * 60,
        _ => 0,
    }
}

/// The shift windows `team_id` works on `day`.
pub async fn shift_options(
    db: &impl ConnectionTrait,
    team_id: Option<i64>,
    day: Option<NaiveDate>,
) -> AppResult<WoShiftOptions> {
    let mut out = WoShiftOptions {
        team_id,
        day: day.map(|d| d.to_string()),
        options: Vec::new(),
        reason: None,
    };
    let Some(team_id) = team_id else {
        out.reason = Some("no_team".to_string());
        return Ok(out);
    };
    let Some(day) = day else {
        out.reason = Some("no_day".to_string());
        return Ok(out);
    };
    let days = capacity::team_capacity_range(db, team_id, day, day).await?;
    let mut by_window: BTreeMap<(String, String, i64), WoShiftOption> = BTreeMap::new();
    for member in days.iter().flat_map(|d| d.members.iter()) {
        // The shift runs that day (worked, or the member is absent from it).
        if !matches!(member.reason.as_str(), "working" | "absent") {
            continue;
        }
        let (Some(version), Some(class_id), Some(start), Some(end)) = (
            member.schedule_version_id,
            member.schedule_class_id,
            member.shift_start.clone(),
            member.shift_end.clone(),
        ) else {
            continue;
        };
        let option = by_window
            .entry((start.clone(), end.clone(), version))
            .or_insert_with(|| WoShiftOption {
                class_version_id: version,
                schedule_class_id: class_id,
                class_label: member.schedule_class_label.clone().unwrap_or_default(),
                net_hours: (window_minutes(&start, &end) - member.break_minutes).max(0) as f64 / 60.0,
                shift_start: start,
                shift_end: end,
                break_minutes: member.break_minutes,
                members: 0,
                members_available: 0,
            });
        option.members += 1;
        if member.reason == "working" {
            option.members_available += 1;
        }
    }
    out.options = by_window.into_values().collect();
    if out.options.is_empty() {
        out.reason = Some("no_shift".to_string());
    }
    Ok(out)
}

async fn wo_team_and_start(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<(Option<i64>, Option<String>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT assigned_team_id, planned_start FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    Ok((
        row.try_get::<Option<i64>>("", "assigned_team_id").ok().flatten(),
        row.try_get::<Option<String>>("", "planned_start").ok().flatten(),
    ))
}

/// The windows a work order can be planned on: its assigned team on `day` (default: the day of
/// its planned start).
pub async fn list_wo_shift_options(
    db: &impl ConnectionTrait,
    wo_id: i64,
    day: Option<&str>,
) -> AppResult<WoShiftOptions> {
    let (team_id, planned_start) = wo_team_and_start(db, wo_id).await?;
    let day = planned_day(day).or_else(|| planned_day(planned_start.as_deref()));
    shift_options(db, team_id, day).await
}

/// Checks that `class_version_id` is one of the windows the work order's team works on the
/// planned day, and returns the window to store.
pub async fn resolve_shift_choice(
    db: &impl ConnectionTrait,
    team_id: Option<i64>,
    day: Option<NaiveDate>,
    class_version_id: i64,
) -> AppResult<ChosenShift> {
    if team_id.is_none() {
        return Err(refuse(
            GATE_WO_SHIFT_NO_TEAM,
            "assign the team before choosing its shift",
        ));
    }
    if day.is_none() {
        return Err(refuse(
            GATE_WO_SHIFT_NO_DAY,
            "enter the planned start before choosing the shift",
        ));
    }
    let offered = shift_options(db, team_id, day).await?;
    offered
        .options
        .into_iter()
        .find(|o| o.class_version_id == class_version_id)
        .map(|o| ChosenShift {
            class_version_id: o.class_version_id,
            shift_start: o.shift_start,
            shift_end: o.shift_end,
        })
        .ok_or_else(|| {
            refuse(
                GATE_WO_SHIFT_NOT_OFFERED,
                "the team does not work this shift on the planned day; choose one of its shifts",
            )
        })
}

/// What a schedule save does to the shift.
#[derive(Debug, Clone, PartialEq)]
pub enum ShiftChange {
    Keep,
    Clear,
    Set(ChosenShift),
}

/// Decides the shift of a schedule save:
/// - a chosen version is checked against the team's windows on the planned day;
/// - `clear` empties it;
/// - otherwise the current shift is kept, except an old fixed code (re-planning such a work
///   order requires a window, or clearing it) and a window the team no longer works on a new
///   planned day (choose again).
pub async fn decide_shift_change(
    db: &impl ConnectionTrait,
    wo_id: i64,
    new_planned_start: Option<&str>,
    class_version_id: Option<i64>,
    clear: bool,
) -> AppResult<ShiftChange> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT assigned_team_id, planned_start, shift, shift_class_version_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let team_id = row.try_get::<Option<i64>>("", "assigned_team_id").ok().flatten();
    let current_start = row.try_get::<Option<String>>("", "planned_start").ok().flatten();
    let legacy = row
        .try_get::<Option<String>>("", "shift")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty());
    let current_version = row.try_get::<Option<i64>>("", "shift_class_version_id").ok().flatten();
    let day = planned_day(new_planned_start).or_else(|| planned_day(current_start.as_deref()));

    if let Some(version) = class_version_id {
        return Ok(ShiftChange::Set(resolve_shift_choice(db, team_id, day, version).await?));
    }
    if clear {
        return Ok(ShiftChange::Clear);
    }
    if legacy.is_some() {
        return Err(refuse(
            GATE_WO_SHIFT_LEGACY_REPLAN,
            "this work order still has an old shift code; choose a shift of the team's Classe horaire, or none",
        ));
    }
    let moved = planned_day(new_planned_start).is_some_and(|d| Some(d) != planned_day(current_start.as_deref()));
    match current_version {
        Some(version) if moved => Ok(ShiftChange::Set(resolve_shift_choice(db, team_id, day, version).await?)),
        _ => Ok(ShiftChange::Keep),
    }
}

/// Writes a shift decision (inside the transaction of the schedule save). Choosing or clearing a
/// window also drops an old fixed code.
pub async fn apply_shift_change(db: &impl ConnectionTrait, wo_id: i64, change: &ShiftChange) -> AppResult<()> {
    let (version, start, end) = match change {
        ShiftChange::Keep => return Ok(()),
        ShiftChange::Clear => (None, None, None),
        ShiftChange::Set(c) => (
            Some(c.class_version_id),
            Some(c.shift_start.clone()),
            Some(c.shift_end.clone()),
        ),
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET shift = NULL, shift_class_version_id = ?, shift_start = ?, shift_end = ? \
         WHERE id = ?",
        [version.into(), start.into(), end.into(), wo_id.into()],
    ))
    .await?;
    Ok(())
}
