//! Planner's start panel: the schedule backlog by readiness, this week's load and schedule
//! compliance — all three are the planning module's figures, with the planning KPIs' definitions
//! (`planning::kpis`): measured against the frozen weekly baseline of each planning site. Without a
//! frozen baseline the figure is not given (a coded reason instead), never computed another way.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde::Serialize;

use super::period_window;
use crate::analytics::smrp;
use crate::errors::AppResult;
use crate::planning::domain::ScheduleCandidateFilter;

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

/// No planning site has crews: there is no week to load.
pub const WEEK_NO_PLANNING_SITE: &str = "WEEK_NO_PLANNING_SITE";
/// No planning site's current week is frozen: there is no baseline to measure yet.
pub const WEEK_NOT_FROZEN: &str = "WEEK_NOT_FROZEN";
/// No frozen week ended in the period: compliance has nothing to be measured on.
pub const NO_FROZEN_BASELINE: &str = "NO_FROZEN_BASELINE";

/// This week's load (`planning::kpis`, the planning KPIs' definition): baseline hours of the
/// current frozen week of every planning site ÷ the crews' available hours, WITHOUT the overtime
/// ceiling — the overtime capacity is given beside it, and above 100 % the plan relies on overtime.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WeekLoad {
    /// Earliest first day and latest last day of the sites' current weeks (local days).
    pub week_start: String,
    pub week_end: String,
    /// Planning sites whose current week is frozen (measured).
    pub sites: i64,
    /// Planning sites whose current week is not frozen (not measured).
    pub sites_not_frozen: i64,
    pub baseline_hours: f64,
    pub available_hours: f64,
    pub overtime_hours: f64,
    /// baseline ÷ available × 100 (pooled over the sites); `None` when the crews have no hours.
    pub load_pct: Option<f64>,
    /// Above 100 %: the plan relies on overtime.
    pub relies_on_overtime: bool,
    /// Current weeks frozen without approval and not approved since.
    pub weeks_awaiting_approval: i64,
}

/// SMRP schedule compliance (`analytics::smrp::schedule_compliance`): baseline hours of the frozen
/// weeks ended in the period whose work was finished by the end of their week ÷ baseline hours
/// (`planning::kpis`, the same number as the planning KPIs).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScheduleComplianceFigure {
    pub period_days: i64,
    /// Baseline hours of the frozen weeks ended in the period.
    pub scheduled_hours: f64,
    pub done_hours: f64,
    /// Baseline jobs later released or cancelled (counted as not done).
    pub released_jobs: i64,
    /// `None` when no frozen week ended in the period (see `unavailable`).
    pub value_pct: Option<f64>,
    /// Why there is no value: [`NO_FROZEN_BASELINE`].
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HomePlanning {
    pub as_of: String,
    pub backlog: PlanningBacklog,
    /// `None` with `week_unavailable` set when no work week applies.
    pub week: Option<WeekLoad>,
    /// Why there is no week figure: [`WEEK_NO_PLANNING_SITE`] or [`WEEK_NOT_FROZEN`].
    pub week_unavailable: Option<String>,
    pub schedule_compliance: ScheduleComplianceFigure,
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

/// This week's load on the planning KPIs' definition, or the code saying why it cannot be given.
pub async fn this_week(db: &DatabaseConnection, now: DateTime<Utc>) -> AppResult<Result<WeekLoad, &'static str>> {
    let (weeks, sites) = crate::planning::kpis::current_weeks(db, now).await?;
    if sites == 0 {
        return Ok(Err(WEEK_NO_PLANNING_SITE));
    }
    if weeks.is_empty() {
        return Ok(Err(WEEK_NOT_FROZEN));
    }
    let baseline: f64 = weeks.iter().map(|w| w.baseline_hours).sum();
    let available: f64 = weeks.iter().map(|w| w.available_hours).sum();
    let overtime: f64 = weeks.iter().map(|w| w.overtime_hours).sum();
    let load_pct = (available > 0.0).then(|| ((baseline / available * 100.0) * 10.0).round() / 10.0);
    Ok(Ok(WeekLoad {
        week_start: weeks.iter().map(|w| w.week_start.clone()).min().unwrap_or_default(),
        week_end: weeks.iter().map(|w| w.week_end.clone()).max().unwrap_or_default(),
        sites: weeks.len() as i64,
        sites_not_frozen: sites.saturating_sub(weeks.len()) as i64,
        baseline_hours: baseline,
        available_hours: available,
        overtime_hours: overtime,
        load_pct,
        relies_on_overtime: load_pct.is_some_and(|p| p > 100.0),
        weeks_awaiting_approval: weeks.iter().filter(|w| w.awaiting_approval).count() as i64,
    }))
}

/// Schedule compliance over the period on the planning KPIs' definition (the analytics number).
pub async fn schedule_compliance(
    db: &DatabaseConnection,
    period_days: i64,
    now: DateTime<Utc>,
) -> AppResult<ScheduleComplianceFigure> {
    let window = period_window(now, period_days);
    let measured =
        crate::planning::kpis::baseline_schedule_compliance(db, window.from, window.to.min(now), None, now).await?;
    let ratio = smrp::SmrpRatio::of(measured.done_hours, measured.baseline_hours);
    Ok(ScheduleComplianceFigure {
        period_days,
        scheduled_hours: ratio.denominator,
        done_hours: ratio.numerator,
        released_jobs: measured.released_jobs,
        value_pct: ratio.value_pct,
        unavailable: (measured.weeks == 0).then(|| NO_FROZEN_BASELINE.to_string()),
    })
}

pub async fn build_planning(db: &DatabaseConnection, period_days: i64, now: DateTime<Utc>) -> AppResult<HomePlanning> {
    let (week, week_unavailable) = match this_week(db, now).await? {
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
