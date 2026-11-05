//! One live calculation of the fleet reliability figures (MTBF, MTTR, A_o, data quality) for a
//! scope = period + equipment class + org node (descendants included). The RAMS home page, the
//! RAMS data-quality page and the Analytique dashboards read their figures from here, so the same
//! filters always give the same numbers (Advisor decisions L2-10 / L2-11, 2026-11-04).
//!
//! # Scope
//! Failure records = `failure_events` that are unplanned (`is_planned = 0`), of equipment not
//! deleted, dated by `COALESCE(failed_at, detected_at, created_at)` inside `[start, end]` (`end`
//! clipped to now), of the equipment class when one is chosen, installed at the org node or below
//! it when one is chosen. The fleet = the equipment of the class / org node (with or without
//! failures): every one of them adds its hours.
//!
//! The decisions also ask to clip the window to the observation start of imported history
//! ("Historique complet depuis le"); no such date is stored anywhere yet, so the window is not
//! clipped (nothing is invented in its place).
//!
//! # Definitions
//! - **Hours (T)** per equipment: operating hours when it has run-hour data (meter, runtime log,
//!   operating schedule; `study::clock`), calendar hours otherwise. The mix is returned
//!   (`operating_basis_count` / `calendar_basis_count`) so every screen states it.
//! - **MTBF** = Σ T over the fleet / number of failures flagged MTBF-eligible
//!   (`eligible_unplanned_mtbf`, ISO 14224; same parser as `compute::eligible_unplanned_mtbf`).
//!   Failures not flagged are counted in `mtbf_excluded_not_eligible`.
//! - **MTTR** = mean time to RESTORE (EN 13306): mean of `restored_at − failed_at` over the
//!   failures that have both times in the right order. Failures without `failed_at`, without
//!   `restored_at`, or restored before they failed (or with an unreadable time) are left out and
//!   counted, each under the first reason that applies.
//! - **Mean active repair time** (second, separately labelled figure): mean of
//!   `active_repair_hours` over the failures where it is recorded (> 0); the others are counted in
//!   `repair_excluded_missing_hours`.
//! - **A_o** = MTBF / (MTBF + MTTR) with the two figures above (decision: A_o uses the restore
//!   MTTR). Undefined when either is undefined.
//! - MTBF, MTTR, mean active repair and A_o are given only for windows of at least
//!   [`MIN_RATE_WINDOW_DAYS`] days; shorter windows give counts only (`window_too_short`).
//! - **Complete record** (data quality): a failure mode (a reference code), a cause or "cause not
//!   determined", and a mechanism (the mechanism list holds its own "unknown" value). The coding of
//!   the work order close-out is used where the failure record lacks it (the record is written at
//!   close and the close-out coding may be completed after). Data quality = complete / records.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::errors::AppResult;
use crate::reliability::study::clock::{load_clock, parse_instant, SOURCE_CALENDAR};
use crate::reliability::study::domain::{FilterOption, FleetKpis};
use crate::reliability::study::store::{get, opt_i64};

/// Shortest window, in days, for which MTBF, MTTR and A_o are given (Advisor Q2: 7 / 30 days give
/// counts only, "période trop courte pour le MTBF").
pub const MIN_RATE_WINDOW_DAYS: i64 = 90;

/// Period, equipment class and org node of a fleet calculation.
#[derive(Debug, Clone, Copy)]
pub struct FleetScope {
    pub start: DateTime<Utc>,
    /// Already clipped to now.
    pub end: DateTime<Utc>,
    pub equipment_class_id: Option<i64>,
    /// Equipment installed at this node or below it.
    pub org_node_id: Option<i64>,
}

impl FleetScope {
    /// Scope from RFC 3339 bounds; an end in the future is clipped to now.
    pub fn new(
        period_start: &str,
        period_end: &str,
        equipment_class_id: Option<i64>,
        org_node_id: Option<i64>,
    ) -> AppResult<Self> {
        let start = parse_instant(period_start)?;
        let end = parse_instant(period_end)?.min(crate::clock::now());
        Ok(Self {
            start,
            end,
            equipment_class_id,
            org_node_id,
        })
    }

