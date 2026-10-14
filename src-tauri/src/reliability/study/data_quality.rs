//! Data quality of the reliability module (replaces the former "foundation" screen): what keeps
//! failure records out of the analyses, with the record to open for each point.
//!
//! - closed corrective / emergency work orders without a failure mode;
//! - failure records with a mode but without cause or mechanism (ISO 14224);
//! - equipment with failures and no operating-hours data (meter, runtime log, schedule);
//! - the remaining issues of `v_ram_data_quality_issues` (small samples, inspection coverage),
//!   which a reliability manager can dismiss.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::errors::AppResult;
use crate::reliability::domain::RamDataQualityIssuesFilter;
use crate::reliability::queries;
use crate::reliability::study::clock::load_clock;
use crate::reliability::study::store::get;

#[derive(Debug, Clone, Serialize)]
pub struct WoCodingGap {
    pub work_order_id: i64,
    pub work_order_code: String,
    pub equipment_id: Option<i64>,
    pub asset_code: Option<String>,
    pub closed_at: Option<String>,
    /// `mode_missing` | `cause_missing` | `mechanism_missing`.
    pub gap: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquipmentGap {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub failures: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewIssue {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    /// `LOW_SAMPLE` | `MISSING_INSPECTION_COVERAGE`.
    pub issue_code: String,
    /// `blocking` | `warning`.
    pub severity: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataQualityView {
    pub period_start: String,
    pub period_end: String,
    pub coding_gaps: Vec<WoCodingGap>,
    pub no_operating_hours: Vec<EquipmentGap>,
    pub review_issues: Vec<ReviewIssue>,
    /// Failure records of the period and those fully coded (mode, cause, mechanism).
    pub records: i64,
    pub records_complete: i64,
}

const MAX_ROWS: i64 = 300;

pub async fn data_quality(db: &DatabaseConnection, months: i64, user_id: i32) -> AppResult<DataQualityView> {
    let end = Utc::now();
    let start = end - Duration::days(months.clamp(1, 120) * 30);
    let (s, e) = (start.to_rfc3339(), end.to_rfc3339());

    let mut coding_gaps = Vec::new();
    for w in queries::list_wos_missing_failure_mode(db, None, Some(MAX_ROWS)).await? {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT wo.code, e.asset_id_code FROM work_orders wo LEFT JOIN equipment e ON e.id = wo.equipment_id
                 WHERE wo.id = ?",
                [w.work_order_id.into()],
            ))
            .await?;
        let (code, asset): (String, Option<String>) = match row {
            Some(r) => (get(&r, "code")?, get(&r, "asset_id_code")?),
            None => continue,
        };
        coding_gaps.push(WoCodingGap {
            work_order_id: w.work_order_id,
            work_order_code: code,
            equipment_id: Some(w.equipment_id),
            asset_code: asset,
            closed_at: w.closed_at,
            gap: "mode_missing".into(),
        });
    }
    let partial = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.id, wo.code, wo.closed_at, fe.equipment_id, e.asset_id_code,
                    COALESCE(fe.failure_cause_id, wfd.failure_cause_id) AS cause,
                    COALESCE(fe.cause_not_determined, 0) AS cause_nd,
                    COALESCE(fe.failure_mechanism_id, wfd.failure_mechanism_id) AS mechanism
             FROM failure_events fe
             JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
             LEFT JOIN equipment e ON e.id = fe.equipment_id
             LEFT JOIN work_order_failure_details wfd ON wfd.id = (
                  SELECT MAX(x.id) FROM work_order_failure_details x WHERE x.work_order_id = wo.id)
             WHERE fe.failure_mode_id IS NOT NULL AND COALESCE(fe.is_planned, 0) = 0
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) >= datetime(?)
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) <= datetime(?)
               AND ((COALESCE(fe.failure_cause_id, wfd.failure_cause_id) IS NULL AND COALESCE(fe.cause_not_determined, 0) = 0)
                    OR COALESCE(fe.failure_mechanism_id, wfd.failure_mechanism_id) IS NULL)
             ORDER BY datetime(wo.closed_at) DESC LIMIT ?",
            [s.clone().into(), e.clone().into(), MAX_ROWS.into()],
        ))
        .await?;
    for r in &partial {
        let cause: Option<i64> = get(r, "cause")?;
        let cause_nd: i64 = get(r, "cause_nd")?;
        coding_gaps.push(WoCodingGap {
            work_order_id: get(r, "id")?,
            work_order_code: get(r, "code")?,
            equipment_id: get(r, "equipment_id")?,
            asset_code: get(r, "asset_id_code")?,
            closed_at: get(r, "closed_at")?,
            gap: if cause.is_none() && cause_nd == 0 {
                "cause_missing"
            } else {
                "mechanism_missing"
            }
            .into(),
        });
    }

    let totals = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS n,
                    SUM(CASE WHEN fe.failure_mode_id IS NOT NULL
                              AND (COALESCE(fe.failure_cause_id, wfd.failure_cause_id) IS NOT NULL OR COALESCE(fe.cause_not_determined, 0) = 1)
                              AND COALESCE(fe.failure_mechanism_id, wfd.failure_mechanism_id) IS NOT NULL
                         THEN 1 ELSE 0 END) AS complete
             FROM failure_events fe
             LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
             LEFT JOIN work_order_failure_details wfd ON wfd.id = (
                  SELECT MAX(x.id) FROM work_order_failure_details x WHERE x.work_order_id = wo.id)
             WHERE COALESCE(fe.is_planned, 0) = 0
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) >= datetime(?)
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) <= datetime(?)",
            [s.clone().into(), e.clone().into()],
        ))
        .await?;
    let (records, records_complete): (i64, i64) = match totals {
        Some(r) => (get(&r, "n")?, get::<Option<i64>>(&r, "complete")?.unwrap_or(0)),
        None => (0, 0),
    };

    let with_failures = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, e.asset_id_code, e.name, COUNT(fe.id) AS n FROM failure_events fe
             JOIN equipment e ON e.id = fe.equipment_id AND e.deleted_at IS NULL
             WHERE COALESCE(fe.is_planned, 0) = 0
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) >= datetime(?)
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) <= datetime(?)
             GROUP BY e.id ORDER BY n DESC, e.asset_id_code ASC LIMIT ?",
            [s.clone().into(), e.clone().into(), MAX_ROWS.into()],
        ))
        .await?;
    let mut no_operating_hours = Vec::new();
    for r in &with_failures {
        let id: i64 = get(r, "id")?;
        if !load_clock(db, id).await?.is_operating() {
            no_operating_hours.push(EquipmentGap {
                equipment_id: id,
                asset_code: get(r, "asset_id_code")?,
                asset_name: get(r, "name")?,
                failures: get(r, "n")?,
            });
        }
    }

    let mut review_issues = Vec::new();
    for i in
        queries::list_ram_data_quality_issues(db, RamDataQualityIssuesFilter { equipment_id: None }, user_id).await?
    {
        if i.issue_code != "LOW_SAMPLE" && i.issue_code != "MISSING_INSPECTION_COVERAGE" {
            continue;
        }
        if let Some(r) = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT asset_id_code, name FROM equipment WHERE id = ? AND deleted_at IS NULL",
                [i.equipment_id.into()],
            ))
            .await?
        {
            review_issues.push(ReviewIssue {
                equipment_id: i.equipment_id,
                asset_code: get(&r, "asset_id_code")?,
                asset_name: get(&r, "name")?,
                issue_code: i.issue_code,
                severity: i.severity,
            });
        }
        if review_issues.len() as i64 >= MAX_ROWS {
            break;
        }
    }
    Ok(DataQualityView {
        period_start: s,
        period_end: e,
        coding_gaps,
        no_operating_hours,
        review_issues,
        records,
        records_complete,
    })
}
