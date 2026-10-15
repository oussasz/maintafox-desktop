//! A team's committed load per day — what planning books against the capacity of
//! `personnel::capacity` (SAP PM capacity load per work centre and day; Maximo crew labour load).
//!
//! - A commitment books its labour hours, stored when it was committed (`planned_labor_hours`,
//!   migration 205): the work's estimated duration × its planned crew size (migration 206). A
//!   commitment from before migration 205 counts its committed slot as the planner set it.
//! - Those hours are spread over the days the slot covers, in proportion to the team's hours that
//!   day (shift time on working days; nothing on rest days and holidays) and to the share of the day
//!   the slot covers. A slot with no working day at all puts its hours on the days it covers, so the
//!   overload shows.
//! - Finished work no longer loads the team: a work order mechanically completed, closed or
//!   cancelled; a PM occurrence completed, cancelled or missed; a request converted, closed,
//!   declined or archived.
//! - Days are local calendar days, like the timetables and the rest of the application.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

/// A planned crew size, when given, is at least one person (no upper bound is assumed).
pub fn validate_crew_size(crew: Option<i64>) -> AppResult<()> {
    match crew {
        Some(n) if n < 1 => Err(AppError::ValidationFailed(vec![
            "planned_crew_size must be at least 1 person.".to_string(),
        ])),
        _ => Ok(()),
    }
}

fn parse_instant(raw: &str, field: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be RFC3339 (got '{raw}').")]))
}

fn local_midnight(day: NaiveDate) -> DateTime<Utc> {
    let naive = day.and_time(NaiveTime::MIN);
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.from_utc_datetime(&naive))
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

/// Spreads `labour_hours` over the slot's days, weighted by the team's hours each day (`available`)
/// and the share of the day the slot covers.
pub fn spread(
    labour_hours: f64,
    days: &[(NaiveDate, f64)],
    available: &BTreeMap<NaiveDate, f64>,
) -> BTreeMap<NaiveDate, f64> {
    let weights: Vec<(NaiveDate, f64)> = days
        .iter()
        .map(|(d, share)| (*d, share * available.get(d).copied().unwrap_or(0.0).max(0.0)))
        .collect();
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    let weights: Vec<(NaiveDate, f64)> = if total > 0.0 {
        weights
    } else {
        // No working hours in the slot: the hours land on the days it covers (the overload shows).
        days.to_vec()
    };
    let total: f64 = weights.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return BTreeMap::new();
    }
    weights
        .into_iter()
        .filter(|(_, w)| *w > 0.0)
        .map(|(d, w)| (d, labour_hours * w / total))
        .collect()
}

