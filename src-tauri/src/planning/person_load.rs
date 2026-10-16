//! A technician's committed hours per day and what remains (SAP PM individual capacity; Maximo
//! labour availability): what the assignee picker shows and the commit gate checks.
//!
//! - A person's day comes from their own timetable (`personnel::capacity::person_capacity_span`):
//!   their shift window, the hours available for work orders (absences and utilisation applied)
//!   and, when they do not work, why.
//! - Their committed hours are all the work they carry: planning commitments assigned to them (the
//!   job's duration, `planned_person_hours`, migration 210; the committed slot for an older one)
//!   and work orders they are responsible for outside planning (planned dates set, not finished,
//!   not committed by planning): the work order's expected duration or, without one, the working
//!   time of its slot (they are booked for it).
//! - Those hours are spread over the person's working time exactly like a team's
//!   (`planning::load::spread`); a night shift belongs to the day it starts.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Duration, NaiveDate};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult, AppValidationIssue};
use crate::planning::load::{self, DayHours, WorkWindow, ACTIVE_COMMITMENT_SQL};

/// One person's day as planning reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonDay {
    /// Hours available for work orders.
    pub available: f64,
    /// `working` | `absent` | `rest_day` | `site_rest_day` | `holiday` | `no_schedule` |
    /// `timetable_not_set_up` | `not_employed`.
    pub reason: String,
    pub shift_start: Option<String>,
    pub shift_end: Option<String>,
    pub hours: DayHours,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("person load decode '{field}': {err}"))
}

/// A person's days over `[from, to]` from their timetable.
pub async fn person_days(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<BTreeMap<NaiveDate, PersonDay>> {
    let mut out = BTreeMap::new();
    if to < from {
        return Ok(out);
    }
    for cap in crate::personnel::capacity::person_capacity_span(db, personnel_id, from, to).await? {
        let day = NaiveDate::parse_from_str(&cap.day, "%Y-%m-%d")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("capacity day '{}': {e}", cap.day)))?;
        let available = cap.plannable_minutes as f64 / 60.0;
        let mut hours = DayHours::default();
        if available > 0.0 {
            let window = match (cap.shift_start.as_deref(), cap.shift_end.as_deref()) {
                (Some(s), Some(e)) => crate::personnel::capacity::shift_window(day, s, e),
                _ => None,
            };
            match window {
                Some((start, end)) => hours.windows.push(WorkWindow {
                    start,
                    end,
                    hours: available,
                }),
                None => hours.unwindowed = available,
            }
        }
        out.insert(
            day,
            PersonDay {
                available,
                reason: cap.reason,
                shift_start: cap.shift_start,
                shift_end: cap.shift_end,
                hours,
            },
        );
    }
    Ok(out)
}

fn hours_of(days: &BTreeMap<NaiveDate, PersonDay>) -> BTreeMap<NaiveDate, DayHours> {
    days.iter().map(|(d, p)| (*d, p.hours.clone())).collect()
}

/// SQL predicate: no planning commitment covers the work order (alias `wo`) — through itself, the
/// PM occurrence it was generated for, or the request it was converted from.
const NOT_COMMITTED_WO_SQL: &str = "NOT EXISTS (SELECT 1 FROM schedule_commitments c2 WHERE \
    (c2.source_type = 'work_order' AND c2.source_id = wo.id) \
    OR (c2.source_type = 'pm_occurrence' AND c2.source_id IN \
        (SELECT po.id FROM pm_occurrences po WHERE po.linked_work_order_id = wo.id)) \
    OR (c2.source_type = 'inspection_follow_up' AND c2.source_id IN \
        (SELECT ir.id FROM intervention_requests ir WHERE ir.converted_to_wo_id = wo.id)))";

struct PersonSlot {
    personnel_id: i64,
    start: String,
    end: String,
    /// `None`: a work order without an expected duration (its slot's working time).
    hours: Option<f64>,
}

