//! Planner's start panel: the schedule backlog by readiness, this week's committed load against
//! the crews' capacity, and schedule compliance — all three are the planning module's figures.
//!
//! The week is the site's work week (Données de référence → Calendrier, `site_work_weeks`): it
//! starts the day after the site's weekly rest days, the rule of the weekly schedule. Without a
//! work week that applies, the week figure is not given (a coded reason instead), never a guessed
//! Monday-to-Sunday.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use super::{decode, period_window};
use crate::analytics::smrp;
use crate::errors::AppResult;
use crate::personnel::calendar::SiteWeekBook;
use crate::planning::domain::{ScheduleCandidateFilter, TeamCapacityLoad};

/// Largest backlog the planning board reads at once (`planning::queries::list_schedule_candidates`).
pub const BACKLOG_READ_LIMIT: i64 = 2_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlockerCount {
    /// `parts`, `skills`, `permits`, `windows` or `prerequisites` (the planning board's dimensions).
    pub dimension: String,
    pub count: i64,
}

/// The schedule backlog (`planning::queries::get_schedule_backlog_snapshot`, as the board reads it:
/// blocked candidates included).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlanningBacklog {
    pub candidates: i64,
    pub ready: i64,
    pub blocked: i64,
    /// Blocked candidates per blocking dimension (one candidate can be blocked on several).
    pub blocked_by: Vec<BlockerCount>,
    /// `true` when the backlog holds more than [`BACKLOG_READ_LIMIT`] candidates: the counts then
    /// cover the first ones only.
    pub truncated: bool,
}

/// No work week is set for any site (Données de référence → Calendrier).
pub const WORK_WEEK_NOT_CONFIGURED: &str = "WORK_WEEK_NOT_CONFIGURED";
/// Sites have different work weeks and the person's own site gives none.
pub const WORK_WEEK_SITE_AMBIGUOUS: &str = "WORK_WEEK_SITE_AMBIGUOUS";
/// The work week has no rest day: no day starts it.
pub const WORK_WEEK_NO_REST_DAY: &str = "WORK_WEEK_NO_REST_DAY";
/// The rest days are split over the week: more than one day follows a rest day.
pub const WORK_WEEK_SPLIT_REST: &str = "WORK_WEEK_SPLIT_REST";

/// Which work week was used.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkWeekBasis {
    /// ISO weekdays (1 = Monday … 7 = Sunday) of the weekly rest.
    pub rest_days: Vec<i64>,
    /// `all_sites`: every site with a work week has this one; `own_site`: the person's own site
    /// (sites differ).
    pub scope: String,
    /// Name of the person's site when `scope` is `own_site`.
    pub site_name: Option<String>,
}

/// This week's committed labour hours against the crews' capacity
/// (`planning::scheduling::list_team_capacity_load`, the board's capacity view).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WeekLoad {
    /// First and last day of the current work week (local days).
    pub week_start: String,
    pub week_end: String,
    pub basis: WorkWeekBasis,
    pub teams: i64,
    /// Available + overtime hours of every active crew over the week.
    pub capacity_hours: f64,
    pub committed_hours: f64,
    /// committed ÷ capacity × 100 over the week (pooled, never a mean of daily ratios); `None` when
    /// the crews have no hours this week.
    pub load_pct: Option<f64>,
    /// Crew-days whose commitments exceed their hours (including days with no hours at all).
    pub overloaded_team_days: i64,
}

/// SMRP schedule compliance (`analytics::smrp::schedule_compliance`): baseline hours of the frozen
/// weeks ended in the period whose work was finished by the end of their week ÷ baseline hours
/// (`planning::kpis`, the same number as the planning KPIs).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScheduleComplianceFigure {
    pub period_days: i64,
    pub scheduled_hours: f64,
    pub done_hours: f64,
    /// `None` when nothing was committed in the period.
    pub value_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HomePlanning {
    pub as_of: String,
    pub backlog: PlanningBacklog,
    /// `None` with `week_unavailable` set when no work week applies.
    pub week: Option<WeekLoad>,
    /// Why there is no week figure: one of the `WORK_WEEK_*` codes.
    pub week_unavailable: Option<String>,
    pub schedule_compliance: ScheduleComplianceFigure,
}

