//! Fleet reliability KPIs as pooled ratios (audit P1-3).
//!
//! Averaging per-asset MTBF / MTTR / availability over every snapshot row is wrong twice: it weights a
//! 10-hour asset like a 10 000-hour asset (mean of ratios ≠ ratio of sums), and overlapping snapshots
//! of the same asset are counted repeatedly. Here each equipment contributes exactly one snapshot — its
//! latest in the window — and fleet values are ratios of sums:
//!
//! - MTBF  = Σ T_exp / Σ F            (repairable equipment)
//! - MTTR  = Σ active repair / Σ repairs
//! - A_i   = Σ T_exp / (Σ T_exp + Σ active repair)
//! - A_o   = Σ T_exp / (Σ T_exp + Σ downtime)
//!
//! Only snapshots with `data_quality_score >= min_dq` enter the ratios (PRD §6.11.1 "qualified assets");
//! the mean DQ is reported over every equipment in the window.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::Serialize;

use crate::errors::{AppError, AppResult};

/// One equipment's latest snapshot inputs.
#[derive(Debug, Clone, PartialEq)]
pub struct FleetSnapshotRow {
    pub equipment_id: i64,
    pub data_quality_score: f64,
    pub repairable: bool,
    pub event_count: i64,
    /// `None` on legacy rows without persisted inputs — such rows are skipped by the ratios they lack.
    pub exposure_hours: Option<f64>,
    pub repair_event_count: Option<i64>,
    pub active_repair_hours_total: Option<f64>,
    pub downtime_hours_total: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FleetPooledKpis {
    pub equipment_count: i64,
    pub qualified_equipment_count: i64,
    pub total_failure_events: i64,
    pub mean_data_quality_score: Option<f64>,
    pub mtbf_hours: Option<f64>,
    pub mttr_hours: Option<f64>,
    pub availability_inherent: Option<f64>,
    pub availability_operational: Option<f64>,
}

fn ratio(num: f64, den: f64) -> Option<f64> {
    (den > 0.0 && num.is_finite() && den.is_finite()).then(|| num / den)
}

/// Pool one-row-per-equipment snapshots into fleet ratios.
#[must_use]
pub fn pool_fleet(rows: &[FleetSnapshotRow], min_dq: f64) -> FleetPooledKpis {
    let mut out = FleetPooledKpis {
        equipment_count: rows.len() as i64,
        total_failure_events: rows.iter().map(|r| r.event_count).sum(),
        mean_data_quality_score: if rows.is_empty() {
            None
        } else {
            Some(rows.iter().map(|r| r.data_quality_score).sum::<f64>() / rows.len() as f64)
        },
        ..FleetPooledKpis::default()
    };

    let (mut t_mtbf, mut f_mtbf) = (0.0_f64, 0_i64);
    let (mut rep_h, mut rep_n) = (0.0_f64, 0_i64);
    let (mut t_ai, mut r_ai) = (0.0_f64, 0.0_f64);
    let (mut t_ao, mut d_ao) = (0.0_f64, 0.0_f64);
    for r in rows.iter().filter(|r| r.data_quality_score >= min_dq) {
        out.qualified_equipment_count += 1;
        if let Some(t) = r.exposure_hours.filter(|t| t.is_finite() && *t > 0.0) {
            if r.repairable {
                t_mtbf += t;
                f_mtbf += r.event_count;
            }
            if let Some(a) = r.active_repair_hours_total {
                t_ai += t;
                r_ai += a.max(0.0);
            }
            if let Some(d) = r.downtime_hours_total {
                t_ao += t;
                d_ao += d.max(0.0);
            }
        }
        if let (Some(h), Some(n)) = (r.active_repair_hours_total, r.repair_event_count) {
            rep_h += h.max(0.0);
            rep_n += n.max(0);
        }
    }
    out.mtbf_hours = ratio(t_mtbf, f_mtbf as f64);
    out.mttr_hours = ratio(rep_h, rep_n as f64);
    out.availability_inherent = ratio(t_ai, t_ai + r_ai);
    out.availability_operational = ratio(t_ao, t_ao + d_ao);
    out
}

/// Snapshot window relative to now: `period_end >= now - since_days` and, when set,
/// `period_end < now - until_days`.
#[derive(Debug, Clone, Copy)]
pub struct FleetWindow {
    pub since_days: i64,
    pub until_days: Option<i64>,
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("fleet decode '{field}': {err}"))
}

