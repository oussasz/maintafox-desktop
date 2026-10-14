//! PM compliance KPI (SMRP Best Practice 5.4.1 style).
//!
//! `PM compliance % = PM occurrences completed within their tolerance window ÷ PM occurrences
//! due in the period × 100`.
//!
//! - **Due in the period**: the occurrence's due date is in `[from, to)` and it was not cancelled
//!   (a cancelled PM is a planning decision, not work owed). Forecasts that were never generated
//!   are not counted.
//! - **Within the window**: its last completed execution is `on_time` (`pm::schedule`). Work done
//!   before the window (`early`) or after it (`late`) is not compliant; an occurrence without a
//!   completed execution (still open, or missed) is `not_done`.
//! - Executions recorded before windows existed carry no timing: they are classified on read
//!   with the default window (the due day).

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::pm::domain::{PmComplianceInput, PmComplianceReport};
use crate::pm::schedule::{classify_timing, compliance_pct, rfc3339, time_window, OccurrenceWindow};

fn parse(ts: &str, field: &str) -> AppResult<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(ts)
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be a valid RFC3339 timestamp.")]))?
        .with_timezone(&Utc))
}

pub async fn get_pm_compliance(db: &impl ConnectionTrait, input: PmComplianceInput) -> AppResult<PmComplianceReport> {
    let from = parse(&input.from, "from")?;
    let to = parse(&input.to, "to")?;
    if to <= from {
        return Err(AppError::ValidationFailed(vec!["to must be after from.".to_string()]));
    }
    let mut sql = String::from(
        "SELECT po.due_at, po.window_start_at, po.window_end_at, po.window_start_meter, po.window_end_meter,
                done.executed_at, done.timing_status, done.meter_reading
         FROM pm_occurrences po
         LEFT JOIN pm_executions done ON done.id = (
             SELECT pe.id FROM pm_executions pe
             WHERE pe.pm_occurrence_id = po.id
               AND pe.execution_result IN ('completed_no_findings','completed_with_findings')
             ORDER BY pe.executed_at DESC, pe.id DESC LIMIT 1)
         WHERE po.due_at >= ? AND po.due_at < ?
           AND po.status NOT IN ('cancelled','forecasted')",
    );
    let mut binds: Vec<sea_orm::Value> = vec![rfc3339(from).into(), rfc3339(to).into()];
    if let Some(plan_id) = input.pm_plan_id {
        sql.push_str(" AND po.pm_plan_id = ?");
        binds.push(plan_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, &sql, binds))
        .await?;

    let (mut on_time, mut early, mut late, mut not_done) = (0_i64, 0_i64, 0_i64, 0_i64);
    for row in &rows {
        let executed_at: Option<String> = row.try_get("", "executed_at")?;
        let Some(executed_at) = executed_at else {
            not_done += 1;
            continue;
        };
        let stored: Option<String> = row.try_get("", "timing_status")?;
        let timing = match stored {
            Some(t) => Some(t),
            None => {
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
                let reading: Option<f64> = row.try_get("", "meter_reading")?;
                classify_timing(&window, parse(&executed_at, "executed_at")?, reading).map(str::to_string)
            }
        };
        match timing.as_deref() {
            Some("on_time") => on_time += 1,
            Some("early") => early += 1,
            Some("late") => late += 1,
            _ => not_done += 1,
        }
    }
    let due_count = rows.len() as i64;
    Ok(PmComplianceReport {
        from: rfc3339(from),
        to: rfc3339(to),
        pm_plan_id: input.pm_plan_id,
        due_count,
        on_time_count: on_time,
        early_count: early,
        late_count: late,
        not_done_count: not_done,
        compliance_pct: compliance_pct(on_time, due_count),
    })
}