/// The work week holding `day` for the weekly rest days `rest` (ISO weekdays): it starts the day
/// after the rest days and runs 7 days. Refused (a `WORK_WEEK_*` code) without a rest day or when
/// the rest days are split, as no single day then starts the week.
pub fn work_week_bounds(day: NaiveDate, rest: &[i64]) -> Result<(NaiveDate, NaiveDate), &'static str> {
    if rest.is_empty() {
        return Err(WORK_WEEK_NO_REST_DAY);
    }
    let is_rest = |iso: i64| rest.contains(&iso);
    let previous = |iso: i64| if iso == 1 { 7 } else { iso - 1 };
    let starts: Vec<i64> = (1..=7).filter(|d| !is_rest(*d) && is_rest(previous(*d))).collect();
    let [start] = starts.as_slice() else {
        return Err(WORK_WEEK_SPLIT_REST);
    };
    let today = i64::from(day.weekday().number_from_monday());
    let back = (today - start).rem_euclid(7);
    let first = day - Duration::days(back);
    Ok((first, first + Duration::days(6)))
}

/// The work week rest days that apply to the person on `today`: the one every configured site
/// shares, else the person's own site's; a `WORK_WEEK_*` code when none can be told.
pub async fn applicable_work_week(
    db: &DatabaseConnection,
    user_id: i64,
    today: NaiveDate,
) -> AppResult<Result<WorkWeekBasis, &'static str>> {
    let configured = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT entity_id FROM site_work_weeks".to_string(),
        ))
        .await?;
    let mut sets: Vec<Vec<i64>> = Vec::new();
    for r in &configured {
        let entity: Option<i64> = decode(r, "entity_id")?;
        let book = SiteWeekBook::load(db, entity).await?;
        if let Some(days) = book.rest_days_on(today) {
            if !sets.iter().any(|s| s.as_slice() == days) {
                sets.push(days.to_vec());
            }
        }
    }
    match sets.as_slice() {
        [] => return Ok(Err(WORK_WEEK_NOT_CONFIGURED)),
        [only] => {
            return Ok(Ok(WorkWeekBasis {
                rest_days: only.clone(),
                scope: "all_sites".into(),
                site_name: None,
            }))
        }
        _ => {}
    }
    // Sites differ: the person's own site (their personnel record's entity).
    let Some(personnel_id) = crate::qualification::gate::personnel_for_user(db, user_id).await? else {
        return Ok(Err(WORK_WEEK_SITE_AMBIGUOUS));
    };
    let site = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.primary_entity_id AS entity_id, n.name AS site_name FROM personnel p \
             LEFT JOIN org_nodes n ON n.id = p.primary_entity_id WHERE p.id = ?",
            [personnel_id.into()],
        ))
        .await?;
    let (entity, site_name): (Option<i64>, Option<String>) = match &site {
        Some(r) => (decode(r, "entity_id")?, decode(r, "site_name")?),
        None => (None, None),
    };
    let Some(entity) = entity else {
        return Ok(Err(WORK_WEEK_SITE_AMBIGUOUS));
    };
    let book = SiteWeekBook::load(db, Some(entity)).await?;
    Ok(match book.rest_days_on(today) {
        Some(days) => Ok(WorkWeekBasis {
            rest_days: days.to_vec(),
            scope: "own_site".into(),
            site_name,
        }),
        None => Err(WORK_WEEK_SITE_AMBIGUOUS),
    })
}

