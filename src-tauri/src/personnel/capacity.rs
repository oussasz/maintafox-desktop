//! Real labour capacity — per person and per team, per day — from the people's shift
//! calendars, minus absences and plant holidays (SAP PM work-centre capacity from shift
//! sequences and the factory calendar; Maximo labour availability from work calendars minus
//! modified availability).
//!
//! A person's day:
//! - outside employment (not active, before hire / contract start, after termination / contract
//!   end) → 0, `not_employed`;
//! - no home schedule, or no line for that weekday → 0, `no_schedule`;
//! - rest day → 0, `rest_day`; a plant holiday (all sites, or the person's org node or one of
//!   its parents) → 0, `holiday`;
//! - otherwise the shift length (a shift ending at or before its start runs past midnight) minus
//!   the absence blocks overlapping it → `working` (or `absent` when nothing is left).
//!
//! A team's day: its members that day (validity-dated team assignments with their allocation
//! percentage; people without any assignment count fully in their primary team), summed. No
//! default hours anywhere: a team whose members have no schedule reports `no_schedule` and 0 h.
//! The team's `capacity_rules` row, if any, is returned alongside, untouched, for the planner.

use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

/// Longest range computed in one call (one quarter).
pub const MAX_RANGE_DAYS: i64 = 92;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersonDayCapacity {
    pub personnel_id: i64,
    pub day: String,
    pub shift_start: Option<String>,
    pub shift_end: Option<String>,
    pub scheduled_minutes: i64,
    pub absent_minutes: i64,
    pub available_minutes: i64,
    /// `working` | `absent` | `rest_day` | `holiday` | `no_schedule` | `not_employed`.
    pub reason: String,
    pub holiday_label: Option<String>,
    pub absence_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamMemberDay {
    pub personnel_id: i64,
    pub full_name: String,
    pub allocation_percent: f64,
    pub available_hours: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamDayCapacity {
    pub team_id: i64,
    pub day: String,
    /// Σ members' available hours × allocation.
    pub available_hours: f64,
    /// Σ members' scheduled hours × allocation (before absences).
    pub scheduled_hours: f64,
    pub absent_hours: f64,
    /// Members on the day.
    pub people: i64,
    /// Members with a shift that day (holidays excluded).
    pub people_scheduled: i64,
    /// Members with a shift that day fully absent.
    pub people_absent: i64,
    /// `roster` | `holiday` | `no_schedule` | `no_members`.
    pub basis: String,
    pub rule_available_hours: Option<f64>,
    pub rule_overtime_hours: Option<f64>,
    pub members: Vec<TeamMemberDay>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("capacity decode '{field}': {err}"))
}

fn range_error() -> AppError {
    crate::errors::org_validation_failed(
        "CAPACITY_RANGE_INVALID",
        "Choose a start date before the end date, at most 92 days apart.",
        HashMap::from([("maxDays".to_string(), MAX_RANGE_DAYS.to_string())]),
    )
}

fn validate_range(from: NaiveDate, to: NaiveDate) -> AppResult<()> {
    if to < from || (to - from).num_days() >= MAX_RANGE_DAYS {
        return Err(range_error());
    }
    Ok(())
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    let prefix: String = s.chars().take(10).collect();
    NaiveDate::parse_from_str(&prefix, "%Y-%m-%d").ok()
}

/// Absence bounds as naive UTC (the shift times are compared in the same frame, as the
/// availability calendar does).
fn parse_instant(s: &str) -> Option<NaiveDateTime> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.naive_utc())
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok())
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").ok())
        .or_else(|| parse_day(s).and_then(|d| d.and_hms_opt(0, 0, 0)))
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M")
        .ok()
        .or_else(|| NaiveTime::parse_from_str(s, "%H:%M:%S").ok())
}

/// Shift window of `day`; a shift whose end is not after its start ends the next day.
pub fn shift_window(day: NaiveDate, start: &str, end: &str) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let s = day.and_time(parse_time(start)?);
    let mut e = day.and_time(parse_time(end)?);
    if e <= s {
        e += Duration::days(1);
    }
    Some((s, e))
}

