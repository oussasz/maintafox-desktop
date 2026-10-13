use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

use super::domain::{
    InspectionReliabilitySignal, InspectionReliabilitySignalsFilter, RefreshInspectionReliabilitySignalsInput,
};

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("inspection_reliability_signals decode '{field}': {err}"))
}

fn map_signal(row: &sea_orm::QueryResult) -> AppResult<InspectionReliabilitySignal> {
    Ok(InspectionReliabilitySignal {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
        period_start: row
            .try_get("", "period_start")
            .map_err(|e| decode_err("period_start", e))?,
        period_end: row.try_get("", "period_end").map_err(|e| decode_err("period_end", e))?,
        warning_count: row
            .try_get("", "warning_count")
            .map_err(|e| decode_err("warning_count", e))?,
        fail_count: row.try_get("", "fail_count").map_err(|e| decode_err("fail_count", e))?,
        anomaly_open_count: row
            .try_get("", "anomaly_open_count")
            .map_err(|e| decode_err("anomaly_open_count", e))?,
        checkpoint_coverage_ratio: row
            .try_get("", "checkpoint_coverage_ratio")
            .map_err(|e| decode_err("checkpoint_coverage_ratio", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        asset_code: row.try_get("", "asset_code").map_err(|e| decode_err("asset_code", e))?,
        asset_name: row.try_get("", "asset_name").map_err(|e| decode_err("asset_name", e))?,
    })
}

const SIGNAL_SELECT: &str = "SELECT s.id, s.entity_sync_id, s.equipment_id, s.period_start, s.period_end, \
     s.warning_count, s.fail_count, s.anomaly_open_count, s.checkpoint_coverage_ratio, s.row_version, \
     e.asset_id_code AS asset_code, e.name AS asset_name \
     FROM inspection_reliability_signals s LEFT JOIN equipment e ON e.id = s.equipment_id";

async fn count_i64(db: &impl ConnectionTrait, sql: &str, binds: Vec<sea_orm::Value>) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("aggregate returned no row")))?;
    row.try_get::<i64>("", "c")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("count decode: {e}")))
}

pub async fn refresh_inspection_reliability_signals(
    db: &DatabaseConnection,
    input: RefreshInspectionReliabilitySignalsInput,
) -> AppResult<Vec<InspectionReliabilitySignal>> {
    let days = input.window_days.clamp(1, 366);
    let end = Utc::now();
    let start = end - Duration::days(days);
    let period_start = start.to_rfc3339();
    let period_end = end.to_rfc3339();
    let txn = db.begin().await?;

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM inspection_reliability_signals WHERE period_start = ? AND period_end = ?",
        [period_start.clone().into(), period_end.clone().into()],
    ))
    .await?;

    let eq_rows = txn
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT asset_id AS equipment_id FROM inspection_checkpoints WHERE asset_id IS NOT NULL ORDER BY asset_id ASC"
                .to_string(),
        ))
        .await?;

    let mut out: Vec<InspectionReliabilitySignal> = Vec::with_capacity(eq_rows.len());

    for er in eq_rows {
        let equipment_id: i64 = er
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?;

        let warning_count = count_i64(
            &txn,
            "SELECT COUNT(*) AS c FROM inspection_results ir \
             INNER JOIN inspection_checkpoints cp ON cp.id = ir.checkpoint_id \
             WHERE cp.asset_id = ? AND ir.recorded_at >= ? AND ir.recorded_at <= ? AND ir.result_status = 'warning'",
            vec![
                equipment_id.into(),
                period_start.clone().into(),
                period_end.clone().into(),
            ],
        )
        .await?;

        let fail_count = count_i64(
            &txn,
            "SELECT COUNT(*) AS c FROM inspection_results ir \
             INNER JOIN inspection_checkpoints cp ON cp.id = ir.checkpoint_id \
             WHERE cp.asset_id = ? AND ir.recorded_at >= ? AND ir.recorded_at <= ? AND ir.result_status = 'fail'",
            vec![
                equipment_id.into(),
                period_start.clone().into(),
                period_end.clone().into(),
            ],
        )
        .await?;

        let anomaly_open_count = count_i64(
            &txn,
            "SELECT COUNT(*) AS c FROM inspection_anomalies a \
             INNER JOIN inspection_results ir ON ir.id = a.result_id \
             INNER JOIN inspection_checkpoints cp ON cp.id = ir.checkpoint_id \
             WHERE cp.asset_id = ? AND a.resolution_status IN ('open', 'triaged') \
             AND ir.recorded_at >= ? AND ir.recorded_at <= ?",
            vec![
                equipment_id.into(),
                period_start.clone().into(),
                period_end.clone().into(),
            ],
        )
        .await?;

        let expected_slots = count_i64(
            &txn,
            "SELECT COUNT(*) AS c FROM inspection_rounds r \
             INNER JOIN inspection_checkpoints cp ON cp.template_version_id = r.template_version_id AND cp.asset_id = ? \
             WHERE r.scheduled_at IS NOT NULL AND r.scheduled_at >= ? AND r.scheduled_at <= ?",
            vec![
                equipment_id.into(),
                period_start.clone().into(),
                period_end.clone().into(),
            ],
        )
        .await?;

        let actual_slots = count_i64(
            &txn,
            "SELECT COUNT(*) AS c FROM ( \
             SELECT DISTINCT ir.round_id, ir.checkpoint_id FROM inspection_results ir \
             INNER JOIN inspection_checkpoints cp ON cp.id = ir.checkpoint_id \
             WHERE cp.asset_id = ? AND ir.recorded_at >= ? AND ir.recorded_at <= ? \
             ) AS t",
            vec![
                equipment_id.into(),
                period_start.clone().into(),
                period_end.clone().into(),
            ],
        )
        .await?;

        let checkpoint_coverage_ratio = if expected_slots > 0 {
            (actual_slots as f64 / expected_slots as f64).min(1.0)
        } else if actual_slots > 0 {
            1.0
        } else {
            0.0
        };

        let entity_sync_id = Uuid::new_v4().to_string();
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO inspection_reliability_signals (\
                entity_sync_id, equipment_id, period_start, period_end, \
                warning_count, fail_count, anomaly_open_count, checkpoint_coverage_ratio, row_version\
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
            [
                entity_sync_id.clone().into(),
                equipment_id.into(),
                period_start.clone().into(),
                period_end.clone().into(),
                warning_count.into(),
                fail_count.into(),
                anomaly_open_count.into(),
                checkpoint_coverage_ratio.into(),
            ],
        ))
        .await?;

        let id_row = txn
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT last_insert_rowid() AS id".to_string(),
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
        let id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;

        let row = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("{SIGNAL_SELECT} WHERE s.id = ?"),
                [id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("re-read signal")))?;
        let sig = map_signal(&row)?;
        out.push(sig);
    }

    txn.commit().await?;
    Ok(out)
}

pub async fn list_inspection_reliability_signals(
    db: &DatabaseConnection,
    filter: InspectionReliabilitySignalsFilter,
) -> AppResult<Vec<InspectionReliabilitySignal>> {
    let rows = if let Some(eid) = filter.equipment_id {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{SIGNAL_SELECT} WHERE s.equipment_id = ? ORDER BY s.period_end DESC, s.equipment_id ASC"),
            [eid.into()],
        ))
        .await?
    } else {
        db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{SIGNAL_SELECT} ORDER BY s.period_end DESC, s.equipment_id ASC"),
        ))
        .await?
    };
    rows.iter().map(map_signal).collect()
}
