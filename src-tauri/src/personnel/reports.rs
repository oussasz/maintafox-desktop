//! Workforce reports and CSV exports (PRD §6.6).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

fn map_csv_err(err: csv::Error) -> AppError {
    AppError::Internal(anyhow::anyhow!("CSV export error: {err}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkforceSummaryRow {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkforceSummaryReport {
    pub total_personnel: i64,
    pub active_personnel: i64,
    pub employment_breakdown: Vec<WorkforceSummaryRow>,
    pub availability_breakdown: Vec<WorkforceSummaryRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkforceSkillsGapRow {
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub position_name: Option<String>,
    pub team_name: Option<String>,
    pub active_skill_count: i64,
    pub gap_score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkforceKpiReport {
    pub avg_skills_per_person: f64,
    pub blocked_ratio: f64,
    pub contractor_ratio: f64,
    pub team_coverage_ratio: f64,
}

pub async fn workforce_summary(db: &DatabaseConnection) -> AppResult<WorkforceSummaryReport> {
    let totals = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                COUNT(*) AS total_personnel,
                SUM(CASE WHEN availability_status <> 'inactive' THEN 1 ELSE 0 END) AS active_personnel
             FROM personnel"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to compute workforce totals.")))?;

    let employment_breakdown = breakdown_query(
        db,
        "SELECT employment_type AS bucket, COUNT(*) AS count
         FROM personnel
         GROUP BY employment_type
         ORDER BY employment_type ASC",
    )
    .await?;

    let availability_breakdown = breakdown_query(
        db,
        "SELECT availability_status AS bucket, COUNT(*) AS count
         FROM personnel
         GROUP BY availability_status
         ORDER BY availability_status ASC",
    )
    .await?;

    Ok(WorkforceSummaryReport {
        total_personnel: decode_col(&totals, "total_personnel")?,
        active_personnel: decode_col::<Option<i64>>(&totals, "active_personnel")?.unwrap_or(0),
        employment_breakdown,
        availability_breakdown,
    })
}

pub async fn workforce_skills_gap(
    db: &DatabaseConnection,
    limit: Option<i64>,
) -> AppResult<Vec<WorkforceSkillsGapRow>> {
    let row_limit = limit.unwrap_or(100).clamp(1, 500);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                p.id AS personnel_id,
                p.employee_code,
                p.full_name,
                pos.name AS position_name,
                t.name AS team_name,
                COALESCE(COUNT(ps.id), 0) AS active_skill_count,
                CASE
                    WHEN COUNT(ps.id) = 0 THEN 3
                    WHEN COUNT(ps.id) = 1 THEN 2
                    WHEN COUNT(ps.id) = 2 THEN 1
                    ELSE 0
                END AS gap_score
             FROM personnel p
             LEFT JOIN positions pos ON pos.id = p.position_id
             LEFT JOIN org_nodes t ON t.id = p.primary_team_id
             LEFT JOIN personnel_skills ps
                ON ps.personnel_id = p.id
               AND (ps.valid_to IS NULL OR date(ps.valid_to) >= date('now'))
             WHERE p.availability_status <> 'inactive'
             GROUP BY p.id, p.employee_code, p.full_name, pos.name, t.name
             ORDER BY gap_score DESC, p.full_name ASC
             LIMIT ?",
            [row_limit.into()],
        ))
        .await?;

    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        result.push(WorkforceSkillsGapRow {
            personnel_id: decode_col(&row, "personnel_id")?,
            employee_code: decode_col(&row, "employee_code")?,
            full_name: decode_col(&row, "full_name")?,
            position_name: decode_col(&row, "position_name")?,
            team_name: decode_col(&row, "team_name")?,
            active_skill_count: decode_col(&row, "active_skill_count")?,
            gap_score: decode_col(&row, "gap_score")?,
        });
    }
    Ok(result)
}

