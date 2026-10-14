//! Maintenance management indicators in the spirit of the SMRP Best Practice metrics, computed
//! from the records Maintafox keeps. Every value carries its numerator and denominator so the
//! screen and the reports can show how it was obtained; nothing is estimated when data is missing.
//!
//! | Indicator | Formula (window `[from, to)`) |
//! |---|---|
//! | Schedule compliance | committed hours whose work order was completed by the end of the committed day ÷ committed hours (`schedule_commitments`, commitments ending in the window) |
//! | Planned / reactive / emergency work | labour hours (`work_order_interveners`, else `actual_duration_hours`) of work orders completed in the window, by type; work-order counts when no hours are recorded |
//! | PM compliance | PM occurrences done inside their tolerance window (not early, not late; window = `window_start_at`–`window_end_at`, else the due day) ÷ occurrences due in the window that are done or whose window has closed (cancelled excluded) — `pm::compliance`, the same number as the PM page |
//! | Backlog | planned hours of work orders not done ÷ weekly crew hours (active personnel × weekly hours of their work schedule) |
//! | Cost / replacement asset value | annualised cost of work orders closed in the window on assets with a replacement value ÷ their total replacement value |
//!
//! Wrench time is not computed: it needs labour activity sampling, which is not recorded.

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TryGetable, Value};
use serde::Serialize;

use crate::analytics::as_of::{self, instant};
use crate::errors::AppResult;