pub async fn backlog(db: &DatabaseConnection) -> AppResult<PlanningBacklog> {
    let snapshot = crate::planning::queries::get_schedule_backlog_snapshot(
        db,
        ScheduleCandidateFilter {
            source_type: None,
            readiness_status: None,
            assigned_personnel_id: None,
            include_resolved_conflicts: Some(true),
            limit: Some(BACKLOG_READ_LIMIT),
        },
    )
    .await?;
    let mut by: BTreeMap<String, i64> = BTreeMap::new();
    for summary in snapshot
        .conflict_summary
        .iter()
        .filter(|c| c.readiness_status != "ready")
    {
        for dimension in &summary.blocker_dimensions {
            *by.entry(dimension.clone()).or_default() += 1;
        }
    }
    Ok(PlanningBacklog {
        candidates: snapshot.candidate_count,
        ready: snapshot.ready_count,
        blocked: snapshot.blocked_count,
        blocked_by: by
            .into_iter()
            .map(|(dimension, count)| BlockerCount { dimension, count })
            .collect(),
        truncated: snapshot.candidate_count >= BACKLOG_READ_LIMIT,
    })
}

/// Sums the crews' days into the week figure.
pub fn week_load(
    rows: &[TeamCapacityLoad],
    week_start: NaiveDate,
    week_end: NaiveDate,
    basis: WorkWeekBasis,
) -> WeekLoad {
    let mut capacity = 0.0_f64;
    let mut committed = 0.0_f64;
    let mut overloaded = 0_i64;
    let mut teams: Vec<i64> = Vec::new();
    for r in rows {
        let hours = r.available_hours + r.overtime_hours;
        capacity += hours;
        committed += r.committed_hours;
        if r.committed_hours > hours + 1e-9 {
            overloaded += 1;
        }
        if !teams.contains(&r.team_id) {
            teams.push(r.team_id);
        }
    }
    WeekLoad {
        week_start: week_start.to_string(),
        week_end: week_end.to_string(),
        basis,
        teams: teams.len() as i64,
        capacity_hours: capacity,
        committed_hours: committed,
        load_pct: (capacity > 0.0).then(|| committed / capacity * 100.0),
        overloaded_team_days: overloaded,
    }
}

/// This work week's load, or the `WORK_WEEK_*` code saying why it cannot be given.
pub async fn this_week(
    db: &DatabaseConnection,
    user_id: i64,
    today: NaiveDate,
) -> AppResult<Result<WeekLoad, &'static str>> {
    let basis = match applicable_work_week(db, user_id, today).await? {
        Ok(b) => b,
        Err(code) => return Ok(Err(code)),
    };
    let (first, last) = match work_week_bounds(today, &basis.rest_days) {
        Ok(bounds) => bounds,
        Err(code) => return Ok(Err(code)),
    };
    // Same period form as the crew screen: whole local days, first to last.
    let rows = crate::planning::scheduling::list_team_capacity_load(
        db,
        format!("{first}T00:00:00Z"),
        format!("{last}T23:59:59Z"),
        None,
    )
    .await?;
    Ok(Ok(week_load(&rows, first, last, basis)))
}

pub async fn schedule_compliance(
    db: &DatabaseConnection,
    period_days: i64,
    now: DateTime<Utc>,
) -> AppResult<ScheduleComplianceFigure> {
    let ratio = smrp::schedule_compliance(db, period_window(now, period_days), now, None).await?;
    Ok(ScheduleComplianceFigure {
        period_days,
        scheduled_hours: ratio.denominator,
        done_hours: ratio.numerator,
        value_pct: ratio.value_pct,
    })
}

pub async fn build_planning(
    db: &DatabaseConnection,
    user_id: i64,
    period_days: i64,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> AppResult<HomePlanning> {
    let (week, week_unavailable) = match this_week(db, user_id, today).await? {
        Ok(w) => (Some(w), None),
        Err(code) => (None, Some(code.to_string())),
    };
    Ok(HomePlanning {
        as_of: crate::analytics::as_of::instant(now),
        backlog: backlog(db).await?,
        week,
        week_unavailable,
        schedule_compliance: schedule_compliance(db, period_days, now).await?,
    })
}
