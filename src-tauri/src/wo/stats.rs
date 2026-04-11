//! WO aggregated statistics for the dashboard.
//!
//! Phase 2 – Sub-phase 05 – File 04 – Sprint S4.
//!
//! Provides `get_wo_stats` which returns aggregated KPI data
//! consumed by `WoDashboardView.tsx`.

use crate::errors::AppResult;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

/// Status distribution entry.
#[derive(Debug, Clone, Serialize)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

/// Urgency distribution entry.
#[derive(Debug, Clone, Serialize)]
pub struct UrgencyCount {
    pub urgency: String,
    pub count: i64,
}

/// Daily completed entry.
#[derive(Debug, Clone, Serialize)]
pub struct DateCount {
    pub date: String,
    pub count: i64,
}

/// Entity backlog entry.
#[derive(Debug, Clone, Serialize)]
pub struct EntityCount {
    pub entity: String,
    pub count: i64,
}

/// Aggregated WO statistics payload returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct WoStatsPayload {
    pub total: i64,
    pub in_progress: i64,
    pub completed: i64,
    pub overdue: i64,
    pub by_status: Vec<StatusCount>,
    pub by_urgency: Vec<UrgencyCount>,
    pub daily_completed: Vec<DateCount>,
    pub by_entity: Vec<EntityCount>,
}

/// Compute aggregated WO statistics for the dashboard.
pub async fn get_wo_stats(db: &DatabaseConnection) -> AppResult<WoStatsPayload> {
    // ── Total WO count ────────────────────────────────────────────────────
    let total = {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM work_orders".to_owned(),
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("stats: total count query failed")))?;
        use sea_orm::TryGetable;
        i64::try_get_by(&row, "cnt").unwrap_or(0)
    };

    // ── In-progress count ─────────────────────────────────────────────────
    let in_progress = count_by_macro_state(db, &["in_progress", "paused"]).await?;

    // ── Completed count (mechanically_complete + technically_verified + closed) ──
    let completed = count_by_macro_state(
        db,
        &["mechanically_complete", "technically_verified", "closed"],
    )
    .await?;

    // ── Overdue count (planned_end < now AND status not terminal) ──────────
    let overdue = {
        let row = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS cnt FROM work_orders wo \
                 JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE wo.planned_end IS NOT NULL \
                   AND wo.planned_end < strftime('%Y-%m-%dT%H:%M:%SZ','now') \
                   AND s.code NOT IN ('closed', 'cancelled')"
                    .to_owned(),
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("stats: overdue query failed")))?;
        use sea_orm::TryGetable;
        i64::try_get_by(&row, "cnt").unwrap_or(0)
    };

    // ── By status ─────────────────────────────────────────────────────────
    let by_status = {
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT s.code AS label, COUNT(*) AS cnt \
                 FROM work_orders wo \
                 JOIN work_order_statuses s ON s.id = wo.status_id \
                 GROUP BY s.code ORDER BY cnt DESC"
                    .to_owned(),
            ))
            .await?;
        map_rows(&rows, |label, count| StatusCount { status: label, count })?
    };

    // ── By urgency ────────────────────────────────────────────────────────
    let by_urgency = {
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COALESCE(u.label, 'N/A') AS label, COUNT(*) AS cnt \
                 FROM work_orders wo \
                 LEFT JOIN work_order_urgencies u ON u.id = wo.urgency_id \
                 GROUP BY label ORDER BY cnt DESC"
                    .to_owned(),
            ))
            .await?;
        map_rows(&rows, |label, count| UrgencyCount { urgency: label, count })?
    };

    // ── Daily completed (last 30 days) ────────────────────────────────────
    let daily_completed = {
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT DATE(wo.closed_at) AS label, COUNT(*) AS cnt \
                 FROM work_orders wo \
                 WHERE wo.closed_at IS NOT NULL \
                   AND wo.closed_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 days') \
                 GROUP BY DATE(wo.closed_at) ORDER BY label ASC"
                    .to_owned(),
            ))
            .await?;
        map_rows(&rows, |label, count| DateCount { date: label, count })?
    };

    // ── By entity ─────────────────────────────────────────────────────────
    let by_entity = {
        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT COALESCE(n.label, 'Non assigné') AS label, COUNT(*) AS cnt \
                 FROM work_orders wo \
                 LEFT JOIN org_nodes n ON n.id = wo.entity_id \
                 WHERE wo.entity_id IS NOT NULL \
                 GROUP BY wo.entity_id ORDER BY cnt DESC"
                    .to_owned(),
            ))
            .await?;
        map_rows(&rows, |label, count| EntityCount { entity: label, count })?
    };

    Ok(WoStatsPayload {
        total,
        in_progress,
        completed,
        overdue,
        by_status,
        by_urgency,
        daily_completed,
        by_entity,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn count_by_macro_state(db: &DatabaseConnection, status_codes: &[&str]) -> AppResult<i64> {
    let placeholders: String = status_codes.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code IN ({placeholders})"
    );
    let values: Vec<sea_orm::Value> = status_codes
        .iter()
        .map(|c| sea_orm::Value::String(Some(Box::new(c.to_string()))))
        .collect();
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            values,
        ))
        .await?
        .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("stats: macro state count failed")))?;
    use sea_orm::TryGetable;
    Ok(i64::try_get_by(&row, "cnt").unwrap_or(0))
}

fn map_rows<T, F>(rows: &[sea_orm::QueryResult], make: F) -> AppResult<Vec<T>>
where
    F: Fn(String, i64) -> T,
{
    use sea_orm::TryGetable;
    rows.iter()
        .map(|r| {
            let label = String::try_get_by(r, "label").unwrap_or_default();
            let count = i64::try_get_by(r, "cnt").unwrap_or(0);
            Ok(make(label, count))
        })
        .collect()
}