/// Committed hours per (person, day) over `[from, to]` for these people: planning commitments
/// assigned to them and work orders they carry outside planning (`exclude_commitment`: a commitment
/// being moved; `exclude_work_order`: the work order being committed).
pub async fn person_day_loads(
    db: &impl ConnectionTrait,
    people: &[i64],
    from: NaiveDate,
    to: NaiveDate,
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<BTreeMap<(i64, NaiveDate), f64>> {
    let mut out = BTreeMap::new();
    if people.is_empty() || to < from {
        return Ok(out);
    }
    let (range_start, range_end) = load::load_query_bounds(from, to);
    let marks = vec!["?"; people.len()].join(", ");
    let ids: Vec<sea_orm::Value> = people.iter().map(|p| sea_orm::Value::from(*p)).collect();

    let mut slots = Vec::new();
    let mut values: Vec<sea_orm::Value> = vec![range_end.clone().into(), range_start.clone().into()];
    values.extend(ids.iter().cloned());
    values.push(exclude_commitment.into());
    values.push(exclude_commitment.into());
    for row in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT cmt.assigned_personnel_id AS personnel_id, cmt.committed_start AS slot_start, \
                        cmt.committed_end AS slot_end, \
                        COALESCE(cmt.planned_person_hours, \
                                 (julianday(cmt.committed_end) - julianday(cmt.committed_start)) * 24.0) AS hours \
                   FROM schedule_commitments cmt \
                  WHERE julianday(cmt.committed_start) < julianday(?) AND julianday(cmt.committed_end) > julianday(?) \
                    AND cmt.assigned_personnel_id IN ({marks}) \
                    AND (? IS NULL OR cmt.id <> ?) AND {ACTIVE_COMMITMENT_SQL}"
            ),
            values,
        ))
        .await?
    {
        slots.push(PersonSlot {
            personnel_id: row
                .try_get("", "personnel_id")
                .map_err(|e| decode_err("personnel_id", e))?,
            start: row.try_get("", "slot_start").map_err(|e| decode_err("slot_start", e))?,
            end: row.try_get("", "slot_end").map_err(|e| decode_err("slot_end", e))?,
            hours: Some(
                row.try_get::<f64>("", "hours")
                    .map_err(|e| decode_err("hours", e))?
                    .max(0.0),
            ),
        });
    }

    let mut values: Vec<sea_orm::Value> = vec![range_end.into(), range_start.into()];
    values.extend(ids);
    values.push(exclude_work_order.into());
    values.push(exclude_work_order.into());
    for row in db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ua.personnel_id AS personnel_id, wo.planned_start AS slot_start, \
                        wo.planned_end AS slot_end, wo.expected_duration_hours AS hours \
                   FROM work_orders wo \
                   JOIN user_accounts ua ON ua.id = wo.primary_responsible_id \
                  WHERE wo.planned_start IS NOT NULL AND wo.planned_end IS NOT NULL \
                    AND julianday(wo.planned_start) < julianday(?) AND julianday(wo.planned_end) > julianday(?) \
                    AND ua.personnel_id IN ({marks}) \
                    AND (? IS NULL OR wo.id <> ?) \
                    AND wo.mechanically_completed_at IS NULL AND wo.closed_at IS NULL AND wo.cancelled_at IS NULL \
                    AND {NOT_COMMITTED_WO_SQL}"
            ),
            values,
        ))
        .await?
    {
        slots.push(PersonSlot {
            personnel_id: row
                .try_get("", "personnel_id")
                .map_err(|e| decode_err("personnel_id", e))?,
            start: row.try_get("", "slot_start").map_err(|e| decode_err("slot_start", e))?,
            end: row.try_get("", "slot_end").map_err(|e| decode_err("slot_end", e))?,
            hours: row
                .try_get::<Option<f64>>("", "hours")
                .map_err(|e| decode_err("hours", e))?
                .filter(|h| *h > 0.0),
        });
    }

    // Each person's working time over every day their slots can use.
    let mut spans: BTreeMap<i64, (NaiveDate, NaiveDate)> = BTreeMap::new();
    for slot in &slots {
        let Some((first, last)) = load::capacity_span(&slot.start, &slot.end)? else {
            continue;
        };
        let span = spans.entry(slot.personnel_id).or_insert((first, last));
        span.0 = span.0.min(first);
        span.1 = span.1.max(last);
    }
    let mut hours: BTreeMap<i64, BTreeMap<NaiveDate, DayHours>> = BTreeMap::new();
    for (pid, (first, last)) in spans {
        hours.insert(pid, hours_of(&person_days(db, pid, first, last).await?));
    }
    let empty = BTreeMap::new();
    for slot in slots {
        let person_hours = hours.get(&slot.personnel_id).unwrap_or(&empty);
        let booked = match slot.hours {
            Some(h) => h,
            None => load::working_hours_in(&slot.start, &slot.end, person_hours)?,
        };
        for (day, share) in load::spread(booked, &slot.start, &slot.end, person_hours)? {
            if day >= from && day <= to {
                *out.entry((slot.personnel_id, day)).or_insert(0.0) += share;
            }
        }
    }
    Ok(out)
}