/// Work-order type codes counted as planned / proactive work.
pub const PLANNED_TYPE_CODES: &[&str] = &["preventive", "condition_based", "inspection", "overhaul", "improvement"];
/// Work-order type codes counted as reactive work (emergency is a subset reported on its own).
pub const REACTIVE_TYPE_CODES: &[&str] = &["corrective", "emergency"];
pub const EMERGENCY_TYPE_CODE: &str = "emergency";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmrpWindow {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

impl SmrpWindow {
    /// The `days` days ending at `now`.
    pub fn last_days(now: DateTime<Utc>, days: i64) -> Self {
        Self {
            from: now - Duration::days(days),
            to: now,
        }
    }

    /// The window of equal length just before this one.
    pub fn previous(self) -> Self {
        let len = self.to - self.from;
        Self {
            from: self.from - len,
            to: self.from,
        }
    }

    pub fn days(self) -> f64 {
        (self.to - self.from).num_seconds() as f64 / 86_400.0
    }

    /// Never look past `now` (work scheduled or due later cannot be judged yet).
    fn until(self, now: DateTime<Utc>) -> DateTime<Utc> {
        self.to.min(now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SmrpRatio {
    pub numerator: f64,
    pub denominator: f64,
    /// `numerator ÷ denominator × 100`; `None` when nothing was measured.
    pub value_pct: Option<f64>,
}

impl SmrpRatio {
    pub fn of(numerator: f64, denominator: f64) -> Self {
        Self {
            numerator,
            denominator,
            value_pct: (denominator > 0.0).then(|| numerator / denominator * 100.0),
        }
    }
}

/// A ratio over the window and the same ratio over the previous window of equal length.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SmrpMetric {
    pub current: SmrpRatio,
    pub previous: SmrpRatio,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SmrpWorkMix {
    /// `"hours"` when labour hours are recorded on the completed work orders, else `"count"`.
    pub basis: String,
    pub planned: SmrpMetric,
    pub reactive: SmrpMetric,
    pub emergency: SmrpMetric,
    /// Completed work orders in the window (all types).
    pub work_orders: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SmrpBacklog {
    /// Planned hours of work orders not done (draft to on hold).
    pub backlog_hours: f64,
    /// Same measure at the start of the window.
    pub previous_backlog_hours: f64,
    /// Planned hours of work orders in status `ready`.
    pub ready_backlog_hours: f64,
    pub open_work_orders: i64,
    /// Open work orders without an estimate (counted as 0 h).
    pub work_orders_without_estimate: i64,
    /// Weekly crew hours used as capacity.
    pub weekly_capacity_hours: f64,
    /// Active personnel whose work schedule gave the capacity.
    pub people_counted: i64,
    /// Active personnel without a work schedule (not in the capacity).
    pub people_without_schedule: i64,
    pub weeks: Option<f64>,
    pub previous_weeks: Option<f64>,
    pub ready_weeks: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SmrpCostToRav {
    /// Cost of work orders closed in the window on assets that have a replacement value.
    pub period_cost: f64,
    pub annualized_cost: f64,
    pub replacement_value_total: f64,
    pub assets_with_value: i64,
    pub assets_total: i64,
    pub value_pct: Option<f64>,
    pub previous_value_pct: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SmrpKpis {
    pub from: String,
    pub to: String,
    pub days: f64,
    pub schedule_compliance: Option<SmrpMetric>,
    pub work_mix: Option<SmrpWorkMix>,
    pub pm_compliance: Option<SmrpMetric>,
    pub backlog: Option<SmrpBacklog>,
    pub cost_to_rav: Option<SmrpCostToRav>,
}

async fn one_row(db: &DatabaseConnection, sql: &str, binds: Vec<Value>) -> AppResult<Option<sea_orm::QueryResult>> {
    Ok(db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?)
}

fn f(row: &Option<sea_orm::QueryResult>, col: &str) -> f64 {
    row.as_ref()
        .and_then(|r| {
            f64::try_get(r, "", col)
                .ok()
                .or_else(|| i64::try_get(r, "", col).ok().map(|v| v as f64))
        })
        .unwrap_or(0.0)
}

/// Whether `table.column` exists (columns added by newer migrations are optional here).
pub async fn has_column(db: &DatabaseConnection, table: &str, column: &str) -> AppResult<bool> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    Ok(rows
        .iter()
        .any(|r| String::try_get(r, "", "name").map(|n| n == column).unwrap_or(false)))
}

// ── Schedule compliance ────────────────────────────────────────────────────────────────────

/// Committed hours and hours completed as scheduled, for commitments ending in the window.
pub async fn schedule_compliance(
    db: &DatabaseConnection,
    window: SmrpWindow,
    now: DateTime<Utc>,
    org: Option<i64>,
) -> AppResult<SmrpRatio> {
    let row = one_row(
        db,
        "WITH c AS ( \
           SELECT (julianday(sc.committed_end) - julianday(sc.committed_start)) * 24.0 AS hours, \
                  sc.committed_end AS committed_end, \
                  CASE sc.source_type \
                    WHEN 'work_order' THEN sc.source_id \
                    WHEN 'pm_occurrence' THEN (SELECT po.linked_work_order_id FROM pm_occurrences po \
                                               WHERE po.id = sc.source_id) \
                  END AS wo_id \
           FROM schedule_commitments sc \
           WHERE datetime(sc.committed_end) >= datetime(?) AND datetime(sc.committed_end) < datetime(?) \
             AND julianday(sc.committed_end) > julianday(sc.committed_start) \
         ) \
         SELECT COALESCE(SUM(c.hours), 0) AS scheduled, \
                COALESCE(SUM(CASE WHEN COALESCE(w.mechanically_completed_at, w.actual_end, w.closed_at) IS NOT NULL \
                  AND date(COALESCE(w.mechanically_completed_at, w.actual_end, w.closed_at)) <= date(c.committed_end) \
                  THEN c.hours ELSE 0 END), 0) AS done \
         FROM c LEFT JOIN work_orders w ON w.id = c.wo_id \
         WHERE (? IS NULL OR w.entity_id = ?)",
        vec![
            instant(window.from).into(),
            instant(window.until(now)).into(),
            org.into(),
            org.into(),
        ],
    )
    .await?;
    Ok(SmrpRatio::of(f(&row, "done"), f(&row, "scheduled")))
}

// ── Planned / reactive / emergency work ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct MixTotals {
    total_hours: f64,
    planned_hours: f64,
    reactive_hours: f64,
    emergency_hours: f64,
    total_count: f64,
    planned_count: f64,
    reactive_count: f64,
    emergency_count: f64,
}

async fn work_mix_totals(db: &DatabaseConnection, window: SmrpWindow, org: Option<i64>) -> AppResult<MixTotals> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.code AS type_code, COUNT(*) AS n, \
                    COALESCE(SUM(COALESCE( \
                      (SELECT SUM(i.hours_worked) FROM work_order_interveners i WHERE i.work_order_id = wo.id), \
                      wo.actual_duration_hours, 0)), 0) AS hours \
             FROM work_orders wo \
             JOIN work_order_types t ON t.id = wo.type_id \
             JOIN work_order_statuses s ON s.id = wo.status_id \
             WHERE s.code <> 'cancelled' \
               AND COALESCE(wo.mechanically_completed_at, wo.closed_at) IS NOT NULL \
               AND datetime(COALESCE(wo.mechanically_completed_at, wo.closed_at)) >= datetime(?) \
               AND datetime(COALESCE(wo.mechanically_completed_at, wo.closed_at)) < datetime(?) \
               AND (? IS NULL OR wo.entity_id = ?) \
             GROUP BY t.code",
            vec![
                instant(window.from).into(),
                instant(window.to).into(),
                org.into(),
                org.into(),
            ],
        ))
        .await?;
    let mut m = MixTotals::default();
    for r in rows {
        let code: String = r.try_get("", "type_code").unwrap_or_default();
        let n = i64::try_get(&r, "", "n").unwrap_or(0) as f64;
        let h = f64::try_get(&r, "", "hours").unwrap_or(0.0);
        m.total_hours += h;
        m.total_count += n;
        if PLANNED_TYPE_CODES.contains(&code.as_str()) {
            m.planned_hours += h;
            m.planned_count += n;
        }
        if REACTIVE_TYPE_CODES.contains(&code.as_str()) {
            m.reactive_hours += h;
            m.reactive_count += n;
        }
        if code == EMERGENCY_TYPE_CODE {
            m.emergency_hours += h;
            m.emergency_count += n;
        }
    }
    Ok(m)
}

fn mix_ratios(m: &MixTotals, hours: bool) -> (SmrpRatio, SmrpRatio, SmrpRatio) {
    if hours {
        (
            SmrpRatio::of(m.planned_hours, m.total_hours),
            SmrpRatio::of(m.reactive_hours, m.total_hours),
            SmrpRatio::of(m.emergency_hours, m.total_hours),
        )
    } else {
        (
            SmrpRatio::of(m.planned_count, m.total_count),
            SmrpRatio::of(m.reactive_count, m.total_count),
            SmrpRatio::of(m.emergency_count, m.total_count),
        )
    }
}

pub async fn work_mix(db: &DatabaseConnection, window: SmrpWindow, org: Option<i64>) -> AppResult<SmrpWorkMix> {
    let cur = work_mix_totals(db, window, org).await?;
    let prev = work_mix_totals(db, window.previous(), org).await?;
    let hours = cur.total_hours > 0.0;
    let (p, r, e) = mix_ratios(&cur, hours);
    let (pp, pr, pe) = mix_ratios(&prev, hours);
    Ok(SmrpWorkMix {
        basis: if hours { "hours" } else { "count" }.to_string(),
        planned: SmrpMetric {
            current: p,
            previous: pp,
        },
        reactive: SmrpMetric {
            current: r,
            previous: pr,
        },
        emergency: SmrpMetric {
            current: e,
            previous: pe,
        },
        work_orders: cur.total_count as i64,
    })
}

// ── PM compliance ──────────────────────────────────────────────────────────────────────────

/// PM compliance over the window, judged at `now`: the single definition of
/// `pm::compliance::compliance_between` (work done inside its tolerance window ÷ occurrences
/// due whose outcome is known), restricted to the assets of `org` when given.
pub async fn pm_compliance(
    db: &DatabaseConnection,
    window: SmrpWindow,
    now: DateTime<Utc>,
    org: Option<i64>,
) -> AppResult<SmrpRatio> {
    let report = crate::pm::compliance::compliance_between(
        db,
        window.from,
        window.until(now),
        now,
        crate::pm::compliance::ComplianceScope {
            pm_plan_id: None,
            org_node_id: org,
        },
    )
    .await?;
    Ok(SmrpRatio::of(report.on_time_count as f64, report.due_count as f64))
}

// ── Backlog ────────────────────────────────────────────────────────────────────────────────

fn minutes(hhmm: &str) -> Option<i64> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.get(..2).unwrap_or(m).parse().ok()?);
    (h <= 24 && m < 60).then_some(h * 60 + m)
}

