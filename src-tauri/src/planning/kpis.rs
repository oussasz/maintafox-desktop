//! Planning KPIs measured against the frozen weekly baseline (planning step E).
//!
//! Basis: for each planning site and week, the LATEST baseline of a frozen or published week
//! (`weekly_schedule_baseline_items` of the schedule's current `baseline_no`). A week that was never
//! frozen has no baseline: it is not measured and is counted apart ("not frozen") once its freeze
//! fell due — never guessed from live commitments. A week frozen without approval (and not
//! approved since) is measured, flagged, counted, and can be left out. A week unfrozen for editing
//! keeps being measured on its last baseline, flagged "reopened", until it is frozen again.
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
    /// Unfrozen for editing since its last freeze: measured on that last baseline, flagged.
    pub reopened: bool,
    pub baseline_no: i64,
    pub baseline_jobs: i64,
    pub baseline_hours: f64,
    /// Baseline jobs whose slot was later released or cancelled: scheduled, counted as not done.
    pub baseline_released_jobs: i64,
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
    /// Of them, weeks unfrozen for editing since their last freeze (measured on that baseline).
    pub weeks_reopened: i64,
    /// Weeks whose freeze fell due and that were never frozen: no baseline, not measured.
    pub weeks_not_frozen: i64,
    pub baseline_hours: f64,
    pub baseline_released_jobs: i64,
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
    /// Baseline jobs whose slot was later released or cancelled (counted as not done).
    released: i64,
}

/// The week's current baseline: jobs, hours, and hours whose work was finished by `week_end_at`.
///
/// "Finished by the week's end" is not one of the two open-OT rules (`wo::backlog`): those read
/// the OT's status now, while schedule compliance asks whether the work ended before a past
/// instant, so it reads the OT's completion time (mechanical completion, else actual end, else
/// closure; a cancelled OT is never done) as `planning::load::finished_at_sql` does.
async fn baseline(db: &impl ConnectionTrait, schedule: &WeeklySchedule, week_end_at: &str) -> AppResult<Baseline> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "WITH b AS ( \
                   SELECT COALESCE(bi.planned_labor_hours, \
                                   (julianday(bi.committed_end) - julianday(bi.committed_start)) * 24.0) AS hours, \
                          (SELECT COALESCE(w.mechanically_completed_at, w.actual_end, w.closed_at) \
                             FROM work_orders w WHERE w.id = {BEHIND_WO_SQL} AND w.cancelled_at IS NULL) AS done_at, \
                          sc.status IN ('released', 'cancelled') AS released \
                     FROM weekly_schedule_baseline_items bi \
                     JOIN schedule_commitments sc ON sc.id = bi.schedule_commitment_id \
                    WHERE bi.weekly_schedule_id = ? AND bi.baseline_no = ?) \
                 SELECT COUNT(*) AS jobs, TOTAL(hours) AS hours, \
                        TOTAL(CASE WHEN NOT released AND done_at IS NOT NULL \
                                        AND julianday(done_at) <= julianday(?) \
                                   THEN hours ELSE 0 END) AS done, \
                        COALESCE(SUM(released), 0) AS released \
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
        released: row.try_get("", "released").map_err(|e| decode_err("released", e))?,
    })
}

/// When the week's current baseline held: from its freeze until it was unfrozen for editing
/// (`None`: still frozen). A week frozen or published reads its own `frozen_at`; a reopened one
/// reads the freeze and unfreeze events of that baseline.
async fn frozen_span(
    db: &impl ConnectionTrait,
    schedule: &WeeklySchedule,
) -> AppResult<(Option<String>, Option<String>)> {
    if matches!(schedule.status.as_str(), STATUS_FROZEN | STATUS_PUBLISHED) {
        return Ok((schedule.frozen_at.clone(), None));
    }
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT (SELECT MAX(created_at) FROM weekly_schedule_events \
                      WHERE weekly_schedule_id = ? AND baseline_no = ? AND action IN ('frozen', 'auto_frozen')) AS since, \
                    (SELECT MAX(created_at) FROM weekly_schedule_events \
                      WHERE weekly_schedule_id = ? AND baseline_no = ? AND action = 'unfrozen') AS until",
            [
                schedule.id.into(),
                schedule.baseline_no.into(),
                schedule.id.into(),
                schedule.baseline_no.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("freeze span missing")))?;
    Ok((
        row.try_get("", "since").map_err(|e| decode_err("since", e))?,
        row.try_get("", "until").map_err(|e| decode_err("until", e))?,
    ))
}

