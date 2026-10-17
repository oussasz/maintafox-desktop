//! A technician's committed hours per day and what remains (SAP PM individual capacity; Maximo
//! labour availability): what the assignee picker shows and the commit gate checks.
//!
//! - A person's day comes from their own timetable (`personnel::capacity::person_capacity_span`):
//!   their shift window, the hours available for work orders (absences and utilisation applied)
//!   and, when they do not work, why. For a crew, the hours are taken at the person's allocation
//!   to it, like the crew's capacity.
//! - Their committed hours are all the work they carry:
//!   - planning commitments assigned to them: the job's duration (`planned_person_hours`,
//!     migration 210), or for an older commitment the crew's labour ÷ crew size; when neither is
//!     known the hours are unknown — reported, never replaced by the slot's length;
//!   - OTs they are responsible for outside planning (planned dates set, open, not covered by an
//!     active commitment that names a technician): the OT's expected duration or, without one,
//!     their net working time in its slot. Typed local dates are read like the shift code reads
//!     them; an OT whose dates cannot be read is skipped and reported, never blocking the gate.
//! - Hours are spread over the person's working time like a team's (`planning::load::spread`); a
//!   night shift belongs to the day it starts.
//! - Only the named technician carries per-person hours: the job's other crew members are counted
//!   in the crew's load (duration × crew size), not named here.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Duration, NaiveDate};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult, AppValidationIssue};
use crate::planning::load::{self, DayHours, WorkWindow, ACTIVE_COMMITMENT_SQL};

/// One person's day as planning reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonDay {
    /// Hours available for work orders (all their crews together).
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

fn parse_day(raw: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("capacity day '{raw}': {e}")))
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
        let day = parse_day(&cap.day)?;
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

/// An OT whose planned dates cannot be read: skipped, shown so it can be corrected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnreadableWorkOrder {
    pub work_order_id: i64,
    pub code: String,
}

/// What a set of people carry over a range of days.
#[derive(Debug, Clone, Default)]
pub struct PersonLoads {
    /// Hours per (person, day), all their crews together.
    pub total: BTreeMap<(i64, NaiveDate), f64>,
    /// Hours per (person, crew, day).
    pub by_team: BTreeMap<(i64, i64, NaiveDate), f64>,
    /// Commitments assigned to the person whose hours per person are unknown: not counted.
    pub unknown_hours: BTreeMap<i64, usize>,
    /// OTs of the person whose planned dates cannot be read: skipped.
    pub unreadable: BTreeMap<i64, Vec<UnreadableWorkOrder>>,
}

struct PersonSlot {
    personnel_id: i64,
    team_id: Option<i64>,
    start: String,
    end: String,
    /// `None`: an OT without a duration (its net working time in the slot).
    hours: Option<f64>,
}

