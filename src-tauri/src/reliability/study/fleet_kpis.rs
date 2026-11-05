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
//! failures) in service during the window: every one of them adds its hours.
//!
//! A failure is dated once, by `COALESCE(failed_at, detected_at, created_at)`: a failure dated
//! inside the window counts with its full restore time even when it is restored after the window
//! end, and a failure dated before the window start is not counted (even if restored inside it).
//!
//! The decisions also ask to clip the window to the observation start of imported history
//! ("Historique complet depuis le"); no such date is stored anywhere yet, so the window is not
//! clipped (nothing is invented in its place).
//!
//! # Definitions
//! - **Hours (T)** per equipment, over its own window = the scope window clipped to
//!   `[equipment.commissioning_date, equipment.decommissioned_at]` where they are set and
//!   readable (equipment not in service at all during the window is not part of the fleet):
//!   operating hours when it has run-hour data (meter, runtime log, operating schedule;
//!   `study::clock`) covering at least [`MIN_OPERATING_COVERAGE`] of its window, calendar hours
//!   of its window otherwise (a meter or runtime log that does not cover the window would
//!   under-count; the equipment is then counted on the calendar basis, not as operating). Above
//!   the threshold the uncovered part (at most 5 % of the window) adds no hours: nothing is
//!   extrapolated. A schedule-based clock always covers its window. The mix
//!   is returned (`operating_basis_count` / `calendar_basis_count`) so every screen states it.
//! - **MTBF** = Σ T over the fleet / number of unplanned failures of the scope (CMMS Advisor
//!   ruling, 2026-11-05). Eligibility is evaluated live from the record, never from the frozen
//!   `eligible_flags_json` flag: every unplanned failure (`is_planned = 0`; a work order raises a
//!   failure record only when its type is corrective) with an equipment and a failure date
//!   counts, whatever its coding — cause "not determined" and no mode at all count too (they only
//!   lower the coding completeness below). Left out of the MTBF only: a failure without
//!   equipment (impossible: `failure_events.equipment_id` is NOT NULL) and a failure without a
//!   readable date (`mtbf_excluded_missing_date`: unplanned records of the class / org node whose
//!   `COALESCE(failed_at, detected_at, created_at)` cannot be read, so they belong to no window).
//!   No action exists for a person to exclude a failure from the MTBF, so none is read.
//! - **MTTR** = mean time to RESTORE (EN 13306): mean of `restored_at − failed_at` over the
//!   failures that have both times in the right order. Failures without `failed_at`, without
//!   `restored_at`, or restored before they failed (or with an unreadable time) are left out and
//!   counted, each under the first reason that applies. For a record raised from a work order the
//!   stored times are the work start (`actual_start`) and the work-order close (`closed_at`), so
//!   the MTTR of those records is work start → close; imported records carry failure → restore.
//! - **Mean repair person-hours** (second, separately labelled figure): mean of
//!   `active_repair_hours` (for a work order: the Σ of the interveners' hours worked) over the
//!   failures where it is recorded (> 0); the others are counted in
//!   `repair_excluded_missing_hours`.
//! - **A_o** = MTBF / (MTBF + MTTR) with the two figures above (decision: A_o uses the restore
//!   MTTR). Undefined when either is undefined.
//! - MTBF, MTTR, mean repair person-hours and A_o are given only for windows of at least
//!   [`MIN_RATE_WINDOW_DAYS`] days; shorter windows give counts only (`window_too_short`).
//! - **Complete record** (data quality): a failure mode (a reference code), a cause or "cause not
//!   determined", and a mechanism (the mechanism list holds its own "unknown" value). The coding of
//!   the work order close-out is used where the failure record lacks it (the record is written at
//!   close and the close-out coding may be completed after). Data quality = complete / records.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};

use crate::errors::AppResult;
use crate::reliability::study::clock::{load_clock, parse_instant, parse_instant_opt, StudyClock, SOURCE_CALENDAR};
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

/// Least share of an equipment's window that its meter or runtime log must cover for its
/// operating hours to be used; below it the equipment counts calendar hours (module header).
pub const MIN_OPERATING_COVERAGE: f64 = 0.95;

