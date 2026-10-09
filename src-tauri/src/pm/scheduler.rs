//! Desktop PM occurrence generation scheduler (Phase B auto-pilot).

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::AppResult;
use crate::pm::domain::GeneratePmOccurrencesInput;
use crate::pm::generation_control::{
    get_pm_generation_control, run_generation_with_audit,
};

pub async fn start_pm_generation_scheduler(db: DatabaseConnection) {
    let mut ticker = interval(TokioDuration::from_secs(60));
    loop {
        ticker.tick().await;
        if let Err(err) = run_due_pm_generation(&db).await {
            tracing::error!(error = %err, "pm::scheduler tick failed");
        }
    }
}

pub async fn run_due_pm_generation(db: &DatabaseConnection) -> AppResult<()> {
    let control = get_pm_generation_control(db).await?;
    if !control.enabled {
        return Ok(());
    }
    let Some(next_run_at) = control.next_run_at.as_deref() else {
        return Ok(());
    };
    let due = DateTime::parse_from_rfc3339(next_run_at)
        .map(|d| d.with_timezone(&Utc) <= Utc::now())
        .unwrap_or(false);
    if !due {
        return Ok(());
    }

    let _ = run_generation_with_audit(
        db,
        GeneratePmOccurrencesInput {
            as_of: None,
            horizon_days: Some(control.horizon_days.clamp(1, 365)),
            pm_plan_id: None,
            equipment_id: None,
            event_codes: None,
            condition_codes: None,
        },
        "schedule",
        true,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
pub async fn run_due_pm_generation_for_test(db: &DatabaseConnection) -> AppResult<()> {
    run_due_pm_generation(db).await
}
