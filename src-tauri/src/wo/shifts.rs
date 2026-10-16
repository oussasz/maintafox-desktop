//! The shift a work order is planned on — a window of its team's Classes horaires on the planned
//! day (SAP PM: operations scheduled inside the work centre's shift sequence; Maximo: labour
//! assigned within the crew's shift pattern).
//!
//! Options come from `personnel::capacity`, the source planning books against: each member's own
//! Classe horaire (exception, else personal, else the team's default), the version in force on the
//! day and its shift window. A start in the early hours also offers the previous day's night
//! shifts that are still running. A work order without a team or a planned day, or a day on which
//! no member has a shift, offers nothing: the shift stays empty, never assumed.
//!
//! The planned day is the local day of the planned start, in the computer's time zone like the
//! rest of the application (`chrono::Local`).
//!
//! The plan keeps a link to the version and the window it was planned with (migration 202). A
//! save that does not change the shift keeps it as stored; it is checked again only when the
//! planned start or the team moves — then matched by Classe horaire, with the version in force on
//! the new day. The old fixed codes are refused (`domain::validate_wo_shift`).

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::personnel::capacity;

pub const GATE_WO_SHIFT_NO_TEAM: &str = "GATE_WO_SHIFT_NO_TEAM";
pub const GATE_WO_SHIFT_NO_DAY: &str = "GATE_WO_SHIFT_NO_DAY";
pub const GATE_WO_SHIFT_NOT_OFFERED: &str = "GATE_WO_SHIFT_NOT_OFFERED";
pub const GATE_WO_SHIFT_LEGACY_REPLAN: &str = "GATE_WO_SHIFT_LEGACY_REPLAN";
pub const GATE_WO_SHIFT_TEAM_CHANGED: &str = "GATE_WO_SHIFT_TEAM_CHANGED";

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
    /// Members whose Classe horaire puts them on this window, and those not fully absent.
    pub members: i64,
    pub members_available: i64,
    /// A night shift that began the day before and is still running at the planned start.
    #[serde(default)]
    pub starts_previous_day: bool,
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

/// Whether an error is the shift refusal `code`.
pub fn is_refusal(err: &AppError, code: &str) -> bool {
    matches!(err, AppError::ValidationFailed(m) if m.iter().any(|s| s.starts_with(code)))
}

/// Local wall-clock formats a planned date may be typed in (no offset: the computer's time zone).
pub const LOCAL_FORMATS: [&str; 4] = [
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
];

/// Local day and time of a planned start: an RFC 3339 instant (as stored, UTC) read in the
/// computer's time zone like the rest of the application; a local `YYYY-MM-DDTHH:MM` as typed in
/// a form; or a bare day.
pub fn planned_local(planned_start: Option<&str>) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let s = planned_start?.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(instant) = chrono::DateTime::parse_from_rfc3339(s) {
        let local = instant.with_timezone(&chrono::Local).naive_local();
        return Some((local.date(), Some(local.time())));
    }
    for format in LOCAL_FORMATS {
        if let Ok(local) = NaiveDateTime::parse_from_str(s, format) {
            return Some((local.date(), Some(local.time())));
        }
    }
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d")
        .ok()
        .map(|d| (d, None))
}

/// A planned date as an instant, read like [`planned_local`]: RFC 3339 as stored, a local typed
/// date-time, or a bare day (its local midnight). `None` when it cannot be read.
pub fn planned_instant(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(instant) = chrono::DateTime::parse_from_rfc3339(raw.trim()) {
        return Some(instant.with_timezone(&chrono::Utc));
    }
    let (day, time) = planned_local(Some(raw))?;
    let naive = day.and_time(time.unwrap_or(NaiveTime::MIN));
    chrono::TimeZone::from_local_datetime(&chrono::Local, &naive)
        .earliest()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// [`planned_instant`] as the RFC 3339 text planning computes with.
pub fn planned_rfc3339(raw: &str) -> Option<String> {
    planned_instant(raw).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M").ok()
}

fn window_minutes(start: &str, end: &str) -> i64 {
    match (parse_hhmm(start), parse_hhmm(end)) {
        (Some(s), Some(e)) if e > s => (e - s).num_minutes(),
        (Some(s), Some(e)) => (e - s).num_minutes() + 24 * 60,
        _ => 0,
    }
}

/// The windows the team works on `day` (`previous_day`: only night shifts still running at
/// `running_at` the next morning).
async fn day_windows(
    db: &impl ConnectionTrait,
    team_id: i64,
    day: NaiveDate,
    previous_day_running_at: Option<NaiveTime>,
    into: &mut BTreeMap<(i64, String, String), WoShiftOption>,
) -> AppResult<()> {
    let days = capacity::team_capacity_range(db, team_id, day, day).await?;
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
        if let Some(at) = previous_day_running_at {
            // Only a window past midnight whose tail still covers `at`.
            let overnight = matches!((parse_hhmm(&start), parse_hhmm(&end)), (Some(s), Some(e)) if e <= s);
            if !overnight || parse_hhmm(&end).is_none_or(|e| at >= e) {
                continue;
            }
        }
        let option = into
            .entry((version, start.clone(), end.clone()))
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
                starts_previous_day: previous_day_running_at.is_some(),
            });
        option.members += 1;
        if member.reason == "working" {
            option.members_available += 1;
        }
    }
    Ok(())
}