/// The work order behind a planning candidate's source, if any (it must not count twice).
pub async fn source_work_order(db: &impl ConnectionTrait, source_type: &str, source_id: i64) -> AppResult<Option<i64>> {
    let sql = match source_type {
        "work_order" => return Ok(Some(source_id)),
        "pm_occurrence" => "SELECT linked_work_order_id AS wo FROM pm_occurrences WHERE id = ?",
        "inspection_follow_up" => "SELECT converted_to_wo_id AS wo FROM intervention_requests WHERE id = ?",
        _ => return Ok(None),
    };
    match db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [source_id.into()],
        ))
        .await?
    {
        Some(row) => row.try_get::<Option<i64>>("", "wo").map_err(|e| decode_err("wo", e)),
        None => Ok(None),
    }
}

/// The crew's members on each day of `[from, to]`.
async fn crew_members_by_day(
    db: &impl ConnectionTrait,
    team_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<BTreeMap<NaiveDate, BTreeSet<i64>>> {
    let mut out = BTreeMap::new();
    for cap in crate::personnel::capacity::team_capacity_span(db, team_id, from, to).await? {
        let day = NaiveDate::parse_from_str(&cap.day, "%Y-%m-%d")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("capacity day '{}': {e}", cap.day)))?;
        out.insert(day, cap.members.iter().map(|m| m.personnel_id).collect());
    }
    Ok(out)
}

/// What the commit gate checks about an assignee (each overridable with a reason, logged): a member
/// of the crew on every day of the slot, working during the slot, and enough hours left each day
/// for the job's duration (`person_hours`, the hours each person works).
#[allow(clippy::too_many_arguments)]
pub async fn assignee_issues(
    db: &impl ConnectionTrait,
    team_id: i64,
    personnel_id: i64,
    start: &str,
    end: &str,
    person_hours: f64,
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<Vec<AppValidationIssue>> {
    let mut issues = Vec::new();
    let calendar = load::slot_days(start, end)?;
    let (Some(first), Some(last)) = (calendar.first().map(|d| d.0), calendar.last().map(|d| d.0)) else {
        return Ok(issues);
    };

    let members = crew_members_by_day(db, team_id, first, last).await?;
    if let Some((day, _)) = calendar
        .iter()
        .find(|(d, _)| !members.get(d).is_some_and(|m| m.contains(&personnel_id)))
    {
        issues.push(AppValidationIssue::error(
            "PLANNING_ASSIGNEE_NOT_IN_CREW",
            format!("The technician is not a member of the crew on {day}."),
            HashMap::from([("day".to_string(), day.to_string())]),
        ));
    }

    let days = person_days(db, personnel_id, first - Duration::days(1), last).await?;
    let hours = hours_of(&days);
    if load::working_hours_in(start, end, &hours)? <= 0.0 {
        // The first day of the slot the person does not work, or "outside their shift".
        let (day, reason) = calendar
            .iter()
            .find_map(|(d, _)| {
                days.get(d)
                    .filter(|p| p.reason != "working")
                    .map(|p| (*d, p.reason.clone()))
            })
            .unwrap_or((first, "outside_shift".to_string()));
        issues.push(AppValidationIssue::error(
            "PLANNING_ASSIGNEE_NOT_WORKING",
            format!("The technician does not work during this slot ({reason} on {day})."),
            HashMap::from([("day".to_string(), day.to_string()), ("reason".to_string(), reason)]),
        ));
        return Ok(issues);
    }

    let shares = load::spread(person_hours, start, end, &hours)?;
    if let (Some(from), Some(to)) = (shares.keys().next().copied(), shares.keys().last().copied()) {
        let committed = person_day_loads(db, &[personnel_id], from, to, exclude_commitment, exclude_work_order).await?;
        for (day, share) in shares {
            let available = days.get(&day).map(|p| p.available).unwrap_or(0.0);
            let already = committed.get(&(personnel_id, day)).copied().unwrap_or(0.0);
            if already + share > available + 0.01 {
                let remaining = (available - already).max(0.0);
                issues.push(AppValidationIssue::error(
                    "PLANNING_ASSIGNEE_OVERLOADED",
                    format!("The technician has {remaining:.1} h left on {day}; this job needs {share:.1} h."),
                    HashMap::from([
                        ("day".to_string(), day.to_string()),
                        ("share".to_string(), format!("{share:.1}")),
                        ("remaining".to_string(), format!("{remaining:.1}")),
                    ]),
                ));
                break;
            }
        }
    }
    Ok(issues)
}

/// One day of an assignee for the picker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssigneeDay {
    pub day: String,
    pub shift_start: Option<String>,
    pub shift_end: Option<String>,
    pub available_hours: f64,
    pub committed_hours: f64,
    pub remaining_hours: f64,
    /// Why the person does (not) work that day (see [`PersonDay::reason`]).
    pub reason: String,
}