/// What these people carry over `[from, to]` (`exclude_commitment`: a commitment being moved;
/// `exclude_work_order`: the OT being committed).
pub async fn person_loads(
    db: &impl ConnectionTrait,
    people: &[i64],
    from: NaiveDate,
    to: NaiveDate,
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<PersonLoads> {
    let mut out = PersonLoads::default();
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
                "SELECT cmt.assigned_personnel_id AS personnel_id, cmt.assigned_team_id AS team_id, \
                        cmt.committed_start AS slot_start, cmt.committed_end AS slot_end, \
                        COALESCE(cmt.planned_person_hours, \
                                 CASE WHEN cmt.planned_labor_hours IS NOT NULL AND cand.planned_crew_size >= 1 \
                                      THEN cmt.planned_labor_hours / cand.planned_crew_size END) AS hours \
                   FROM schedule_commitments cmt \
                   LEFT JOIN schedule_candidates cand ON cand.id = cmt.schedule_candidate_id \
                  WHERE julianday(cmt.committed_start) < julianday(?) AND julianday(cmt.committed_end) > julianday(?) \
                    AND cmt.assigned_personnel_id IN ({marks}) \
                    AND (? IS NULL OR cmt.id <> ?) AND {ACTIVE_COMMITMENT_SQL}"
            ),
            values,
        ))
        .await?
    {
        let personnel_id: i64 = row
            .try_get("", "personnel_id")
            .map_err(|e| decode_err("personnel_id", e))?;
        let hours: Option<f64> = row.try_get("", "hours").map_err(|e| decode_err("hours", e))?;
        let Some(hours) = hours else {
            *out.unknown_hours.entry(personnel_id).or_insert(0) += 1;
            continue;
        };
        slots.push(PersonSlot {
            personnel_id,
            team_id: row.try_get("", "team_id").map_err(|e| decode_err("team_id", e))?,
            start: row.try_get("", "slot_start").map_err(|e| decode_err("slot_start", e))?,
            end: row.try_get("", "slot_end").map_err(|e| decode_err("slot_end", e))?,
            hours: Some(hours.max(0.0)),
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
                "SELECT wo.id, wo.code, ua.personnel_id AS personnel_id, wo.assigned_team_id AS team_id, \
                        wo.planned_start AS slot_start, wo.planned_end AS slot_end, \
                        wo.expected_duration_hours AS hours \
                   FROM work_orders wo \
                   JOIN user_accounts ua ON ua.id = wo.primary_responsible_id \
                  WHERE {in_range} AND ua.personnel_id IN ({marks}) \
                    AND (? IS NULL OR wo.id <> ?) AND {open} AND {not_committed}",
                in_range = load::WO_PLANNED_IN_RANGE_SQL,
                open = load::WO_OPEN_SQL,
                not_committed = load::wo_not_committed_sql(true),
            ),
            values,
        ))
        .await?
    {
        let personnel_id: i64 = row
            .try_get("", "personnel_id")
            .map_err(|e| decode_err("personnel_id", e))?;
        let raw_start: String = row.try_get("", "slot_start").map_err(|e| decode_err("slot_start", e))?;
        let raw_end: String = row.try_get("", "slot_end").map_err(|e| decode_err("slot_end", e))?;
        let dates = (
            crate::wo::shifts::planned_rfc3339(&raw_start),
            crate::wo::shifts::planned_rfc3339(&raw_end),
        );
        let (Some(start), Some(end)) = dates else {
            out.unreadable
                .entry(personnel_id)
                .or_default()
                .push(UnreadableWorkOrder {
                    work_order_id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
                    code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
                });
            continue;
        };
        if end <= start {
            out.unreadable
                .entry(personnel_id)
                .or_default()
                .push(UnreadableWorkOrder {
                    work_order_id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
                    code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
                });
            continue;
        }
        slots.push(PersonSlot {
            personnel_id,
            team_id: row.try_get("", "team_id").map_err(|e| decode_err("team_id", e))?,
            start,
            end,
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
            None => load::net_hours_in(&slot.start, &slot.end, person_hours)?,
        };
        for (day, share) in load::spread(booked, &slot.start, &slot.end, person_hours)? {
            if day >= from && day <= to {
                *out.total.entry((slot.personnel_id, day)).or_insert(0.0) += share;
                if let Some(team_id) = slot.team_id {
                    *out.by_team.entry((slot.personnel_id, team_id, day)).or_insert(0.0) += share;
                }
            }
        }
    }
    Ok(out)
}