/// The team's available hours per day over `[from, to]` (timetables minus absences × utilisation).
pub async fn team_available(
    db: &impl ConnectionTrait,
    team_id: i64,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<BTreeMap<NaiveDate, f64>> {
    let mut out = BTreeMap::new();
    if to < from {
        return Ok(out);
    }
    for day in crate::personnel::capacity::team_capacity_span(db, team_id, from, to).await? {
        if let Ok(d) = NaiveDate::parse_from_str(&day.day, "%Y-%m-%d") {
            out.insert(d, day.available_hours);
        }
    }
    Ok(out)
}

/// SQL predicate: the commitment's work is still to do (alias `cmt`).
pub const ACTIVE_COMMITMENT_SQL: &str = "NOT ( \
    (cmt.source_type = 'work_order' AND EXISTS (SELECT 1 FROM work_orders fw WHERE fw.id = cmt.source_id \
        AND (fw.mechanically_completed_at IS NOT NULL OR fw.closed_at IS NOT NULL OR fw.cancelled_at IS NOT NULL))) \
    OR (cmt.source_type = 'pm_occurrence' AND EXISTS (SELECT 1 FROM pm_occurrences fp WHERE fp.id = cmt.source_id \
        AND fp.status IN ('completed', 'cancelled', 'missed'))) \
    OR (cmt.source_type = 'inspection_follow_up' AND EXISTS (SELECT 1 FROM intervention_requests fr \
        WHERE fr.id = cmt.source_id AND (fr.converted_to_wo_id IS NOT NULL OR fr.closed_at IS NOT NULL \
        OR fr.declined_at IS NOT NULL OR fr.archived_at IS NOT NULL))))";

struct LoadRow {
    team_id: i64,
    start: String,
    end: String,
    labour_hours: f64,
}

/// Committed labour hours per (team, day) over `[from, to]`, from the active commitments that touch
/// those days (`teams`: these teams only; `exclude`: a commitment being moved).
pub async fn team_day_loads(
    db: &impl ConnectionTrait,
    teams: Option<&[i64]>,
    from: NaiveDate,
    to: NaiveDate,
    exclude: Option<i64>,
) -> AppResult<BTreeMap<(i64, NaiveDate), f64>> {
    let mut out = BTreeMap::new();
    if to < from {
        return Ok(out);
    }
    let range_start = local_midnight(from).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let range_end = local_midnight(to + Duration::days(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let mut sql = format!(
        "SELECT cmt.assigned_team_id, cmt.committed_start, cmt.committed_end, \
                COALESCE(cmt.planned_labor_hours, \
                         (julianday(cmt.committed_end) - julianday(cmt.committed_start)) * 24.0) AS labour_hours \
           FROM schedule_commitments cmt \
          WHERE julianday(cmt.committed_start) < julianday(?) AND julianday(cmt.committed_end) > julianday(?) \
            AND (? IS NULL OR cmt.id <> ?) AND {ACTIVE_COMMITMENT_SQL}"
    );
    let mut values: Vec<sea_orm::Value> = vec![range_end.into(), range_start.into(), exclude.into(), exclude.into()];
    if let Some(teams) = teams {
        if teams.is_empty() {
            return Ok(out);
        }
        sql.push_str(&format!(
            " AND cmt.assigned_team_id IN ({})",
            vec!["?"; teams.len()].join(", ")
        ));
        values.extend(teams.iter().map(|t| sea_orm::Value::from(*t)));
    }
    let rows: Vec<LoadRow> = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
        .await?
        .iter()
        .map(|r| {
            Ok(LoadRow {
                team_id: r
                    .try_get("", "assigned_team_id")
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("load team: {e}")))?,
                start: r
                    .try_get("", "committed_start")
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("load start: {e}")))?,
                end: r
                    .try_get("", "committed_end")
                    .map_err(|e| AppError::Internal(anyhow::anyhow!("load end: {e}")))?,
                labour_hours: r.try_get::<f64>("", "labour_hours").unwrap_or(0.0).max(0.0),
            })
        })
        .collect::<AppResult<_>>()?;

    // Each team's hours over every day its commitments cover (they may reach outside [from, to]).
    let mut spans: BTreeMap<i64, (NaiveDate, NaiveDate)> = BTreeMap::new();
    let mut slots = Vec::with_capacity(rows.len());
    for row in rows {
        let days = slot_days(&row.start, &row.end)?;
        let (Some(first), Some(last)) = (days.first().map(|d| d.0), days.last().map(|d| d.0)) else {
            continue;
        };
        let span = spans.entry(row.team_id).or_insert((first, last));
        span.0 = span.0.min(first);
        span.1 = span.1.max(last);
        slots.push((row, days));
    }
    let mut available: BTreeMap<i64, BTreeMap<NaiveDate, f64>> = BTreeMap::new();
    for (team_id, (first, last)) in spans {
        available.insert(team_id, team_available(db, team_id, first, last).await?);
    }
    for (row, days) in slots {
        let team_hours = available.get(&row.team_id).cloned().unwrap_or_default();
        for (day, hours) in spread(row.labour_hours, &days, &team_hours) {
            if day >= from && day <= to {
                *out.entry((row.team_id, day)).or_insert(0.0) += hours;
            }
        }
    }
    Ok(out)
}
