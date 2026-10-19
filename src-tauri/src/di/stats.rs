//! DI dashboard statistics aggregation.

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

use crate::errors::AppResult;

#[derive(Debug, Clone, Deserialize)]
pub struct DiStatsFilter {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub entity_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiStatusCount {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiPriorityCount {
    pub priority: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiTypeCount {
    pub origin_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiTrendPoint {
    pub period: String,
    pub created: i64,
    pub closed: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiEquipmentCount {
    pub equipment_id: i64,
    pub asset_label: String,
    pub count: i64,
    pub percentage: f64,
}

/// An open request past an SLA target it has not met.
#[derive(Debug, Clone, Serialize)]
pub struct DiOverdueDi {
    pub id: i64,
    pub code: String,
    pub title: String,
    pub priority: String,
    /// Hours since the first missed target (response if not screened in time, else completion).
    pub hours_overdue: f64,
}

/// Open requests of one status: how many and how old (days since submission).
#[derive(Debug, Clone, Serialize)]
pub struct DiBacklogStatus {
    pub status: String,
    pub count: i64,
    pub avg_age_days: f64,
    pub max_age_days: f64,
}

/// Open requests split by where they stand against their frozen SLA (same rule as the lists).
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct DiSlaOpenSplit {
    pub on_track: i64,
    pub at_risk: i64,
    pub breached: i64,
    /// Waiting on the requester or deferred: the clock is paused.
    pub paused: i64,
    /// Open requests without an SLA target (urgency level without target times).
    pub no_target: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiDispositionCount {
    pub disposition_code: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiStatsPayload {
    pub total: i64,
    pub pending: i64,
    pub in_progress: i64,
    pub closed: i64,
    pub closed_this_month: i64,
    pub overdue: i64,
    /// SLA outcome of the requests closed in the period (`closed_at` in the period, with or without
    /// a work order): met = no clock past its target, pauses excluded.
    pub sla_met_count: i64,
    pub sla_breached_count: i64,
    pub sla_total: i64,
    pub safety_issues: i64,
    pub status_distribution: Vec<DiStatusCount>,
    pub disposition_distribution: Vec<DiDispositionCount>,
    pub priority_distribution: Vec<DiPriorityCount>,
    pub type_distribution: Vec<DiTypeCount>,
    pub monthly_trend: Vec<DiTrendPoint>,
    pub available_years: Vec<i32>,
    pub avg_age_days: f64,
    pub max_age_days: f64,
    pub top_equipment: Vec<DiEquipmentCount>,
    pub overdue_dis: Vec<DiOverdueDi>,
    /// Open backlog by status (current state; only the site scope applies, not the period).
    pub backlog_by_status: Vec<DiBacklogStatus>,
    /// Open backlog against its SLA (current state; site scope only).
    pub sla_open: DiSlaOpenSplit,
    /// Requests closed in the period (`closed_at` in the period), with or without a work order.
    pub closed_in_period: i64,
    /// Of those, the ones that became a work order (`converted_to_wo`).
    pub converted_count: i64,
    /// Average hours from submission to screening, over the requests screened in the period
    /// (`screened_at` in the period), time waiting on the requester or deferred excluded.
    pub avg_response_hours: Option<f64>,
    /// Average hours from submission to the work order, over the requests converted in the period
    /// (`converted_at` in the period), paused time excluded.
    pub avg_conversion_hours: Option<f64>,
}

/// The terminal DI statuses, as an SQL list: `closed`, plus the legacy terminal codes that
/// migration 139 folded into `closed` (still read as closed for rows restored from older backups).
/// A macro so the one list can be spliced into `const` SQL (`concat!`): [`OPEN_SQL`] and
/// `analytics::as_of` (when a request stopped being open) read it.
#[macro_export]
macro_rules! di_terminal_statuses_sql {
    () => {
        "('closed','rejected','converted_to_work_order','closed_as_non_executable','archived')"
    };
}

/// [`di_terminal_statuses_sql!`] as a value.
pub const TERMINAL_STATUSES_SQL: &str = di_terminal_statuses_sql!();

/// Open requests (alias `ir`): not in a terminal status and not yet turned into a work order. The
/// one rule of the DI page, the home page, the asset counts, the list's `open_only` filter and,
/// at now, `analytics::as_of::di_open_at`.
pub const OPEN_SQL: &str = concat!(
    "(ir.status NOT IN ",
    di_terminal_statuses_sql!(),
    " AND ir.converted_at IS NULL)"
);

/// Requests waiting since a given instant (alias `ir`, one bind: [`waiting_since`]): submitted (or,
/// for a draft, created) at or before it. With [`OPEN_SQL`], the "waiting more than N days" rule of
/// the overdue figure (`analytics::as_of::di_open_at` with `waiting_days`, at now).
pub const WAITING_SQL: &str = "datetime(COALESCE(ir.submitted_at, ir.created_at)) <= datetime(?)";

/// Bind of [`WAITING_SQL`]: the instant `days` before `now` (negative days read as 0).
pub fn waiting_since(now: chrono::DateTime<chrono::Utc>, days: i64) -> String {
    crate::analytics::as_of::instant(crate::analytics::as_of::days_before(now, days.max(0)))
}

/// Site scope only (the open backlog is a current state, the period does not apply).
fn entity_scope_sql() -> &'static str {
    " (? IS NULL OR ir.org_node_id = ?) "
}

fn entity_binds(entity_id: Option<i64>) -> Vec<sea_orm::Value> {
    vec![entity_id.into(), entity_id.into()]
}

fn base_scope_sql() -> &'static str {
    " (? IS NULL OR date(ir.submitted_at) >= date(?))
      AND (? IS NULL OR date(ir.submitted_at) <= date(?))
      AND (? IS NULL OR ir.org_node_id = ?) "
}

/// Period on the date of an event of the request (`column`, alias `ir`) instead of its submission;
/// takes the same binds as [`base_scope_sql`].
fn event_scope_sql(column: &str) -> String {
    format!(
        " (? IS NULL OR date({column}) >= date(?)) \
          AND (? IS NULL OR date({column}) <= date(?)) \
          AND (? IS NULL OR ir.org_node_id = ?) "
    )
}

fn scope_binds(filter: &DiStatsFilter) -> [sea_orm::Value; 6] {
    [
        filter.date_from.clone().into(),
        filter.date_from.clone().into(),
        filter.date_to.clone().into(),
        filter.date_to.clone().into(),
        filter.entity_id.into(),
        filter.entity_id.into(),
    ]
}

fn row_i64(row: &QueryResult, col: &str) -> AppResult<i64> {
    Ok(row.try_get("", col)?)
}

fn row_f64(row: &QueryResult, col: &str) -> AppResult<f64> {
    Ok(row.try_get("", col)?)
}

pub async fn get_di_stats(db: &DatabaseConnection, filter: DiStatsFilter) -> AppResult<DiStatsPayload> {
    get_di_stats_in_scope(db, filter, &super::access::ScopeSql::everywhere()).await
}

/// [`get_di_stats`] over the requests of the reader's scope (rule §2.8: a site user's figures
/// cover their sites only; the explicit site filter is intersected with it).
pub async fn get_di_stats_in_scope(
    db: &DatabaseConnection,
    filter: DiStatsFilter,
    scope: &super::access::ScopeSql,
) -> AppResult<DiStatsPayload> {
    // The scope predicate follows the period / site predicate, and its binds follow theirs; no
    // placeholder comes after it in any query below.
    let mut binds = scope_binds(&filter).to_vec();
    binds.extend(scope.binds.iter().cloned());
    let base_where = format!("{} AND ({})", base_scope_sql(), scope.predicate);

    let total = scalar_i64(
        db,
        &format!("SELECT COUNT(*) AS value FROM intervention_requests ir WHERE {base_where}"),
        binds.to_vec(),
    )
    .await?;

    let pending = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where} AND ir.status IN ('submitted', 'in_review', 'awaiting_approval')"
        ),
        binds.to_vec(),
    )
    .await?;

