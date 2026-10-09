use chrono::{DateTime, Duration, Utc};
use cron::Schedule;
use sea_orm::DatabaseConnection;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::AppResult;
use crate::reports::artifacts::persist_run_artifact;
use crate::reports::export::export_report_document;
use crate::reports::queries::{
    finish_run_failed, get_template_by_id, insert_run, list_enabled_schedules, update_schedule_times,
};

pub fn next_run_rfc3339(schedule: &Schedule) -> String {
    let now = Utc::now();
    schedule
        .upcoming(Utc)
        .next()
        .unwrap_or(now + Duration::hours(1))
        .to_rfc3339()
}

pub async fn start_report_scheduler(db: DatabaseConnection, app_data_dir: PathBuf) {
    let mut ticker = interval(TokioDuration::from_secs(60));
    loop {
        ticker.tick().await;
        if let Err(err) = run_due_schedules(&db, &app_data_dir).await {
            tracing::error!(error = %err, "reports::scheduler tick failed");
        }
    }
}

async fn run_due_schedules(db: &DatabaseConnection, app_data_dir: &std::path::Path) -> AppResult<()> {
    let now = Utc::now();
    let all = list_enabled_schedules(db).await?;
    let due: Vec<_> = all
        .into_iter()
        .filter(|s| {
            DateTime::parse_from_rfc3339(&s.next_run_at)
                .map(|d| d.with_timezone(&Utc) <= now)
                .unwrap_or(false)
        })
        .collect();
    for s in due {
        let template = match get_template_by_id(db, s.template_id).await? {
            Some(t) if t.is_active => t,
            _ => continue,
        };
        let run_id = insert_run(db, Some(s.id), s.template_id, s.user_id, "running", &s.export_format).await?;

        let doc = match export_report_document(db, &template.code, &s.export_format).await {
            Ok(d) => d,
            Err(e) => {
                let _ = finish_run_failed(db, run_id, &e.to_string()).await;
                let cron = Schedule::from_str(s.cron_expr.trim())
                    .unwrap_or_else(|_| Schedule::from_str("0 0 8 * * *").expect("default cron"));
                let next = next_run_rfc3339(&cron);
                let ts = Utc::now().to_rfc3339();
                let _ = update_schedule_times(db, s.id, &ts, &next).await;
                continue;
            }
        };

        if let Err(e) = persist_run_artifact(
            db,
            app_data_dir,
            run_id,
            &s.export_format,
            &doc.file_name,
            &doc.bytes,
        )
        .await
        {
            let _ = finish_run_failed(db, run_id, &e.to_string()).await;
            let cron = Schedule::from_str(s.cron_expr.trim())
                .unwrap_or_else(|_| Schedule::from_str("0 0 8 * * *").expect("default cron"));
            let next = next_run_rfc3339(&cron);
            let ts = Utc::now().to_rfc3339();
            let _ = update_schedule_times(db, s.id, &ts, &next).await;
            continue;
        }

        let cron = Schedule::from_str(s.cron_expr.trim())
            .unwrap_or_else(|_| Schedule::from_str("0 0 8 * * *").expect("default cron"));
        let next = next_run_rfc3339(&cron);
        let ts = Utc::now().to_rfc3339();
        update_schedule_times(db, s.id, &ts, &next).await?;
    }
    Ok(())
}