pub async fn workforce_kpis(db: &DatabaseConnection) -> AppResult<WorkforceKpiReport> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                AVG(COALESCE(skill_counts.active_skill_count, 0)) AS avg_skills_per_person,
                CAST(SUM(CASE WHEN p.availability_status = 'blocked' THEN 1 ELSE 0 END) AS REAL) / NULLIF(COUNT(*), 0) AS blocked_ratio,
                CAST(SUM(CASE WHEN p.employment_type IN ('contractor', 'vendor') THEN 1 ELSE 0 END) AS REAL) / NULLIF(COUNT(*), 0) AS contractor_ratio,
                CAST(SUM(CASE WHEN p.primary_team_id IS NOT NULL THEN 1 ELSE 0 END) AS REAL) / NULLIF(COUNT(*), 0) AS team_coverage_ratio
             FROM personnel p
             LEFT JOIN (
                SELECT personnel_id, COUNT(*) AS active_skill_count
                FROM personnel_skills
                WHERE valid_to IS NULL OR date(valid_to) >= date('now')
                GROUP BY personnel_id
             ) skill_counts ON skill_counts.personnel_id = p.id
             WHERE p.availability_status <> 'inactive'"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("Failed to compute workforce KPIs.")))?;

    Ok(WorkforceKpiReport {
        avg_skills_per_person: decode_col::<Option<f64>>(&row, "avg_skills_per_person")?.unwrap_or(0.0),
        blocked_ratio: decode_col::<Option<f64>>(&row, "blocked_ratio")?.unwrap_or(0.0),
        contractor_ratio: decode_col::<Option<f64>>(&row, "contractor_ratio")?.unwrap_or(0.0),
        team_coverage_ratio: decode_col::<Option<f64>>(&row, "team_coverage_ratio")?.unwrap_or(0.0),
    })
}

pub async fn export_summary_csv(db: &DatabaseConnection) -> AppResult<String> {
    let report = workforce_summary(db).await?;
    let mut writer = csv::Writer::from_writer(vec![]);
    writer.write_record(["section", "bucket", "count"]).map_err(map_csv_err)?;
    writer.write_record([
        "totals",
        "total_personnel",
        report.total_personnel.to_string().as_str(),
    ])
    .map_err(map_csv_err)?;
    writer.write_record([
        "totals",
        "active_personnel",
        report.active_personnel.to_string().as_str(),
    ])
    .map_err(map_csv_err)?;
    for row in &report.employment_breakdown {
        writer
            .write_record(["employment", row.bucket.as_str(), row.count.to_string().as_str()])
            .map_err(map_csv_err)?;
    }
    for row in &report.availability_breakdown {
        writer.write_record([
            "availability",
            row.bucket.as_str(),
            row.count.to_string().as_str(),
        ])
        .map_err(map_csv_err)?;
    }
    let data = writer
        .into_inner()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("summary csv writer finalize failed: {e}")))?;
    String::from_utf8(data)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("summary csv utf8 conversion failed: {e}")))
}

pub async fn export_skills_gap_csv(db: &DatabaseConnection) -> AppResult<String> {
    let rows = workforce_skills_gap(db, Some(500)).await?;
    let mut writer = csv::Writer::from_writer(vec![]);
    writer.write_record([
        "personnel_id",
        "employee_code",
        "full_name",
        "position_name",
        "team_name",
        "active_skill_count",
        "gap_score",
    ])
    .map_err(map_csv_err)?;
    for row in rows {
        writer.write_record([
            row.personnel_id.to_string(),
            row.employee_code,
            row.full_name,
            row.position_name.unwrap_or_default(),
            row.team_name.unwrap_or_default(),
            row.active_skill_count.to_string(),
            row.gap_score.to_string(),
        ])
        .map_err(map_csv_err)?;
    }
    let data = writer
        .into_inner()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("skills csv writer finalize failed: {e}")))?;
    String::from_utf8(data)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("skills csv utf8 conversion failed: {e}")))
}

pub async fn export_kpis_csv(db: &DatabaseConnection) -> AppResult<String> {
    let kpis = workforce_kpis(db).await?;
    let mut writer = csv::Writer::from_writer(vec![]);
    writer.write_record(["kpi", "value"]).map_err(map_csv_err)?;
    writer
        .write_record(["avg_skills_per_person", &kpis.avg_skills_per_person.to_string()])
        .map_err(map_csv_err)?;
    writer
        .write_record(["blocked_ratio", &kpis.blocked_ratio.to_string()])
        .map_err(map_csv_err)?;
    writer
        .write_record(["contractor_ratio", &kpis.contractor_ratio.to_string()])
        .map_err(map_csv_err)?;
    writer
        .write_record(["team_coverage_ratio", &kpis.team_coverage_ratio.to_string()])
        .map_err(map_csv_err)?;
    let data = writer
        .into_inner()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("kpi csv writer finalize failed: {e}")))?;
    String::from_utf8(data)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("kpi csv utf8 conversion failed: {e}")))
}

async fn breakdown_query(db: &DatabaseConnection, sql: &str) -> AppResult<Vec<WorkforceSummaryRow>> {
    let rows = db
        .query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(WorkforceSummaryRow {
            bucket: decode_col(&row, "bucket")?,
            count: decode_col(&row, "count")?,
        });
    }
    Ok(out)
}

fn decode_col<T: sea_orm::TryGetable>(row: &QueryResult, col: &str) -> AppResult<T> {
    row.try_get("", col)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Failed to decode column {col}: {e}")))
}