/// ISO weekday 1 = Monday … 7 = Sunday (the `schedule_details.day_of_week` convention).
fn iso_weekday(day: NaiveDate) -> i64 {
    i64::from(day.weekday().number_from_monday())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Loading
// ═══════════════════════════════════════════════════════════════════════════════

struct ShiftLine {
    start: String,
    end: String,
    rest: bool,
}

struct Absence {
    start: NaiveDateTime,
    end: NaiveDateTime,
    block_type: String,
}

struct PersonFacts {
    id: i64,
    full_name: String,
    employed_from: Option<NaiveDate>,
    employed_to: Option<NaiveDate>,
    active: bool,
    shifts: HashMap<i64, ShiftLine>,
    absences: Vec<Absence>,
    /// Holidays applying to the person, by date.
    holidays: HashMap<NaiveDate, String>,
}

fn opt_day(row: &sea_orm::QueryResult, col: &str) -> Option<NaiveDate> {
    row.try_get::<Option<String>>("", col)
        .ok()
        .flatten()
        .as_deref()
        .and_then(parse_day)
}

async fn load_person(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<PersonFacts> {
    let p = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, full_name, employment_status, hire_date, termination_date, contract_start_date, \
                    contract_end_date, home_schedule_reference_value_id, primary_entity_id \
             FROM personnel WHERE id = ?",
            [personnel_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: personnel_id.to_string(),
        })?;
    let schedule_id: Option<i64> = p.try_get("", "home_schedule_reference_value_id").ok().flatten();
    let entity_id: Option<i64> = p.try_get("", "primary_entity_id").ok().flatten();
    let employed_from = match (opt_day(&p, "hire_date"), opt_day(&p, "contract_start_date")) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let employed_to = match (opt_day(&p, "termination_date"), opt_day(&p, "contract_end_date")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };

    let mut shifts = HashMap::new();
    if let Some(schedule_id) = schedule_id {
        for r in db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT day_of_week, shift_start, shift_end, is_rest_day FROM schedule_details \
                 WHERE reference_value_id = ?",
                [schedule_id.into()],
            ))
            .await?
        {
            shifts.insert(
                r.try_get::<i64>("", "day_of_week").map_err(|e| decode_err("day_of_week", e))?,
                ShiftLine {
                    start: r.try_get("", "shift_start").map_err(|e| decode_err("shift_start", e))?,
                    end: r.try_get("", "shift_end").map_err(|e| decode_err("shift_end", e))?,
                    rest: r.try_get::<i64>("", "is_rest_day").map_err(|e| decode_err("is_rest_day", e))? != 0,
                },
            );
        }
    }

    // Blocks that can overlap a shift of the range (a night shift reaches into the next day).
    let window_start = from.and_hms_opt(0, 0, 0).unwrap_or_default();
    let window_end = (to + Duration::days(2)).and_hms_opt(0, 0, 0).unwrap_or_default();
    let mut absences = Vec::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT start_at, end_at, block_type FROM personnel_availability_blocks \
             WHERE personnel_id = ? AND julianday(start_at) < julianday(?) AND julianday(end_at) > julianday(?)",
            [
                personnel_id.into(),
                window_end.format("%Y-%m-%dT%H:%M:%SZ").to_string().into(),
                window_start.format("%Y-%m-%dT%H:%M:%SZ").to_string().into(),
            ],
        ))
        .await?
    {
        let start: String = r.try_get("", "start_at").map_err(|e| decode_err("start_at", e))?;
        let end: String = r.try_get("", "end_at").map_err(|e| decode_err("end_at", e))?;
        // An unreadable block is skipped by SQL date arithmetic too; refuse to guess here.
        let (Some(start), Some(end)) = (parse_instant(&start), parse_instant(&end)) else {
            continue;
        };
        absences.push(Absence {
            start,
            end,
            block_type: r.try_get("", "block_type").map_err(|e| decode_err("block_type", e))?,
        });
    }

    let mut holidays = HashMap::new();
    for r in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "WITH RECURSIVE ancestors(id, parent_id) AS ( \
                SELECT id, parent_id FROM org_nodes WHERE id = ? \
                UNION ALL \
                SELECT n.id, n.parent_id FROM org_nodes n JOIN ancestors a ON n.id = a.parent_id \
             ) \
             SELECT holiday_date, label FROM work_calendar_holidays \
             WHERE is_active = 1 AND holiday_date BETWEEN ? AND ? \
               AND (entity_id IS NULL OR entity_id IN (SELECT id FROM ancestors)) \
             ORDER BY holiday_date, entity_id IS NULL",
            [entity_id.into(), from.to_string().into(), to.to_string().into()],
        ))
        .await?
    {
        let raw: String = r.try_get("", "holiday_date").map_err(|e| decode_err("holiday_date", e))?;
        if let Some(day) = parse_day(&raw) {
            holidays
                .entry(day)
                .or_insert(r.try_get("", "label").map_err(|e| decode_err("label", e))?);
        }
    }

    Ok(PersonFacts {
        id: personnel_id,
        full_name: p.try_get("", "full_name").map_err(|e| decode_err("full_name", e))?,
        employed_from,
        employed_to,
        active: p
            .try_get::<String>("", "employment_status")
            .map_err(|e| decode_err("employment_status", e))?
            == "active",
        shifts,
        absences,
        holidays,
    })
}