    let in_progress = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND {OPEN_SQL}"
        ),
        binds.to_vec(),
    )
    .await?;

    let closed = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND ir.status IN ('closed')"
        ),
        binds.to_vec(),
    )
    .await?;

    let closed_this_month = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND ir.closed_at IS NOT NULL
             AND strftime('%Y-%m', ir.closed_at) = strftime('%Y-%m', 'now')"
        ),
        binds.to_vec(),
    )
    .await?;

    let safety_issues = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where} AND ir.safety_flag = 1"
        ),
        binds.to_vec(),
    )
    .await?;

    // Overdue = open and past an SLA target it has not met (the frozen targets of each request).
    let overdue = scalar_i64(
        db,
        &format!(
            "SELECT COUNT(*) AS value
             FROM intervention_requests ir
             WHERE {base_where}
             AND {breached}",
            breached = super::sla::SLA_BREACHED_SQL
        ),
        binds.to_vec(),
    )
    .await?;

    let status_distribution = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT ir.status AS status, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY ir.status
                     ORDER BY count DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiStatusCount {
                    status: row.try_get("", "status")?,
                    count: row_i64(&row, "count")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let disposition_distribution = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT COALESCE(ir.disposition_code, 'unknown') AS disposition_code, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                       AND ir.status = 'closed'
                     GROUP BY COALESCE(ir.disposition_code, 'unknown')
                     ORDER BY count DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiDispositionCount {
                    disposition_code: row.try_get("", "disposition_code")?,
                    count: row_i64(&row, "count")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let priority_distribution = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT ir.reported_urgency AS priority, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY ir.reported_urgency
                     ORDER BY count DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiPriorityCount {
                    priority: row.try_get("", "priority")?,
                    count: row_i64(&row, "count")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let type_distribution = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT ir.origin_type AS origin_type, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY ir.origin_type
                     ORDER BY count DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiTypeCount {
                    origin_type: row.try_get("", "origin_type")?,
                    count: row_i64(&row, "count")?,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let monthly_trend = {
        let created_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT strftime('%Y-%m', ir.submitted_at) AS period, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     GROUP BY period
                     ORDER BY period ASC"
                ),
                binds.to_vec(),
            ))
            .await?;
        let closed_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT strftime('%Y-%m', ir.closed_at) AS period, COUNT(*) AS count
                     FROM intervention_requests ir
                     WHERE {base_where}
                     AND ir.closed_at IS NOT NULL
                     GROUP BY period
                     ORDER BY period ASC"
                ),
                binds.to_vec(),
            ))
            .await?;

        let mut trend_map: BTreeMap<String, DiTrendPoint> = BTreeMap::new();
        for row in created_rows {
            let period: String = row.try_get("", "period")?;
            let created_count = row_i64(&row, "count")?;
            trend_map
                .entry(period.clone())
                .and_modify(|p| p.created = created_count)
                .or_insert(DiTrendPoint {
                    period,
                    created: created_count,
                    closed: 0,
                });
        }
        for row in closed_rows {
            let period: String = row.try_get("", "period")?;
            let closed_count = row_i64(&row, "count")?;
            trend_map
                .entry(period.clone())
                .and_modify(|p| p.closed = closed_count)
                .or_insert(DiTrendPoint {
                    period,
                    created: 0,
                    closed: closed_count,
                });
        }
        trend_map.into_values().collect::<Vec<_>>()
    };

    let available_years = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT DISTINCT CAST(strftime('%Y', ir.submitted_at) AS INTEGER) AS year
                     FROM intervention_requests ir
                     WHERE {base_where}
                     ORDER BY year DESC"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| Ok(row.try_get::<i32>("", "year")?))
            .collect::<AppResult<Vec<_>>>()?
    };

    let (avg_age_days, max_age_days) = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT
                       COALESCE(AVG(julianday('now') - julianday(ir.submitted_at)), 0.0) AS avg_age_days,
                       COALESCE(MAX(julianday('now') - julianday(ir.submitted_at)), 0.0) AS max_age_days
                     FROM intervention_requests ir
                     WHERE {base_where}
                     AND {OPEN_SQL}"
                ),
                binds.to_vec(),
            ))
            .await?;
        if let Some(row) = row {
            (row_f64(&row, "avg_age_days")?, row_f64(&row, "max_age_days")?)
        } else {
            (0.0, 0.0)
        }
    };

    let top_equipment = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT
                       ir.equipment_id AS equipment_id,
                       COALESCE(e.name, e.asset_id_code, CAST(ir.equipment_id AS TEXT)) AS asset_label,
                       COUNT(*) AS count
                     FROM intervention_requests ir
                     LEFT JOIN equipment e ON e.id = ir.equipment_id
                     WHERE {base_where}
                     GROUP BY ir.equipment_id, asset_label
                     ORDER BY count DESC, asset_label ASC
                     LIMIT 10"
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                let count = row_i64(&row, "count")?;
                let percentage = if total > 0 {
                    (count as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                Ok(DiEquipmentCount {
                    equipment_id: row.try_get("", "equipment_id")?,
                    asset_label: row.try_get("", "asset_label")?,
                    count,
                    percentage,
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let overdue_dis = {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT
                       ir.id,
                       ir.code,
                       ir.title,
                       COALESCE(ir.validated_urgency, ir.reported_urgency) AS priority,
                       {overdue} AS hours_overdue
                     FROM intervention_requests ir
                     WHERE {base_where}
                     AND {breached}
                     ORDER BY hours_overdue DESC, ir.id DESC
                     LIMIT 10",
                    breached = super::sla::SLA_BREACHED_SQL,
                    overdue = super::sla::SLA_HOURS_OVERDUE_SQL,
                ),
                binds.to_vec(),
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(DiOverdueDi {
                    id: row.try_get("", "id")?,
                    code: row.try_get("", "code")?,
                    title: row.try_get("", "title")?,
                    priority: row.try_get("", "priority")?,
                    hours_overdue: row_f64(&row, "hours_overdue")?.max(0.0),
                })
            })
            .collect::<AppResult<Vec<_>>>()?
    };

    let closing = sla_closing_outcome_in_scope(db, &filter, scope).await?;
    let (sla_met_count, sla_breached_count, sla_total) = (closing.met, closing.breached, closing.total);

    let backlog_by_status = open_backlog_by_status_in_scope(db, filter.entity_id, scope).await?;

    let sla_open = sla_open_split_in_scope(db, filter.entity_id, scope).await?;

    let finished = format!("{} AND ({})", event_scope_sql("ir.closed_at"), scope.predicate);
    let (closed_in_period, converted_count) = {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!(
                    "SELECT COUNT(*) AS closed_count,
                            COALESCE(SUM(CASE WHEN ir.disposition_code = '{converted}' THEN 1 ELSE 0 END), 0)
                              AS converted_count
                     FROM intervention_requests ir
                     WHERE {finished} AND ir.status = 'closed' AND ir.closed_at IS NOT NULL",
                    converted = super::disposition::DISPOSITION_CONVERTED_TO_WO
                ),
                binds.to_vec(),
            ))
            .await?;
        match row {
            Some(r) => (row_i64(&r, "closed_count")?, row_i64(&r, "converted_count")?),
            None => (0, 0),
        }
    };

    let avg_response_hours = scalar_opt_f64(
        db,
        &format!(
            "SELECT AVG(MAX(0.0, {elapsed})) AS value
             FROM intervention_requests ir
             WHERE {screened} AND ({scope}) AND ir.screened_at IS NOT NULL",
            elapsed = super::sla::SLA_RESPONSE_ELAPSED_SQL,
            screened = event_scope_sql("ir.screened_at"),
            scope = scope.predicate,
        ),
        binds.to_vec(),
    )
    .await?;

    let avg_conversion_hours = scalar_opt_f64(
        db,
        &format!(
            "SELECT AVG(MAX(0.0, {elapsed})) AS value
             FROM intervention_requests ir
             WHERE {converted} AND ({scope}) AND ir.converted_at IS NOT NULL",
            elapsed = super::sla::SLA_RESOLUTION_ELAPSED_SQL,
            converted = event_scope_sql("ir.converted_at"),
            scope = scope.predicate,
        ),
        binds.to_vec(),
    )
    .await?;

    Ok(DiStatsPayload {
        total,
        pending,
        in_progress,
        closed,
        closed_this_month,
        overdue,
        sla_met_count,
        sla_breached_count,
        sla_total,
        safety_issues,
        status_distribution,
        disposition_distribution,
        priority_distribution,
        type_distribution,
        monthly_trend,
        available_years,
        avg_age_days,
        max_age_days,
        top_equipment,
        overdue_dis,
        backlog_by_status,
        sla_open,
        closed_in_period,
        converted_count,
        avg_response_hours,
        avg_conversion_hours,
    })
}