/// Hours of one shift; a shift ending at or before its start runs past midnight.
pub fn shift_hours(start: &str, end: &str) -> Option<f64> {
    let (s, e) = (minutes(start)?, minutes(end)?);
    let mut d = e - s;
    if d <= 0 {
        d += 24 * 60;
    }
    Some(d as f64 / 60.0)
}

/// Weekly crew hours: Σ over active personnel of the working-day shifts of their home schedule.
/// Returns (hours, people counted, people without schedule).
pub async fn weekly_capacity(db: &DatabaseConnection, org: Option<i64>) -> AppResult<(f64, i64, i64)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT p.id AS pid, sd.shift_start AS s, sd.shift_end AS e \
             FROM personnel p \
             LEFT JOIN schedule_details sd ON sd.reference_value_id = p.home_schedule_reference_value_id \
                  AND sd.is_rest_day = 0 \
             WHERE COALESCE(p.employment_status, 'active') = 'active' \
               AND (p.termination_date IS NULL OR date(p.termination_date) >= date('now')) \
               AND (? IS NULL OR p.primary_entity_id = ?) \
             ORDER BY p.id",
            vec![org.into(), org.into()],
        ))
        .await?;
    use std::collections::BTreeMap;
    let mut per_person: BTreeMap<i64, f64> = BTreeMap::new();
    for r in rows {
        let pid = i64::try_get(&r, "", "pid").unwrap_or(0);
        let entry = per_person.entry(pid).or_insert(0.0);
        let (s, e) = (
            Option::<String>::try_get(&r, "", "s").ok().flatten(),
            Option::<String>::try_get(&r, "", "e").ok().flatten(),
        );
        if let (Some(s), Some(e)) = (s, e) {
            *entry += shift_hours(&s, &e).unwrap_or(0.0);
        }
    }
    let counted = per_person.values().filter(|h| **h > 0.0).count() as i64;
    let without = per_person.len() as i64 - counted;
    Ok((per_person.values().sum(), counted, without))
}