fn person_day(facts: &PersonFacts, day: NaiveDate) -> PersonDayCapacity {
    let mut out = PersonDayCapacity {
        personnel_id: facts.id,
        day: day.to_string(),
        shift_start: None,
        shift_end: None,
        scheduled_minutes: 0,
        absent_minutes: 0,
        available_minutes: 0,
        reason: "working".to_string(),
        holiday_label: None,
        absence_types: Vec::new(),
    };
    let employed = facts.active
        && facts.employed_from.is_none_or(|d| d <= day)
        && facts.employed_to.is_none_or(|d| d >= day);
    if !employed {
        out.reason = "not_employed".to_string();
        return out;
    }
    let Some(line) = facts.shifts.get(&iso_weekday(day)) else {
        out.reason = "no_schedule".to_string();
        return out;
    };
    if line.rest {
        out.reason = "rest_day".to_string();
        return out;
    }
    let Some((start, end)) = shift_window(day, &line.start, &line.end) else {
        out.reason = "no_schedule".to_string();
        return out;
    };
    out.shift_start = Some(line.start.clone());
    out.shift_end = Some(line.end.clone());
    if let Some(label) = facts.holidays.get(&day) {
        out.reason = "holiday".to_string();
        out.holiday_label = Some(label.clone());
        return out;
    }
    out.scheduled_minutes = (end - start).num_minutes();

    // Union of the absence intervals clipped to the shift (overlapping blocks count once).
    let mut clipped: Vec<(NaiveDateTime, NaiveDateTime)> = facts
        .absences
        .iter()
        .filter(|a| a.start < end && a.end > start)
        .map(|a| (a.start.max(start), a.end.min(end)))
        .collect();
    for a in facts.absences.iter().filter(|a| a.start < end && a.end > start) {
        if !out.absence_types.contains(&a.block_type) {
            out.absence_types.push(a.block_type.clone());
        }
    }
    clipped.sort();
    let mut absent = 0i64;
    let mut cursor: Option<(NaiveDateTime, NaiveDateTime)> = None;
    for (s, e) in clipped {
        match cursor {
            Some((cs, ce)) if s <= ce => cursor = Some((cs, ce.max(e))),
            Some((cs, ce)) => {
                absent += (ce - cs).num_minutes();
                cursor = Some((s, e));
            }
            None => cursor = Some((s, e)),
        }
    }
    if let Some((cs, ce)) = cursor {
        absent += (ce - cs).num_minutes();
    }
    out.absent_minutes = absent.min(out.scheduled_minutes);
    out.available_minutes = out.scheduled_minutes - out.absent_minutes;
    if out.available_minutes == 0 {
        out.reason = "absent".to_string();
    }
    out
}

