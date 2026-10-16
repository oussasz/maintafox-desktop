//! Planning KPIs measured against the frozen weekly baseline (planning step E).
//!
//! Basis: for each planning site and week, the LATEST baseline of a frozen or published week
//! (`weekly_schedule_baseline_items` of the schedule's current `baseline_no`). A week that was never
//! frozen has no baseline: it is not measured and is counted apart ("not frozen") once its freeze
//! fell due — never guessed from live commitments. A week frozen without approval (and not
//! approved since) is measured, flagged, counted, and can be left out.
//!
//! - **Schedule compliance (SMRP 5.4.2, hours)**: baseline hours whose work was finished by the end
//!   of the baseline week ÷ baseline hours. A job's hours are its planned labour hours (else its slot
//!   length). Finished: the work order behind it (the job's own, the PM occurrence's, or the one a
//!   request became) mechanically completed, ended or closed, and not cancelled. A baseline job later
//!   released or cancelled counts as not done: it was scheduled. Judged once the week is over.
//! - **Break-in %**: hours of work that entered the frozen week as break-ins of kind `insert` ÷ the
//!   week's baseline hours. Moved jobs (break-ins of kind `move` touching the week after its freeze)
//!   are counted apart as displaced jobs, not in the %.
//! - **Loading %**: baseline hours ÷ the crews' available hours over the week (members' timetables,
//!   site calendar, absences), WITHOUT the overtime ceiling; the overtime capacity is given beside
//!   it, so above 100 % the plan relies on overtime.

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

use super::weekly::site_week;
use super::weekly_schedule::{
    crews_with_sites, freeze_due_at, local_midnight_utc, map_schedule, rfc3339, site_crews, WeeklySchedule,
    SCHEDULE_COLS, STATUS_FROZEN, STATUS_PUBLISHED,
};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Internal(anyhow::anyhow!("planning KPI decode '{field}': {err}"))
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn pct(numerator: f64, denominator: f64) -> Option<f64> {
    (denominator > 0.0).then(|| round1(numerator / denominator * 100.0))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningKpiFilter {
    /// One planning site; `None`: every site.
    pub site_id: Option<i64>,
    /// Weeks whose first day falls in `[from_day, to_day]` (`YYYY-MM-DD`).
    pub from_day: String,
    pub to_day: String,
    /// Leave out the weeks frozen without approval and not approved since.
    #[serde(default)]
    pub exclude_unapproved: bool,
}

/// One frozen (or published) week measured against its baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeekKpi {
    pub weekly_schedule_id: i64,
    pub site_id: i64,
    pub site_name: String,
    pub week_start: String,
    pub week_end: String,
    pub status: String,
    /// Frozen without approval and not approved since.
    pub awaiting_approval: bool,
    pub baseline_no: i64,
    pub baseline_jobs: i64,
    pub baseline_hours: f64,
    /// `None` while the week is not over.
    pub done_hours: Option<f64>,
    pub compliance_pct: Option<f64>,
    pub break_in_jobs: i64,
    pub break_in_hours: f64,
    pub break_in_pct: Option<f64>,
    pub displaced_jobs: i64,
    pub available_hours: f64,
    pub overtime_hours: f64,
    pub loading_pct: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanningKpiTotals {
    /// Weeks measured (frozen or published, with a baseline).
    pub weeks: i64,
    /// Of them, weeks over (compliance judged).
    pub weeks_judged: i64,
    pub weeks_awaiting_approval: i64,
    /// Weeks whose freeze fell due and that were never frozen: no baseline, not measured.
    pub weeks_not_frozen: i64,
    pub baseline_hours: f64,
    /// Baseline hours of the weeks judged.
    pub judged_baseline_hours: f64,
    pub done_hours: f64,
    pub compliance_pct: Option<f64>,
    pub break_in_hours: f64,
    pub break_in_pct: Option<f64>,
    pub displaced_jobs: i64,
    pub available_hours: f64,
    pub overtime_hours: f64,
    pub loading_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningKpis {
    pub weeks: Vec<WeekKpi>,
    pub totals: PlanningKpiTotals,
}

fn parse_day(raw: &str, field: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| {
        crate::errors::org_validation_failed(
            "PLANNING_WEEK_DAY_INVALID",
            format!("'{raw}' is not a date (YYYY-MM-DD)."),
            std::collections::HashMap::from([
                ("day".to_string(), raw.to_string()),
                ("field".to_string(), field.to_string()),
            ]),
        )
    })
}

/// The work order behind a commitment (alias `sc`): its own, the PM occurrence's, or the one a
/// request was converted to.
const BEHIND_WO_SQL: &str = "(CASE sc.source_type \
    WHEN 'work_order' THEN sc.source_id \
    WHEN 'pm_occurrence' THEN (SELECT po.linked_work_order_id FROM pm_occurrences po WHERE po.id = sc.source_id) \
    WHEN 'inspection_follow_up' THEN (SELECT ir.converted_to_wo_id FROM intervention_requests ir WHERE ir.id = sc.source_id) \
  END)";

/// Hours of a slot without planned labour hours: its length.
const SLOT_HOURS_SQL: &str = "((julianday(sc.committed_end) - julianday(sc.committed_start)) * 24.0)";

struct Baseline {
    jobs: i64,
    hours: f64,
    done: f64,
}

/// The week's current baseline: jobs, hours, and hours whose work was finished by `week_end_at`.
async fn baseline(db: &impl ConnectionTrait, schedule: &WeeklySchedule, week_end_at: &str) -> AppResult<Baseline> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "WITH b AS ( \
                   SELECT COALESCE(bi.planned_labor_hours, \
                                   (julianday(bi.committed_end) - julianday(bi.committed_start)) * 24.0) AS hours, \
                          (SELECT COALESCE(w.mechanically_completed_at, w.actual_end, w.closed_at) \
                             FROM work_orders w WHERE w.id = {BEHIND_WO_SQL} AND w.cancelled_at IS NULL) AS done_at \
                     FROM weekly_schedule_baseline_items bi \
                     JOIN schedule_commitments sc ON sc.id = bi.schedule_commitment_id \
                    WHERE bi.weekly_schedule_id = ? AND bi.baseline_no = ?) \
                 SELECT COUNT(*) AS jobs, COALESCE(SUM(hours), 0) AS hours, \
                        COALESCE(SUM(CASE WHEN done_at IS NOT NULL AND julianday(done_at) <= julianday(?) \
                                          THEN hours ELSE 0 END), 0) AS done \
                   FROM b"
            ),
            [schedule.id.into(), schedule.baseline_no.into(), week_end_at.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("baseline aggregate missing")))?;
    Ok(Baseline {
        jobs: row.try_get("", "jobs").map_err(|e| decode_err("jobs", e))?,
        hours: row.try_get("", "hours").map_err(|e| decode_err("hours", e))?,
        done: row.try_get("", "done").map_err(|e| decode_err("done", e))?,
    })
}

