use chrono::{DateTime, Duration, Utc};
use cron::Schedule;
use sea_orm::DatabaseConnection;
use std::str::FromStr;
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::AppResult;
use crate::reports::export::export_report_document;
use crate::reports::queries::{
    finish_run_failed, finish_run_success, get_template_by_id, insert_run, list_enabled_schedules,
    update_schedule_times,
};

pub fn next_run_rfc3339(schedule: &Schedule) -> String {
    let now = Utc::now();
    schedule
        .upcoming(Utc)
        .next()
        .unwrap_or(now + Duration::hours(1))
        .to_rfc3339()
}

fn reports_artifact_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("maintafox").join("reports")
}

pub async fn start_report_scheduler(db: DatabaseConnection) {
    let mut ticker = interval(TokioDuration::from_secs(60));
    loop {
        ticker.tick().await;
        if let Err(err) = run_due_schedules(&db).await {
            tracing::error!(error = %err, "reports::scheduler tick failed");
        }
    }
}

async fn run_due_schedules(db: &DatabaseConnection) -> AppResult<()> {
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
        let run_id = insert_run(
            db,
            Some(s.id),
            s.template_id,
            s.user_id,
            "running",
            &s.export_format,
        )
        .await?;

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

        let dir = reports_artifact_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            let _ = finish_run_failed(db, run_id, "failed to create reports directory").await;
            continue;
        }
        let ext = if s.export_format.eq_ignore_ascii_case("xlsx") {
            "xlsx"
        } else {
            "pdf"
        };
        let name = format!("scheduled-{}-{}.{ext}", s.id, Utc::now().timestamp_millis());
        let path = dir.join(&name);
        if std::fs::write(&path, &doc.bytes).is_err() {
            let _ = finish_run_failed(db, run_id, "failed to write artifact").await;
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        let sz = doc.bytes.len() as i64;
        finish_run_success(db, run_id, &path_str, sz).await?;

        let cron = Schedule::from_str(s.cron_expr.trim())
            .unwrap_or_else(|_| Schedule::from_str("0 0 8 * * *").expect("default cron"));
        let next = next_run_rfc3339(&cron);
        let ts = Utc::now().to_rfc3339();
        update_schedule_times(db, s.id, &ts, &next).await?;
    }
    Ok(())
}