/// One person's capacity for each day of `[from, to]`.
pub async fn person_capacity_range(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<PersonDayCapacity>> {
    validate_range(from, to)?;
    let facts = load_person(db, personnel_id, from, to).await?;
    Ok(from.iter_days().take_while(|d| *d <= to).map(|d| person_day(&facts, d)).collect())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Teams
// ═══════════════════════════════════════════════════════════════════════════════

struct Membership {
    personnel_id: i64,
    allocation_percent: f64,
    valid_from: Option<NaiveDate>,
    valid_to: Option<NaiveDate>,
}

impl Membership {
    fn covers(&self, day: NaiveDate) -> bool {
        self.valid_from.is_none_or(|d| d <= day) && self.valid_to.is_none_or(|d| d >= day)
    }
}

/// Members of a team over a range: explicit assignments, and primary-team people who have no
/// assignment row at all (they work fully in their primary team).
async fn team_memberships(
    db: &impl ConnectionTrait,
    team_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<Membership>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT pta.personnel_id, pta.allocation_percent, pta.valid_from, pta.valid_to \
               FROM personnel_team_assignments pta \
               JOIN personnel p ON p.id = pta.personnel_id \
              WHERE pta.team_id = ? AND p.employment_status = 'active' \
                AND (pta.valid_from IS NULL OR date(pta.valid_from) <= date(?)) \
                AND (pta.valid_to IS NULL OR date(pta.valid_to) >= date(?)) \
             UNION ALL \
             SELECT p.id, 100.0, NULL, NULL FROM personnel p \
              WHERE p.primary_team_id = ? AND p.employment_status = 'active' \
                AND NOT EXISTS (SELECT 1 FROM personnel_team_assignments x WHERE x.personnel_id = p.id)",
            [
                team_id.into(),
                to.to_string().into(),
                from.to_string().into(),
                team_id.into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(Membership {
                personnel_id: r.try_get("", "personnel_id").map_err(|e| decode_err("personnel_id", e))?,
                allocation_percent: r
                    .try_get::<f64>("", "allocation_percent")
                    .map_err(|e| decode_err("allocation_percent", e))?
                    .clamp(0.0, 100.0),
                valid_from: opt_day(r, "valid_from"),
                valid_to: opt_day(r, "valid_to"),
            })
        })
        .collect()
}

async fn capacity_rule(db: &impl ConnectionTrait, team_id: i64, day: NaiveDate) -> AppResult<(Option<f64>, Option<f64>)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT available_hours_per_day, max_overtime_hours_per_day FROM capacity_rules \
             WHERE team_id = ? AND effective_start <= ? AND (effective_end IS NULL OR effective_end >= ?) \
             ORDER BY effective_start DESC LIMIT 1",
            [team_id.into(), day.to_string().into(), day.to_string().into()],
        ))
        .await?;
    Ok(match row {
        Some(r) => (
            Some(
                r.try_get("", "available_hours_per_day")
                    .map_err(|e| decode_err("available_hours_per_day", e))?,
            ),
            Some(
                r.try_get("", "max_overtime_hours_per_day")
                    .map_err(|e| decode_err("max_overtime_hours_per_day", e))?,
            ),
        ),
        None => (None, None),
    })
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Team capacity for each day of `[from, to]` (at most [`MAX_RANGE_DAYS`]).
pub async fn team_capacity_range(
    db: &impl ConnectionTrait,
    team_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<TeamDayCapacity>> {
    validate_range(from, to)?;
    let memberships = team_memberships(db, team_id, from, to).await?;
    let mut facts: BTreeMap<i64, PersonFacts> = BTreeMap::new();
    for m in &memberships {
        if !facts.contains_key(&m.personnel_id) {
            facts.insert(m.personnel_id, load_person(db, m.personnel_id, from, to).await?);
        }
    }

    let mut out = Vec::new();
    for day in from.iter_days().take_while(|d| *d <= to) {
        // One allocation per person and day (the largest valid one).
        let mut allocation: BTreeMap<i64, f64> = BTreeMap::new();
        for m in memberships.iter().filter(|m| m.covers(day)) {
            let entry = allocation.entry(m.personnel_id).or_insert(0.0);
            *entry = entry.max(m.allocation_percent);
        }
        let (rule_available_hours, rule_overtime_hours) = capacity_rule(db, team_id, day).await?;
        let mut cap = TeamDayCapacity {
            team_id,
            day: day.to_string(),
            available_hours: 0.0,
            scheduled_hours: 0.0,
            absent_hours: 0.0,
            people: 0,
            people_scheduled: 0,
            people_absent: 0,
            basis: "no_members".to_string(),
            rule_available_hours,
            rule_overtime_hours,
            members: Vec::new(),
        };
        let mut holidays = 0;
        for (pid, percent) in allocation {
            let person = &facts[&pid];
            let d = person_day(person, day);
            if d.reason == "not_employed" {
                continue;
            }
            let share = percent / 100.0;
            cap.people += 1;
            if d.reason == "holiday" {
                holidays += 1;
            }
            if d.scheduled_minutes > 0 {
                cap.people_scheduled += 1;
                if d.available_minutes == 0 {
                    cap.people_absent += 1;
                }
            }
            let available = d.available_minutes as f64 / 60.0 * share;
            cap.available_hours += available;
            cap.scheduled_hours += d.scheduled_minutes as f64 / 60.0 * share;
            cap.absent_hours += d.absent_minutes as f64 / 60.0 * share;
            cap.members.push(TeamMemberDay {
                personnel_id: pid,
                full_name: person.full_name.clone(),
                allocation_percent: percent,
                available_hours: round2(available),
                reason: d.reason,
            });
        }
        cap.basis = if cap.people == 0 {
            "no_members"
        } else if cap.people_scheduled > 0 {
            "roster"
        } else if holidays > 0 {
            "holiday"
        } else {
            "no_schedule"
        }
        .to_string();
        cap.available_hours = round2(cap.available_hours);
        cap.scheduled_hours = round2(cap.scheduled_hours);
        cap.absent_hours = round2(cap.absent_hours);
        cap.members.sort_by(|a, b| a.full_name.cmp(&b.full_name));
        out.push(cap);
    }
    Ok(out)
}

/// Team capacity of one day — the planning interface replacing a fixed hours-per-day figure.
pub async fn team_capacity_hours(db: &impl ConnectionTrait, team_id: i64, day: NaiveDate) -> AppResult<TeamDayCapacity> {
    let mut days = team_capacity_range(db, team_id, day, day).await?;
    days.pop().ok_or_else(range_error)
}

#[cfg(test)]
mod tests {
    use super::shift_window;
    use chrono::NaiveDate;

    #[test]
    fn night_shift_runs_past_midnight() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let (s, e) = shift_window(day, "21:00", "05:00").unwrap();
        assert_eq!((e - s).num_minutes(), 8 * 60);
        let (s, e) = shift_window(day, "07:30", "16:00").unwrap();
        assert_eq!((e - s).num_minutes(), 8 * 60 + 30);
        assert!(shift_window(day, "7h", "16:00").is_none());
    }
}
