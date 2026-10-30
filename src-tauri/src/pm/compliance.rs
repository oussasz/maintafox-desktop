//! PM compliance KPI (SMRP Best Practice 5.4.1 style) — the one definition used by the PM page,
//! the home tile, the Analytics SMRP tile and the monthly report.
//!
//! `PM compliance % = PM occurrences done within their tolerance window ÷ PM occurrences due in
//! the period whose outcome is known × 100`.
//!
//! - **Due in the period**: due date in `[from, to)`, never later than `now`, not cancelled
//!   (a cancelled PM is a planning decision, not work owed).
//! - **Outcome known**: the work was done, or its window has closed (end of window, else the end
//!   of the due day) by `now`. Work still inside its window is `pending` and left out: it cannot
//!   be judged yet.
//! - **Within the window**: the completion is `on_time` (`pm::schedule`). Work done before the
//!   window opens (`early`) or after it closes (`late`) is not compliant; an occurrence whose
//!   window closed without work is `not_done`.
//! - **Completion**: the last completed execution; an occurrence completed by a status change
//!   only counts from that change. Executions recorded before windows existed carry no timing:
//!   they are classified on read with the default window (the due day).

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::pm::domain::{PmComplianceInput, PmComplianceReport};
use crate::pm::schedule::{classify_timing, compliance_pct, rfc3339, time_window, OccurrenceWindow};

fn parse(ts: &str, field: &str) -> AppResult<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(ts)
        .map_err(|_| {
            AppError::ValidationFailed(vec![format!(
                "GATE_PM_DATE_INVALID: {field} must be a valid RFC3339 timestamp."
            )])
        })?
        .with_timezone(&Utc))
}

/// Which occurrences count: one plan, and/or the assets installed at one organisation node.
#[derive(Debug, Clone, Copy, Default)]
pub struct ComplianceScope {
    pub pm_plan_id: Option<i64>,
    pub org_node_id: Option<i64>,
}

/// Compliance of the occurrences due in `[from, to)`, judged at `now` (see module docs).
pub async fn compliance_between(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    scope: ComplianceScope,
) -> AppResult<PmComplianceReport> {
    compliance_between_for_criticality(db, from, to, now, scope, None).await
}

/// [`compliance_between`] limited to the plans of one criticality (`pm_plans.criticality_value_id`
/// → `lookup_values.code`, compared upper-case) — the PM governance report's filter.
pub async fn compliance_between_for_criticality(
    db: &impl ConnectionTrait,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    now: DateTime<Utc>,
    scope: ComplianceScope,
    criticality_code: Option<&str>,
) -> AppResult<PmComplianceReport> {
    let upper = to.min(now);
    // Timestamps are normalised to RFC3339 in SQL (rows written by SQLite defaults use another form).
    let mut sql = String::from(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', po.due_at) AS due_at, po.window_start_at, po.window_end_at,
                po.window_start_meter, po.window_end_meter,
                strftime('%Y-%m-%dT%H:%M:%SZ', COALESCE(done.executed_at,
                    CASE WHEN po.status = 'completed' THEN COALESCE(
                        (SELECT MIN(t.transitioned_at) FROM pm_occurrence_transitions t
                         WHERE t.pm_occurrence_id = po.id AND t.to_status = 'completed'),
                        po.updated_at) END)) AS done_at,
                done.timing_status, done.meter_reading
         FROM pm_occurrences po
         JOIN pm_plans pp ON pp.id = po.pm_plan_id
         LEFT JOIN equipment eq ON eq.id =
             COALESCE(po.equipment_id, CASE WHEN pp.asset_scope_type = 'equipment' THEN pp.asset_scope_id END)
         LEFT JOIN lookup_values lv ON lv.id = pp.criticality_value_id
         LEFT JOIN pm_executions done ON done.id = (
             SELECT pe.id FROM pm_executions pe
             WHERE pe.pm_occurrence_id = po.id
               AND pe.execution_result IN ('completed_no_findings','completed_with_findings')
             ORDER BY pe.executed_at DESC, pe.id DESC LIMIT 1)
         WHERE po.due_at IS NOT NULL AND po.status <> 'cancelled'
           AND datetime(po.due_at) >= datetime(?) AND datetime(po.due_at) < datetime(?)",
    );
    let mut binds: Vec<sea_orm::Value> = vec![rfc3339(from).into(), rfc3339(upper).into()];
    if let Some(plan_id) = scope.pm_plan_id {
        sql.push_str(" AND po.pm_plan_id = ?");
        binds.push(plan_id.into());
    }
    if let Some(node) = scope.org_node_id {
        sql.push_str(" AND eq.installed_at_node_id = ?");
        binds.push(node.into());
    }
    if let Some(code) = criticality_code {
        sql.push_str(" AND UPPER(COALESCE(lv.code, '')) = ?");
        binds.push(code.trim().to_uppercase().into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let (mut on_time, mut early, mut late, mut not_done, mut pending) = (0_i64, 0_i64, 0_i64, 0_i64, 0_i64);
    for row in &rows {
        let due_at: String = row.try_get("", "due_at")?;
        let mut window = OccurrenceWindow {
            start_at: row.try_get("", "window_start_at")?,
            end_at: row.try_get("", "window_end_at")?,
            start_meter: row.try_get("", "window_start_meter")?,
            end_meter: row.try_get("", "window_end_meter")?,
        };
        if window.end_at.is_none() {
            let (start, end) = time_window(parse(&due_at, "due_at")?, None, None);
            window.start_at = start.map(rfc3339);
            window.end_at = Some(rfc3339(end));
        }
        let done_at: Option<String> = row.try_get("", "done_at")?;
        let Some(done_at) = done_at else {
            let closed = window
                .end_at
                .as_deref()
                .map(|e| parse(e, "window_end_at"))
                .transpose()?
                .is_some_and(|end| end <= now);
            if closed {
                not_done += 1;
            } else {
                pending += 1;
            }
            continue;
        };
        let stored: Option<String> = row.try_get("", "timing_status")?;
        let timing = match stored {
            Some(t) => Some(t),
            None => {
                let reading: Option<f64> = row.try_get("", "meter_reading")?;
                classify_timing(&window, parse(&done_at, "done_at")?, reading).map(str::to_string)
            }
        };
        match timing.as_deref() {
            Some("on_time") => on_time += 1,
            Some("early") => early += 1,
            Some("late") => late += 1,
            _ => not_done += 1,
        }
    }
    let due_count = on_time + early + late + not_done;
    Ok(PmComplianceReport {
        from: rfc3339(from),
        to: rfc3339(to),
        pm_plan_id: scope.pm_plan_id,
        due_count,
        on_time_count: on_time,
        early_count: early,
        late_count: late,
        not_done_count: not_done,
        pending_count: pending,
        compliance_pct: compliance_pct(on_time, due_count),
    })
}

/// IPC entry point (PM page): the period given, judged now.
pub async fn get_pm_compliance(db: &impl ConnectionTrait, input: PmComplianceInput) -> AppResult<PmComplianceReport> {
    let from = parse(&input.from, "from")?;
    let to = parse(&input.to, "to")?;
    if to <= from {
        return Err(AppError::ValidationFailed(vec![
            "GATE_PM_PERIOD_ORDER: to must be after from.".to_string(),
        ]));
    }
    compliance_between(
        db,
        from,
        to,
        crate::clock::now(),
        ComplianceScope {
            pm_plan_id: input.pm_plan_id,
            org_node_id: None,
        },
    )
    .await
}