/// The shift windows `team_id` works at the planned start (`at`: local day, and time when known).
pub async fn shift_options(
    db: &impl ConnectionTrait,
    team_id: Option<i64>,
    at: Option<(NaiveDate, Option<NaiveTime>)>,
) -> AppResult<WoShiftOptions> {
    let mut out = WoShiftOptions {
        team_id,
        day: at.map(|(d, _)| d.to_string()),
        options: Vec::new(),
        reason: None,
    };
    let Some(team_id) = team_id else {
        out.reason = Some("no_team".to_string());
        return Ok(out);
    };
    let Some((day, time)) = at else {
        out.reason = Some("no_day".to_string());
        return Ok(out);
    };
    // A start in the early hours: the previous day's night shift still running is the shift being
    // worked; the same window starting that evening is a different shift.
    let mut windows = BTreeMap::new();
    if let Some(time) = time {
        day_windows(db, team_id, day - Duration::days(1), Some(time), &mut windows).await?;
    }
    let mut same_day = BTreeMap::new();
    day_windows(db, team_id, day, None, &mut same_day).await?;
    for (key, option) in same_day {
        windows.entry(key).or_insert(option);
    }
    let mut options: Vec<WoShiftOption> = windows.into_values().collect();
    options.sort_by(|a, b| {
        (!a.starts_previous_day, &a.shift_start, &a.shift_end).cmp(&(
            !b.starts_previous_day,
            &b.shift_start,
            &b.shift_end,
        ))
    });
    out.options = options;
    if out.options.is_empty() {
        out.reason = Some("no_shift".to_string());
    }
    Ok(out)
}

struct WoShiftFacts {
    team_id: Option<i64>,
    planned_start: Option<String>,
    legacy: Option<String>,
    version: Option<i64>,
    window: (Option<String>, Option<String>),
}

async fn wo_facts(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<WoShiftFacts> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT assigned_team_id, planned_start, shift, shift_class_version_id, shift_start, shift_end \
             FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let text = |col: &str| row.try_get::<Option<String>>("", col).ok().flatten();
    Ok(WoShiftFacts {
        team_id: row.try_get::<Option<i64>>("", "assigned_team_id").ok().flatten(),
        planned_start: text("planned_start"),
        legacy: text("shift").filter(|s| !s.trim().is_empty()),
        version: row.try_get::<Option<i64>>("", "shift_class_version_id").ok().flatten(),
        window: (text("shift_start"), text("shift_end")),
    })
}

/// The windows a work order can be planned on: its assigned team — or `team_id`, the team being
/// assigned — at `planned_start` (local, as typed; default: its stored planned start).
pub async fn list_wo_shift_options(
    db: &impl ConnectionTrait,
    wo_id: i64,
    planned_start: Option<&str>,
    team_id: Option<i64>,
) -> AppResult<WoShiftOptions> {
    let facts = wo_facts(db, wo_id).await?;
    let at = planned_local(planned_start).or_else(|| planned_local(facts.planned_start.as_deref()));
    shift_options(db, team_id.or(facts.team_id), at).await
}

async fn class_of_version(db: &impl ConnectionTrait, version_id: i64) -> AppResult<Option<i64>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reference_value_id FROM schedule_class_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i64>("", "reference_value_id").ok()))
}

