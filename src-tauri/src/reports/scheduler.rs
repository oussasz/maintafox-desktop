use chrono::{DateTime, Duration, Utc};
use cron::Schedule;
use sea_orm::DatabaseConnection;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::time::{interval, Duration as TokioDuration};

use crate::errors::{AppError, AppResult};
use crate::reports::artifacts::persist_run_artifact;
use crate::reports::export::{export_report_document, ReportAudience, ReportLocale};
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

        // Scheduled runs have no UI: the report follows the app language, and shows what the
        // subscriber may see today (a demoted subscriber no longer receives those figures).
        let locale = ReportLocale::resolve(db, None).await;
        let doc = match subscriber_audience(db, s.user_id).await {
            Ok(audience) => export_report_document(db, &template.code, &s.export_format, locale, audience).await,
            Err(e) => Err(e),
        };
        let doc = match doc {
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

        if let Err(e) =
            persist_run_artifact(db, app_data_dir, run_id, &s.export_format, &doc.file_name, &doc.bytes).await
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

/// The subscriber's rights read from the database at run time (no session, no cache).
pub(crate) async fn subscriber_audience(db: &DatabaseConnection, user_id: i64) -> AppResult<ReportAudience> {
    let uid = i32::try_from(user_id)
        .map_err(|_| AppError::ValidationFailed(vec![format!("report subscriber id out of range: {user_id}")]))?;
    ReportAudience::of_user(db, None, uid).await
}

#[cfg(test)]
mod tests {
    use super::subscriber_audience;
    use crate::auth::scope_tests::{assign, exec, role, user};
    use crate::rbac::permissions::{FIN_VIEW, OT_VIEW, PM_VIEW, REP_EXPORT};

    #[tokio::test]
    async fn a_scheduled_run_follows_the_subscribers_current_rights() {
        let db = crate::db::test_db::seeded().await;
        let subscriber = user(&db, "report.subscriber").await;
        let r = role(&db, "monthly readers", &[REP_EXPORT, OT_VIEW, PM_VIEW, FIN_VIEW]).await;
        assign(&db, subscriber, r, "tenant", None).await;

        let before = subscriber_audience(&db, i64::from(subscriber)).await.expect("audience");
        assert!(before.can_ot && before.can_pm && before.can_fin);

        // Demoted after subscribing: the next run no longer carries cost figures.
        exec(
            &db,
            &format!(
                "DELETE FROM role_permissions WHERE role_id = {r} \
                 AND permission_id = (SELECT id FROM permissions WHERE name = '{FIN_VIEW}')"
            ),
        )
        .await;
        let after = subscriber_audience(&db, i64::from(subscriber)).await.expect("audience");
        assert!(after.can_ot && after.can_pm && !after.can_fin);
        let data =
            crate::reports::monthly::gather(&db, crate::reports::export::ReportLocale::En, chrono::Utc::now(), after)
                .await
                .expect("gather");
        assert!(data.cost_centres.is_none() && data.smrp.cost_to_rav.is_none());
    }
}
