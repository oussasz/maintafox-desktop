//! Point-in-time ("as of") counts used for KPI values and their baselines.
//!
//! A trend compares the same measure at two instants: the value now and the value at the start of
//! the window (now − horizon). Each function below evaluates its measure at an arbitrary instant
//! from the records' own lifecycle timestamps, so `f(now)` and `f(now − horizon)` are the same
//! formula — never "currently open records created before X", which can only ever be smaller.
//!
//! Lifecycle timestamps used (a terminal record without its timestamp ends at `updated_at`, the
//! last time the row changed, so `f(now)` always equals the status-based count):
//! - DI: open from `submitted_at` until `closed_at` / `converted_at` / `declined_at` / `archived_at`.
//! - WO: open from `created_at` until `closed_at` / `cancelled_at`; "not yet done" (backlog) also
//!   ends at `mechanically_completed_at`. In-progress state comes from `wo_state_transition_log`.
//! - PM occurrence: pending from `created_at` until its transition to completed / cancelled / missed
//!   (`pm_occurrence_transitions.transitioned_at`).
//! - Asset: exists from `created_at` until `deleted_at`.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TryGetable, Value};

use crate::errors::AppResult;

/// RFC 3339 UTC with a `Z` suffix — the format SQLite `datetime()` parses.
pub fn instant(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The instant `days` before `at`.
pub fn days_before(at: DateTime<Utc>, days: i64) -> DateTime<Utc> {
    at - Duration::days(days)
}

/// SQL expression: when the DI stopped being open (NULL while open). Terminal statuses are the
/// current `closed` plus the legacy terminal codes still present on old rows.
const DI_END: &str = "CASE WHEN ir.status IN ('closed','rejected','converted_to_work_order',\
'closed_as_non_executable','archived') THEN COALESCE(ir.closed_at, ir.converted_at, ir.declined_at, \
ir.archived_at, ir.updated_at) END";

/// SQL expression: when the WO stopped being open (closed / cancelled).
const WO_END_OPEN: &str = "CASE WHEN s.code IN ('closed','cancelled') \
THEN COALESCE(wo.closed_at, wo.cancelled_at, wo.updated_at) END";

/// SQL expression: when the WO's work was done or dropped (completed / closed / cancelled).
const WO_END_WORK: &str = "CASE WHEN s.code IN ('completed','closed','cancelled','mechanically_complete',\
'technically_verified') THEN COALESCE(wo.mechanically_completed_at, wo.closed_at, wo.cancelled_at, \
wo.updated_at) END";

/// SQL expression: when the PM occurrence was completed / cancelled / missed.
const PM_END: &str = "CASE WHEN po.status IN ('completed','cancelled','missed') THEN COALESCE(\
(SELECT MAX(t.transitioned_at) FROM pm_occurrence_transitions t \
 WHERE t.pm_occurrence_id = po.id AND t.to_status = po.status), po.updated_at) END";

async fn count(db: &DatabaseConnection, sql: &str, binds: Vec<Value>) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?;
    Ok(row.and_then(|r| i64::try_get(&r, "", "cnt").ok()).unwrap_or(0))
}

async fn total(db: &DatabaseConnection, sql: &str, binds: Vec<Value>) -> AppResult<f64> {
    let row = db
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?;
    Ok(row.and_then(|r| f64::try_get(&r, "", "total").ok()).unwrap_or(0.0))
}

/// Intervention requests open at `at`. With `waiting_days`, only those submitted at least that many
/// days before `at` (the "waiting too long" measure).
pub async fn di_open_at(
    db: &DatabaseConnection,
    at: DateTime<Utc>,
    waiting_days: Option<i64>,
    org: Option<i64>,
) -> AppResult<i64> {
    let at_s = instant(at);
    let submitted_before = instant(days_before(at, waiting_days.unwrap_or(0)));
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM intervention_requests ir \
         WHERE datetime(COALESCE(ir.submitted_at, ir.created_at)) <= datetime(?) \
           AND ({DI_END} IS NULL OR datetime({DI_END}) > datetime(?)) \
           AND (? IS NULL OR ir.org_node_id = ?)"
    );
    count(
        db,
        &sql,
        vec![submitted_before.into(), at_s.into(), org.into(), org.into()],
    )
    .await
}

/// Work orders open (not closed / cancelled) at `at`.
pub async fn wo_open_at(db: &DatabaseConnection, at: DateTime<Utc>, org: Option<i64>) -> AppResult<i64> {
    let at_s = instant(at);
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE datetime(wo.created_at) <= datetime(?) \
           AND ({WO_END_OPEN} IS NULL OR datetime({WO_END_OPEN}) > datetime(?)) \
           AND (? IS NULL OR wo.entity_id = ?)"
    );
    count(db, &sql, vec![at_s.clone().into(), at_s.into(), org.into(), org.into()]).await
}

/// Work orders not done at `at` whose planned end was already past.
pub async fn wo_overdue_at(db: &DatabaseConnection, at: DateTime<Utc>, org: Option<i64>) -> AppResult<i64> {
    let at_s = instant(at);
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE wo.planned_end IS NOT NULL AND datetime(wo.planned_end) < datetime(?) \
           AND datetime(wo.created_at) <= datetime(?) \
           AND ({WO_END_OPEN} IS NULL OR datetime({WO_END_OPEN}) > datetime(?)) \
           AND (? IS NULL OR wo.entity_id = ?)"
    );
    count(
        db,
        &sql,
        vec![
            at_s.clone().into(),
            at_s.clone().into(),
            at_s.into(),
            org.into(),
            org.into(),
        ],
    )
    .await
}