    /// Scope of the last `days` days up to now (Analytique windows).
    #[must_use]
    pub fn last_days(days: i64, equipment_class_id: Option<i64>, org_node_id: Option<i64>) -> Self {
        let end = crate::clock::now();
        Self {
            start: end - chrono::Duration::days(days),
            end,
            equipment_class_id,
            org_node_id,
        }
    }

    #[must_use]
    pub fn window_too_short(&self) -> bool {
        self.end - self.start < chrono::Duration::days(MIN_RATE_WINDOW_DAYS)
    }

    /// Bind values of [`scope_from_sql`], in order.
    pub(crate) fn binds(&self) -> Vec<Value> {
        vec![
            self.start.to_rfc3339().into(),
            self.end.to_rfc3339().into(),
            opt_i64(self.equipment_class_id),
            opt_i64(self.equipment_class_id),
            opt_i64(self.org_node_id),
            opt_i64(self.org_node_id),
        ]
    }
}

/// `FROM … WHERE …` of the failure records of a scope (6 binds: [`FleetScope::binds`]). Aliases:
/// `fe` failure record, `e` equipment, `n` its org node, `wo` source work order, `wfd` its
/// close-out coding, `di` the request the work order came from.
pub(crate) fn scope_from_sql() -> String {
    format!(
        "FROM failure_events fe
         JOIN equipment e ON e.id = fe.equipment_id AND e.deleted_at IS NULL
         LEFT JOIN equipment_classes ec ON ec.id = e.class_id
         LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
         LEFT JOIN work_orders wo ON fe.source_type = 'work_order' AND wo.id = fe.source_id
         LEFT JOIN work_order_failure_details wfd ON wfd.id = {wfd}
         LEFT JOIN intervention_requests di ON di.id = wo.source_di_id
         WHERE COALESCE(fe.is_planned, 0) = 0
           AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) >= datetime(?)
           AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) <= datetime(?)
           AND (? IS NULL OR e.class_id = ?)
           AND (? IS NULL OR n.ancestor_path LIKE '%/' || ? || '/%')",
        wfd = crate::wo::closeout::failure_detail_of_wo_sql("wo.id")
    )
}

/// Failure mode of a record (close-out coding as fallback).
pub(crate) const MODE_SQL: &str = "COALESCE(fe.failure_mode_id, wfd.failure_mode_id)";
/// Cause coded: a cause, or "cause not determined" on the record or its close-out coding.
pub(crate) const CAUSE_CODED_SQL: &str = "(COALESCE(fe.failure_cause_id, wfd.failure_cause_id) IS NOT NULL \
     OR COALESCE(fe.cause_not_determined, 0) = 1 OR COALESCE(wfd.cause_not_determined, 0) = 1)";
/// Failure mechanism of a record (close-out coding as fallback).
pub(crate) const MECHANISM_SQL: &str = "COALESCE(fe.failure_mechanism_id, wfd.failure_mechanism_id)";

/// The one completeness predicate (home card, quality page and its gap list).
pub(crate) fn complete_sql() -> String {
    format!("({MODE_SQL} IS NOT NULL AND {CAUSE_CODED_SQL} AND {MECHANISM_SQL} IS NOT NULL)")
}

/// MTBF eligibility, read exactly as `compute::eligible_unplanned_mtbf`: the flag is JSON `true`
/// or the integer 1; invalid JSON or any other value is not eligible.
/// (Nested `CASE`: SQLite evaluates a `CASE` lazily, so the JSON functions never see invalid JSON.)
const ELIGIBLE_SQL: &str = "(CASE WHEN json_valid(fe.eligible_flags_json) THEN
       (CASE WHEN json_type(fe.eligible_flags_json, '$.eligible_unplanned_mtbf') IN ('true', 'integer')
              AND json_extract(fe.eligible_flags_json, '$.eligible_unplanned_mtbf') = 1
         THEN 1 ELSE 0 END)
     ELSE 0 END)";

