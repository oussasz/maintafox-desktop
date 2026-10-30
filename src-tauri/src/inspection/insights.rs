//! Module figures and the anomaly follow-up list (PRD §6.25, Phase 6.3).
//!
//! - KPIs over a period ending now: rounds due, done, done on time, missed (compliance =
//!   done on time / due), rounds overdue right now, and open anomalies. Every figure has a list
//!   behind it: the round list filters (`InspectionRoundSummaryFilter`) and the anomaly list
//!   reproduce exactly the same conditions.
//! - Anomaly rows: an anomaly with its round, checkpoint, equipment, reading and the request or
//!   work order created from it, so a decision can be taken (and its result opened) in one place.

use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::errors::{AppError, AppResult};

use super::domain::{
    InspectionAnomalyPage, InspectionAnomalyRow, InspectionAnomalyRowFilter, InspectionCheckpoint, InspectionKpis,
    InspectionKpisInput,
};
use super::results::{map_anomaly, violated_limit, ANOMALY_STATUSES};

/// Longest KPI period.
pub const MAX_KPI_PERIOD_DAYS: i64 = 366;
/// Largest page of anomalies returned at once.
pub const MAX_ANOMALY_PAGE: i64 = 500;
const DEFAULT_ANOMALY_PAGE: i64 = 200;

/// SQL predicate (alias `ir`): a round still to do (scheduled, released or under way).
pub const ROUND_TODO_SQL: &str = "ir.status IN ('scheduled', 'released', 'in_progress')";

/// SQL predicate (alias `ir`): a round still to do whose planned time is past — the "overdue"
/// KPI, the round list's overdue filter and the home page use this one rule.
pub const ROUND_OVERDUE_SQL: &str = "ir.status IN ('scheduled', 'released', 'in_progress') \
     AND ir.scheduled_at IS NOT NULL AND julianday(ir.scheduled_at) < julianday('now')";

/// Round statuses that count as done for the KPIs.
pub const DONE_STATUSES: &[&str] = &["completed", "completed_with_findings", "reviewed"];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("inspection insights decode '{field}': {err}"))
}

fn ts(t: chrono::DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

async fn count(db: &impl ConnectionTrait, sql: &str, binds: Vec<sea_orm::Value>) -> AppResult<i64> {
    db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, binds))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("count returned no row")))?
        .try_get("", "c")
        .map_err(|e| decode_err("c", e))
}

/// End of the window of a round: its own window, else the end of its planned day (UTC).
const WINDOW_END_SQL: &str = "julianday(COALESCE(ir.window_end_at, date(ir.scheduled_at) || 'T23:59:59Z'))";

pub async fn get_inspection_kpis(db: &impl ConnectionTrait, input: InspectionKpisInput) -> AppResult<InspectionKpis> {
    if !(1..=MAX_KPI_PERIOD_DAYS).contains(&input.period_days) {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_KPI_PERIOD_RANGE: the period must be between 1 and {MAX_KPI_PERIOD_DAYS} days."
        )]));
    }
    let end = crate::clock::now();
    let start = end - Duration::days(input.period_days);
    let (period_start, period_end) = (ts(start), ts(end));
    let in_period = "ir.scheduled_at IS NOT NULL AND julianday(ir.scheduled_at) >= julianday(?) \
                     AND julianday(ir.scheduled_at) <= julianday(?) AND ir.status <> 'cancelled'";
    let done_list = DONE_STATUSES
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let binds = || -> Vec<sea_orm::Value> { vec![period_start.clone().into(), period_end.clone().into()] };

    let due_count = count(
        db,
        &format!("SELECT COUNT(*) AS c FROM inspection_rounds ir WHERE {in_period}"),
        binds(),
    )
    .await?;
    let done_count = count(
        db,
        &format!("SELECT COUNT(*) AS c FROM inspection_rounds ir WHERE {in_period} AND ir.status IN ({done_list})"),
        binds(),
    )
    .await?;
    let done_on_time_count = count(
        db,
        &format!(
            "SELECT COUNT(*) AS c FROM inspection_rounds ir WHERE {in_period} AND ir.status IN ({done_list}) \
             AND ir.completed_at IS NOT NULL AND julianday(ir.completed_at) <= {WINDOW_END_SQL}"
        ),
        binds(),
    )
    .await?;
    let missed_count = count(
        db,
        &format!("SELECT COUNT(*) AS c FROM inspection_rounds ir WHERE {in_period} AND ir.status = 'missed'"),
        binds(),
    )
    .await?;
    let overdue_count = count(
        db,
        &format!("SELECT COUNT(*) AS c FROM inspection_rounds ir WHERE {ROUND_OVERDUE_SQL}"),
        vec![],
    )
    .await?;
    let open_anomaly_count = count(
        db,
        "SELECT COUNT(*) AS c FROM inspection_anomalies WHERE resolution_status = 'open'",
        vec![],
    )
    .await?;
    let compliance_pct = (due_count > 0).then(|| done_on_time_count as f64 * 100.0 / due_count as f64);
    Ok(InspectionKpis {
        period_start,
        period_end,
        due_count,
        done_count,
        done_on_time_count,
        missed_count,
        overdue_count,
        compliance_pct,
        open_anomaly_count,
    })
}