/// Break-ins of the site's crews touching the week: `insert` jobs and their hours, and displaced
/// jobs (`move` break-ins recorded while the baseline held, `[frozen_since, frozen_until)`).
async fn break_ins(
    db: &impl ConnectionTrait,
    crews: &[i64],
    week_start_at: &str,
    week_end_at: &str,
    frozen_since: Option<&str>,
    frozen_until: Option<&str>,
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
                "SELECT COUNT(*) AS jobs, TOTAL(COALESCE(sc.planned_labor_hours, {SLOT_HOURS_SQL})) AS hours \
                   FROM schedule_break_ins sb JOIN schedule_commitments sc ON sc.id = sb.schedule_commitment_id \
                  WHERE sb.kind = 'insert' AND sb.sync_refused_at IS NULL AND sc.assigned_team_id IN ({marks}) \
                    AND julianday(sb.new_slot_start) >= julianday(?) AND julianday(sb.new_slot_start) < julianday(?)"
            ),
            inserted_values,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("break-in aggregate missing")))?;

    // A week never frozen has no displacement; the bound below excludes every move then.
    let since = frozen_since.unwrap_or("9999-12-31T00:00:00Z");
    let until = frozen_until.unwrap_or("9999-12-31T00:00:00Z");
    let mut moved_values = crew_values();
    moved_values.extend([
        week_start_at.into(),
        week_end_at.into(),
        week_start_at.into(),
        week_end_at.into(),
        since.into(),
        until.into(),
    ]);
    let displaced = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(DISTINCT sb.schedule_commitment_id) AS jobs \
                   FROM schedule_break_ins sb JOIN schedule_commitments sc ON sc.id = sb.schedule_commitment_id \
                  WHERE sb.kind = 'move' AND sb.sync_refused_at IS NULL AND sc.assigned_team_id IN ({marks}) \
                    AND ((julianday(sb.new_slot_start) >= julianday(?) AND julianday(sb.new_slot_start) < julianday(?)) \
                      OR (julianday(sb.old_slot_start) >= julianday(?) AND julianday(sb.old_slot_start) < julianday(?))) \
                    AND julianday(sb.created_at) >= julianday(?) AND julianday(sb.created_at) < julianday(?)"
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

/// Measures one week on its latest baseline (frozen, published, or reopened since).
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
    let (frozen_since, frozen_until) = frozen_span(db, schedule).await?;
    let (break_in_jobs, break_in_hours, displaced_jobs) = break_ins(
        db,
        &crews,
        &start_at,
        &end_at,
        frozen_since.as_deref(),
        frozen_until.as_deref(),
    )
    .await?;
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
        reopened: !matches!(schedule.status.as_str(), STATUS_FROZEN | STATUS_PUBLISHED),
        baseline_no: schedule.baseline_no,
        baseline_jobs: base.jobs,
        baseline_hours: round1(base.hours),
        baseline_released_jobs: base.released,
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

/// The weeks of `sites` whose first day is in `[from, to]` and that have a baseline: frozen,
/// published, or unfrozen for editing since (still measured on their last baseline).
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
    values.extend([from.to_string().into(), to.to_string().into()]);
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
                    AND ws.baseline_no > 0 \
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
    // Every week of the range is looked at: a freeze can fall due before its week starts.
    let mut count = 0;
    for site in sites {
        let mut day = from;
        while day <= to {
            let week = match site_week(db, *site, day).await {
                Ok(week) => week,
                // No planning week for this site (no weekend set): nothing to freeze.
                Err(AppError::OrgValidationFailed(issues))
                    if issues.iter().any(|i| i.code == "PLANNING_WEEK_NO_WEEKEND") =>
                {
                    break;
                }
                Err(err) => return Err(err),
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
        t.weeks_reopened += i64::from(w.reopened);
        t.baseline_hours += w.baseline_hours;
        t.baseline_released_jobs += w.baseline_released_jobs;
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

/// The current week of every planning site that has one frozen (or published), measured, and how
/// many planning sites there are: what "this week's load" reads.
pub async fn current_weeks(db: &impl ConnectionTrait, now: DateTime<Utc>) -> AppResult<(Vec<WeekKpi>, usize)> {
    let sites = sites_of(db, None).await?;
    let today = now.with_timezone(&Local).date_naive();
    let mut weeks = Vec::new();
    for (schedule, site_name) in frozen_schedules(db, &sites, today - Duration::days(13), today).await? {
        let start = parse_day(&schedule.week_start, "week_start")?;
        let end = parse_day(&schedule.week_end, "week_end")?;
        if start <= today && today <= end {
            weeks.push(week_kpi(db, &schedule, &site_name, now).await?);
        }
    }
    Ok((weeks, sites.len()))
}

/// Schedule compliance on the baseline basis, with what it was measured on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BaselineCompliance {
    pub done_hours: f64,
    pub baseline_hours: f64,
    /// Baseline jobs later released or cancelled (counted as not done).
    pub released_jobs: i64,
    /// Frozen weeks measured.
    pub weeks: i64,
}

/// SMRP schedule compliance (hours) on the baseline basis, for analytics, the home dashboard and
/// the monthly report: the frozen weeks whose end falls in `[from, until)` and is past (`now`), of
/// the sites at or under `org` (every site without one).
pub async fn baseline_schedule_compliance(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
    org: Option<i64>,
    now: DateTime<Utc>,
) -> AppResult<BaselineCompliance> {
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
    let mut out = BaselineCompliance::default();
    for (schedule, _) in frozen_schedules(db, &sites, first, last).await? {
        let end = parse_day(&schedule.week_end, "week_end")?;
        let end_instant = local_midnight_utc(end + Duration::days(1))?;
        if end_instant < from || end_instant >= until || end_instant > now {
            continue;
        }
        let base = baseline(db, &schedule, &rfc3339(end_instant)).await?;
        out.done_hours += base.done;
        out.baseline_hours += base.hours;
        out.released_jobs += base.released;
        out.weeks += 1;
    }
    Ok(out)
}
