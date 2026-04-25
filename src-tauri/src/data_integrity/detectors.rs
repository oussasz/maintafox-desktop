use crate::errors::AppResult;
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde_json::json;
use uuid::Uuid;

use super::queries::last_insert_rowid;
use super::sync_stage::stage_finding_sync;

const EPS_HOURS: f64 = 0.25;

fn map_err(col: &str, e: sea_orm::DbErr) -> crate::errors::AppError {
    crate::errors::AppError::Internal(anyhow::anyhow!("data_integrity detector {col}: {e}"))
}

async fn insert_finding(
    db: &impl ConnectionTrait,
    severity: &str,
    domain: &str,
    record_class: &str,
    record_id: i64,
    finding_code: &str,
    details: serde_json::Value,
) -> AppResult<()> {
    let esid = Uuid::new_v4().to_string();
    let details_json = details.to_string();
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO data_integrity_findings \
         (entity_sync_id, row_version, severity, domain, record_class, record_id, finding_code, details_json, detected_at, status) \
         VALUES (?, 1, ?, ?, ?, ?, ?, ?, ?, 'open')",
        [
            esid.clone().into(),
            severity.into(),
            domain.into(),
            record_class.into(),
            record_id.into(),
            finding_code.into(),
            details_json.clone().into(),
            now.into(),
        ],
    ))
    .await?;
    let id = last_insert_rowid(db).await?;
    stage_finding_sync(
        db,
        id,
        &esid,
        1,
        severity,
        domain,
        record_class,
        record_id,
        finding_code,
        &details_json,
        "open",
    )
    .await?;
    Ok(())
}

/// Clears prior open findings for detector domains, then re-runs checks.
pub async fn run_data_integrity_detectors(db: &DatabaseConnection) -> AppResult<i64> {
    let txn = db.begin().await?;
    txn.execute(Statement::from_string(
        DbBackend::Sqlite,
        "DELETE FROM data_integrity_findings WHERE status = 'open' \
         AND domain IN ('wo_closeout', 'downtime', 'failure_coding')"
            .to_string(),
    ))
    .await?;

    let mut n: i64 = 0;

    let rows = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT fd.id AS fd_id, fd.work_order_id, fd.failure_mode_id \
             FROM work_order_failure_details fd \
             LEFT JOIN failure_codes fc ON fc.id = fd.failure_mode_id \
             WHERE fd.failure_mode_id IS NOT NULL \
               AND (fc.id IS NULL OR fc.is_active = 0 OR fc.code_type != 'mode')"
                .to_string(),
        ))
        .await?;
    for row in &rows {
        let fd_id: i64 = row.try_get("", "fd_id").map_err(|e| map_err("fd_id", e))?;
        let wo_id: i64 = row.try_get("", "work_order_id").map_err(|e| map_err("work_order_id", e))?;
        let fmid: i64 = row.try_get("", "failure_mode_id").map_err(|e| map_err("failure_mode_id", e))?;
        insert_finding(
            &txn,
            "warning",
            "failure_coding",
            "wo_failure_details",
            fd_id,
            "FK_ORPHAN_FAILURE_MODE",
            json!({"work_order_id": wo_id, "failure_mode_id": fmid}),
        )
        .await?;
        n += 1;
    }

    let neg = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, work_order_id FROM work_order_downtime_segments \
             WHERE ended_at IS NOT NULL AND started_at IS NOT NULL \
             AND julianday(ended_at) < julianday(started_at)"
                .to_string(),
        ))
        .await?;
    for row in &neg {
        let sid: i64 = row.try_get("", "id").map_err(|e| map_err("id", e))?;
        let wo_id: i64 = row.try_get("", "work_order_id").map_err(|e| map_err("work_order_id", e))?;
        insert_finding(
            &txn,
            "error",
            "downtime",
            "work_order_downtime_segments",
            sid,
            "WO_DOWNTIME_NEGATIVE_DURATION",
            json!({"work_order_id": wo_id, "segment_id": sid}),
        )
        .await?;
        n += 1;
    }

    let unclosed = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, work_order_id FROM work_order_downtime_segments WHERE ended_at IS NULL"
                .to_string(),
        ))
        .await?;
    for row in &unclosed {
        let sid: i64 = row.try_get("", "id").map_err(|e| map_err("id", e))?;
        let wo_id: i64 = row.try_get("", "work_order_id").map_err(|e| map_err("work_order_id", e))?;
        insert_finding(
            &txn,
            "warning",
            "downtime",
            "work_order_downtime_segments",
            sid,
            "WO_DOWNTIME_UNCLOSED",
            json!({"work_order_id": wo_id, "segment_id": sid}),
        )
        .await?;
        n += 1;
    }

    let wo_rows = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, actual_start, actual_end FROM work_orders \
             WHERE actual_start IS NOT NULL AND actual_end IS NOT NULL"
                .to_string(),
        ))
        .await?;
    for row in &wo_rows {
        let wo_id: i64 = row.try_get("", "id").map_err(|e| map_err("id", e))?;
        let a_start: String = row.try_get("", "actual_start").map_err(|e| map_err("actual_start", e))?;
        let a_end: String = row.try_get("", "actual_end").map_err(|e| map_err("actual_end", e))?;
        let win_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT ROUND((JULIANDAY(?) - JULIANDAY(?)) * 24, 4) AS w",
                [a_end.clone().into(), a_start.clone().into()],
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("win")))?;
        let window_h: f64 = win_row
            .try_get::<Option<f64>>("", "w")
            .ok()
            .flatten()
            .unwrap_or(0.0);

        let sum_row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(SUM(\
                    (JULIANDAY(COALESCE(ended_at, started_at)) - JULIANDAY(started_at)) * 24\
                 ), 0) AS s FROM work_order_downtime_segments WHERE work_order_id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("sum")))?;
        let seg_sum: f64 = sum_row
            .try_get::<Option<f64>>("", "s")
            .ok()
            .flatten()
            .unwrap_or(0.0);

        if seg_sum > window_h + EPS_HOURS && window_h >= 0.0 {
            insert_finding(
                &txn,
                "warning",
                "downtime",
                "work_orders",
                wo_id,
                "WO_DOWNTIME_SUM_EXCEEDS_WINDOW",
                json!({
                    "work_order_id": wo_id,
                    "segment_sum_hours": seg_sum,
                    "window_hours": window_h,
                    "epsilon_hours": EPS_HOURS
                }),
            )
            .await?;
            n += 1;
        }
    }

    let ov = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT a.id AS a_id, a.work_order_id AS wo_id, b.id AS b_id \
             FROM work_order_downtime_segments a \
             JOIN work_order_downtime_segments b \
               ON a.work_order_id = b.work_order_id AND a.id < b.id \
             WHERE a.ended_at IS NOT NULL AND b.ended_at IS NOT NULL \
               AND a.started_at < b.ended_at AND b.started_at < a.ended_at"
                .to_string(),
        ))
        .await?;
    for row in &ov {
        let a_id: i64 = row.try_get("", "a_id").map_err(|e| map_err("a_id", e))?;
        let wo_id: i64 = row.try_get("", "wo_id").map_err(|e| map_err("wo_id", e))?;
        let b_id: i64 = row.try_get("", "b_id").map_err(|e| map_err("b_id", e))?;
        insert_finding(
            &txn,
            "warning",
            "downtime",
            "work_order_downtime_segments",
            a_id,
            "WO_DOWNTIME_OVERLAP",
            json!({"work_order_id": wo_id, "segment_id_a": a_id, "segment_id_b": b_id}),
        )
        .await?;
        n += 1;
    }

    txn.commit().await?;
    Ok(n)
}
