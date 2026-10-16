//! Planner's start panel: the schedule backlog by readiness, this week's committed load against
//! the crews' capacity, and schedule compliance — all three are the planning module's figures.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use sea_orm::DatabaseConnection;
use serde::Serialize;

use crate::analytics::smrp::{self, SmrpWindow};
use crate::errors::AppResult;
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

/// This week's committed labour hours against the crews' capacity
/// (`planning::scheduling::list_team_capacity_load`, the board's capacity view).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WeekLoad {
    /// Monday and Sunday of the current week (local days).
    pub week_start: String,
    pub week_end: String,
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

/// SMRP schedule compliance (`analytics::smrp::schedule_compliance`): committed hours completed by
/// the end of their committed day ÷ committed hours, commitments ending in the period.
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
    pub week: WeekLoad,
    pub schedule_compliance: ScheduleComplianceFigure,
}

/// Monday and Sunday of the week holding `day`.
pub fn week_bounds(day: NaiveDate) -> (NaiveDate, NaiveDate) {
    let monday = day - Duration::days(i64::from(day.weekday().num_days_from_monday()));
    (monday, monday + Duration::days(6))
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
pub fn week_load(rows: &[TeamCapacityLoad], week_start: NaiveDate, week_end: NaiveDate) -> WeekLoad {
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
        teams: teams.len() as i64,
        capacity_hours: capacity,
        committed_hours: committed,
        load_pct: (capacity > 0.0).then(|| committed / capacity * 100.0),
        overloaded_team_days: overloaded,
    }
}

pub async fn this_week(db: &DatabaseConnection, today: NaiveDate) -> AppResult<WeekLoad> {
    let (monday, sunday) = week_bounds(today);
    // Same period form as the crew screen: whole local days, first to last.
    let rows = crate::planning::scheduling::list_team_capacity_load(
        db,
        format!("{monday}T00:00:00Z"),
        format!("{sunday}T23:59:59Z"),
        None,
    )
    .await?;
    Ok(week_load(&rows, monday, sunday))
}

pub async fn schedule_compliance(
    db: &DatabaseConnection,
    period_days: i64,
    now: DateTime<Utc>,
) -> AppResult<ScheduleComplianceFigure> {
    let ratio = smrp::schedule_compliance(db, SmrpWindow::last_days(now, period_days), now, None).await?;
    Ok(ScheduleComplianceFigure {
        period_days,
        scheduled_hours: ratio.denominator,
        done_hours: ratio.numerator,
        value_pct: ratio.value_pct,
    })
}

pub async fn build_planning(
    db: &DatabaseConnection,
    period_days: i64,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> AppResult<HomePlanning> {
    Ok(HomePlanning {
        as_of: crate::analytics::as_of::instant(now),
        backlog: backlog(db).await?,
        week: this_week(db, today).await?,
        schedule_compliance: schedule_compliance(db, period_days, now).await?,
    })
}
