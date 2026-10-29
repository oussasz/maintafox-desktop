use chrono::Utc;
use sea_orm::TryGetable;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::reports::domain::{ReportRun, ReportSchedule, ReportTemplate, UpsertReportScheduleInput};

/// `COUNT(*) AS cnt`: a failed query, a missing row or an unreadable count is an error, never 0.
async fn count_scalar(db: &DatabaseConnection, sql: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("report count returned no row")))?;
    i64::try_get(&row, "", "cnt").map_err(|e| AppError::Internal(anyhow::anyhow!("report count decode: {e:?}")))
}

/// Report templates added after the original seed (migration 096). Inserted once per database
/// with the deterministic sync identity every device computes for the same code
/// (`sync::seeded_identity`: `report_templates` keyed by `code`), so replicas agree.
const BUILTIN_TEMPLATES: &[(&str, &str, &str, &str)] = &[(
    crate::reports::monthly::TEMPLATE_CODE,
    "Monthly maintenance report",
    "Last complete month: availability, MTBF/MTTR, SMRP indicators, top bad actors, cost by cost centre and by work-order type.",
    "pdf",
)];

pub async fn ensure_builtin_templates(db: &DatabaseConnection) -> AppResult<()> {
    for (code, title, description, format) in BUILTIN_TEMPLATES {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO report_templates                (entity_sync_id, code, title, description, default_format, spec_json, is_active, created_at)              VALUES (?, ?, ?, ?, ?, '{}', 1, ?)",
            [
                crate::sync::identity::seeded_entity_sync_id("report_templates", code).into(),
                (*code).into(),
                (*title).into(),
                (*description).into(),
                (*format).into(),
                Utc::now().to_rfc3339().into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

pub async fn list_report_templates(db: &DatabaseConnection) -> AppResult<Vec<ReportTemplate>> {
    ensure_builtin_templates(db).await?;
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, title, description, default_format, spec_json, is_active \
             FROM report_templates WHERE is_active = 1 ORDER BY id"
                .to_string(),
        ))
        .await?;
    rows.iter().map(map_template).collect()
}

fn map_template(row: &sea_orm::QueryResult) -> AppResult<ReportTemplate> {
    Ok(ReportTemplate {
        id: row
            .try_get("", "id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?,
        code: row
            .try_get("", "code")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("code: {e}"))))?,
        title: row
            .try_get("", "title")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("title: {e}"))))?,
        description: row
            .try_get("", "description")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("description: {e}"))))?,
        default_format: row
            .try_get("", "default_format")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("default_format: {e}"))))?,
        spec_json: row
            .try_get("", "spec_json")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("spec_json: {e}"))))?,
        is_active: row
            .try_get::<i64>("", "is_active")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("is_active: {e}"))))?
            != 0,
    })
}

pub async fn get_template_by_id(db: &DatabaseConnection, id: i64) -> AppResult<Option<ReportTemplate>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, title, description, default_format, spec_json, is_active FROM report_templates WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.map(|r| map_template(&r)).transpose()
}

pub async fn get_template_by_code(db: &DatabaseConnection, code: &str) -> AppResult<Option<ReportTemplate>> {
    ensure_builtin_templates(db).await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, title, description, default_format, spec_json, is_active FROM report_templates WHERE code = ?",
            [code.into()],
        ))
        .await?;
    row.map(|r| map_template(&r)).transpose()
}

pub async fn list_my_schedules(db: &DatabaseConnection, user_id: i64) -> AppResult<Vec<ReportSchedule>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, user_id, template_id, cron_expr, export_format, enabled, next_run_at, last_run_at \
             FROM report_schedules WHERE user_id = ? ORDER BY id DESC",
            [user_id.into()],
        ))
        .await?;
    rows.iter().map(map_schedule).collect()
}

fn map_schedule(row: &sea_orm::QueryResult) -> AppResult<ReportSchedule> {
    Ok(ReportSchedule {
        id: row
            .try_get("", "id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?,
        user_id: row
            .try_get("", "user_id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("user_id: {e}"))))?,
        template_id: row
            .try_get("", "template_id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("template_id: {e}"))))?,
        cron_expr: row
            .try_get("", "cron_expr")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("cron_expr: {e}"))))?,
        export_format: row
            .try_get("", "export_format")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("export_format: {e}"))))?,
        enabled: row
            .try_get::<i64>("", "enabled")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("enabled: {e}"))))?
            != 0,
        next_run_at: row
            .try_get("", "next_run_at")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("next_run_at: {e}"))))?,
        last_run_at: row.try_get("", "last_run_at").ok(),
    })
}