pub async fn backlog(
    db: &DatabaseConnection,
    window: SmrpWindow,
    now: DateTime<Utc>,
    org: Option<i64>,
) -> AppResult<SmrpBacklog> {
    let backlog_hours = as_of::wo_backlog_hours_at(db, now, org).await?;
    let previous_backlog_hours = as_of::wo_backlog_hours_at(db, window.from, org).await?;
    let row = one_row(
        db,
        "SELECT COUNT(*) AS n, \
                SUM(CASE WHEN COALESCE(wo.expected_duration_hours, 0) <= 0 THEN 1 ELSE 0 END) AS no_estimate, \
                COALESCE(SUM(CASE WHEN s.code = 'ready' THEN COALESCE(wo.expected_duration_hours, 0) END), 0) AS ready \
         FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE s.code NOT IN ('completed','closed','cancelled','mechanically_complete','technically_verified') \
           AND (? IS NULL OR wo.entity_id = ?)",
        vec![org.into(), org.into()],
    )
    .await?;
    let (capacity, counted, without) = weekly_capacity(db, org).await?;
    let weeks = |h: f64| (capacity > 0.0).then(|| h / capacity);
    let ready = f(&row, "ready");
    Ok(SmrpBacklog {
        backlog_hours,
        previous_backlog_hours,
        ready_backlog_hours: ready,
        open_work_orders: f(&row, "n") as i64,
        work_orders_without_estimate: f(&row, "no_estimate") as i64,
        weekly_capacity_hours: capacity,
        people_counted: counted,
        people_without_schedule: without,
        weeks: weeks(backlog_hours),
        previous_weeks: weeks(previous_backlog_hours),
        ready_weeks: weeks(ready),
    })
}

// ── Cost / replacement asset value ─────────────────────────────────────────────────────────

