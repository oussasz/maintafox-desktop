//! A crew's site change never carries its committed jobs across a frozen week (audit finding
//! AU-129; CMMS Advisor decision 2026-10-27).
//!
//! A crew's jobs are planned on the weekly schedule of its planning site (`teams.primary_node_id`,
//! resolved by [`planning_site_of_node`]). Changing that site moves every committed job of the crew
//! from one site's schedule to the other's without any commitment row changing, so the frozen-week
//! four-eyes rule would be bypassed. The change is refused while a committed job of the crew that
//! has not ended:
//! - lies in a week of the site it leaves that is frozen (or due to freeze), or is frozen itself:
//!   frozen work cannot move;
//! - would lie in a week of the site it joins that is frozen (or due to freeze): work enters a
//!   frozen week only as a break-in.
//!
//! Jobs in open weeks move with the crew. A change that keeps the planning site (another node of
//! the same site) is never refused. The same rule judges a crew received by synchronisation
//! (`planning::sync_check::inbound_crew_site_check`).

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Local, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

use super::lifecycle::STATUS_COMMITTED;
use super::weekly_schedule::{frozen_week_at_site, planning_site_of_node};

/// Why a crew's site change is refused: the frozen weeks its committed jobs lie in (old or new
/// site, earliest first) and how many jobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewSiteBlock {
    pub weeks: Vec<String>,
    pub jobs: usize,
}

impl CrewSiteBlock {
    /// The planner-facing sentence (English; the coded issue carries `week_start`, `weeks` and
    /// `count`).
    pub fn message(&self) -> String {
        format!(
            "The crew's site cannot change: {} committed job(s) of the crew are in the frozen week(s) {} of the \
             site it leaves or joins. Move them with a break-in, or change the site once those weeks are over.",
            self.jobs,
            self.weeks.join(", ")
        )
    }

    /// The local refusal: `PLANNING_WEEK_FROZEN`, as planning refuses work entering a frozen week.
    pub fn refusal(&self) -> AppError {
        crate::errors::org_validation_failed(
            "PLANNING_WEEK_FROZEN",
            self.message(),
            HashMap::from([
                (
                    "week_start".to_string(),
                    self.weeks.first().cloned().unwrap_or_default(),
                ),
                ("weeks".to_string(), self.weeks.join(", ")),
                ("count".to_string(), self.jobs.to_string()),
            ]),
        )
    }
}

async fn node_site(db: &impl ConnectionTrait, node: Option<i64>) -> AppResult<Option<i64>> {
    match node {
        Some(node) => planning_site_of_node(db, node).await,
        None => Ok(None),
    }
}

/// Whether moving crew `team_id` to the node `new_node` (its planning site) is refused at `now`;
/// `None` when it may move. Reads the crew's current node from the database: call it before the
/// row is written, in the transaction that writes it.
pub async fn crew_site_change_block(
    db: &impl ConnectionTrait,
    team_id: i64,
    new_node: Option<i64>,
    now: DateTime<Utc>,
) -> AppResult<Option<CrewSiteBlock>> {
    let current_node: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT primary_node_id FROM teams WHERE id = ?",
            [team_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "primary_node_id").ok().flatten());
    let old_site = node_site(db, current_node).await?;
    let new_site = node_site(db, new_node).await?;
    if old_site == new_site {
        return Ok(None);
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT committed_start, committed_end, frozen_at FROM schedule_commitments \
              WHERE assigned_team_id = ? AND status = ? ORDER BY committed_start, id",
            [team_id.into(), STATUS_COMMITTED.into()],
        ))
        .await?;
    let mut weeks = BTreeSet::new();
    let mut jobs = 0;
    for row in rows {
        let decode =
            |column: &str, e: sea_orm::DbErr| AppError::Internal(anyhow::anyhow!("schedule_commitments.{column}: {e}"));
        let start: String = row
            .try_get("", "committed_start")
            .map_err(|e| decode("committed_start", e))?;
        let end: String = row
            .try_get("", "committed_end")
            .map_err(|e| decode("committed_end", e))?;
        let frozen_at: Option<String> = row.try_get("", "frozen_at").map_err(|e| decode("frozen_at", e))?;
        // Ended work leaves no week to protect (an unreadable end is judged, never skipped).
        if crate::wo::shifts::planned_instant(&end).is_some_and(|e| e <= now) {
            continue;
        }
        let mut week = None;
        if let Some(site) = old_site {
            week = frozen_week_at_site(db, site, &start, &end, now)
                .await?
                .map(|w| w.week_start);
        }
        if week.is_none() && frozen_at.is_some() {
            // Frozen with no frozen week found at the site it leaves: named by its first day.
            week = Some(
                crate::wo::shifts::planned_instant(&start)
                    .map_or_else(|| start.clone(), |s| s.with_timezone(&Local).date_naive().to_string()),
            );
        }
        if week.is_none() {
            if let Some(site) = new_site {
                week = frozen_week_at_site(db, site, &start, &end, now)
                    .await?
                    .map(|w| w.week_start);
            }
        }
        if let Some(week) = week {
            weeks.insert(week);
            jobs += 1;
        }
    }
    Ok((jobs > 0).then(|| CrewSiteBlock {
        weeks: weeks.into_iter().collect(),
        jobs,
    }))
}