/// Restore time of a record in hours (only meaningful when both times exist).
const RESTORE_HOURS_SQL: &str = "((julianday(fe.restored_at) - julianday(fe.failed_at)) * 24.0)";

/// Failure records of one equipment within a scope.
#[derive(Debug, Clone, Default)]
pub(crate) struct AssetAgg {
    pub equipment_id: i64,
    pub asset_code: String,
    pub asset_name: String,
    pub class_id: Option<i64>,
    pub class_name: Option<String>,
    pub failures: i64,
    pub eligible: i64,
    pub downtime: f64,
    pub restore_hours: f64,
    pub restore_count: i64,
    pub missing_failed_at: i64,
    pub missing_restored_at: i64,
    pub restore_inconsistent: i64,
    pub repair_hours: f64,
    pub repair_count: i64,
    pub complete: i64,
    pub missing_mode: i64,
    pub missing_cause: i64,
    pub missing_mechanism: i64,
    /// Records incomplete or whose request was observed more than 1 h before the failure time.
    pub to_check: i64,
}

/// Per-equipment aggregates of the failure records of a scope, ranked by downtime.
pub(crate) async fn load_asset_aggs(db: &impl ConnectionTrait, scope: &FleetScope) -> AppResult<Vec<AssetAgg>> {
    let complete = complete_sql();
    let has_times = "fe.failed_at IS NOT NULL AND fe.restored_at IS NOT NULL";
    let sql = format!(
        "SELECT e.id, e.asset_id_code, e.name, e.class_id, ec.name AS class_name,
                COUNT(fe.id) AS failures,
                SUM({ELIGIBLE_SQL}) AS eligible,
                COALESCE(SUM(fe.downtime_duration_hours), 0.0) AS downtime,
                COALESCE(SUM(CASE WHEN {has_times} AND {RESTORE_HOURS_SQL} >= 0 THEN {RESTORE_HOURS_SQL} ELSE 0.0 END), 0.0)
                    AS restore_hours,
                SUM(CASE WHEN {has_times} AND {RESTORE_HOURS_SQL} >= 0 THEN 1 ELSE 0 END) AS restore_count,
                SUM(CASE WHEN fe.failed_at IS NULL THEN 1 ELSE 0 END) AS missing_failed_at,
                SUM(CASE WHEN fe.failed_at IS NOT NULL AND fe.restored_at IS NULL THEN 1 ELSE 0 END)
                    AS missing_restored_at,
                SUM(CASE WHEN {has_times} AND COALESCE({RESTORE_HOURS_SQL} >= 0, 0) = 0 THEN 1 ELSE 0 END)
                    AS restore_inconsistent,
                COALESCE(SUM(CASE WHEN fe.active_repair_hours > 0 THEN fe.active_repair_hours ELSE 0.0 END), 0.0)
                    AS repair_hours,
                SUM(CASE WHEN fe.active_repair_hours > 0 THEN 1 ELSE 0 END) AS repair_count,
                SUM(CASE WHEN {complete} THEN 1 ELSE 0 END) AS complete,
                SUM(CASE WHEN {MODE_SQL} IS NULL THEN 1 ELSE 0 END) AS missing_mode,
                SUM(CASE WHEN NOT {CAUSE_CODED_SQL} THEN 1 ELSE 0 END) AS missing_cause,
                SUM(CASE WHEN {MECHANISM_SQL} IS NULL THEN 1 ELSE 0 END) AS missing_mechanism,
                SUM(CASE WHEN NOT {complete}
                          OR (di.id IS NOT NULL AND fe.failed_at IS NOT NULL
                              AND (julianday(fe.failed_at) - julianday(COALESCE(di.observed_at, di.submitted_at))) * 24.0 > 1.0)
                     THEN 1 ELSE 0 END) AS to_check
         {from}
         GROUP BY e.id
         ORDER BY downtime DESC, failures DESC, e.asset_id_code ASC",
        from = scope_from_sql()
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, scope.binds()))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(AssetAgg {
                equipment_id: get(r, "id")?,
                asset_code: get(r, "asset_id_code")?,
                asset_name: get(r, "name")?,
                class_id: get(r, "class_id")?,
                class_name: get(r, "class_name")?,
                failures: get(r, "failures")?,
                eligible: get(r, "eligible")?,
                downtime: get(r, "downtime")?,
                restore_hours: get(r, "restore_hours")?,
                restore_count: get(r, "restore_count")?,
                missing_failed_at: get(r, "missing_failed_at")?,
                missing_restored_at: get(r, "missing_restored_at")?,
                restore_inconsistent: get(r, "restore_inconsistent")?,
                repair_hours: get(r, "repair_hours")?,
                repair_count: get(r, "repair_count")?,
                complete: get(r, "complete")?,
                missing_mode: get(r, "missing_mode")?,
                missing_cause: get(r, "missing_cause")?,
                missing_mechanism: get(r, "missing_mechanism")?,
                to_check: get(r, "to_check")?,
            })
        })
        .collect()
}

