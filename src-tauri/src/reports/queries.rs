use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use sea_orm::TryGetable;

use crate::errors::{AppError, AppResult};
use crate::reports::domain::{ReportRun, ReportSchedule, ReportTemplate, UpsertReportScheduleInput};

async fn count_scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    let result = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await;
    match result {
        Ok(Some(row)) => i64::try_get(&row, "", "cnt").unwrap_or(0),
        _ => 0,
    }
}

pub async fn list_report_templates(db: &DatabaseConnection) -> AppResult<Vec<ReportTemplate>> {
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
        id: row.try_get("", "id").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?,
        code: row.try_get("", "code").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("code: {e}"))))?,
        title: row.try_get("", "title").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("title: {e}"))))?,
        description: row
            .try_get("", "description")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("description: {e}"))))?,
        default_format: row
            .try_get("", "default_format")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("default_format: {e}"))))?,
        spec_json: row
            .try_get("", "spec_json")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("spec_json: {e}"))))?,
        is_active: row.try_get::<i64>("", "is_active").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("is_active: {e}"))))? != 0,
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
        id: row.try_get("", "id").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?,
        user_id: row.try_get("", "user_id").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("user_id: {e}"))))?,
        template_id: row
            .try_get("", "template_id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("template_id: {e}"))))?,
        cron_expr: row
            .try_get("", "cron_expr")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("cron_expr: {e}"))))?,
        export_format: row
            .try_get("", "export_format")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("export_format: {e}"))))?,
        enabled: row.try_get::<i64>("", "enabled").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("enabled: {e}"))))? != 0,
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
            "SELECT id, schedule_id, template_id, user_id, status, export_format, artifact_path, byte_size, \
             error_message, started_at, finished_at \
             FROM report_runs WHERE user_id = ? ORDER BY id DESC LIMIT ?",
            [user_id.into(), lim.into()],
        ))
        .await?;
    rows.iter().map(map_run).collect()
}

fn map_run(row: &sea_orm::QueryResult) -> AppResult<ReportRun> {
    Ok(ReportRun {
        id: row.try_get("", "id").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?,
        schedule_id: row.try_get("", "schedule_id").ok(),
        template_id: row
            .try_get("", "template_id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("template_id: {e}"))))?,
        user_id: row.try_get("", "user_id").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("user_id: {e}"))))?,
        status: row.try_get("", "status").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("status: {e}"))))?,
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
    db.execute(Statement::from_sql_and_values(
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
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?;
    let id: i64 = row
        .as_ref()
        .ok_or_else(|| AppError::Database(sea_orm::DbErr::Custom("last_insert_rowid".into())))?
        .try_get("", "id")
        .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))?;
    Ok(id)
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
        db.execute(Statement::from_sql_and_values(
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
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?;
        row.as_ref()
            .ok_or_else(|| AppError::Database(sea_orm::DbErr::Custom("last_insert_rowid".into())))?
            .try_get("", "id")
            .map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("id: {e}"))))
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

/// KPI-style counts for dashboard summary report.
pub async fn fetch_dashboard_summary_pairs(db: &DatabaseConnection) -> AppResult<Vec<(String, String)>> {
    let open_di = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM intervention_requests \
         WHERE status NOT IN ('rejected','converted_to_work_order','closed_as_non_executable','archived')",
    )
    .await;
    let open_wo = count_scalar(
        db,
        "SELECT COUNT(*) AS cnt FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id WHERE s.code NOT IN ('closed','cancelled')",
    )
    .await;
    let assets = count_scalar(db, "SELECT COUNT(*) AS cnt FROM equipment WHERE deleted_at IS NULL").await;
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            r"SELECT COUNT(*) AS snapshot_count,
                     AVG(data_quality_score) AS avg_dq,
                     AVG(CASE WHEN mtbf IS NOT NULL AND mtbf > 0 THEN mtbf END) AS avg_mtbf
              FROM reliability_kpi_snapshots
              WHERE datetime(period_end) >= datetime('now', '-365 days')"
                .to_string(),
        ))
        .await?;
    let (snap_n, dq, mtbf) = if let Some(r) = row {
        let n: i64 = i64::try_get(&r, "", "snapshot_count").unwrap_or(0);
        let dq: Option<f64> = f64::try_get(&r, "", "avg_dq").ok();
        let m: Option<f64> = f64::try_get(&r, "", "avg_mtbf").ok();
        (n, dq, m)
    } else {
        (0, None, None)
    };

    let mut out = vec![
        ("Open DIs".to_string(), format!("{open_di}")),
        ("Open WOs".to_string(), format!("{open_wo}")),
        ("Assets".to_string(), format!("{assets}")),
        ("Reliability snapshots (365d)".to_string(), format!("{snap_n}")),
    ];
    if let Some(v) = dq {
        out.push(("Avg data quality (365d)".to_string(), format!("{v:.4}")));
    } else {
        out.push(("Avg data quality (365d)".to_string(), "—".to_string()));
    }
    if let Some(v) = mtbf {
        out.push(("Avg MTBF hours (365d)".to_string(), format!("{v:.2}")));
    } else {
        out.push(("Avg MTBF hours (365d)".to_string(), "—".to_string()));
    }
    Ok(out)
}

pub async fn fetch_open_wo_by_status(db: &DatabaseConnection) -> AppResult<Vec<(String, i64)>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT s.code AS st, COUNT(*) AS cnt FROM work_orders wo \
             JOIN work_order_statuses s ON s.id = wo.status_id \
             WHERE s.code NOT IN ('closed','cancelled') GROUP BY s.code ORDER BY cnt DESC"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::new();
    for r in rows {
        let st: String = r.try_get("", "st").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("st: {e}"))))?;
        let cnt: i64 = r.try_get("", "cnt").map_err(|e| AppError::Database(sea_orm::DbErr::Custom(format!("cnt: {e}"))))?;
        out.push((st, cnt));
    }
    Ok(out)
}
