//! A team's committed load per day — what planning books against the capacity of
//! `personnel::capacity` (SAP PM capacity load per work centre and day; Maximo crew labour load).
//!
//! - A commitment books its labour hours, stored when it was committed (`planned_labor_hours`,
//!   migration 205): the work's estimated duration × its planned crew size (migration 206). A
//!   commitment from before migration 205 counts its committed slot as the planner set it.
//! - Those hours are spread over the days by working time, not by calendar time: each day weighs
//!   the hours of its members' shift windows that the slot overlaps (a member's window counted in
//!   proportion to the part of it inside the slot). A shift that runs past midnight belongs to the
//!   day it starts, like its capacity. Hours of a member whose window is unknown count in full on
//!   the days the slot touches. Only a slot that touches no working hours at all puts its hours on
//!   the calendar days it covers, so the overload shows instead of vanishing.
//! - Finished work no longer loads the team: a work order mechanically completed, closed or
//!   cancelled; a PM occurrence completed, cancelled or missed; a request converted, closed,
//!   declined or archived.
//! - Days and shift windows are local time, like the timetables and the rest of the application.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{org_validation_failed, AppError, AppResult};

/// Largest planned crew on one job. A larger team is planned as several jobs (operations); the
/// bound also guards against a typing slip (500 instead of 5). Enforced again by the database
/// (migration 207).
pub const MAX_CREW_SIZE: i64 = 50;

/// Longest slot a commitment may span; longer work is split (a shutdown is planned per phase).
pub const MAX_SLOT_DAYS: i64 = 31;

/// A planned crew size, when given, is a whole number of people between 1 and [`MAX_CREW_SIZE`].
pub fn validate_crew_size(crew: Option<i64>) -> AppResult<()> {
    match crew {
        Some(n) if !(1..=MAX_CREW_SIZE).contains(&n) => Err(org_validation_failed(
            "PLANNING_CREW_SIZE_INVALID",
            format!("The crew size must be between 1 and {MAX_CREW_SIZE} people."),
            HashMap::from([
                ("min".to_string(), "1".to_string()),
                ("max".to_string(), MAX_CREW_SIZE.to_string()),
            ]),
        )),
        _ => Ok(()),
    }
}

fn parse_instant(raw: &str, field: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be RFC3339 (got '{raw}').")]))
}

/// A slot spans at most [`MAX_SLOT_DAYS`] days.
pub fn validate_slot_length(start: &str, end: &str) -> AppResult<()> {
    let (start, end) = (parse_instant(start, "start")?, parse_instant(end, "end")?);
    if end - start > Duration::days(MAX_SLOT_DAYS) {
        return Err(org_validation_failed(
            "PLANNING_SLOT_TOO_LONG",
            format!("A slot spans at most {MAX_SLOT_DAYS} days: split longer work into several jobs."),
            HashMap::from([("maxDays".to_string(), MAX_SLOT_DAYS.to_string())]),
        ));
    }
    Ok(())
}

fn local_midnight(day: NaiveDate) -> DateTime<Utc> {
    let naive = day.and_time(NaiveTime::MIN);
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.from_utc_datetime(&naive))
}

fn local_naive(instant: DateTime<Utc>) -> NaiveDateTime {
    instant.with_timezone(&Local).naive_local()
}

/// The local days a slot covers, with the share (0–1] of each day it covers.
pub fn slot_days(start: &str, end: &str) -> AppResult<Vec<(NaiveDate, f64)>> {
    let start = parse_instant(start, "start")?;
    let end = parse_instant(end, "end")?;
    if end <= start {
        return Ok(Vec::new());
    }
    let first = start.with_timezone(&Local).date_naive();
    // The last instant inside the slot decides the last day (a slot ending at midnight stops the day before).
    let last = (end - Duration::seconds(1)).with_timezone(&Local).date_naive();
    let mut out = Vec::new();
    let mut day = first;
    while day <= last {
        let from = local_midnight(day).max(start);
        let to = local_midnight(day + Duration::days(1)).min(end);
        let covered = (to - from).num_seconds() as f64 / 86_400.0;
        if covered > 0.0 {
            out.push((day, covered.min(1.0)));
        }
        day += Duration::days(1);
    }
    Ok(out)
}