/// SLA outcome of the requests finished in a period.
#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq)]
pub struct DiSlaClosingOutcome {
    pub met: i64,
    pub breached: i64,
    pub total: i64,
}

/// SLA compliance over the requests finished in the period of `filter` (`closed_at` in the period,
/// with or without a work order): met when no clock went past its target. Each clock stops when
/// the request is screened / converted, or when it is closed without them; paused time is not
/// counted. Requests without a frozen target are left out.
pub async fn sla_closing_outcome(db: &DatabaseConnection, filter: &DiStatsFilter) -> AppResult<DiSlaClosingOutcome> {
    sla_closing_outcome_in_scope(db, filter, &super::access::ScopeSql::everywhere()).await
}

/// [`sla_closing_outcome`] over the requests of the reader's scope.
pub async fn sla_closing_outcome_in_scope(
    db: &DatabaseConnection,
    filter: &DiStatsFilter,
    scope: &super::access::ScopeSql,
) -> AppResult<DiSlaClosingOutcome> {
    let finished = format!("{} AND ({})", event_scope_sql("ir.closed_at"), scope.predicate);
    let mut binds = scope_binds(filter).to_vec();
    binds.extend(scope.binds.iter().cloned());
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT
                   COALESCE(SUM(CASE WHEN {late} THEN 1 ELSE 0 END), 0) AS breached,
                   COALESCE(SUM(CASE WHEN {late} THEN 0 ELSE 1 END), 0) AS met
                 FROM intervention_requests ir
                 WHERE {finished}
                   AND ir.status = 'closed' AND ir.closed_at IS NOT NULL
                   AND ir.sla_target_response_hours IS NOT NULL
                   AND ir.sla_response_deadline IS NOT NULL",
                late = super::sla::SLA_LATE_SQL,
            ),
            binds,
        ))
        .await?;
    let (breached, met) = match row {
        Some(r) => (row_i64(&r, "breached")?, row_i64(&r, "met")?),
        None => (0, 0),
    };
    Ok(DiSlaClosingOutcome {
        met,
        breached,
        total: met + breached,
    })
}