/// Break-ins of the site's crews touching the week: `insert` jobs and their hours, and displaced
/// jobs (`move` break-ins recorded after the freeze).
async fn break_ins(
    db: &impl ConnectionTrait,
    crews: &[i64],
    week_start_at: &str,
    week_end_at: &str,
    frozen_at: Option<&str>,
) -> AppResult<(i64, f64, i64)> {
    if crews.is_empty() {
        return Ok((0, 0.0, 0));
    }
    let marks = vec!["?"; crews.len()].join(",");
    let crew_values = || crews.iter().map(|c| sea_orm::Value::from(*c)).collect::<Vec<_>>();

    let mut inserted_values = crew_values();
    inserted_values.extend([week_start_at.into(), week_end_at.into()]);
    let inserted = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS jobs, COALESCE(SUM(COALESCE(sc.planned_labor_hours, {SLOT_HOURS_SQL})), 0) AS hours \
                   FROM schedule_break_ins sb JOIN schedule_commitments sc ON sc.id = sb.schedule_commitment_id \
                  WHERE sb.kind = 'insert' AND sc.assigned_team_id IN ({marks}) \
                    AND julianday(sb.new_slot_start) >= julianday(?) AND julianday(sb.new_slot_start) < julianday(?)"
            ),
            inserted_values,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("break-in aggregate missing")))?;

    // A week never frozen has no displacement; the bound below excludes every move then.
    let since = frozen_at.unwrap_or("9999-12-31T00:00:00Z");
    let mut moved_values = crew_values();
    moved_values.extend([
        week_start_at.into(),
        week_end_at.into(),
        week_start_at.into(),
        week_end_at.into(),
        since.into(),
    ]);
    let displaced = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(DISTINCT sb.schedule_commitment_id) AS jobs \
                   FROM schedule_break_ins sb JOIN schedule_commitments sc ON sc.id = sb.schedule_commitment_id \
                  WHERE sb.kind = 'move' AND sc.assigned_team_id IN ({marks}) \
                    AND ((julianday(sb.new_slot_start) >= julianday(?) AND julianday(sb.new_slot_start) < julianday(?)) \
                      OR (julianday(sb.old_slot_start) >= julianday(?) AND julianday(sb.old_slot_start) < julianday(?))) \
                    AND julianday(sb.created_at) >= julianday(?)"
            ),
            moved_values,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("displaced aggregate missing")))?;
    Ok((
        inserted.try_get("", "jobs").map_err(|e| decode_err("jobs", e))?,
        inserted.try_get("", "hours").map_err(|e| decode_err("hours", e))?,
        displaced.try_get("", "jobs").map_err(|e| decode_err("jobs", e))?,
    ))
}