/// A stored instant: RFC 3339, SQLite `YYYY-MM-DD HH:MM:SS` or a bare `YYYY-MM-DD` (start of the
/// day, UTC). Unreadable or empty values give `None` (no clipping).
fn stored_instant(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw?.trim();
    parse_instant_opt(Some(raw)).or_else(|| {
        chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map(|n| n.and_utc())
    })
}

/// The scope window of one equipment: clipped to its commissioning and decommissioning dates.
/// `None` when it was not in service at any time of the window.
pub(crate) fn asset_window(
    scope: &FleetScope,
    commissioned: Option<DateTime<Utc>>,
    decommissioned: Option<DateTime<Utc>>,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let lo = commissioned.map_or(scope.start, |c| c.max(scope.start));
    let hi = decommissioned.map_or(scope.end, |d| d.min(scope.end));
    (hi > lo).then_some((lo, hi))
}

/// Hours of one equipment over its window: operating hours when its clock covers the window
/// (at least [`MIN_OPERATING_COVERAGE`]), calendar hours otherwise.
pub(crate) fn asset_hours(clock: &StudyClock, lo: DateTime<Utc>, hi: DateTime<Utc>) -> AssetHours {
    if clock.is_operating() && clock.coverage(lo, hi) >= MIN_OPERATING_COVERAGE {
        AssetHours {
            hours: clock.hours_between(lo, hi),
            source: clock.source(),
        }
    } else {
        AssetHours {
            hours: StudyClock::Calendar.hours_between(lo, hi),
            source: SOURCE_CALENDAR,
        }
    }
}

/// Every equipment of the scope's fleet in service during the window (with or without failures)
/// and its hours.
pub(crate) async fn fleet_hours(db: &DatabaseConnection, scope: &FleetScope) -> AppResult<HashMap<i64, AssetHours>> {
    let fleet = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT e.id, e.commissioning_date, e.decommissioned_at
             FROM equipment e LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
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
        let commissioned: Option<String> = get(r, "commissioning_date")?;
        let decommissioned: Option<String> = get(r, "decommissioned_at")?;
        let Some((lo, hi)) = asset_window(
            scope,
            stored_instant(commissioned.as_deref()),
            stored_instant(decommissioned.as_deref()),
        ) else {
            continue;
        };
        let clock = load_clock(db, id).await?;
        out.insert(id, asset_hours(&clock, lo, hi));
    }
    Ok(out)
}

/// Unplanned failure records of the scope's class / org node whose failure date cannot be read:
/// they belong to no window, so they are left out of the MTBF (and counted).
pub(crate) async fn missing_date_count(db: &impl ConnectionTrait, scope: &FleetScope) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(fe.id) AS n FROM failure_events fe
             JOIN equipment e ON e.id = fe.equipment_id AND e.deleted_at IS NULL
             LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id
             WHERE COALESCE(fe.is_planned, 0) = 0
               AND datetime(COALESCE(fe.failed_at, fe.detected_at, fe.created_at)) IS NULL
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
    match row {
        Some(r) => get(&r, "n"),
        None => Ok(0),
    }
}

fn mean(sum: f64, n: i64) -> Option<f64> {
    (n > 0 && sum.is_finite()).then(|| sum / n as f64)
}

/// The fleet figures from per-equipment aggregates and hours (definitions: module header).
/// `missing_date` = [`missing_date_count`] of the scope.
pub(crate) fn kpis_of(
    scope: &FleetScope,
    aggs: &[AssetAgg],
    hours: &HashMap<i64, AssetHours>,
    missing_date: i64,
) -> FleetKpis {
    let sum_i = |f: fn(&AssetAgg) -> i64| aggs.iter().map(f).sum::<i64>();
    let sum_f = |f: fn(&AssetAgg) -> f64| aggs.iter().map(f).sum::<f64>();
    let exposure: f64 = hours.values().map(|h| h.hours.max(0.0)).sum();
    let operating = hours.values().filter(|h| h.source != SOURCE_CALENDAR).count() as i64;
    let failures = sum_i(|a| a.failures);
    // Every unplanned failure of the scope counts (CMMS Advisor ruling 2026-11-05).
    let eligible = failures;
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
        mtbf_excluded_missing_date: missing_date,
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
    let missing_date = missing_date_count(db, scope).await?;
    Ok(kpis_of(scope, &aggs, &hours, missing_date))
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