const ANOMALY_ROW_SELECT: &str = concat!(
    "SELECT a.id, a.round_id, a.result_id, a.anomaly_type, a.severity, a.description, \
       a.linked_di_id, a.linked_work_order_id, a.requires_permit_review, a.resolution_status, \
       a.routing_decision, a.entity_sync_id, a.row_version, \
       ir.status AS round_status, ir.scheduled_at AS round_scheduled_at, ir.completed_at AS round_completed_at, \
       t.code AS template_code, t.name AS template_name, \
       c.id AS checkpoint_id, c.checkpoint_code, c.description AS checkpoint_description, c.check_type, \
       c.measurement_unit_id, ",
    crate::reference::units::unit_symbol_of!("c.measurement_unit_id"),
    " AS measurement_unit, c.normal_min, c.normal_max, c.warning_min, c.warning_max, \
       e.id AS asset_id, e.asset_id_code AS asset_code, e.name AS asset_name, \
       (CASE WHEN e.installed_at_node_id IS NOT NULL THEN 1 ELSE 0 END) AS asset_has_location, \
       r.result_status, r.numeric_value, r.text_value, r.comment AS result_comment, r.recorded_at, \
       di.code AS linked_di_code, di.title AS linked_di_title, di.status AS linked_di_status, \
       wo.code AS linked_wo_code, wo.title AS linked_wo_title \
     FROM inspection_anomalies a \
     JOIN inspection_rounds ir ON ir.id = a.round_id \
     JOIN inspection_templates t ON t.id = ir.template_id \
     LEFT JOIN inspection_results r ON r.id = a.result_id \
     LEFT JOIN inspection_checkpoints c ON c.id = r.checkpoint_id \
     LEFT JOIN equipment e ON e.id = c.asset_id \
     LEFT JOIN intervention_requests di ON di.id = a.linked_di_id \
     LEFT JOIN work_orders wo ON wo.id = a.linked_work_order_id"
);