/// Latest snapshot per equipment (per equipment and `date(period_end)` when `per_date`) whose
/// `period_end` falls in `window`, optionally restricted to one org node.
async fn load_latest_rows(
    db: &DatabaseConnection,
    window: FleetWindow,
    org_node_id: Option<i64>,
    per_date: bool,
) -> AppResult<Vec<(Option<String>, FleetSnapshotRow)>> {
    let partition = if per_date {
        "s.equipment_id, date(s.period_end)"
    } else {
        "s.equipment_id"
    };
    let sql = format!(
        "WITH ranked AS (
            SELECT s.equipment_id, date(s.period_end) AS d, s.data_quality_score, s.event_count,
                   s.exposure_hours, s.repair_event_count, s.active_repair_hours_total,
                   s.downtime_hours_total, COALESCE(e.rams_is_repairable, 1) AS repairable,
                   ROW_NUMBER() OVER (PARTITION BY {partition}
                                      ORDER BY datetime(s.period_end) DESC, s.id DESC) AS rn
            FROM reliability_kpi_snapshots s
            LEFT JOIN equipment e ON e.id = s.equipment_id
            WHERE s.equipment_id IS NOT NULL
              AND datetime(s.period_end) >= datetime('now', ?)
              AND (? IS NULL OR datetime(s.period_end) < datetime('now', ?))
              AND (? IS NULL OR e.installed_at_node_id = ?)
         )
         SELECT * FROM ranked WHERE rn = 1 ORDER BY d, equipment_id"
    );
    let since = format!("-{} days", window.since_days);
    let until: sea_orm::Value = window
        .until_days
        .map(|d| sea_orm::Value::from(format!("-{d} days")))
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>));
    let org: sea_orm::Value = org_node_id
        .map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>));
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [since.into(), until.clone(), until, org.clone(), org],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let repairable: i64 = r.try_get("", "repairable").map_err(|e| decode_err("repairable", e))?;
        out.push((
            r.try_get::<Option<String>>("", "d").map_err(|e| decode_err("d", e))?,
            FleetSnapshotRow {
                equipment_id: r
                    .try_get("", "equipment_id")
                    .map_err(|e| decode_err("equipment_id", e))?,
                data_quality_score: r
                    .try_get("", "data_quality_score")
                    .map_err(|e| decode_err("data_quality_score", e))?,
                repairable: repairable != 0,
                event_count: r.try_get("", "event_count").map_err(|e| decode_err("event_count", e))?,
                exposure_hours: r
                    .try_get("", "exposure_hours")
                    .map_err(|e| decode_err("exposure_hours", e))?,
                repair_event_count: r
                    .try_get("", "repair_event_count")
                    .map_err(|e| decode_err("repair_event_count", e))?,
                active_repair_hours_total: r
                    .try_get("", "active_repair_hours_total")
                    .map_err(|e| decode_err("active_repair_hours_total", e))?,
                downtime_hours_total: r
                    .try_get("", "downtime_hours_total")
                    .map_err(|e| decode_err("downtime_hours_total", e))?,
            },
        ));
    }
    Ok(out)
}

/// Pooled fleet KPIs over the latest snapshot per equipment in `window`.
pub async fn pooled_fleet_kpis(
    db: &DatabaseConnection,
    window: FleetWindow,
    org_node_id: Option<i64>,
    min_dq: f64,
) -> AppResult<FleetPooledKpis> {
    let rows: Vec<FleetSnapshotRow> = load_latest_rows(db, window, org_node_id, false)
        .await?
        .into_iter()
        .map(|(_, r)| r)
        .collect();
    Ok(pool_fleet(&rows, min_dq))
}

