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
    /// Level code of the shared DI / OT urgency scale; empty for work orders without urgency.
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
    /// Counts per PRD lifecycle state (`wo::lifecycle`, derived), in the PRD order.
    pub by_lifecycle: Vec<StatusCount>,
    pub by_urgency: Vec<UrgencyCount>,
    pub daily_completed: Vec<DateCount>,
    pub by_entity: Vec<EntityCount>,
}

/// Compute aggregated WO statistics for the dashboard (plant-wide).
pub async fn get_wo_stats(db: &DatabaseConnection) -> AppResult<WoStatsPayload> {
    get_wo_stats_in_scope(db, &super::access::ScopeSql::everywhere()).await
}

/// [`get_wo_stats`] over the work orders of the reader's scope (rule §2.8: a site user's figures
/// cover their sites only). Every query reads the alias `wo`; the scope predicate is its last
/// `WHERE` condition, so its binds follow the query's own.
pub async fn get_wo_stats_in_scope(
    db: &DatabaseConnection,
    scope: &super::access::ScopeSql,
) -> AppResult<WoStatsPayload> {
    let pred = scope.predicate.as_str();
    let with_scope = |own: Vec<sea_orm::Value>| -> Vec<sea_orm::Value> {
        let mut values = own;
        values.extend(scope.binds.iter().cloned());
        values
    };

    // ── Total WO count ────────────────────────────────────────────────────
    let total = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("SELECT COUNT(*) AS cnt FROM work_orders wo WHERE ({pred})"),
                with_scope(Vec::new()),
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("stats: total count query failed")))?;
        use sea_orm::TryGetable;
        i64::try_get_by(&row, "cnt").unwrap_or(0)
    };

    // ── In-progress count ─────────────────────────────────────────────────
    // `in_progress` and `completed` are slices of the status breakdown (being executed; finished),
    // not an "open" figure: neither of the two open-OT rules (`wo::backlog::OPEN_STATUS_SQL`,
    // `NOT_CLOSED_SQL`) is meant here, so their own status lists stay.
    let in_progress = count_by_macro_state(db, &["in_progress", "on_hold"], scope).await?;

    // ── Completed count (completed + closed) ──
    let completed = count_by_macro_state(db, &["completed", "closed"], scope).await?;

    // ── Overdue count (`wo::backlog::WO_OVERDUE_SQL`: work still ahead, planned end past) ──
    let overdue = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS cnt FROM work_orders wo \
                     JOIN work_order_statuses s ON s.id = wo.status_id WHERE {} AND ({pred})",
                    crate::wo::backlog::WO_OVERDUE_SQL
                ),
                with_scope(vec![crate::analytics::as_of::instant(chrono::Utc::now()).into()]),
            ))
            .await?
            .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("stats: overdue query failed")))?;
        use sea_orm::TryGetable;
        i64::try_get_by(&row, "cnt").unwrap_or(0)
    };

    // ── By status ─────────────────────────────────────────────────────────
    let by_status = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT s.code AS label, COUNT(*) AS cnt \
                     FROM work_orders wo \
                     JOIN work_order_statuses s ON s.id = wo.status_id \
                     WHERE ({pred}) \
                     GROUP BY s.code ORDER BY cnt DESC"
                ),
                with_scope(Vec::new()),
            ))
            .await?;
        map_rows(&rows, |label, count| StatusCount { status: label, count })?
    };

    // ── By lifecycle state (derived) ──────────────────────────────────────
    let by_lifecycle = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT {} AS label, COUNT(*) AS cnt FROM work_orders wo \
                     JOIN work_order_statuses wos ON wos.id = wo.status_id WHERE ({pred}) GROUP BY label",
                    crate::wo::lifecycle::SQL_CASE
                ),
                with_scope(Vec::new()),
            ))
            .await?;
        let counted = map_rows(&rows, |label, count| StatusCount { status: label, count })?;
        crate::wo::lifecycle::LifecycleState::ALL
            .iter()
            .filter_map(|st| counted.iter().find(|c| c.status == st.as_str()).cloned())
            .collect::<Vec<_>>()
    };

    // ── By urgency ────────────────────────────────────────────────────────
    let by_urgency = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT COALESCE(u.code, '') AS label, COUNT(*) AS cnt \
                     FROM work_orders wo \
                     LEFT JOIN reference_values u ON u.id = wo.urgency_id \
                     WHERE ({pred}) \
                     GROUP BY u.code ORDER BY COALESCE(MIN(u.sort_order), 99) ASC"
                ),
                with_scope(Vec::new()),
            ))
            .await?;
        map_rows(&rows, |label, count| UrgencyCount { urgency: label, count })?
    };

    // ── Daily completed (last 30 days) ────────────────────────────────────
    let daily_completed = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT DATE(wo.closed_at) AS label, COUNT(*) AS cnt \
                     FROM work_orders wo \
                     WHERE wo.closed_at IS NOT NULL \
                       AND wo.closed_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-30 days') \
                       AND ({pred}) \
                     GROUP BY DATE(wo.closed_at) ORDER BY label ASC"
                ),
                with_scope(Vec::new()),
            ))
            .await?;
        map_rows(&rows, |label, count| DateCount { date: label, count })?
    };

    // ── By entity ─────────────────────────────────────────────────────────
    let by_entity = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT COALESCE(n.name, 'Non assigné') AS label, COUNT(*) AS cnt \
                     FROM work_orders wo \
                     LEFT JOIN org_nodes n ON n.id = wo.entity_id \
                     WHERE wo.entity_id IS NOT NULL AND ({pred}) \
                     GROUP BY wo.entity_id ORDER BY cnt DESC"
                ),
                with_scope(Vec::new()),
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
        by_lifecycle,
        by_urgency,
        daily_completed,
        by_entity,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn count_by_macro_state(
    db: &DatabaseConnection,
    status_codes: &[&str],
    scope: &super::access::ScopeSql,
) -> AppResult<i64> {
    let placeholders: String = status_codes.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM work_orders wo \
         JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code IN ({placeholders}) AND ({})",
        scope.predicate
    );
    let mut values: Vec<sea_orm::Value> = status_codes
        .iter()
        .map(|c| sea_orm::Value::String(Some(Box::new(c.to_string()))))
        .collect();
    values.extend(scope.binds.iter().cloned());
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, values))
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
