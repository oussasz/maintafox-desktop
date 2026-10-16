//! Planning background loop (planning step D): every minute, closes the commitments whose work is
//! finished and freezes, for each planning site, the next week once the current week's last
//! working day is over. Errors are logged; the loop carries on (each tick starts again from the
//! database state).

use sea_orm::DatabaseConnection;
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::AppResult;

pub async fn start_planning_scheduler(db: DatabaseConnection) {
    let mut ticker = interval(TokioDuration::from_secs(60));
    loop {
        ticker.tick().await;
        if let Err(err) = run_planning_tick(&db, chrono::Utc::now()).await {
            tracing::error!(error = %err, "planning::scheduler tick failed");
        }
    }
}

/// One tick: close finished commitments, then freeze the weeks that are due.
pub async fn run_planning_tick(db: &DatabaseConnection, now: chrono::DateTime<chrono::Utc>) -> AppResult<()> {
    let closed = super::lifecycle::close_finished_commitments(db).await?;
    if closed > 0 {
        tracing::info!(closed, "planning: commitments of finished work closed");
    }
    for (site, week_start) in super::weekly_schedule::auto_freeze_due(db, now).await? {
        tracing::info!(site, %week_start, "planning: week frozen automatically");
    }
    Ok(())
}
