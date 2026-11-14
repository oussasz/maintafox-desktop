//! Planning background loop (planning step D): every minute, closes the commitments whose work is
//! finished, freezes for each planning site the weeks that are due, and recalls the weeks frozen
//! without approval. Errors are logged; the loop carries on (each tick starts again from the
//! database state).
//!
//! These act on data every device shares. A device that synchronises waits until it has synced
//! once since it started, so it does not freeze, close or notify from a stale copy — a week
//! another device already froze, approved or unfroze while this one was off. A device that does not
//! synchronise (sync not configured, not licensed, not activated) acts at once.

use chrono::{DateTime, SubsecRound, Utc};
use sea_orm::DatabaseConnection;
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::AppResult;
use crate::sync::transport::worker::{
    SyncWorkerHandle, SyncWorkerStatus, BLOCKER_NOT_ACTIVATED, BLOCKER_NOT_CONFIGURED, BLOCKER_NOT_LICENSED,
};

pub async fn start_planning_scheduler(db: DatabaseConnection, sync: SyncWorkerHandle) {
    let started_at = crate::clock::now();
    let mut waiting_logged = false;
    let mut ticker = interval(TokioDuration::from_secs(60));
    loop {
        ticker.tick().await;
        if !shared_actions_allowed(&sync.status(), started_at) {
            if !waiting_logged {
                tracing::info!("planning: automatic freeze and reminders wait for the first synchronisation");
                waiting_logged = true;
            }
            continue;
        }
        if let Err(err) = run_planning_tick(&db, crate::clock::now()).await {
            tracing::error!(error = %err, "planning::scheduler tick failed");
        }
    }
}

/// Whether this device may act on shared planning data on its own: it does not synchronise, or
/// it completed a synchronisation since it started (`started_at`).
pub fn shared_actions_allowed(status: &SyncWorkerStatus, started_at: DateTime<Utc>) -> bool {
    if status
        .blocker
        .as_deref()
        .is_some_and(|b| [BLOCKER_NOT_CONFIGURED, BLOCKER_NOT_LICENSED, BLOCKER_NOT_ACTIVATED].contains(&b))
    {
        return true;
    }
    status
        .last_success_at
        .as_deref()
        .and_then(crate::notifications::watchers::parse_time)
        .is_some_and(|synced| synced >= started_at.trunc_subsecs(0))
}

/// One tick: close finished commitments, freeze the weeks that are due, recall the weeks frozen
/// without approval.
pub async fn run_planning_tick(db: &DatabaseConnection, now: DateTime<Utc>) -> AppResult<()> {
    let closed = super::lifecycle::close_finished_commitments(db).await?;
    if closed > 0 {
        tracing::info!(closed, "planning: commitments of finished work closed");
    }
    for (site, week_start) in super::weekly_schedule::auto_freeze_due(db, now).await? {
        tracing::info!(site, %week_start, "planning: week frozen automatically");
    }
    let recalled = super::weekly_schedule::remind_unapproved_weeks(db, now).await?;
    if recalled > 0 {
        tracing::info!(recalled, "planning: weeks frozen without approval notified");
    }
    Ok(())
}