/// Committed hours per (person, day) over `[from, to]` (see [`person_loads`]).
pub async fn person_day_loads(
    db: &impl ConnectionTrait,
    people: &[i64],
    from: NaiveDate,
    to: NaiveDate,
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<BTreeMap<(i64, NaiveDate), f64>> {
    Ok(
        person_loads(db, people, from, to, exclude_commitment, exclude_work_order)
            .await?
            .total,
    )
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

/// Everything the assignee checks need for one slot, loaded once for the whole crew.
struct SlotContext {
    team_id: i64,
    calendar: Vec<(NaiveDate, f64)>,
    /// Crew members and their allocation (%) on each day (from the day before the slot).
    members: BTreeMap<NaiveDate, BTreeMap<i64, f64>>,
    /// Each person's days, from the day before the slot (a night shift that started then).
    people: BTreeMap<i64, BTreeMap<NaiveDate, PersonDay>>,
    loads: PersonLoads,
}

async fn slot_context(
    db: &impl ConnectionTrait,
    team_id: i64,
    start: &str,
    end: &str,
    extra: &[i64],
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<Option<SlotContext>> {
    let calendar = load::slot_days(start, end)?;
    let (Some(first), Some(last)) = (calendar.first().map(|d| d.0), calendar.last().map(|d| d.0)) else {
        return Ok(None);
    };
    let from = first - Duration::days(1);
    let mut members: BTreeMap<NaiveDate, BTreeMap<i64, f64>> = BTreeMap::new();
    for cap in crate::personnel::capacity::team_capacity_span(db, team_id, from, last).await? {
        members.insert(
            parse_day(&cap.day)?,
            cap.members
                .iter()
                .map(|m| (m.personnel_id, m.allocation_percent))
                .collect(),
        );
    }
    let ids: Vec<i64> = members
        .values()
        .flat_map(|m| m.keys().copied())
        .chain(extra.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut people = BTreeMap::new();
    for pid in &ids {
        people.insert(*pid, person_days(db, *pid, from, last).await?);
    }
    let loads = person_loads(db, &ids, from, last, exclude_commitment, exclude_work_order).await?;
    Ok(Some(SlotContext {
        team_id,
        calendar,
        members,
        people,
        loads,
    }))
}

/// One day of an assignee for the picker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssigneeDay {
    pub day: String,
    pub shift_start: Option<String>,
    pub shift_end: Option<String>,
    /// Hours available to this crew (the person's hours × their allocation to it).
    pub available_hours: f64,
    pub committed_hours: f64,
    pub remaining_hours: f64,
    /// Why the person does (not) work that day (see [`PersonDay::reason`]).
    pub reason: String,
}

struct Evaluation {
    issues: Vec<AppValidationIssue>,
    days: Vec<AssigneeDay>,
    slot_working_hours: f64,
}

impl SlotContext {
    /// Hours the person can still give this crew on `day`: their share for the crew less what they
    /// carry for it, and never more than their whole day less everything they carry.
    fn remaining(&self, pid: i64, day: NaiveDate) -> (f64, f64) {
        let available = self
            .people
            .get(&pid)
            .and_then(|d| d.get(&day))
            .map(|p| p.available)
            .unwrap_or(0.0);
        let allocation = self
            .members
            .get(&day)
            .and_then(|m| m.get(&pid))
            .copied()
            .unwrap_or(100.0)
            / 100.0;
        let for_crew = available * allocation;
        let used_crew = self
            .loads
            .by_team
            .get(&(pid, self.team_id, day))
            .copied()
            .unwrap_or(0.0);
        let used_all = self.loads.total.get(&(pid, day)).copied().unwrap_or(0.0);
        (for_crew, (for_crew - used_crew).min(available - used_all).max(0.0))
    }

    fn evaluate(&self, pid: i64, start: &str, end: &str, person_hours: Option<f64>) -> AppResult<Evaluation> {
        let empty = BTreeMap::new();
        let days = self.people.get(&pid).unwrap_or(&empty);
        let hours = hours_of(days);
        let mut issues = Vec::new();

        // The days shown: the slot's, and the day before when its night shift runs into the slot.
        let Some(first) = self.calendar.first().map(|d| d.0) else {
            return Ok(Evaluation {
                issues,
                days: Vec::new(),
                slot_working_hours: 0.0,
            });
        };
        let previous = first - Duration::days(1);
        let mut shown: Vec<NaiveDate> = Vec::new();
        if let Some(prev) = hours.get(&previous) {
            let only_prev = BTreeMap::from([(
                previous,
                DayHours {
                    windows: prev.windows.clone(),
                    unwindowed: 0.0,
                },
            )]);
            if load::working_hours_in(start, end, &only_prev)? > 0.0 {
                shown.push(previous);
            }
        }
        shown.extend(self.calendar.iter().map(|(d, _)| *d));
        let rows = shown
            .iter()
            .map(|d| {
                let p = days.get(d);
                let (available, remaining) = self.remaining(pid, *d);
                AssigneeDay {
                    day: d.to_string(),
                    shift_start: p.and_then(|p| p.shift_start.clone()),
                    shift_end: p.and_then(|p| p.shift_end.clone()),
                    available_hours: round2(available),
                    committed_hours: round2((available - remaining).max(0.0)),
                    remaining_hours: round2(remaining),
                    reason: p.map(|p| p.reason.clone()).unwrap_or_else(|| "no_schedule".to_string()),
                }
            })
            .collect();

        // A member of the crew on every day of the slot.
        if let Some((day, _)) = self
            .calendar
            .iter()
            .find(|(d, _)| !self.members.get(d).is_some_and(|m| m.contains_key(&pid)))
        {
            issues.push(AppValidationIssue::error(
                "PLANNING_ASSIGNEE_NOT_IN_CREW",
                format!("The technician is not a member of the crew on {day}."),
                HashMap::from([("day".to_string(), day.to_string())]),
            ));
        }

        // Working during the slot.
        let slot_working_hours = load::working_hours_in(start, end, &hours)?;
        if slot_working_hours <= 0.0 {
            let (day, reason) = self
                .calendar
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
            return Ok(Evaluation {
                issues,
                days: rows,
                slot_working_hours: 0.0,
            });
        }

        // Enough hours left each day for the job's duration (unknown hours: not checked).
        if let Some(person_hours) = person_hours {
            for (day, share) in load::spread(person_hours, start, end, &hours)? {
                let (_, remaining) = self.remaining(pid, day);
                if share > remaining + 0.01 {
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
        Ok(Evaluation {
            issues,
            days: rows,
            slot_working_hours,
        })
    }
}

/// What the commit gate checks about an assignee (each overridable with a reason, logged): a member
/// of the crew on every day of the slot, working during the slot, and — when the job's hours per
/// person are known — enough hours left each day.
#[allow(clippy::too_many_arguments)]
pub async fn assignee_issues(
    db: &impl ConnectionTrait,
    team_id: i64,
    personnel_id: i64,
    start: &str,
    end: &str,
    person_hours: Option<f64>,
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<Vec<AppValidationIssue>> {
    let Some(ctx) = slot_context(
        db,
        team_id,
        start,
        end,
        &[personnel_id],
        exclude_commitment,
        exclude_work_order,
    )
    .await?
    else {
        return Ok(Vec::new());
    };
    Ok(ctx.evaluate(personnel_id, start, end, person_hours)?.issues)
}

/// A crew member as the assignee picker shows them for a slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssigneeCapacity {
    pub personnel_id: i64,
    pub full_name: String,
    pub employee_code: Option<String>,
    /// Each day of the slot (and the day before when its night shift runs into it).
    pub days: Vec<AssigneeDay>,
    /// Working time of the person inside the slot (hours).
    pub slot_working_hours: f64,
    /// Codes of what the commit gate would refuse for this job (`PLANNING_ASSIGNEE_*`).
    pub issues: Vec<String>,
    /// Commitments of the person whose hours are unknown (not counted in the figures).
    pub unknown_hours_jobs: usize,
    /// OTs of the person whose planned dates cannot be read (skipped).
    pub unreadable_work_orders: Vec<UnreadableWorkOrder>,
}

/// Every member of the crew over the slot, with each day's shift and hours left and — for a job of
/// `person_hours` per person — what the commit gate would refuse. Computed once for the crew.
pub async fn crew_assignee_capacity(
    db: &impl ConnectionTrait,
    team_id: i64,
    start: &str,
    end: &str,
    person_hours: Option<f64>,
    exclude_commitment: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<Vec<AssigneeCapacity>> {
    let Some(ctx) = slot_context(db, team_id, start, end, &[], exclude_commitment, exclude_work_order).await? else {
        return Ok(Vec::new());
    };
    // The crew over the slot's days.
    let people: Vec<i64> = ctx
        .calendar
        .iter()
        .filter_map(|(d, _)| ctx.members.get(d))
        .flat_map(|m| m.keys().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if people.is_empty() {
        return Ok(Vec::new());
    }
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
        let evaluation = ctx.evaluate(pid, start, end, person_hours)?;
        let (full_name, employee_code) = names
            .get(&pid)
            .cloned()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("crew member {pid} missing from personnel")))?;
        out.push(AssigneeCapacity {
            personnel_id: pid,
            full_name,
            employee_code,
            days: evaluation.days,
            slot_working_hours: round2(evaluation.slot_working_hours),
            issues: evaluation.issues.into_iter().map(|i| i.code).collect(),
            unknown_hours_jobs: ctx.loads.unknown_hours.get(&pid).copied().unwrap_or(0),
            unreadable_work_orders: ctx.loads.unreadable.get(&pid).cloned().unwrap_or_default(),
        });
    }
    out.sort_by(|a, b| a.full_name.cmp(&b.full_name));
    Ok(out)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