async fn cost_on_valued_assets(db: &DatabaseConnection, window: SmrpWindow, org: Option<i64>) -> AppResult<f64> {
    let row = one_row(
        db,
        "SELECT COALESCE(SUM(wo.total_cost), 0) AS total FROM work_orders wo \
         JOIN equipment e ON e.id = wo.equipment_id \
         WHERE wo.closed_at IS NOT NULL AND e.replacement_value > 0 AND e.deleted_at IS NULL \
           AND datetime(wo.closed_at) >= datetime(?) AND datetime(wo.closed_at) < datetime(?) \
           AND (? IS NULL OR e.installed_at_node_id = ?)",
        vec![
            instant(window.from).into(),
            instant(window.to).into(),
            org.into(),
            org.into(),
        ],
    )
    .await?;
    Ok(f(&row, "total"))
}

pub async fn cost_to_rav(db: &DatabaseConnection, window: SmrpWindow, org: Option<i64>) -> AppResult<SmrpCostToRav> {
    let assets = one_row(
        db,
        "SELECT COUNT(*) AS total, \
                COALESCE(SUM(CASE WHEN e.replacement_value > 0 THEN 1 ELSE 0 END), 0) AS valued, \
                COALESCE(SUM(CASE WHEN e.replacement_value > 0 THEN e.replacement_value END), 0) AS rav \
         FROM equipment e WHERE e.deleted_at IS NULL AND (? IS NULL OR e.installed_at_node_id = ?)",
        vec![org.into(), org.into()],
    )
    .await?;
    let rav = f(&assets, "rav");
    let period_cost = cost_on_valued_assets(db, window, org).await?;
    let previous_cost = cost_on_valued_assets(db, window.previous(), org).await?;
    let annual = |cost: f64| cost * 365.0 / window.days().max(1.0);
    let pct = |cost: f64| (rav > 0.0).then(|| annual(cost) / rav * 100.0);
    Ok(SmrpCostToRav {
        period_cost,
        annualized_cost: annual(period_cost),
        replacement_value_total: rav,
        assets_with_value: f(&assets, "valued") as i64,
        assets_total: f(&assets, "total") as i64,
        value_pct: pct(period_cost),
        previous_value_pct: pct(previous_cost),
    })
}

// ── All together ───────────────────────────────────────────────────────────────────────────

/// SMRP indicators over `window` (and its previous window), within what the caller may see.
pub async fn build_smrp_kpis(
    db: &DatabaseConnection,
    window: SmrpWindow,
    now: DateTime<Utc>,
    org: Option<i64>,
    can_ot: bool,
    can_pm: bool,
) -> AppResult<SmrpKpis> {
    let metric = |current: SmrpRatio, previous: SmrpRatio| SmrpMetric { current, previous };
    let schedule = if can_ot {
        Some(metric(
            schedule_compliance(db, window, now, org).await?,
            schedule_compliance(db, window.previous(), now, org).await?,
        ))
    } else {
        None
    };
    let pm = if can_pm {
        Some(metric(
            pm_compliance(db, window, now, org).await?,
            pm_compliance(db, window.previous(), now, org).await?,
        ))
    } else {
        None
    };
    Ok(SmrpKpis {
        from: instant(window.from),
        to: instant(window.to),
        days: window.days(),
        schedule_compliance: schedule,
        work_mix: if can_ot {
            Some(work_mix(db, window, org).await?)
        } else {
            None
        },
        pm_compliance: pm,
        backlog: if can_ot {
            Some(backlog(db, window, now, org).await?)
        } else {
            None
        },
        cost_to_rav: if can_ot {
            Some(cost_to_rav(db, window, org).await?)
        } else {
            None
        },
    })
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn ratios_and_windows() {
        assert_eq!(SmrpRatio::of(3.0, 4.0).value_pct, Some(75.0));
        assert_eq!(SmrpRatio::of(0.0, 0.0).value_pct, None);
        let now = DateTime::parse_from_rfc3339("2026-10-14T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let w = SmrpWindow::last_days(now, 30);
        assert_eq!(w.previous().to, w.from);
        assert_eq!(w.previous().days(), 30.0);
    }

    #[test]
    fn shift_hours_handle_night_shifts() {
        assert_eq!(shift_hours("08:00", "16:00"), Some(8.0));
        assert_eq!(shift_hours("22:00", "06:00"), Some(8.0));
        assert_eq!(shift_hours("07:30", "12:00"), Some(4.5));
        assert_eq!(shift_hours("08:00:00", "17:00:00"), Some(9.0));
        assert_eq!(shift_hours("x", "17:00"), None);
    }
}