/// One member's shift window on a day (local time) and the hours they bring to it.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkWindow {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub hours: f64,
}

/// A team's working time on one day: the members' shift windows, and the hours of members whose
/// window is not known (counted in full on the days a slot touches).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DayHours {
    pub windows: Vec<WorkWindow>,
    pub unwindowed: f64,
}

/// A team's day as planning reads it from `personnel::capacity`.
#[derive(Debug, Clone, PartialEq)]
pub struct TeamDay {
    pub available: f64,
    /// The planner's overtime ceiling for the day (`capacity_rules`), 0 without a rule.
    pub overtime: f64,
    pub basis: String,
    pub hours: DayHours,
}

impl TeamDay {
    pub fn from_capacity(cap: &crate::personnel::capacity::TeamDayCapacity) -> AppResult<(NaiveDate, Self)> {
        let day = NaiveDate::parse_from_str(&cap.day, "%Y-%m-%d")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("capacity day '{}': {e}", cap.day)))?;
        let mut hours = DayHours::default();
        for member in cap.members.iter().filter(|m| m.available_hours > 0.0) {
            let window = match (member.shift_start.as_deref(), member.shift_end.as_deref()) {
                (Some(s), Some(e)) => crate::personnel::capacity::shift_window(day, s, e),
                _ => None,
            };
            match window {
                Some((start, end)) => hours.windows.push(WorkWindow {
                    start,
                    end,
                    hours: member.available_hours,
                }),
                None => hours.unwindowed += member.available_hours,
            }
        }
        Ok((
            day,
            Self {
                available: cap.available_hours,
                overtime: cap.rule_overtime_hours.unwrap_or(0.0),
                basis: cap.basis.clone(),
                hours,
            },
        ))
    }
}

/// The team's days over `[from, to]` from its timetables (absences and utilisation applied).
pub async fn team_days(
    db: &impl ConnectionTrait,
    team_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<BTreeMap<NaiveDate, TeamDay>> {
    let mut out = BTreeMap::new();
    if to < from {
        return Ok(out);
    }
    for cap in crate::personnel::capacity::team_capacity_span(db, team_id, from, to).await? {
        let (day, team_day) = TeamDay::from_capacity(&cap)?;
        out.insert(day, team_day);
    }
    Ok(out)
}

/// The days whose capacity a slot can use: the day before it starts (a night shift that started
/// then may run into the slot) through its last day.
pub fn capacity_span(start: &str, end: &str) -> AppResult<Option<(NaiveDate, NaiveDate)>> {
    let days = slot_days(start, end)?;
    Ok(match (days.first(), days.last()) {
        (Some(first), Some(last)) => Some((first.0 - Duration::days(1), last.0)),
        _ => None,
    })
}

fn overlap_seconds(a: (NaiveDateTime, NaiveDateTime), b: (NaiveDateTime, NaiveDateTime)) -> f64 {
    let from = a.0.max(b.0);
    let to = a.1.min(b.1);
    if to > from {
        (to - from).num_seconds() as f64
    } else {
        0.0
    }
}

/// Spreads `labour_hours` over the days, weighted by the working time the slot covers each day
/// (see the module notes). Days with no working time in the slot get nothing, unless the slot
/// touches no working time at all.
pub fn spread(
    labour_hours: f64,
    start: &str,
    end: &str,
    days: &BTreeMap<NaiveDate, DayHours>,
) -> AppResult<BTreeMap<NaiveDate, f64>> {
    let calendar = slot_days(start, end)?;
    if calendar.is_empty() {
        return Ok(BTreeMap::new());
    }
    let slot = (
        local_naive(parse_instant(start, "start")?),
        local_naive(parse_instant(end, "end")?),
    );
    let touched = |day: &NaiveDate| calendar.iter().any(|(d, _)| d == day);
    let mut weights: Vec<(NaiveDate, f64)> = Vec::new();
    for (day, hours) in days {
        let mut weight = 0.0;
        for window in &hours.windows {
            let length = (window.end - window.start).num_seconds() as f64;
            if length > 0.0 {
                weight += window.hours * overlap_seconds(slot, (window.start, window.end)) / length;
            }
        }
        if hours.unwindowed > 0.0 && touched(day) {
            weight += hours.unwindowed;
        }
        if weight > 0.0 {
            weights.push((*day, weight));
        }
    }
    // No working time anywhere in the slot: the hours land on the days it covers (the overload shows).
    if weights.is_empty() {
        weights = calendar;
    }
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return Ok(BTreeMap::new());
    }
    Ok(weights
        .into_iter()
        .map(|(d, w)| (d, labour_hours * w / total))
        .collect())
}