/// The crews' available hours and overtime ceiling over the week.
async fn capacity(db: &impl ConnectionTrait, crews: &[i64], from: NaiveDate, to: NaiveDate) -> AppResult<(f64, f64)> {
    let mut available = 0.0;
    let mut overtime = 0.0;
    for crew in crews {
        for day in super::load::team_days(db, *crew, from, to).await?.values() {
            available += day.available;
            overtime += day.overtime;
        }
    }
    Ok((available, overtime))
}

/// Measures one frozen or published week.
async fn week_kpi(
    db: &impl ConnectionTrait,
    schedule: &WeeklySchedule,
    site_name: &str,
    now: DateTime<Utc>,
) -> AppResult<WeekKpi> {
    let start = parse_day(&schedule.week_start, "week_start")?;
    let end = parse_day(&schedule.week_end, "week_end")?;
    let start_at = rfc3339(local_midnight_utc(start)?);
    let end_instant = local_midnight_utc(end + Duration::days(1))?;
    let end_at = rfc3339(end_instant);
    let crews: Vec<i64> = site_crews(db, schedule.entity_id)
        .await?
        .into_iter()
        .map(|c| c.team_id)
        .collect();
    let base = baseline(db, schedule, &end_at).await?;
    let (break_in_jobs, break_in_hours, displaced_jobs) =
        break_ins(db, &crews, &start_at, &end_at, schedule.frozen_at.as_deref()).await?;
    let (available, overtime) = capacity(db, &crews, start, end).await?;
    let over = end_instant <= now;
    Ok(WeekKpi {
        weekly_schedule_id: schedule.id,
        site_id: schedule.entity_id,
        site_name: site_name.to_string(),
        week_start: schedule.week_start.clone(),
        week_end: schedule.week_end.clone(),
        status: schedule.status.clone(),
        awaiting_approval: schedule.frozen_without_approval == 1 && schedule.approved_at.is_none(),
        baseline_no: schedule.baseline_no,
        baseline_jobs: base.jobs,
        baseline_hours: round1(base.hours),
        done_hours: over.then(|| round1(base.done)),
        compliance_pct: if over { pct(base.done, base.hours) } else { None },
        break_in_jobs,
        break_in_hours: round1(break_in_hours),
        break_in_pct: pct(break_in_hours, base.hours),
        displaced_jobs,
        available_hours: round1(available),
        overtime_hours: round1(overtime),
        loading_pct: pct(base.hours, available),
    })
}

/// The frozen or published weeks of `sites` whose first day is in `[from, to]`.
async fn frozen_schedules(
    db: &impl ConnectionTrait,
    sites: &[i64],
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<(WeeklySchedule, String)>> {
    if sites.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; sites.len()].join(",");
    let mut values: Vec<sea_orm::Value> = sites.iter().map(|s| (*s).into()).collect();
    values.extend([
        from.to_string().into(),
        to.to_string().into(),
        STATUS_FROZEN.into(),
        STATUS_PUBLISHED.into(),
    ]);
    let cols = SCHEDULE_COLS
        .split(',')
        .map(|c| format!("ws.{}", c.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {cols}, n.name AS site_name FROM weekly_schedules ws JOIN org_nodes n ON n.id = ws.entity_id \
                  WHERE ws.entity_id IN ({marks}) AND ws.week_start >= ? AND ws.week_start <= ? \
                    AND ws.status IN (?, ?) AND ws.baseline_no > 0 \
                  ORDER BY ws.week_start, n.name, ws.id"
            ),
            values,
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok((
                map_schedule(r)?,
                r.try_get::<String>("", "site_name")
                    .map_err(|e| decode_err("site_name", e))?,
            ))
        })
        .collect()
}

/// The planning sites to measure: one, or every site that has crews.
async fn sites_of(db: &impl ConnectionTrait, site_id: Option<i64>) -> AppResult<Vec<i64>> {
    if let Some(site) = site_id {
        return Ok(vec![site]);
    }
    let sites: BTreeSet<i64> = crews_with_sites(db).await?.into_iter().filter_map(|(_, s)| s).collect();
    Ok(sites.into_iter().collect())
}