/// Planned hours (`expected_duration_hours`) of work orders whose work was not done at `at`.
pub async fn wo_backlog_hours_at(db: &DatabaseConnection, at: DateTime<Utc>, org: Option<i64>) -> AppResult<f64> {
    let at_s = instant(at);
    let sql = format!(
        "SELECT COALESCE(SUM(COALESCE(wo.expected_duration_hours, 0)), 0) AS total \
         FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE datetime(wo.created_at) <= datetime(?) \
           AND ({WO_END_WORK} IS NULL OR datetime({WO_END_WORK}) > datetime(?)) \
           AND (? IS NULL OR wo.entity_id = ?)"
    );
    total(db, &sql, vec![at_s.clone().into(), at_s.into(), org.into(), org.into()]).await
}

/// Work orders in progress at `at`: the last logged status at or before `at` is `in_progress`.
/// A work order with no transition log at all is read from its current status.
pub async fn wo_in_progress_at(db: &DatabaseConnection, at: DateTime<Utc>, org: Option<i64>) -> AppResult<i64> {
    let at_s = instant(at);
    let sql = "SELECT COUNT(*) AS cnt FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE datetime(wo.created_at) <= datetime(?) \
           AND (? IS NULL OR wo.entity_id = ?) \
           AND COALESCE( \
             (SELECT l.to_status FROM wo_state_transition_log l \
               WHERE l.wo_id = wo.id AND datetime(l.acted_at) <= datetime(?) \
               ORDER BY datetime(l.acted_at) DESC, l.id DESC LIMIT 1), \
             CASE WHEN NOT EXISTS (SELECT 1 FROM wo_state_transition_log l2 WHERE l2.wo_id = wo.id) \
                  THEN s.code END \
           ) = 'in_progress'";
    count(db, sql, vec![at_s.clone().into(), org.into(), org.into(), at_s.into()]).await
}

/// PM occurrences still pending at `at` whose due date was already past.
pub async fn pm_overdue_at(db: &DatabaseConnection, at: DateTime<Utc>, org: Option<i64>) -> AppResult<i64> {
    let at_s = instant(at);
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM pm_occurrences po \
         JOIN pm_plans pp ON pp.id = po.pm_plan_id \
         LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment' \
         WHERE po.due_at IS NOT NULL AND datetime(po.due_at) < datetime(?) \
           AND datetime(po.created_at) <= datetime(?) \
           AND ({PM_END} IS NULL OR datetime({PM_END}) > datetime(?)) \
           AND (? IS NULL OR e.installed_at_node_id = ?)"
    );
    count(
        db,
        &sql,
        vec![
            at_s.clone().into(),
            at_s.clone().into(),
            at_s.into(),
            org.into(),
            org.into(),
        ],
    )
    .await
}

/// Which due dates a PM "due" count looks at, relative to the instant it is evaluated at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmDueWindow {
    /// Due on the calendar day (UTC) of the instant.
    SameDay,
    /// Due between the instant and `n` days after it.
    NextDays(i64),
}

/// PM occurrences pending at `at` and due in `window` (relative to `at`).
pub async fn pm_due_at(
    db: &DatabaseConnection,
    at: DateTime<Utc>,
    window: PmDueWindow,
    org: Option<i64>,
) -> AppResult<i64> {
    let at_s = instant(at);
    let (window_sql, mut binds): (&str, Vec<Value>) = match window {
        PmDueWindow::SameDay => ("date(po.due_at) = date(?)", vec![at_s.clone().into()]),
        PmDueWindow::NextDays(n) => (
            "datetime(po.due_at) >= datetime(?) AND datetime(po.due_at) <= datetime(?)",
            vec![at_s.clone().into(), instant(at + Duration::days(n)).into()],
        ),
    };
    let sql = format!(
        "SELECT COUNT(*) AS cnt FROM pm_occurrences po          JOIN pm_plans pp ON pp.id = po.pm_plan_id          LEFT JOIN equipment e ON e.id = pp.asset_scope_id AND pp.asset_scope_type = 'equipment'          WHERE po.due_at IS NOT NULL AND {window_sql}            AND datetime(po.created_at) <= datetime(?)            AND ({PM_END} IS NULL OR datetime({PM_END}) > datetime(?))            AND (? IS NULL OR e.installed_at_node_id = ?)"
    );
    binds.extend([at_s.clone().into(), at_s.into(), org.into(), org.into()]);
    count(db, &sql, binds).await
}

/// Registered (not deleted) assets at `at`.
pub async fn assets_at(db: &DatabaseConnection, at: DateTime<Utc>, org: Option<i64>) -> AppResult<i64> {
    let at_s = instant(at);
    count(
        db,
        "SELECT COUNT(*) AS cnt FROM equipment e \
         WHERE datetime(e.created_at) <= datetime(?) \
           AND (e.deleted_at IS NULL OR datetime(e.deleted_at) > datetime(?)) \
           AND (? IS NULL OR e.installed_at_node_id = ?)",
        vec![at_s.clone().into(), at_s.into(), org.into(), org.into()],
    )
    .await
}