/// Coding completeness of the failure records of a scope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodingCounts {
    pub records: i64,
    pub complete: i64,
    pub missing_mode: i64,
    pub missing_cause: i64,
    pub missing_mechanism: i64,
}

impl CodingCounts {
    pub(crate) fn of(aggs: &[AssetAgg]) -> Self {
        Self {
            records: aggs.iter().map(|a| a.failures).sum(),
            complete: aggs.iter().map(|a| a.complete).sum(),
            missing_mode: aggs.iter().map(|a| a.missing_mode).sum(),
            missing_cause: aggs.iter().map(|a| a.missing_cause).sum(),
            missing_mechanism: aggs.iter().map(|a| a.missing_mechanism).sum(),
        }
    }

    /// Complete / records (0–1); `None` without records.
    #[must_use]
    pub fn share(&self) -> Option<f64> {
        (self.records > 0).then(|| self.complete as f64 / self.records as f64)
    }
}

/// Coding completeness of a scope (the quality page; the same numbers as [`fleet_kpis`]).
pub async fn coding_counts(db: &impl ConnectionTrait, scope: &FleetScope) -> AppResult<CodingCounts> {
    Ok(CodingCounts::of(&load_asset_aggs(db, scope).await?))
}

/// Hours of one equipment over the scope and their basis (`study::clock` source).
#[derive(Debug, Clone, Copy)]
pub(crate) struct AssetHours {
    pub hours: f64,
    pub source: &'static str,
}

/// Every equipment of the scope's fleet (with or without failures) and its hours.
pub(crate) async fn fleet_hours(db: &DatabaseConnection, scope: &FleetScope) -> AppResult<HashMap<i64, AssetHours>> {
    let fleet = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id FROM equipment e LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
             WHERE e.deleted_at IS NULL
               AND (? IS NULL OR e.class_id = ?)
               AND (? IS NULL OR n.ancestor_path LIKE '%/' || ? || '/%')",
            [
                opt_i64(scope.equipment_class_id),
                opt_i64(scope.equipment_class_id),
                opt_i64(scope.org_node_id),
                opt_i64(scope.org_node_id),
            ],
        ))
        .await?;
    let mut out = HashMap::with_capacity(fleet.len());
    for r in &fleet {
        let id: i64 = get(r, "id")?;
        let clock = load_clock(db, id).await?;
        out.insert(
            id,
            AssetHours {
                hours: clock.hours_between(scope.start, scope.end),
                source: clock.source(),
            },
        );
    }
    Ok(out)
}

fn mean(sum: f64, n: i64) -> Option<f64> {
    (n > 0 && sum.is_finite()).then(|| sum / n as f64)
}