/// Open backlog by status ([`OPEN_SQL`]): how many and how old (days since submission). Current
/// state; only the site scope applies.
pub async fn open_backlog_by_status(
    db: &DatabaseConnection,
    entity_id: Option<i64>,
) -> AppResult<Vec<DiBacklogStatus>> {
    open_backlog_by_status_in_scope(db, entity_id, &super::access::ScopeSql::everywhere()).await
}

/// [`open_backlog_by_status`] over the requests of the reader's scope.
pub async fn open_backlog_by_status_in_scope(
    db: &DatabaseConnection,
    entity_id: Option<i64>,
    scope: &super::access::ScopeSql,
) -> AppResult<Vec<DiBacklogStatus>> {
    let entity = format!("{} AND ({})", entity_scope_sql(), scope.predicate);
    let mut binds = entity_binds(entity_id);
    binds.extend(scope.binds.iter().cloned());
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ir.status AS status, COUNT(*) AS count,
                        AVG(julianday('now') - julianday(ir.submitted_at)) AS avg_age_days,
                        MAX(julianday('now') - julianday(ir.submitted_at)) AS max_age_days
                   FROM intervention_requests ir
                  WHERE {entity} AND {OPEN_SQL}
                  GROUP BY ir.status
                  ORDER BY count DESC"
            ),
            binds,
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(DiBacklogStatus {
                status: row.try_get("", "status")?,
                count: row_i64(&row, "count")?,
                avg_age_days: row_f64(&row, "avg_age_days")?,
                max_age_days: row_f64(&row, "max_age_days")?,
            })
        })
        .collect()
}

