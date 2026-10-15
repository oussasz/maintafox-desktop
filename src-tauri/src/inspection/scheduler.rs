//! Background generation of inspection rounds.
//!
//! Without it, rounds were generated (and overdue rounds marked missed) only when a schedule
//! was saved or someone clicked "Generate due rounds". Generation is idempotent (one round per
//! schedule occurrence), so running it on a timer and at start-up only adds what is due.

use sea_orm::DatabaseConnection;
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::AppResult;
use crate::inspection::domain::{GenerateInspectionRoundsInput, GenerateInspectionRoundsResult};
use crate::inspection::scheduling::generate_inspection_rounds;

/// How often due rounds are generated and missed rounds marked. A round's completion window is
/// at least one hour, so a quarter-hour tick marks a missed round within 15 minutes of its end.
pub const INSPECTION_ROUND_TICK_SECS: u64 = 15 * 60;

/// Runs one pass at start-up, then every [`INSPECTION_ROUND_TICK_SECS`].
pub async fn start_inspection_round_scheduler(db: DatabaseConnection) {
    let mut ticker = interval(TokioDuration::from_secs(INSPECTION_ROUND_TICK_SECS));
    loop {
        ticker.tick().await;
        match run_inspection_round_pass(&db).await {
            Ok(result) => {
                if result.generated > 0 || result.missed > 0 || !result.issues.is_empty() {
                    tracing::info!(
                        generated = result.generated,
                        missed = result.missed,
                        issues = result.issues.len(),
                        "inspection::scheduler pass"
                    );
                }
            }
            Err(err) => tracing::error!(error = %err, "inspection::scheduler pass failed"),
        }
    }
}

/// One generation pass over every active schedule, as of now.
pub async fn run_inspection_round_pass(db: &DatabaseConnection) -> AppResult<GenerateInspectionRoundsResult> {
    generate_inspection_rounds(
        db,
        GenerateInspectionRoundsInput {
            as_of: None,
            schedule_id: None,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_pass_without_schedules_generates_nothing_and_does_not_fail() {
        let db = crate::db::test_db::seeded().await;
        let result = run_inspection_round_pass(&db).await.expect("pass");
        assert_eq!((result.generated, result.missed), (0, 0));
        assert!(result.issues.is_empty());
        // Idempotent: a second pass right after is identical.
        let again = run_inspection_round_pass(&db).await.expect("second pass");
        assert_eq!((again.generated, again.missed), (0, 0));
    }
}