/// Weeks of `sites` starting in `[from, to]` whose freeze fell due by `now` and that were never
/// frozen (no baseline): counted, not measured.
async fn weeks_not_frozen(
    db: &impl ConnectionTrait,
    sites: &[i64],
    from: NaiveDate,
    to: NaiveDate,
    now: DateTime<Utc>,
) -> AppResult<i64> {
    let today = now.with_timezone(&Local).date_naive();
    let mut count = 0;
    for site in sites {
        let mut day = from;
        while day <= to.min(today) {
            let Ok(week) = site_week(db, *site, day).await else {
                break; // No planning week for this site: nothing to freeze.
            };
            day = week.week_end + Duration::days(1);
            if week.week_start < from || week.week_start > to {
                continue;
            }
            let frozen = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT 1 AS x FROM weekly_schedules WHERE entity_id = ? AND week_start = ? AND baseline_no > 0",
                    [(*site).into(), week.week_start.to_string().into()],
                ))
                .await?
                .is_some();
            if !frozen && freeze_due_at(db, &week).await? <= now {
                count += 1;
            }
        }
    }
    Ok(count)
}

/// Planning KPIs of the weeks starting in the filter's range, per week and in total.
pub async fn planning_kpis(
    db: &impl ConnectionTrait,
    filter: &PlanningKpiFilter,
    now: DateTime<Utc>,
) -> AppResult<PlanningKpis> {
    let from = parse_day(&filter.from_day, "from_day")?;
    let to = parse_day(&filter.to_day, "to_day")?;
    let sites = sites_of(db, filter.site_id).await?;
    let mut weeks = Vec::new();
    for (schedule, site_name) in frozen_schedules(db, &sites, from, to).await? {
        let kpi = week_kpi(db, &schedule, &site_name, now).await?;
        if filter.exclude_unapproved && kpi.awaiting_approval {
            continue;
        }
        weeks.push(kpi);
    }
    let mut t = PlanningKpiTotals {
        weeks_not_frozen: weeks_not_frozen(db, &sites, from, to, now).await?,
        ..PlanningKpiTotals::default()
    };
    for w in &weeks {
        t.weeks += 1;
        t.weeks_awaiting_approval += i64::from(w.awaiting_approval);
        t.baseline_hours += w.baseline_hours;
        t.break_in_hours += w.break_in_hours;
        t.displaced_jobs += w.displaced_jobs;
        t.available_hours += w.available_hours;
        t.overtime_hours += w.overtime_hours;
        if let Some(done) = w.done_hours {
            t.weeks_judged += 1;
            t.judged_baseline_hours += w.baseline_hours;
            t.done_hours += done;
        }
    }
    t.compliance_pct = pct(t.done_hours, t.judged_baseline_hours);
    t.break_in_pct = pct(t.break_in_hours, t.baseline_hours);
    t.loading_pct = pct(t.baseline_hours, t.available_hours);
    for v in [
        &mut t.baseline_hours,
        &mut t.judged_baseline_hours,
        &mut t.done_hours,
        &mut t.break_in_hours,
        &mut t.available_hours,
        &mut t.overtime_hours,
    ] {
        *v = round1(*v);
    }
    Ok(PlanningKpis { weeks, totals: t })
}

/// SMRP schedule compliance (hours) on the baseline basis, for analytics, the home dashboard and
/// the monthly report: the frozen weeks whose end falls in `[from, until)` and is past (`now`), of
/// the sites at or under `org` (every site without one). Returns `(done hours, baseline hours)`.
pub async fn baseline_schedule_compliance(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
    org: Option<i64>,
    now: DateTime<Utc>,
) -> AppResult<(f64, f64)> {
    let mut sites = sites_of(db, None).await?;
    if let Some(org) = org {
        let under: BTreeSet<i64> = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "WITH RECURSIVE sub(id) AS (SELECT ? UNION ALL SELECT n.id FROM org_nodes n JOIN sub ON n.parent_id = sub.id) \
                 SELECT id FROM sub",
                [org.into()],
            ))
            .await?
            .iter()
            .map(|r| r.try_get::<i64>("", "id").map_err(|e| decode_err("id", e)))
            .collect::<AppResult<_>>()?;
        sites.retain(|s| under.contains(s));
    }
    // A week starts at most 13 days before it ends.
    let first = (from - Duration::days(14)).with_timezone(&Local).date_naive();
    let last = until.with_timezone(&Local).date_naive();
    let (mut done, mut scheduled) = (0.0, 0.0);
    for (schedule, _) in frozen_schedules(db, &sites, first, last).await? {
        let end = parse_day(&schedule.week_end, "week_end")?;
        let end_instant = local_midnight_utc(end + Duration::days(1))?;
        if end_instant < from || end_instant >= until || end_instant > now {
            continue;
        }
        let base = baseline(db, &schedule, &rfc3339(end_instant)).await?;
        done += base.done;
        scheduled += base.hours;
    }
    Ok((done, scheduled))
}