/// Pooled fleet KPIs per snapshot date (`date(period_end)`), latest snapshot per equipment per date.
pub async fn pooled_fleet_series(
    db: &DatabaseConnection,
    window: FleetWindow,
    org_node_id: Option<i64>,
    min_dq: f64,
) -> AppResult<Vec<(String, FleetPooledKpis)>> {
    let mut by_date: std::collections::BTreeMap<String, Vec<FleetSnapshotRow>> = std::collections::BTreeMap::new();
    for (d, r) in load_latest_rows(db, window, org_node_id, true).await? {
        if let Some(d) = d {
            by_date.entry(d).or_default().push(r);
        }
    }
    Ok(by_date
        .into_iter()
        .map(|(d, rows)| (d, pool_fleet(&rows, min_dq)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, dq: f64, t: f64, f: i64, rep_n: i64, rep_h: f64, down: f64) -> FleetSnapshotRow {
        FleetSnapshotRow {
            equipment_id: id,
            data_quality_score: dq,
            repairable: true,
            event_count: f,
            exposure_hours: Some(t),
            repair_event_count: Some(rep_n),
            active_repair_hours_total: Some(rep_h),
            downtime_hours_total: Some(down),
        }
    }

    /// A: 10 h, 1 failure (MTBF 10). B: 9 990 h, 9 failures (MTBF 1 110).
    /// Mean of ratios would be 560 h; the pooled fleet MTBF is 10 000 / 10 = 1 000 h.
    #[test]
    fn mtbf_is_ratio_of_sums_not_mean_of_ratios() {
        let rows = [
            row(1, 1.0, 10.0, 1, 1, 2.0, 4.0),
            row(2, 1.0, 9_990.0, 9, 9, 18.0, 36.0),
        ];
        let k = pool_fleet(&rows, 0.85);
        assert_eq!(k.mtbf_hours, Some(1_000.0));
        assert_eq!(k.mttr_hours, Some(2.0));
        assert_eq!(k.availability_inherent, Some(10_000.0 / 10_020.0));
        assert_eq!(k.availability_operational, Some(10_000.0 / 10_040.0));
        assert_eq!(
            (k.equipment_count, k.qualified_equipment_count, k.total_failure_events),
            (2, 2, 10)
        );
    }

    #[test]
    fn low_dq_assets_are_excluded_from_ratios_but_counted() {
        let rows = [
            row(1, 1.0, 1_000.0, 5, 5, 10.0, 20.0),
            row(2, 0.2, 50.0, 1, 1, 30.0, 30.0),
        ];
        let k = pool_fleet(&rows, 0.85);
        assert_eq!(k.mtbf_hours, Some(200.0));
        assert_eq!(k.mttr_hours, Some(2.0));
        assert_eq!((k.equipment_count, k.qualified_equipment_count), (2, 1));
        assert_eq!(k.mean_data_quality_score, Some(0.6));
    }

    #[test]
    fn non_repairable_and_legacy_rows_do_not_enter_mtbf_inputs_they_lack() {
        let mut nr = row(1, 1.0, 500.0, 1, 0, 0.0, 0.0);
        nr.repairable = false;
        let legacy = FleetSnapshotRow {
            exposure_hours: Some(800.0),
            repair_event_count: None,
            active_repair_hours_total: None,
            downtime_hours_total: None,
            ..row(2, 1.0, 0.0, 4, 0, 0.0, 0.0)
        };
        let k = pool_fleet(&[nr, legacy], 0.85);
        assert_eq!(k.mtbf_hours, Some(200.0));
        assert_eq!(k.mttr_hours, None);
        assert_eq!(k.availability_operational, Some(1.0));
    }

    #[test]
    fn empty_fleet_has_no_values() {
        assert_eq!(pool_fleet(&[], 0.85), FleetPooledKpis::default());
    }
}