pub async fn get_schedule(db: &DatabaseConnection, id: i64, user_id: i64) -> AppResult<Option<ReportSchedule>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, user_id, template_id, cron_expr, export_format, enabled, next_run_at, last_run_at \
             FROM report_schedules WHERE id = ? AND user_id = ?",
            [id.into(), user_id.into()],
        ))
        .await?;
    row.map(|r| map_schedule(&r)).transpose()
}

pub async fn list_my_runs(db: &DatabaseConnection, user_id: i64, limit: i64) -> AppResult<Vec<ReportRun>> {
    let lim = limit.clamp(1, 200);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT r.id, r.schedule_id, r.template_id, r.user_id, r.status, r.export_format, r.artifact_path, \
             r.byte_size, r.error_message, r.started_at, r.finished_at, \
             CASE WHEN a.id IS NOT NULL OR (r.artifact_path IS NOT NULL AND length(trim(r.artifact_path)) > 0) THEN 1 ELSE 0 END AS has_downloadable_artifact \
             FROM report_runs r \
             LEFT JOIN report_artifacts a ON a.run_id = r.id \
             WHERE r.user_id = ? ORDER BY r.id DESC LIMIT ?",
            [user_id.into(), lim.into()],
        ))
        .await?;
    rows.iter().map(map_run).collect()
}

fn map_run(row: &sea_orm::QueryResult) -> AppResult<ReportRun> {
    let has_flag: i64 = row.try_get("", "has_downloadable_artifact").unwrap_or(0);
    Ok(ReportRun {
        id: row
            .try_get("", "id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?,
        schedule_id: row.try_get("", "schedule_id").ok(),
        template_id: row
            .try_get("", "template_id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("template_id: {e}"))))?,
        user_id: row
            .try_get("", "user_id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("user_id: {e}"))))?,
        status: row
            .try_get("", "status")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("status: {e}"))))?,
        export_format: row
            .try_get("", "export_format")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("export_format: {e}"))))?,
        artifact_path: row.try_get("", "artifact_path").ok(),
        byte_size: row.try_get("", "byte_size").ok(),
        error_message: row.try_get("", "error_message").ok(),
        started_at: row
            .try_get("", "started_at")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("started_at: {e}"))))?,
        finished_at: row.try_get("", "finished_at").ok(),
        has_downloadable_artifact: has_flag != 0,
    })
}

pub async fn insert_run(
    db: &DatabaseConnection,
    schedule_id: Option<i64>,
    template_id: i64,
    user_id: i64,
    status: &str,
    export_format: &str,
) -> AppResult<i64> {
    let now = Utc::now().to_rfc3339();
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO report_runs (schedule_id, template_id, user_id, status, export_format, started_at)
         VALUES (?, ?, ?, ?, ?, ?)",
            [
                schedule_id.into(),
                template_id.into(),
                user_id.into(),
                status.into(),
                export_format.into(),
                now.clone().into(),
            ],
        ))
        .await?;
    crate::db::inserted_id(&inserted)
}