/// The fleet figures from per-equipment aggregates and hours (definitions: module header).
pub(crate) fn kpis_of(scope: &FleetScope, aggs: &[AssetAgg], hours: &HashMap<i64, AssetHours>) -> FleetKpis {
    let sum_i = |f: fn(&AssetAgg) -> i64| aggs.iter().map(f).sum::<i64>();
    let sum_f = |f: fn(&AssetAgg) -> f64| aggs.iter().map(f).sum::<f64>();
    let exposure: f64 = hours.values().map(|h| h.hours.max(0.0)).sum();
    let operating = hours.values().filter(|h| h.source != SOURCE_CALENDAR).count() as i64;
    let failures = sum_i(|a| a.failures);
    let eligible = sum_i(|a| a.eligible);
    let coding = CodingCounts::of(aggs);
    let too_short = scope.window_too_short();
    let rate = |v: Option<f64>| if too_short { None } else { v };

    let mtbf = rate((eligible > 0 && exposure > 0.0).then(|| exposure / eligible as f64));
    let mttr = rate(mean(sum_f(|a| a.restore_hours), sum_i(|a| a.restore_count)));
    let repair = rate(mean(sum_f(|a| a.repair_hours), sum_i(|a| a.repair_count)));
    let ao = match (mtbf, mttr) {
        (Some(b), Some(r)) if b + r > 0.0 => Some(b / (b + r)),
        _ => None,
    };
    FleetKpis {
        period_start: scope.start.to_rfc3339(),
        period_end: scope.end.to_rfc3339(),
        window_too_short: too_short,
        equipment_count: hours.len() as i64,
        failure_count: failures,
        downtime_hours: sum_f(|a| a.downtime),
        exposure_hours: exposure,
        availability_operational: ao,
        mtbf_hours: mtbf,
        eligible_failure_count: eligible,
        mtbf_excluded_not_eligible: failures - eligible,
        mttr_hours: mttr,
        restore_count: sum_i(|a| a.restore_count),
        restore_excluded_missing_failed_at: sum_i(|a| a.missing_failed_at),
        restore_excluded_missing_restored_at: sum_i(|a| a.missing_restored_at),
        restore_excluded_inconsistent: sum_i(|a| a.restore_inconsistent),
        mean_active_repair_hours: repair,
        active_repair_count: sum_i(|a| a.repair_count),
        repair_excluded_missing_hours: failures - sum_i(|a| a.repair_count),
        data_quality: coding.share(),
        records_complete: coding.complete,
        records_missing_mode: coding.missing_mode,
        records_missing_cause: coding.missing_cause,
        records_missing_mechanism: coding.missing_mechanism,
        operating_basis_count: operating,
        calendar_basis_count: hours.len() as i64 - operating,
    }
}

/// The fleet figures of a scope — the entry point Analytique and any other screen call.
pub async fn fleet_kpis(db: &DatabaseConnection, scope: &FleetScope) -> AppResult<FleetKpis> {
    let aggs = load_asset_aggs(db, scope).await?;
    let hours = fleet_hours(db, scope).await?;
    Ok(kpis_of(scope, &aggs, &hours))
}

/// Org nodes holding equipment and equipment classes in use (the filters of the fleet screens).
pub(crate) async fn filter_options(db: &DatabaseConnection) -> AppResult<(Vec<FilterOption>, Vec<FilterOption>)> {
    let org_nodes = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT n.id, n.name, n.depth, n.ancestor_path FROM org_nodes n
             WHERE n.deleted_at IS NULL AND EXISTS (
                SELECT 1 FROM equipment e JOIN org_nodes h ON h.id = e.installed_at_node_id
                WHERE e.deleted_at IS NULL AND h.ancestor_path LIKE '%/' || n.id || '/%')
             ORDER BY n.ancestor_path ASC"
                .to_string(),
        ))
        .await?
        .iter()
        .map(|r| {
            Ok(FilterOption {
                id: get(r, "id")?,
                label: get(r, "name")?,
                depth: get(r, "depth")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let equipment_classes = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT DISTINCT ec.id, ec.name FROM equipment_classes ec
             JOIN equipment e ON e.class_id = ec.id AND e.deleted_at IS NULL
             ORDER BY ec.name ASC"
                .to_string(),
        ))
        .await?
        .iter()
        .map(|r| {
            Ok(FilterOption {
                id: get(r, "id")?,
                label: get(r, "name")?,
                depth: 0,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((org_nodes, equipment_classes))
}