/// A crew member as the assignee picker shows them for a slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssigneeCapacity {
    pub personnel_id: i64,
    pub full_name: String,
    pub employee_code: Option<String>,
    pub days: Vec<AssigneeDay>,
    /// Working time of the person inside the slot (hours).
    pub slot_working_hours: f64,
    /// Codes of what the commit gate would refuse for this job (`PLANNING_ASSIGNEE_*`); empty when
    /// no job duration is given.
    pub issues: Vec<String>,
}

/// Every member of the crew over the slot, with each day's shift and hours left and — for a job of
/// `person_hours` per person — what the commit gate would refuse.
pub async fn crew_assignee_capacity(
    db: &impl ConnectionTrait,
    team_id: i64,
    start: &str,
    end: &str,
    person_hours: Option<f64>,
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<Vec<AssigneeCapacity>> {
    let calendar = load::slot_days(start, end)?;
    let (Some(first), Some(last)) = (calendar.first().map(|d| d.0), calendar.last().map(|d| d.0)) else {
        return Ok(Vec::new());
    };
    let people: Vec<i64> = crew_members_by_day(db, team_id, first, last)
        .await?
        .into_values()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if people.is_empty() {
        return Ok(Vec::new());
    }
    let committed = person_day_loads(db, &people, first, last, exclude_commitment, exclude_work_order).await?;
    let marks = vec!["?"; people.len()].join(", ");
    let names: HashMap<i64, (String, Option<String>)> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT id, full_name, employee_code FROM personnel WHERE id IN ({marks})"),
            people.iter().map(|p| sea_orm::Value::from(*p)).collect::<Vec<_>>(),
        ))
        .await?
        .iter()
        .map(|r| {
            Ok((
                r.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
                (
                    r.try_get::<String>("", "full_name")
                        .map_err(|e| decode_err("full_name", e))?,
                    r.try_get::<Option<String>>("", "employee_code")
                        .map_err(|e| decode_err("employee_code", e))?,
                ),
            ))
        })
        .collect::<AppResult<_>>()?;

    let mut out = Vec::new();
    for pid in people {
        let days = person_days(db, pid, first - Duration::days(1), last).await?;
        let slot_working_hours = load::working_hours_in(start, end, &hours_of(&days))?;
        let rows = calendar
            .iter()
            .map(|(d, _)| {
                let p = days.get(d);
                let available = p.map(|p| p.available).unwrap_or(0.0);
                let used = committed.get(&(pid, *d)).copied().unwrap_or(0.0);
                AssigneeDay {
                    day: d.to_string(),
                    shift_start: p.and_then(|p| p.shift_start.clone()),
                    shift_end: p.and_then(|p| p.shift_end.clone()),
                    available_hours: round2(available),
                    committed_hours: round2(used),
                    remaining_hours: round2((available - used).max(0.0)),
                    reason: p.map(|p| p.reason.clone()).unwrap_or_else(|| "no_schedule".to_string()),
                }
            })
            .collect();
        let issues = match person_hours {
            Some(h) => assignee_issues(db, team_id, pid, start, end, h, exclude_commitment, exclude_work_order)
                .await?
                .into_iter()
                .map(|i| i.code)
                .collect(),
            None => Vec::new(),
        };
        let (full_name, employee_code) = names
            .get(&pid)
            .cloned()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("crew member {pid} missing from personnel")))?;
        out.push(AssigneeCapacity {
            personnel_id: pid,
            full_name,
            employee_code,
            days: rows,
            slot_working_hours: round2(slot_working_hours),
            issues,
        });
    }
    out.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    Ok(out)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