/// Open requests ([`OPEN_SQL`]) split by where they stand against their frozen SLA — the same
/// rule as the lists (`sla::SLA_BREACHED_SQL`, `sla::SLA_AT_RISK_OR_BREACHED_SQL`; a paused
/// request is never at risk or breached). Current state; only the site scope applies.
pub async fn sla_open_split(db: &DatabaseConnection, entity_id: Option<i64>) -> AppResult<DiSlaOpenSplit> {
    sla_open_split_in_scope(db, entity_id, &super::access::ScopeSql::everywhere()).await
}

/// [`sla_open_split`] over the requests of the reader's scope.
pub async fn sla_open_split_in_scope(
    db: &DatabaseConnection,
    entity_id: Option<i64>,
    scope: &super::access::ScopeSql,
) -> AppResult<DiSlaOpenSplit> {
    let entity = format!("{} AND ({})", entity_scope_sql(), scope.predicate);
    let mut binds = entity_binds(entity_id);
    binds.extend(scope.binds.iter().cloned());
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT
                   COALESCE(SUM(CASE WHEN {breached} THEN 1 ELSE 0 END), 0) AS breached,
                   COALESCE(SUM(CASE WHEN {risk} AND NOT {breached} THEN 1 ELSE 0 END), 0) AS at_risk,
                   COALESCE(SUM(CASE WHEN ir.sla_target_response_hours IS NULL
                                      OR ir.sla_response_deadline IS NULL THEN 1 ELSE 0 END), 0) AS no_target,
                   COALESCE(SUM(CASE WHEN ir.sla_target_response_hours IS NOT NULL
                                      AND ir.sla_response_deadline IS NOT NULL
                                      AND ir.status IN ('returned_for_clarification', 'deferred')
                                     THEN 1 ELSE 0 END), 0) AS paused,
                   COUNT(*) AS open_total
                 FROM intervention_requests ir
                 WHERE {entity} AND {OPEN_SQL}",
                breached = super::sla::SLA_BREACHED_SQL,
                risk = super::sla::SLA_AT_RISK_OR_BREACHED_SQL,
            ),
            binds,
        ))
        .await?;
    Ok(match row {
        Some(row) => {
            let breached = row_i64(&row, "breached")?;
            let at_risk = row_i64(&row, "at_risk")?;
            let no_target = row_i64(&row, "no_target")?;
            let paused = row_i64(&row, "paused")?;
            let open_total = row_i64(&row, "open_total")?;
            DiSlaOpenSplit {
                on_track: (open_total - breached - at_risk - no_target - paused).max(0),
                at_risk,
                breached,
                paused,
                no_target,
            }
        }
        None => DiSlaOpenSplit::default(),
    })
}

async fn scalar_opt_f64(db: &DatabaseConnection, sql: &str, binds: Vec<sea_orm::Value>) -> AppResult<Option<f64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql.to_string(),
            binds,
        ))
        .await?;
    match row {
        Some(row) => Ok(row.try_get::<Option<f64>>("", "value")?),
        None => Ok(None),
    }
}

async fn scalar_i64(db: &DatabaseConnection, sql: &str, binds: Vec<sea_orm::Value>) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql.to_string(),
            binds,
        ))
        .await?;
    if let Some(row) = row {
        row_i64(&row, "value")
    } else {
        Ok(0)
    }
}
