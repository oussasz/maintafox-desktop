//! Pareto of failure modes (Phase 6.6): unplanned failure events grouped by failure mode, ranked
//! by count, maintenance cost (total cost of the work order that recorded the failure) or
//! downtime, with each mode's share and the cumulative share (the 80/20 view).

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};

pub const METRICS: &[&str] = &["count", "cost", "downtime"];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FailureModeParetoInput {
    /// `count`, `cost` or `downtime`.
    pub metric: String,
    #[serde(default)]
    pub period_start: Option<String>,
    #[serde(default)]
    pub period_end: Option<String>,
    #[serde(default)]
    pub equipment_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ParetoRow {
    /// `None` for failures recorded without a failure mode.
    pub failure_mode_id: Option<i64>,
    pub mode_code: Option<String>,
    pub mode_label: Option<String>,
    pub failure_count: i64,
    pub total_cost: f64,
    pub downtime_hours: f64,
    /// Value of the ranking metric.
    pub value: f64,
    pub share: f64,
    pub cumulative_share: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureModePareto {
    pub metric: String,
    pub total: f64,
    pub rows: Vec<ParetoRow>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("pareto decode '{field}': {err}"))
}

/// Ranks rows by `value` (descending, ties by label) and fills share / cumulative share.
pub fn rank(mut rows: Vec<ParetoRow>) -> (f64, Vec<ParetoRow>) {
    rows.sort_by(|a, b| {
        b.value
            .partial_cmp(&a.value)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.mode_label.cmp(&b.mode_label))
    });
    let total: f64 = rows.iter().map(|r| r.value).sum();
    let mut running = 0.0;
    for r in &mut rows {
        running += r.value;
        if total > 0.0 {
            r.share = r.value / total;
            r.cumulative_share = running / total;
        }
    }
    (total, rows)
}

pub async fn get_failure_mode_pareto(
    db: &impl ConnectionTrait,
    input: FailureModeParetoInput,
) -> AppResult<FailureModePareto> {
    if !METRICS.contains(&input.metric.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "metric must be one of {METRICS:?}."
        )]));
    }
    for (name, v) in [("period_start", &input.period_start), ("period_end", &input.period_end)] {
        if let Some(s) = v {
            chrono::DateTime::parse_from_rfc3339(s)
                .map_err(|_| AppError::ValidationFailed(vec![format!("{name} must be an RFC 3339 timestamp.")]))?;
        }
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT fe.failure_mode_id, fc.code, fc.label, COUNT(*) AS n, \
             COALESCE(SUM(fe.downtime_duration_hours), 0.0) AS dt, \
             COALESCE(SUM(CASE WHEN fe.source_type = 'work_order' THEN wo.total_cost END), 0.0) AS cost \
             FROM failure_events fe \
             LEFT JOIN failure_codes fc ON fc.id = fe.failure_mode_id \
             LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id \
             WHERE fe.is_planned = 0 \
               AND (? IS NULL OR COALESCE(fe.failed_at, fe.detected_at, fe.created_at) >= ?) \
               AND (? IS NULL OR COALESCE(fe.failed_at, fe.detected_at, fe.created_at) <= ?) \
               AND (? IS NULL OR fe.equipment_id = ?) \
             GROUP BY fe.failure_mode_id, fc.code, fc.label",
            [
                input.period_start.clone().into(),
                input.period_start.clone().into(),
                input.period_end.clone().into(),
                input.period_end.clone().into(),
                input.equipment_id.into(),
                input.equipment_id.into(),
            ],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let n: i64 = r.try_get("", "n").map_err(|e| decode_err("n", e))?;
        let cost: f64 = r.try_get("", "cost").map_err(|e| decode_err("cost", e))?;
        let dt: f64 = r.try_get("", "dt").map_err(|e| decode_err("dt", e))?;
        out.push(ParetoRow {
            failure_mode_id: r
                .try_get("", "failure_mode_id")
                .map_err(|e| decode_err("failure_mode_id", e))?,
            mode_code: r.try_get("", "code").map_err(|e| decode_err("code", e))?,
            mode_label: r.try_get("", "label").map_err(|e| decode_err("label", e))?,
            failure_count: n,
            total_cost: cost,
            downtime_hours: dt,
            value: match input.metric.as_str() {
                "cost" => cost,
                "downtime" => dt,
                _ => n as f64,
            },
            share: 0.0,
            cumulative_share: 0.0,
        });
    }
    let (total, rows) = rank(out);
    Ok(FailureModePareto {
        metric: input.metric,
        total,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(label: &str, value: f64) -> ParetoRow {
        ParetoRow {
            failure_mode_id: None,
            mode_code: None,
            mode_label: Some(label.into()),
            failure_count: 0,
            total_cost: 0.0,
            downtime_hours: 0.0,
            value,
            share: 0.0,
            cumulative_share: 0.0,
        }
    }

    #[test]
    fn ranking_and_cumulative_shares() {
        let (total, rows) = rank(vec![row("b", 20.0), row("a", 50.0), row("c", 30.0)]);
        assert!((total - 100.0).abs() < 1e-12);
        let labels: Vec<_> = rows.iter().map(|r| r.mode_label.clone().unwrap()).collect();
        assert_eq!(labels, ["a", "c", "b"]);
        assert!((rows[0].share - 0.5).abs() < 1e-12);
        assert!((rows[1].cumulative_share - 0.8).abs() < 1e-12);
        assert!((rows[2].cumulative_share - 1.0).abs() < 1e-12);
    }

    #[test]
    fn empty_and_zero_totals() {
        let (total, rows) = rank(vec![row("a", 0.0)]);
        assert_eq!(total, 0.0);
        assert_eq!(rows[0].cumulative_share, 0.0);
    }
}