fn map_anomaly_row(row: &sea_orm::QueryResult) -> AppResult<InspectionAnomalyRow> {
    macro_rules! get {
        ($name:literal) => {
            row.try_get("", $name).map_err(|e| decode_err($name, e))?
        };
    }
    let anomaly = map_anomaly(row)?;
    let check_type: Option<String> = get!("check_type");
    let numeric_value: Option<f64> = get!("numeric_value");
    let checkpoint_code: Option<String> = get!("checkpoint_code");
    let measurement_unit: Option<String> = get!("measurement_unit");
    let normal_min: Option<f64> = get!("normal_min");
    let normal_max: Option<f64> = get!("normal_max");
    let warning_min: Option<f64> = get!("warning_min");
    let warning_max: Option<f64> = get!("warning_max");
    let violation = match (check_type.as_deref(), numeric_value) {
        (Some("numeric"), Some(v)) => violated_limit(
            v,
            &InspectionCheckpoint {
                id: 0,
                entity_sync_id: String::new(),
                template_version_id: 0,
                sequence_order: 0,
                asset_id: None,
                component_id: None,
                checkpoint_code: checkpoint_code.clone().unwrap_or_default(),
                description: None,
                check_type: "numeric".into(),
                measurement_unit_id: None,
                measurement_unit: measurement_unit.clone(),
                normal_min,
                normal_max,
                warning_min,
                warning_max,
                requires_photo: false,
                requires_comment_on_exception: false,
                row_version: 0,
            },
        ),
        _ => None,
    };
    let has_location: i64 = get!("asset_has_location");
    Ok(InspectionAnomalyRow {
        anomaly,
        round_status: get!("round_status"),
        round_scheduled_at: get!("round_scheduled_at"),
        round_completed_at: get!("round_completed_at"),
        template_code: get!("template_code"),
        template_name: get!("template_name"),
        checkpoint_id: get!("checkpoint_id"),
        checkpoint_code,
        checkpoint_description: get!("checkpoint_description"),
        check_type,
        measurement_unit,
        normal_min,
        normal_max,
        warning_min,
        warning_max,
        asset_id: get!("asset_id"),
        asset_code: get!("asset_code"),
        asset_name: get!("asset_name"),
        asset_has_location: has_location != 0,
        result_status: get!("result_status"),
        numeric_value,
        text_value: get!("text_value"),
        result_comment: get!("result_comment"),
        recorded_at: get!("recorded_at"),
        violation_direction: violation.map(|(d, _)| d.to_string()),
        violation_limit: violation.map(|(_, l)| l),
        linked_di_code: get!("linked_di_code"),
        linked_di_title: get!("linked_di_title"),
        linked_di_status: get!("linked_di_status"),
        linked_wo_code: get!("linked_wo_code"),
        linked_wo_title: get!("linked_wo_title"),
    })
}

/// Anomalies for the follow-up list, newest first, by resolution status and/or round.
pub async fn list_inspection_anomaly_rows(
    db: &impl ConnectionTrait,
    filter: InspectionAnomalyRowFilter,
) -> AppResult<InspectionAnomalyPage> {
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(statuses) = filter.statuses.as_ref().filter(|s| !s.is_empty()) {
        if let Some(bad) = statuses.iter().find(|s| !ANOMALY_STATUSES.contains(&s.as_str())) {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_INSP_ANOMALY_STATUS_INVALID: Unknown anomaly status '{bad}'."
            )]));
        }
        clauses.push(format!(
            "a.resolution_status IN ({})",
            vec!["?"; statuses.len()].join(", ")
        ));
        values.extend(statuses.iter().cloned().map(sea_orm::Value::from));
    }
    if let Some(round_id) = filter.round_id {
        clauses.push("a.round_id = ?".into());
        values.push(round_id.into());
    }
    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    let total = count(
        db,
        &format!("SELECT COUNT(*) AS c FROM inspection_anomalies a {where_clause}"),
        values.clone(),
    )
    .await?;
    let limit = filter.limit.unwrap_or(DEFAULT_ANOMALY_PAGE).clamp(1, MAX_ANOMALY_PAGE);
    let offset = filter.offset.unwrap_or(0).max(0);
    values.push(limit.into());
    values.push(offset.into());
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{ANOMALY_ROW_SELECT} {where_clause} ORDER BY a.id DESC LIMIT ? OFFSET ?"),
            values,
        ))
        .await?;
    let items = rows.iter().map(map_anomaly_row).collect::<AppResult<Vec<_>>>()?;
    Ok(InspectionAnomalyPage { items, total })
}

/// One anomaly with its context (the follow-up dialog reads it before writing the request).
pub async fn get_inspection_anomaly_row(db: &impl ConnectionTrait, anomaly_id: i64) -> AppResult<InspectionAnomalyRow> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{ANOMALY_ROW_SELECT} WHERE a.id = ?"),
            [anomaly_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionAnomaly".into(),
            id: anomaly_id.to_string(),
        })?;
    map_anomaly_row(&row)
}