/// Checks that `class_version_id` — or, across a version boundary, its Classe horaire — is one of
/// the windows the team works at the planned start, and returns the window to store (with the
/// version in force that day).
pub async fn resolve_shift_choice(
    db: &impl ConnectionTrait,
    team_id: Option<i64>,
    at: Option<(NaiveDate, Option<NaiveTime>)>,
    class_version_id: i64,
) -> AppResult<ChosenShift> {
    if team_id.is_none() {
        return Err(refuse(
            GATE_WO_SHIFT_NO_TEAM,
            "assign the team before choosing its shift",
        ));
    }
    if at.is_none() {
        return Err(refuse(
            GATE_WO_SHIFT_NO_DAY,
            "enter the planned start before choosing the shift",
        ));
    }
    let offered = shift_options(db, team_id, at).await?.options;
    let found = match offered.iter().find(|o| o.class_version_id == class_version_id) {
        Some(o) => Some(o),
        None => match class_of_version(db, class_version_id).await? {
            Some(class_id) => offered.iter().find(|o| o.schedule_class_id == class_id),
            None => None,
        },
    };
    found
        .map(|o| ChosenShift {
            class_version_id: o.class_version_id,
            shift_start: o.shift_start.clone(),
            shift_end: o.shift_end.clone(),
        })
        .ok_or_else(|| {
            refuse(
                GATE_WO_SHIFT_NOT_OFFERED,
                "the team does not work this shift on the planned day; choose one of its shifts",
            )
        })
}

/// What a save does to the shift.
#[derive(Debug, Clone, PartialEq)]
pub enum ShiftChange {
    Keep,
    Clear,
    Set(ChosenShift),
}

/// Decides the shift when a save changes nothing of it, or the planner chose one:
/// - a chosen version is checked against the team's windows at the planned start; `clear`
///   empties the shift (both only on the planner's explicit choice);
/// - otherwise the stored shift is kept as it is — even if the timetable was edited since —
///   unless the planned start (`new_planned_start`) or the team (`new_team_id`) moves: then it is
///   matched again by Classe horaire on the new day; an old fixed code then needs a window (or
///   none); a window the team does not work there is refused.
pub async fn decide_shift(
    db: &impl ConnectionTrait,
    wo_id: i64,
    new_team_id: Option<i64>,
    new_planned_start: Option<&str>,
    class_version_id: Option<i64>,
    clear: bool,
) -> AppResult<ShiftChange> {
    let facts = wo_facts(db, wo_id).await?;
    let team = new_team_id.or(facts.team_id);
    let current_at = planned_local(facts.planned_start.as_deref());
    let at = planned_local(new_planned_start).or(current_at);
    let team_changed = team != facts.team_id;
    let moved = new_planned_start.is_some() && at != current_at;

    if let Some(version) = class_version_id {
        return Ok(ShiftChange::Set(resolve_shift_choice(db, team, at, version).await?));
    }
    if clear {
        return Ok(ShiftChange::Clear);
    }
    if !team_changed && !moved {
        return Ok(ShiftChange::Keep);
    }
    if facts.legacy.is_some() && facts.version.is_none() {
        return Err(refuse(
            GATE_WO_SHIFT_LEGACY_REPLAN,
            "this work order still has an old shift code; choose a shift of the team's Classe horaire, or none",
        ));
    }
    let Some(version) = facts.version else {
        return Ok(ShiftChange::Keep);
    };
    match resolve_shift_choice(db, team, at, version).await {
        Ok(chosen)
            if chosen.class_version_id == version
                && (Some(chosen.shift_start.clone()), Some(chosen.shift_end.clone())) == facts.window =>
        {
            Ok(ShiftChange::Keep)
        }
        Ok(chosen) => Ok(ShiftChange::Set(chosen)),
        Err(e) if team_changed && is_refusal(&e, GATE_WO_SHIFT_NOT_OFFERED) => Err(refuse(
            GATE_WO_SHIFT_TEAM_CHANGED,
            "the new team does not work the chosen shift on the planned day; choose one of its shifts, or no shift",
        )),
        Err(e) => Err(e),
    }
}

/// Writes a shift decision (inside the transaction of the save). Choosing or clearing a window
/// also drops an old fixed code.
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

/// The shift a work order had, for the note left when an emergency reassignment clears it.
pub async fn current_shift_note(db: &impl ConnectionTrait, wo_id: i64) -> AppResult<serde_json::Value> {
    let facts = wo_facts(db, wo_id).await?;
    Ok(serde_json::json!({
        "shift_class_version_id": facts.version,
        "shift_start": facts.window.0,
        "shift_end": facts.window.1,
        "legacy_shift": facts.legacy,
    }))
}