pub async fn finish_run_success(
    db: &DatabaseConnection,
    run_id: i64,
    artifact_path: &str,
    byte_size: i64,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE report_runs SET status = 'success', artifact_path = ?, byte_size = ?, finished_at = ? WHERE id = ?",
        [artifact_path.into(), byte_size.into(), now.into(), run_id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn finish_run_failed(db: &DatabaseConnection, run_id: i64, msg: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE report_runs SET status = 'failed', error_message = ?, finished_at = ? WHERE id = ?",
        [msg.into(), now.into(), run_id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn update_schedule_times(
    db: &DatabaseConnection,
    schedule_id: i64,
    last_run_at: &str,
    next_run_at: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE report_schedules SET last_run_at = ?, next_run_at = ?, updated_at = ? WHERE id = ?",
        [last_run_at.into(), next_run_at.into(), now.into(), schedule_id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn list_enabled_schedules(db: &DatabaseConnection) -> AppResult<Vec<ReportSchedule>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, user_id, template_id, cron_expr, export_format, enabled, next_run_at, last_run_at \
             FROM report_schedules WHERE enabled = 1 ORDER BY id"
                .to_string(),
        ))
        .await?;
    rows.iter().map(map_schedule).collect()
}

pub async fn upsert_schedule(
    db: &DatabaseConnection,
    user_id: i64,
    input: &UpsertReportScheduleInput,
    next_run_at: &str,
) -> AppResult<i64> {
    let ts = Utc::now().to_rfc3339();
    let en = if input.enabled { 1 } else { 0 };
    if let Some(id) = input.id {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE report_schedules SET template_id = ?, cron_expr = ?, export_format = ?, enabled = ?, next_run_at = ?, updated_at = ? \
             WHERE id = ? AND user_id = ?",
            [
                input.template_id.into(),
                input.cron_expr.clone().into(),
                input.export_format.clone().into(),
                en.into(),
                next_run_at.into(),
                ts.clone().into(),
                id.into(),
                user_id.into(),
            ],
        ))
        .await?;
        Ok(id)
    } else {
        let inserted = db
            .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO report_schedules (user_id, template_id, cron_expr, export_format, enabled, next_run_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            [
                user_id.into(),
                input.template_id.into(),
                input.cron_expr.clone().into(),
                input.export_format.clone().into(),
                en.into(),
                next_run_at.into(),
                ts.clone().into(),
                ts.into(),
            ],
        ))
        .await?;
        crate::db::inserted_id(&inserted)
    }
}

pub async fn delete_schedule(db: &DatabaseConnection, user_id: i64, schedule_id: i64) -> AppResult<u64> {
    let r = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM report_schedules WHERE id = ? AND user_id = ?",
            [schedule_id.into(), user_id.into()],
        ))
        .await?;
    Ok(r.rows_affected())
}

/// Figures of the dashboard summary report (labels are localised by `reports::export`).
#[derive(Debug, Clone, PartialEq)]
pub struct DashboardSummaryFigures {
    pub open_di: i64,
    /// OTs "not yet closed" (`wo::backlog::NOT_CLOSED_SQL`, rule `RULE_NOT_YET_CLOSED`); the
    /// report row is labelled with that rule's name.
    pub open_wo: i64,
    pub assets: i64,
    /// Assets with a reliability snapshot over the last 365 days.
    pub assets_with_snapshots: i64,
    /// Mean data-quality score, 0..=1.
    pub data_quality: Option<f64>,
    /// Pooled fleet MTBF in hours.
    pub mtbf_hours: Option<f64>,
}

/// KPI-style counts for dashboard summary report.
pub async fn fetch_dashboard_summary(db: &DatabaseConnection) -> AppResult<DashboardSummaryFigures> {
    // Open requests: the DI page's / home page's one rule (`di::stats::OPEN_SQL`).
    let open_di = count_scalar(
        db,
        &format!(
            "SELECT COUNT(*) AS cnt FROM intervention_requests ir WHERE {}",
            crate::di::stats::OPEN_SQL
        ),
    )
    .await?;
    let open_wo = count_scalar(
        db,
        &format!(
            "SELECT COUNT(*) AS cnt FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id WHERE {}",
            crate::wo::backlog::NOT_CLOSED_SQL
        ),
    )
    .await?;
    let assets = count_scalar(db, "SELECT COUNT(*) AS cnt FROM equipment WHERE deleted_at IS NULL").await?;
    // Pooled fleet ratios over the latest snapshot per equipment (never mean of ratios).
    let fleet = crate::reliability::fleet::pooled_fleet_kpis(
        db,
        crate::reliability::fleet::FleetWindow {
            since_days: 365,
            until_days: None,
        },
        None,
        crate::reliability::readiness::DQ_GREEN_THRESHOLD,
    )
    .await?;
    Ok(DashboardSummaryFigures {
        open_di,
        open_wo,
        assets,
        assets_with_snapshots: fleet.equipment_count,
        data_quality: fleet.mean_data_quality_score,
        mtbf_hours: fleet.mtbf_hours,
    })
}

pub async fn fetch_open_wo_by_status(db: &DatabaseConnection) -> AppResult<Vec<(String, i64)>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT s.code AS st, COUNT(*) AS cnt FROM work_orders wo \
                 JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE {} GROUP BY s.code ORDER BY cnt DESC",
                crate::wo::backlog::NOT_CLOSED_SQL
            ),
        ))
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let st: String = r
            .try_get("", "st")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("st: {e}"))))?;
        let cnt: i64 = r
            .try_get("", "cnt")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("cnt: {e}"))))?;
        out.push((st, cnt));
    }
    Ok(out)
}