/// Instants bounding the slots that can load the days `[from, to]`: local midnight of `from`, and
/// local midnight two days after `to` (a slot early the next morning may sit in a night shift that
/// starts on `to`).
pub fn load_query_bounds(from: NaiveDate, to: NaiveDate) -> (String, String) {
    (
        local_midnight(from).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        local_midnight(to + Duration::days(2)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

/// Working time a slot covers (hours): the parts of the shift windows inside it, plus the hours of
/// members without a known window on the days it touches.
pub fn working_hours_in(start: &str, end: &str, days: &BTreeMap<NaiveDate, DayHours>) -> AppResult<f64> {
    let calendar = slot_days(start, end)?;
    if calendar.is_empty() {
        return Ok(0.0);
    }
    let slot = (
        local_naive(parse_instant(start, "start")?),
        local_naive(parse_instant(end, "end")?),
    );
    let mut hours = 0.0;
    for (day, day_hours) in days {
        for window in &day_hours.windows {
            hours += overlap_seconds(slot, (window.start, window.end)) / 3600.0;
        }
        if day_hours.unwindowed > 0.0 && calendar.iter().any(|(d, _)| d == day) {
            hours += day_hours.unwindowed;
        }
    }
    Ok(hours)
}

/// Net working hours a slot covers: each shift window's hours (net of break and utilisation) in
/// proportion to the part of it inside the slot, plus the hours of members without a known window
/// on the days it touches — what an OT without a duration books.
pub fn net_hours_in(start: &str, end: &str, days: &BTreeMap<NaiveDate, DayHours>) -> AppResult<f64> {
    let calendar = slot_days(start, end)?;
    if calendar.is_empty() {
        return Ok(0.0);
    }
    let slot = (
        local_naive(parse_instant(start, "start")?),
        local_naive(parse_instant(end, "end")?),
    );
    let mut hours = 0.0;
    for (day, day_hours) in days {
        for window in &day_hours.windows {
            let length = (window.end - window.start).num_seconds() as f64;
            if length > 0.0 {
                hours += window.hours * overlap_seconds(slot, (window.start, window.end)) / length;
            }
        }
        if day_hours.unwindowed > 0.0 && calendar.iter().any(|(d, _)| d == day) {
            hours += day_hours.unwindowed;
        }
    }
    Ok(hours)
}

/// SQL predicate: the commitment's work is still to do (alias `cmt`): the slot is held (not
/// released, cancelled or closed — migration 212) and its source is not finished.
pub const ACTIVE_COMMITMENT_SQL: &str = "(cmt.status = 'committed' AND NOT ( \
    (cmt.source_type = 'work_order' AND EXISTS (SELECT 1 FROM work_orders fw WHERE fw.id = cmt.source_id \
        AND (fw.mechanically_completed_at IS NOT NULL OR fw.closed_at IS NOT NULL OR fw.cancelled_at IS NOT NULL))) \
    OR (cmt.source_type = 'pm_occurrence' AND EXISTS (SELECT 1 FROM pm_occurrences fp WHERE fp.id = cmt.source_id \
        AND fp.status IN ('completed', 'cancelled', 'missed'))) \
    OR (cmt.source_type = 'inspection_follow_up' AND EXISTS (SELECT 1 FROM intervention_requests fr \
        WHERE fr.id = cmt.source_id AND (fr.converted_to_wo_id IS NOT NULL OR fr.closed_at IS NOT NULL \
        OR fr.declined_at IS NOT NULL OR fr.archived_at IS NOT NULL)))))";

/// [`ACTIVE_COMMITMENT_SQL`] for another alias of `schedule_commitments`.
pub fn active_commitment_sql_as(alias: &str) -> String {
    ACTIVE_COMMITMENT_SQL.replace("cmt.", &format!("{alias}."))
}

/// SQL expression: when the work of a finished commitment (alias `alias` of
/// `schedule_commitments`) ended — the work order's mechanical completion (else its cancellation,
/// else its closure), the PM occurrence's last move to completed / cancelled / missed (else its
/// last change), the request's conversion, decline, closure or archiving. `NULL` while the work is
/// not finished. Every device computes the same value from the same synchronised rows.
pub fn finished_at_sql(alias: &str) -> String {
    format!(
        "(CASE {a}.source_type \
            WHEN 'work_order' THEN (SELECT COALESCE(fw.mechanically_completed_at, fw.cancelled_at, fw.closed_at) \
                FROM work_orders fw WHERE fw.id = {a}.source_id) \
            WHEN 'pm_occurrence' THEN (SELECT COALESCE((SELECT MAX(t.transitioned_at) FROM pm_occurrence_transitions t \
                    WHERE t.pm_occurrence_id = fp.id AND t.to_status IN ('completed', 'cancelled', 'missed')), \
                    fp.updated_at) \
                FROM pm_occurrences fp WHERE fp.id = {a}.source_id \
                 AND fp.status IN ('completed', 'cancelled', 'missed')) \
            WHEN 'inspection_follow_up' THEN (SELECT COALESCE(fr.converted_at, fr.declined_at, fr.closed_at, fr.archived_at) \
                FROM intervention_requests fr WHERE fr.id = {a}.source_id) \
         END)",
        a = alias
    )
}

/// SQL predicate: no active planning commitment covers the work order (alias `wo`) — through itself,
/// the PM occurrence it was generated for, or the request it was converted from. With
/// `assigned_only`, only commitments that name a technician count (a person's view).
pub fn wo_not_committed_sql(assigned_only: bool) -> String {
    format!(
        "NOT EXISTS (SELECT 1 FROM schedule_commitments c2 WHERE ( \
            (c2.source_type = 'work_order' AND c2.source_id = wo.id) \
            OR (c2.source_type = 'pm_occurrence' AND c2.source_id IN \
                (SELECT po.id FROM pm_occurrences po WHERE po.linked_work_order_id = wo.id)) \
            OR (c2.source_type = 'inspection_follow_up' AND c2.source_id IN \
                (SELECT ir.id FROM intervention_requests ir WHERE ir.converted_to_wo_id = wo.id))) \
            {assigned} AND {active})",
        assigned = if assigned_only {
            "AND c2.assigned_personnel_id IS NOT NULL"
        } else {
            ""
        },
        active = active_commitment_sql_as("c2"),
    )
}

/// SQL predicate: the work order (alias `wo`) is still to do.
pub const WO_OPEN_SQL: &str =
    "wo.mechanically_completed_at IS NULL AND wo.closed_at IS NULL AND wo.cancelled_at IS NULL";

/// SQL predicate: the work order's planned dates (alias `wo`) may touch `[?, ?]` (end, start bounds,
/// a day of margin for typed local dates) — or cannot be read by SQLite, so the caller reads them.
pub const WO_PLANNED_IN_RANGE_SQL: &str = "wo.planned_start IS NOT NULL AND wo.planned_end IS NOT NULL \
    AND (julianday(wo.planned_start) IS NULL OR julianday(wo.planned_end) IS NULL \
         OR (julianday(wo.planned_start) < julianday(?) + 1 AND julianday(wo.planned_end) > julianday(?) - 1))";

/// The hours a slot of the team books.
enum Booked {
    Fixed(f64),
    /// An OT without a duration: its responsible technician's net working time in the slot.
    PersonWorkingTime(i64),
}

struct LoadRow {
    team_id: i64,
    start: String,
    end: String,
    booked: Booked,
}

fn load_decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("load decode '{field}': {err}"))
}

/// Committed labour hours per (team, day) over `[from, to]`: the active commitments that touch those
/// days, and the OTs the team carries outside planning (planned dates, open, not committed) — the
/// same work the technicians' own figures count (`planning::person_load`). `teams`: these teams
/// only; `exclude`: a commitment being moved; `exclude_work_order`: the OT being committed.
///
/// An OT outside planning books its duration × planned crew size; without a crew size, its named
/// responsible technician's duration (the others are not known); without a duration, that
/// technician's net working time in its slot. An OT with neither is not counted (hours unknown);
/// one whose planned dates cannot be read is skipped.
pub async fn team_day_loads(
    db: &impl ConnectionTrait,
    teams: Option<&[i64]>,
    from: NaiveDate,
    to: NaiveDate,
    exclude: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<BTreeMap<(i64, NaiveDate), f64>> {
    let mut out = BTreeMap::new();
    if to < from {
        return Ok(out);
    }
    let range_start = local_midnight(from).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    // A day after `to`: a slot early next morning may sit in the night shift that starts on `to`.
    let range_end = local_midnight(to + Duration::days(2)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut sql = format!(
        "SELECT cmt.assigned_team_id, cmt.committed_start, cmt.committed_end, \
                COALESCE(cmt.planned_labor_hours, \
                         (julianday(cmt.committed_end) - julianday(cmt.committed_start)) * 24.0) AS labour_hours \
           FROM schedule_commitments cmt \
          WHERE julianday(cmt.committed_start) < julianday(?) AND julianday(cmt.committed_end) > julianday(?) \
            AND cmt.assigned_team_id IS NOT NULL \
            AND (? IS NULL OR cmt.id <> ?) AND {ACTIVE_COMMITMENT_SQL}"
    );
    let mut values: Vec<sea_orm::Value> = vec![
        range_end.clone().into(),
        range_start.clone().into(),
        exclude.into(),
        exclude.into(),
    ];
    let team_filter = match teams {
        Some([]) => return Ok(out),
        Some(teams) => format!(
            " AND {{alias}}.assigned_team_id IN ({})",
            vec!["?"; teams.len()].join(", ")
        ),
        None => String::new(),
    };
    sql.push_str(&team_filter.replace("{alias}", "cmt"));
    if let Some(teams) = teams {
        values.extend(teams.iter().map(|t| sea_orm::Value::from(*t)));
    }
    let mut rows: Vec<LoadRow> = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .iter()
        .map(|r| {
            Ok(LoadRow {
                team_id: r
                    .try_get("", "assigned_team_id")
                    .map_err(|e| load_decode_err("assigned_team_id", e))?,
                start: r
                    .try_get("", "committed_start")
                    .map_err(|e| load_decode_err("committed_start", e))?,
                end: r
                    .try_get("", "committed_end")
                    .map_err(|e| load_decode_err("committed_end", e))?,
                booked: Booked::Fixed(
                    r.try_get::<f64>("", "labour_hours")
                        .map_err(|e| load_decode_err("labour_hours", e))?
                        .max(0.0),
                ),
            })
        })
        .collect::<AppResult<_>>()?;

    // OTs the team carries outside planning.
    let wo_sql = format!(
        "SELECT wo.assigned_team_id, wo.planned_start, wo.planned_end, wo.expected_duration_hours, \
                wo.planned_crew_size, ua.personnel_id AS responsible_personnel_id \
           FROM work_orders wo LEFT JOIN user_accounts ua ON ua.id = wo.primary_responsible_id \
          WHERE wo.assigned_team_id IS NOT NULL AND {WO_PLANNED_IN_RANGE_SQL} \
            AND (? IS NULL OR wo.id <> ?) AND {WO_OPEN_SQL} AND {not_committed}{teams}",
        not_committed = wo_not_committed_sql(false),
        teams = team_filter.replace("{alias}", "wo"),
    );
    let mut values: Vec<sea_orm::Value> = vec![
        range_end.into(),
        range_start.into(),
        exclude_work_order.into(),
        exclude_work_order.into(),
    ];
    if let Some(teams) = teams {
        values.extend(teams.iter().map(|t| sea_orm::Value::from(*t)));
    }
    for r in db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &wo_sql, values))
        .await?
    {
        let start: String = r
            .try_get("", "planned_start")
            .map_err(|e| load_decode_err("planned_start", e))?;
        let end: String = r
            .try_get("", "planned_end")
            .map_err(|e| load_decode_err("planned_end", e))?;
        // Typed local dates are read like the shift code reads them; unreadable ones are skipped.
        let (Some(start), Some(end)) = (
            crate::wo::shifts::planned_rfc3339(&start),
            crate::wo::shifts::planned_rfc3339(&end),
        ) else {
            continue;
        };
        if end <= start {
            continue;
        }
        let duration: Option<f64> = r
            .try_get::<Option<f64>>("", "expected_duration_hours")
            .map_err(|e| load_decode_err("expected_duration_hours", e))?
            .filter(|h| *h > 0.0);
        let crew: Option<i64> = r
            .try_get::<Option<i64>>("", "planned_crew_size")
            .map_err(|e| load_decode_err("planned_crew_size", e))?
            .filter(|n| *n >= 1);
        let responsible: Option<i64> = r
            .try_get("", "responsible_personnel_id")
            .map_err(|e| load_decode_err("responsible_personnel_id", e))?;
        let booked = match (duration, crew, responsible) {
            (Some(d), Some(c), _) => Booked::Fixed(d * c as f64),
            (Some(d), None, Some(_)) => Booked::Fixed(d),
            (None, _, Some(person)) => Booked::PersonWorkingTime(person),
            _ => continue,
        };
        rows.push(LoadRow {
            team_id: r
                .try_get("", "assigned_team_id")
                .map_err(|e| load_decode_err("assigned_team_id", e))?,
            start,
            end,
            booked,
        });
    }

    // Each team's working time over every day its commitments can use (they may reach outside
    // [from, to]).
    let mut spans: BTreeMap<i64, (NaiveDate, NaiveDate)> = BTreeMap::new();
    for row in &rows {
        let Some((first, last)) = capacity_span(&row.start, &row.end)? else {
            continue;
        };
        let span = spans.entry(row.team_id).or_insert((first, last));
        span.0 = span.0.min(first);
        span.1 = span.1.max(last);
    }
    let mut hours: BTreeMap<i64, BTreeMap<NaiveDate, DayHours>> = BTreeMap::new();
    for (team_id, (first, last)) in spans {
        let days = team_days(db, team_id, first, last).await?;
        hours.insert(team_id, days.into_iter().map(|(d, t)| (d, t.hours)).collect());
    }
    let empty = BTreeMap::new();
    for row in rows {
        let team_hours = hours.get(&row.team_id).unwrap_or(&empty);
        let labour = match row.booked {
            Booked::Fixed(h) => h,
            Booked::PersonWorkingTime(person) => {
                let Some((first, last)) = capacity_span(&row.start, &row.end)? else {
                    continue;
                };
                let person_hours: BTreeMap<NaiveDate, DayHours> =
                    crate::planning::person_load::person_days(db, person, first, last)
                        .await?
                        .into_iter()
                        .map(|(d, p)| (d, p.hours))
                        .collect();
                net_hours_in(&row.start, &row.end, &person_hours)?
            }
        };
        for (day, share) in spread(labour, &row.start, &row.end, team_hours)? {
            if day >= from && day <= to {
                *out.entry((row.team_id, day)).or_insert(0.0) += share;
            }
        }
    }
    Ok(out)
}

/// Committed labour hours per (team, month 1–12) of `year`, counted like the board
/// ([`team_day_loads`]): what finance compares with each team's monthly capacity.
pub async fn team_month_loads(db: &impl ConnectionTrait, year: i32) -> AppResult<BTreeMap<(i64, u32), f64>> {
    let mut out = BTreeMap::new();
    let (Some(first), Some(last)) = (
        NaiveDate::from_ymd_opt(year, 1, 1),
        NaiveDate::from_ymd_opt(year, 12, 31),
    ) else {
        return Ok(out);
    };
    for ((team_id, day), hours) in team_day_loads(db, None, first, last, None, None).await? {
        *out.entry((team_id, chrono::Datelike::month(&day))).or_insert(0.0) += hours;
    }
    Ok(out)
}
