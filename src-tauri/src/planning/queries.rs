use std::collections::{BTreeSet, HashMap};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};

use super::domain::{
    CandidateConflictSummary, RefreshScheduleCandidatesInput, RefreshScheduleCandidatesResult, ScheduleBacklogSnapshot,
    ScheduleCandidate, ScheduleCandidateFilter, SchedulingConflict,
};

const CANDIDATE_COLS: &str = "id,source_type,source_id,source_di_id,readiness_status,readiness_score,priority_id,required_skill_set_json,required_parts_ready,permit_status,shutdown_requirement,prerequisite_status,estimated_duration_hours,assigned_personnel_id,assigned_team_id,window_start,window_end,suggested_assignees_json,availability_conflict_count,skill_match_score,estimated_labor_cost_range_json,blocking_flags_json,open_work_count,next_available_window,estimated_assignment_risk,risk_reason_codes_json,row_version,created_at,updated_at";
const CONFLICT_COLS: &str =
    "id,candidate_id,conflict_type,reference_type,reference_id,reason_code,severity,details_json,resolved_at,created_at";

#[derive(Debug, Clone)]
struct EvaluatedConflict {
    conflict_type: String,
    reference_type: Option<String>,
    reference_id: Option<i64>,
    reason_code: String,
    severity: String,
    details_json: Option<String>,
    dimension: &'static str,
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "planning row decode failed for column '{column}': {e}"
    ))
}

fn map_candidate(row: &QueryResult) -> AppResult<ScheduleCandidate> {
    Ok(ScheduleCandidate {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        source_type: row
            .try_get("", "source_type")
            .map_err(|e| decode_err("source_type", e))?,
        source_id: row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?,
        source_di_id: row
            .try_get("", "source_di_id")
            .map_err(|e| decode_err("source_di_id", e))?,
        readiness_status: row
            .try_get("", "readiness_status")
            .map_err(|e| decode_err("readiness_status", e))?,
        readiness_score: row
            .try_get("", "readiness_score")
            .map_err(|e| decode_err("readiness_score", e))?,
        priority_id: row
            .try_get("", "priority_id")
            .map_err(|e| decode_err("priority_id", e))?,
        required_skill_set_json: row
            .try_get("", "required_skill_set_json")
            .map_err(|e| decode_err("required_skill_set_json", e))?,
        required_parts_ready: row
            .try_get("", "required_parts_ready")
            .map_err(|e| decode_err("required_parts_ready", e))?,
        permit_status: row
            .try_get("", "permit_status")
            .map_err(|e| decode_err("permit_status", e))?,
        shutdown_requirement: row
            .try_get("", "shutdown_requirement")
            .map_err(|e| decode_err("shutdown_requirement", e))?,
        prerequisite_status: row
            .try_get("", "prerequisite_status")
            .map_err(|e| decode_err("prerequisite_status", e))?,
        estimated_duration_hours: row
            .try_get("", "estimated_duration_hours")
            .map_err(|e| decode_err("estimated_duration_hours", e))?,
        assigned_personnel_id: row
            .try_get("", "assigned_personnel_id")
            .map_err(|e| decode_err("assigned_personnel_id", e))?,
        assigned_team_id: row
            .try_get("", "assigned_team_id")
            .map_err(|e| decode_err("assigned_team_id", e))?,
        window_start: row
            .try_get("", "window_start")
            .map_err(|e| decode_err("window_start", e))?,
        window_end: row
            .try_get("", "window_end")
            .map_err(|e| decode_err("window_end", e))?,
        suggested_assignees_json: row
            .try_get("", "suggested_assignees_json")
            .map_err(|e| decode_err("suggested_assignees_json", e))?,
        availability_conflict_count: row
            .try_get("", "availability_conflict_count")
            .map_err(|e| decode_err("availability_conflict_count", e))?,
        skill_match_score: row
            .try_get("", "skill_match_score")
            .map_err(|e| decode_err("skill_match_score", e))?,
        estimated_labor_cost_range_json: row
            .try_get("", "estimated_labor_cost_range_json")
            .map_err(|e| decode_err("estimated_labor_cost_range_json", e))?,
        blocking_flags_json: row
            .try_get("", "blocking_flags_json")
            .map_err(|e| decode_err("blocking_flags_json", e))?,
        open_work_count: row
            .try_get("", "open_work_count")
            .map_err(|e| decode_err("open_work_count", e))?,
        next_available_window: row
            .try_get("", "next_available_window")
            .map_err(|e| decode_err("next_available_window", e))?,
        estimated_assignment_risk: row
            .try_get("", "estimated_assignment_risk")
            .map_err(|e| decode_err("estimated_assignment_risk", e))?,
        risk_reason_codes_json: row
            .try_get("", "risk_reason_codes_json")
            .map_err(|e| decode_err("risk_reason_codes_json", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_conflict(row: &QueryResult) -> AppResult<SchedulingConflict> {
    Ok(SchedulingConflict {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        candidate_id: row
            .try_get("", "candidate_id")
            .map_err(|e| decode_err("candidate_id", e))?,
        conflict_type: row
            .try_get("", "conflict_type")
            .map_err(|e| decode_err("conflict_type", e))?,
        reference_type: row
            .try_get("", "reference_type")
            .map_err(|e| decode_err("reference_type", e))?,
        reference_id: row
            .try_get("", "reference_id")
            .map_err(|e| decode_err("reference_id", e))?,
        reason_code: row
            .try_get("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        severity: row
            .try_get("", "severity")
            .map_err(|e| decode_err("severity", e))?,
        details_json: row
            .try_get("", "details_json")
            .map_err(|e| decode_err("details_json", e))?,
        resolved_at: row
            .try_get("", "resolved_at")
            .map_err(|e| decode_err("resolved_at", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

fn parse_string_array(input: Option<&str>) -> Vec<String> {
    let Some(raw) = input else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(arr) = parsed.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|value| {
            if let Some(v) = value.as_str() {
                return Some(v.trim().to_string());
            }
            if let Some(obj) = value.as_object() {
                for key in ["code", "skill_code", "part", "part_code", "article_code", "name"] {
                    if let Some(v) = obj.get(key).and_then(Value::as_str) {
                        return Some(v.trim().to_string());
                    }
                }
            }
            None
        })
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn get_candidate(db: &DatabaseConnection, candidate_id: i64) -> AppResult<ScheduleCandidate> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {CANDIDATE_COLS} FROM schedule_candidates WHERE id = ?"),
            [candidate_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleCandidate".to_string(),
            id: candidate_id.to_string(),
        })?;
    map_candidate(&row)
}

pub async fn list_schedule_candidates(
    db: &DatabaseConnection,
    filter: ScheduleCandidateFilter,
) -> AppResult<Vec<ScheduleCandidate>> {
    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(source_type) = filter.source_type {
        where_sql.push("source_type = ?".to_string());
        values.push(source_type.into());
    }
    if let Some(readiness_status) = filter.readiness_status {
        where_sql.push("readiness_status = ?".to_string());
        values.push(readiness_status.into());
    }
    if let Some(assigned_personnel_id) = filter.assigned_personnel_id {
        where_sql.push("assigned_personnel_id = ?".to_string());
        values.push(assigned_personnel_id.into());
    }

    let limit = filter.limit.unwrap_or(400).clamp(1, 2_000);
    let include_resolved = filter.include_resolved_conflicts.unwrap_or(false);
    if !include_resolved {
        where_sql.push(
            "NOT EXISTS (SELECT 1 FROM scheduling_conflicts sc WHERE sc.candidate_id = schedule_candidates.id AND sc.resolved_at IS NULL)"
                .to_string(),
        );
    }

    values.push(limit.into());
    let sql = format!(
        "SELECT {CANDIDATE_COLS}
         FROM schedule_candidates
         WHERE {}
         ORDER BY readiness_status DESC, readiness_score DESC, updated_at DESC
         LIMIT ?",
        where_sql.join(" AND ")
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_candidate).collect()
}

pub async fn list_scheduling_conflicts(
    db: &DatabaseConnection,
    candidate_id: Option<i64>,
    include_resolved: Option<bool>,
) -> AppResult<Vec<SchedulingConflict>> {
    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(candidate_id) = candidate_id {
        where_sql.push("candidate_id = ?".to_string());
        values.push(candidate_id.into());
    }
    if !include_resolved.unwrap_or(false) {
        where_sql.push("resolved_at IS NULL".to_string());
    }

    let sql = format!(
        "SELECT {CONFLICT_COLS}
         FROM scheduling_conflicts
         WHERE {}
         ORDER BY created_at DESC, id DESC",
        where_sql.join(" AND ")
    );
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
        .await?;
    rows.iter().map(map_conflict).collect()
}

async fn refresh_from_work_orders(db: &DatabaseConnection, limit: i64) -> AppResult<(i64, i64)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                wo.id AS source_id,
                wo.source_di_id,
                wo.primary_responsible_id AS assigned_personnel_id,
                wo.assigned_group_id AS assigned_team_id,
                wo.planned_start AS window_start,
                wo.planned_end AS window_end,
                wo.expected_duration_hours,
                wos.code AS status_code
             FROM work_orders wo
             JOIN work_order_statuses wos ON wos.id = wo.status_id
             WHERE wos.code IN ('planned','ready_to_schedule','assigned','waiting_for_prerequisite')
             ORDER BY wo.updated_at DESC
             LIMIT ?",
            [limit.into()],
        ))
        .await?;

    let mut inserted = 0_i64;
    let mut updated = 0_i64;
    for row in rows {
        let source_id: i64 = row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?;
        let assigned_personnel_id: Option<i64> = row
            .try_get("", "assigned_personnel_id")
            .map_err(|e| decode_err("assigned_personnel_id", e))?;
        let assigned_team_id: Option<i64> = row
            .try_get("", "assigned_team_id")
            .map_err(|e| decode_err("assigned_team_id", e))?;
        let source_di_id: Option<i64> = row
            .try_get("", "source_di_id")
            .map_err(|e| decode_err("source_di_id", e))?;
        let window_start: Option<String> =
            row.try_get("", "window_start").map_err(|e| decode_err("window_start", e))?;
        let window_end: Option<String> =
            row.try_get("", "window_end").map_err(|e| decode_err("window_end", e))?;
        let estimated_duration_hours: Option<f64> = row
            .try_get("", "expected_duration_hours")
            .map_err(|e| decode_err("expected_duration_hours", e))?;
        let status_code: String = row
            .try_get("", "status_code")
            .map_err(|e| decode_err("status_code", e))?;

        let prerequisite_status = if status_code == "waiting_for_prerequisite" {
            "blocked"
        } else {
            "ready"
        };

        let effect = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO schedule_candidates
                    (source_type, source_id, source_di_id, assigned_personnel_id, assigned_team_id, window_start, window_end, estimated_duration_hours, prerequisite_status, permit_status, shutdown_requirement)
                 VALUES
                    ('work_order', ?, ?, ?, ?, ?, ?, ?, ?, 'unknown', NULL)
                 ON CONFLICT(source_type, source_id) DO UPDATE SET
                    source_di_id = excluded.source_di_id,
                    assigned_personnel_id = excluded.assigned_personnel_id,
                    assigned_team_id = excluded.assigned_team_id,
                    window_start = excluded.window_start,
                    window_end = excluded.window_end,
                    estimated_duration_hours = excluded.estimated_duration_hours,
                    prerequisite_status = excluded.prerequisite_status,
                    row_version = schedule_candidates.row_version + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                [
                    source_id.into(),
                    source_di_id.into(),
                    assigned_personnel_id.into(),
                    assigned_team_id.into(),
                    window_start.into(),
                    window_end.into(),
                    estimated_duration_hours.into(),
                    prerequisite_status.to_string().into(),
                ],
            ))
            .await?;

        if effect.rows_affected() == 1 {
            inserted += 1;
        } else {
            updated += 1;
        }
    }
    Ok((inserted, updated))
}

async fn refresh_from_pm_occurrences(db: &DatabaseConnection, limit: i64) -> AppResult<(i64, i64)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                po.id AS source_id,
                po.due_at AS window_start,
                datetime(po.due_at, '+' || CAST(COALESCE(pv.estimated_duration_hours, 2) AS TEXT) || ' hours') AS window_end,
                COALESCE(pv.estimated_duration_hours, 2) AS estimated_duration_hours,
                po.status AS occurrence_status,
                pp.requires_permit,
                pp.requires_shutdown,
                wo.source_di_id,
                wo.primary_responsible_id AS assigned_personnel_id,
                wo.assigned_group_id AS assigned_team_id,
                pv.required_skills_json
             FROM pm_occurrences po
             JOIN pm_plan_versions pv ON pv.id = po.plan_version_id
             JOIN pm_plans pp ON pp.id = po.pm_plan_id
             LEFT JOIN work_orders wo ON wo.id = po.linked_work_order_id
             WHERE po.status IN ('generated','ready_for_scheduling','deferred')
             ORDER BY po.updated_at DESC
             LIMIT ?",
            [limit.into()],
        ))
        .await?;

    let mut inserted = 0_i64;
    let mut updated = 0_i64;
    for row in rows {
        let source_id: i64 = row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?;
        let source_di_id: Option<i64> = row
            .try_get("", "source_di_id")
            .map_err(|e| decode_err("source_di_id", e))?;
        let assigned_personnel_id: Option<i64> = row
            .try_get("", "assigned_personnel_id")
            .map_err(|e| decode_err("assigned_personnel_id", e))?;
        let assigned_team_id: Option<i64> = row
            .try_get("", "assigned_team_id")
            .map_err(|e| decode_err("assigned_team_id", e))?;
        let window_start: Option<String> =
            row.try_get("", "window_start").map_err(|e| decode_err("window_start", e))?;
        let window_end: Option<String> =
            row.try_get("", "window_end").map_err(|e| decode_err("window_end", e))?;
        let estimated_duration_hours: Option<f64> = row
            .try_get("", "estimated_duration_hours")
            .map_err(|e| decode_err("estimated_duration_hours", e))?;
        let occurrence_status: String = row
            .try_get("", "occurrence_status")
            .map_err(|e| decode_err("occurrence_status", e))?;
        let requires_permit: i64 = row
            .try_get("", "requires_permit")
            .map_err(|e| decode_err("requires_permit", e))?;
        let requires_shutdown: i64 = row
            .try_get("", "requires_shutdown")
            .map_err(|e| decode_err("requires_shutdown", e))?;
        let required_skills_json: Option<String> = row
            .try_get("", "required_skills_json")
            .map_err(|e| decode_err("required_skills_json", e))?;

        let prerequisite_status = if occurrence_status == "deferred" {
            "blocked"
        } else {
            "ready"
        };
        let permit_status = if requires_permit == 1 {
            "required_missing"
        } else {
            "not_required"
        };
        let shutdown_requirement = if requires_shutdown == 1 {
            Some("required".to_string())
        } else {
            None
        };

        let effect = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO schedule_candidates
                    (source_type, source_id, source_di_id, assigned_personnel_id, assigned_team_id, window_start, window_end, estimated_duration_hours, prerequisite_status, permit_status, shutdown_requirement, required_skill_set_json)
                 VALUES
                    ('pm_occurrence', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(source_type, source_id) DO UPDATE SET
                    source_di_id = excluded.source_di_id,
                    assigned_personnel_id = excluded.assigned_personnel_id,
                    assigned_team_id = excluded.assigned_team_id,
                    window_start = excluded.window_start,
                    window_end = excluded.window_end,
                    estimated_duration_hours = excluded.estimated_duration_hours,
                    prerequisite_status = excluded.prerequisite_status,
                    permit_status = excluded.permit_status,
                    shutdown_requirement = excluded.shutdown_requirement,
                    required_skill_set_json = excluded.required_skill_set_json,
                    row_version = schedule_candidates.row_version + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                [
                    source_id.into(),
                    source_di_id.into(),
                    assigned_personnel_id.into(),
                    assigned_team_id.into(),
                    window_start.into(),
                    window_end.into(),
                    estimated_duration_hours.into(),
                    prerequisite_status.to_string().into(),
                    permit_status.to_string().into(),
                    shutdown_requirement.into(),
                    required_skills_json.into(),
                ],
            ))
            .await?;
        if effect.rows_affected() == 1 {
            inserted += 1;
        } else {
            updated += 1;
        }
    }

    Ok((inserted, updated))
}

async fn refresh_from_approved_di(db: &DatabaseConnection, limit: i64) -> AppResult<(i64, i64)> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                ir.id AS source_id,
                ir.submitted_at AS window_start,
                datetime(ir.submitted_at, '+4 hours') AS window_end,
                4.0 AS estimated_duration_hours,
                ir.deferred_until
             FROM intervention_requests ir
             WHERE ir.status = 'approved_for_planning'
               AND ir.converted_to_wo_id IS NULL
             ORDER BY ir.updated_at DESC
             LIMIT ?",
            [limit.into()],
        ))
        .await?;
    let mut inserted = 0_i64;
    let mut updated = 0_i64;
    for row in rows {
        let source_id: i64 = row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?;
        let window_start: Option<String> =
            row.try_get("", "window_start").map_err(|e| decode_err("window_start", e))?;
        let window_end: Option<String> =
            row.try_get("", "window_end").map_err(|e| decode_err("window_end", e))?;
        let estimated_duration_hours: Option<f64> = row
            .try_get("", "estimated_duration_hours")
            .map_err(|e| decode_err("estimated_duration_hours", e))?;
        let deferred_until: Option<String> = row
            .try_get("", "deferred_until")
            .map_err(|e| decode_err("deferred_until", e))?;
        let prerequisite_status = if deferred_until.is_some() {
            "blocked"
        } else {
            "ready"
        };

        let effect = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO schedule_candidates
                    (source_type, source_id, source_di_id, window_start, window_end, estimated_duration_hours, prerequisite_status, permit_status)
                 VALUES
                    ('inspection_follow_up', ?, ?, ?, ?, ?, ?, 'unknown')
                 ON CONFLICT(source_type, source_id) DO UPDATE SET
                    source_di_id = excluded.source_di_id,
                    window_start = excluded.window_start,
                    window_end = excluded.window_end,
                    estimated_duration_hours = excluded.estimated_duration_hours,
                    prerequisite_status = excluded.prerequisite_status,
                    row_version = schedule_candidates.row_version + 1,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                [
                    source_id.into(),
                    Some(source_id).into(),
                    window_start.into(),
                    window_end.into(),
                    estimated_duration_hours.into(),
                    prerequisite_status.to_string().into(),
                ],
            ))
            .await?;
        if effect.rows_affected() == 1 {
            inserted += 1;
        } else {
            updated += 1;
        }
    }
    Ok((inserted, updated))
}

async fn evaluate_candidate(db: &DatabaseConnection, candidate_id: i64) -> AppResult<(String, f64, i64)> {
    let candidate = get_candidate(db, candidate_id).await?;
    let mut conflicts: Vec<EvaluatedConflict> = Vec::new();

    let mut required_skills = parse_string_array(candidate.required_skill_set_json.as_deref());
    let mut required_parts_missing = false;
    let mut permit_blocked = candidate.permit_status == "required_missing";
    let mut shutdown_required = candidate
        .shutdown_requirement
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case("required"))
        .unwrap_or(false);
    let mut prerequisite_blocked = candidate.prerequisite_status == "blocked";

    if candidate.source_type == "work_order" {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT
                    wos.code AS status_code,
                    COALESCE(SUM(CASE WHEN wop.quantity_planned > COALESCE(wop.quantity_reserved, 0) THEN 1 ELSE 0 END), 0) AS uncovered_parts
                 FROM work_orders wo
                 JOIN work_order_statuses wos ON wos.id = wo.status_id
                 LEFT JOIN work_order_parts wop ON wop.work_order_id = wo.id
                 WHERE wo.id = ?
                 GROUP BY wo.id, wos.code",
                [candidate.source_id.into()],
            ))
            .await?;
        if let Some(row) = row {
            let status_code: String = row.try_get("", "status_code").map_err(|e| decode_err("status_code", e))?;
            let uncovered_parts: i64 = row
                .try_get("", "uncovered_parts")
                .map_err(|e| decode_err("uncovered_parts", e))?;
            if status_code == "waiting_for_prerequisite" {
                prerequisite_blocked = true;
            }
            if uncovered_parts > 0 {
                required_parts_missing = true;
            }
        }
    } else if candidate.source_type == "pm_occurrence" {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT
                    pp.requires_permit,
                    pp.requires_shutdown,
                    pv.required_parts_json,
                    pv.required_skills_json,
                    po.status
                 FROM pm_occurrences po
                 JOIN pm_plans pp ON pp.id = po.pm_plan_id
                 JOIN pm_plan_versions pv ON pv.id = po.plan_version_id
                 WHERE po.id = ?",
                [candidate.source_id.into()],
            ))
            .await?;
        if let Some(row) = row {
            let requires_permit: i64 = row
                .try_get("", "requires_permit")
                .map_err(|e| decode_err("requires_permit", e))?;
            let requires_shutdown: i64 = row
                .try_get("", "requires_shutdown")
                .map_err(|e| decode_err("requires_shutdown", e))?;
            let required_parts_json: Option<String> = row
                .try_get("", "required_parts_json")
                .map_err(|e| decode_err("required_parts_json", e))?;
            let required_skills_json: Option<String> = row
                .try_get("", "required_skills_json")
                .map_err(|e| decode_err("required_skills_json", e))?;
            let pm_status: String = row.try_get("", "status").map_err(|e| decode_err("status", e))?;
            if pm_status == "deferred" {
                prerequisite_blocked = true;
            }
            if parse_string_array(required_parts_json.as_deref()).len() > 0 {
                required_parts_missing = true;
            }
            if required_skills.is_empty() {
                required_skills = parse_string_array(required_skills_json.as_deref());
            }
            permit_blocked = requires_permit == 1 && candidate.permit_status != "approved";
            shutdown_required = requires_shutdown == 1;
        }
    }

    if required_parts_missing {
        conflicts.push(EvaluatedConflict {
            conflict_type: "missing_critical_part".to_string(),
            reference_type: Some(candidate.source_type.clone()),
            reference_id: Some(candidate.source_id),
            reason_code: "PARTS_NOT_RESERVED".to_string(),
            severity: "high".to_string(),
            details_json: Some(
                json!({
                    "message": "Critical parts are not fully reserved for this candidate.",
                    "source_type": candidate.source_type,
                    "source_id": candidate.source_id
                })
                .to_string(),
            ),
            dimension: "parts",
        });
    }

    let mut skill_match_score: Option<f64> = None;
    if !required_skills.is_empty() {
        if let Some(personnel_id) = candidate.assigned_personnel_id {
            let rows = db
                .query_all(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT rv.code
                     FROM personnel_skills ps
                     JOIN reference_values rv ON rv.id = ps.reference_value_id
                     WHERE ps.personnel_id = ?
                       AND (ps.valid_to IS NULL OR ps.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
                    [personnel_id.into()],
                ))
                .await?;
            let owned: BTreeSet<String> = rows
                .iter()
                .filter_map(|row| row.try_get::<String>("", "code").ok())
                .collect();
            let matched = required_skills.iter().filter(|code| owned.contains(*code)).count();
            skill_match_score = Some((matched as f64) / (required_skills.len() as f64));
            if matched < required_skills.len() {
                conflicts.push(EvaluatedConflict {
                    conflict_type: "no_qualified_technician".to_string(),
                    reference_type: Some("personnel".to_string()),
                    reference_id: Some(personnel_id),
                    reason_code: "SKILL_GAP".to_string(),
                    severity: "high".to_string(),
                    details_json: Some(
                        json!({
                            "required_skills": required_skills,
                            "matched_skills": matched
                        })
                        .to_string(),
                    ),
                    dimension: "skills",
                });
            }
        } else {
            conflicts.push(EvaluatedConflict {
                conflict_type: "no_qualified_technician".to_string(),
                reference_type: Some("personnel".to_string()),
                reference_id: None,
                reason_code: "ASSIGNMENT_REQUIRED".to_string(),
                severity: "high".to_string(),
                details_json: Some(
                    json!({
                        "required_skills": required_skills
                    })
                    .to_string(),
                ),
                dimension: "skills",
            });
        }
    }

    if permit_blocked {
        conflicts.push(EvaluatedConflict {
            conflict_type: "permit_not_ready".to_string(),
            reference_type: Some(candidate.source_type.clone()),
            reference_id: Some(candidate.source_id),
            reason_code: "PERMIT_REQUIRED".to_string(),
            severity: "medium".to_string(),
            details_json: Some(
                json!({
                    "permit_status": candidate.permit_status
                })
                .to_string(),
            ),
            dimension: "permits",
        });
    }

    if prerequisite_blocked {
        conflicts.push(EvaluatedConflict {
            conflict_type: "prerequisite_incomplete".to_string(),
            reference_type: Some(candidate.source_type.clone()),
            reference_id: Some(candidate.source_id),
            reason_code: "PREREQUISITE_BLOCKED".to_string(),
            severity: "medium".to_string(),
            details_json: None,
            dimension: "prerequisites",
        });
    }

    if shutdown_required {
        let has_window = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS ok
                 FROM planning_windows pw
                 WHERE pw.is_locked = 0
                   AND (? IS NULL OR pw.window_start <= ?)
                   AND (? IS NULL OR pw.window_end >= ?)
                 LIMIT 1",
                [
                    candidate.window_start.clone().into(),
                    candidate.window_start.clone().into(),
                    candidate.window_end.clone().into(),
                    candidate.window_end.clone().into(),
                ],
            ))
            .await?
            .is_some();
        if !has_window {
            conflicts.push(EvaluatedConflict {
                conflict_type: "locked_window".to_string(),
                reference_type: Some("planning_window".to_string()),
                reference_id: None,
                reason_code: "NO_FEASIBLE_WINDOW".to_string(),
                severity: "medium".to_string(),
                details_json: Some(
                    json!({
                        "window_start": candidate.window_start,
                        "window_end": candidate.window_end
                    })
                    .to_string(),
                ),
                dimension: "windows",
            });
        }
    }

    let mut availability_conflict_count = 0_i64;
    let mut next_available_window: Option<String> = None;
    let mut open_work_count: Option<i64> = None;
    let mut suggested_assignees_json: Option<String> = None;
    let mut labor_cost_range: Option<String> = None;
    let mut risk_reasons: Vec<String> = Vec::new();
    if let Some(personnel_id) = candidate.assigned_personnel_id {
        let conflict_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT
                    COUNT(*) AS c,
                    MIN(ab.end_at) AS next_window
                 FROM personnel_availability_blocks ab
                 WHERE ab.personnel_id = ?
                   AND (? IS NOT NULL AND ? IS NOT NULL)
                   AND ab.start_at < ?
                   AND ab.end_at > ?",
                [
                    personnel_id.into(),
                    candidate.window_start.clone().into(),
                    candidate.window_end.clone().into(),
                    candidate.window_end.clone().into(),
                    candidate.window_start.clone().into(),
                ],
            ))
            .await?;
        if let Some(conflict_row) = conflict_row {
            availability_conflict_count = conflict_row.try_get("", "c").unwrap_or_default();
            next_available_window = conflict_row.try_get("", "next_window").unwrap_or(None);
        }

        let overlap_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c
                 FROM work_orders wo
                 JOIN work_order_statuses wos ON wos.id = wo.status_id
                 WHERE wo.primary_responsible_id = ?
                   AND wos.code IN ('planned','ready_to_schedule','assigned','waiting_for_prerequisite','in_progress')
                   AND wo.id != (
                     CASE WHEN ? = 'work_order' THEN ? ELSE -1 END
                   )
                   AND (? IS NOT NULL AND ? IS NOT NULL)
                   AND wo.planned_start IS NOT NULL
                   AND wo.planned_end IS NOT NULL
                   AND wo.planned_start < ?
                   AND wo.planned_end > ?",
                [
                    personnel_id.into(),
                    candidate.source_type.clone().into(),
                    candidate.source_id.into(),
                    candidate.window_start.clone().into(),
                    candidate.window_end.clone().into(),
                    candidate.window_end.clone().into(),
                    candidate.window_start.clone().into(),
                ],
            ))
            .await?;
        let overlap_count: i64 = overlap_row
            .and_then(|row| row.try_get("", "c").ok())
            .unwrap_or_default();
        availability_conflict_count += overlap_count;

        if availability_conflict_count > 0 {
            conflicts.push(EvaluatedConflict {
                conflict_type: "double_booking".to_string(),
                reference_type: Some("personnel".to_string()),
                reference_id: Some(personnel_id),
                reason_code: "ASSIGNEE_UNAVAILABLE".to_string(),
                severity: "high".to_string(),
                details_json: Some(
                    json!({
                        "availability_conflict_count": availability_conflict_count,
                        "nearest_feasible_window": next_available_window
                    })
                    .to_string(),
                ),
                dimension: "windows",
            });
        }

        open_work_count = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COUNT(*) AS c
                 FROM work_orders wo
                 JOIN work_order_statuses wos ON wos.id = wo.status_id
                 WHERE wo.primary_responsible_id = ?
                   AND wos.code IN ('planned','ready_to_schedule','assigned','waiting_for_prerequisite','in_progress')",
                [personnel_id.into()],
            ))
            .await?
            .and_then(|row| row.try_get("", "c").ok());

        let rate = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT labor_rate, overtime_rate
                 FROM personnel_rate_cards
                 WHERE personnel_id = ?
                 ORDER BY effective_from DESC
                 LIMIT 1",
                [personnel_id.into()],
            ))
            .await?;
        if let (Some(duration), Some(rate_row)) = (candidate.estimated_duration_hours, rate) {
            let labor_rate: f64 = rate_row.try_get("", "labor_rate").unwrap_or(0.0);
            let overtime_rate: f64 = rate_row.try_get("", "overtime_rate").unwrap_or(labor_rate);
            labor_cost_range = Some(
                json!({
                    "min": duration * labor_rate,
                    "max": duration * overtime_rate
                })
                .to_string(),
            );
        }
    }

    if candidate.assigned_personnel_id.is_none() && !required_skills.is_empty() {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT
                    p.id,
                    p.full_name,
                    COUNT(DISTINCT rv.code) AS matching_skills
                 FROM personnel p
                 JOIN personnel_skills ps ON ps.personnel_id = p.id
                 JOIN reference_values rv ON rv.id = ps.reference_value_id
                 WHERE p.availability_status <> 'inactive'
                 GROUP BY p.id, p.full_name
                 ORDER BY matching_skills DESC, p.full_name ASC
                 LIMIT 5",
                [],
            ))
            .await?;
        let suggestions: Vec<Value> = rows
            .into_iter()
            .map(|row| {
                json!({
                    "personnel_id": row.try_get::<i64>("", "id").unwrap_or_default(),
                    "full_name": row.try_get::<String>("", "full_name").unwrap_or_default(),
                    "matching_skills": row.try_get::<i64>("", "matching_skills").unwrap_or_default()
                })
            })
            .collect();
        if !suggestions.is_empty() {
            suggested_assignees_json = Some(Value::Array(suggestions).to_string());
        }
    }

    let mut blocker_dimensions: BTreeSet<&'static str> = BTreeSet::new();
    let blocker_codes: Vec<String> = conflicts
        .iter()
        .map(|conflict| {
            blocker_dimensions.insert(conflict.dimension);
            conflict.conflict_type.clone()
        })
        .collect();
    let readiness_score = (100.0 - (blocker_dimensions.len() as f64 * 20.0)).max(0.0);
    let readiness_status = if conflicts.is_empty() { "ready" } else { "blocked" };

    if availability_conflict_count > 0 {
        risk_reasons.push("ASSIGNEE_UNAVAILABLE".to_string());
    }
    if blocker_codes.iter().any(|code| code == "missing_critical_part") {
        risk_reasons.push("PARTS_BLOCKED".to_string());
    }
    if blocker_codes.iter().any(|code| code == "no_qualified_technician") {
        risk_reasons.push("SKILL_GAP".to_string());
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM scheduling_conflicts WHERE candidate_id = ?",
        [candidate_id.into()],
    ))
    .await?;
    for conflict in &conflicts {
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO scheduling_conflicts
                (candidate_id, conflict_type, reference_type, reference_id, reason_code, severity, details_json)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                candidate_id.into(),
                conflict.conflict_type.clone().into(),
                conflict.reference_type.clone().into(),
                conflict.reference_id.into(),
                conflict.reason_code.clone().into(),
                conflict.severity.clone().into(),
                conflict.details_json.clone().into(),
            ],
        ))
        .await?;
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_candidates
         SET readiness_status = ?,
             readiness_score = ?,
             required_parts_ready = ?,
             availability_conflict_count = ?,
             skill_match_score = ?,
             suggested_assignees_json = ?,
             estimated_labor_cost_range_json = ?,
             blocking_flags_json = ?,
             open_work_count = ?,
             next_available_window = ?,
             estimated_assignment_risk = ?,
             risk_reason_codes_json = ?,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [
            readiness_status.into(),
            readiness_score.into(),
            i64::from(!required_parts_missing).into(),
            availability_conflict_count.into(),
            skill_match_score.into(),
            suggested_assignees_json.into(),
            labor_cost_range.into(),
            json!(blocker_codes).to_string().into(),
            open_work_count.into(),
            next_available_window.into(),
            (if conflicts.is_empty() { 0.0 } else { 0.35 + (conflicts.len() as f64 * 0.1) }).into(),
            json!(risk_reasons).to_string().into(),
            candidate_id.into(),
        ],
    ))
    .await?;

    Ok((readiness_status.to_string(), readiness_score, conflicts.len() as i64))
}

pub async fn refresh_schedule_candidates(
    db: &DatabaseConnection,
    input: RefreshScheduleCandidatesInput,
) -> AppResult<RefreshScheduleCandidatesResult> {
    let limit = input.limit_per_source.unwrap_or(300).clamp(1, 1_000);
    let mut inserted = 0_i64;
    let mut updated = 0_i64;

    if input.include_work_orders.unwrap_or(true) {
        let (ins, upd) = refresh_from_work_orders(db, limit).await?;
        inserted += ins;
        updated += upd;
    }
    if input.include_pm_occurrences.unwrap_or(true) {
        let (ins, upd) = refresh_from_pm_occurrences(db, limit).await?;
        inserted += ins;
        updated += upd;
    }
    if input.include_approved_di.unwrap_or(true) {
        let (ins, upd) = refresh_from_approved_di(db, limit).await?;
        inserted += ins;
        updated += upd;
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM schedule_candidates ORDER BY updated_at DESC LIMIT ?",
            [((limit * 3).max(200)).into()],
        ))
        .await?;

    let mut ready_count = 0_i64;
    let mut blocked_count = 0_i64;
    let mut evaluated_count = 0_i64;
    for row in rows {
        let candidate_id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let (readiness_status, _, _) = evaluate_candidate(db, candidate_id).await?;
        evaluated_count += 1;
        if readiness_status == "ready" {
            ready_count += 1;
        } else {
            blocked_count += 1;
        }
    }

    Ok(RefreshScheduleCandidatesResult {
        inserted_count: inserted,
        updated_count: updated,
        evaluated_count,
        ready_count,
        blocked_count,
    })
}

pub async fn get_schedule_backlog_snapshot(
    db: &DatabaseConnection,
    filter: ScheduleCandidateFilter,
) -> AppResult<ScheduleBacklogSnapshot> {
    let candidates = list_schedule_candidates(db, filter).await?;
    let mut conflict_rows = list_scheduling_conflicts(db, None, Some(false)).await?;

    let mut conflict_by_candidate: HashMap<i64, Vec<SchedulingConflict>> = HashMap::new();
    for conflict in conflict_rows.drain(..) {
        conflict_by_candidate
            .entry(conflict.candidate_id)
            .or_default()
            .push(conflict);
    }

    let mut ready_count = 0_i64;
    let mut blocked_count = 0_i64;
    let mut conflict_summary: Vec<CandidateConflictSummary> = Vec::new();
    for candidate in &candidates {
        if candidate.readiness_status == "ready" {
            ready_count += 1;
        } else {
            blocked_count += 1;
        }
        let conflicts = conflict_by_candidate.remove(&candidate.id).unwrap_or_default();
        let blocker_codes: Vec<String> = conflicts.iter().map(|c| c.conflict_type.clone()).collect();
        let blocker_dimensions: Vec<String> = blocker_codes
            .iter()
            .map(|code| match code.as_str() {
                "missing_critical_part" => "parts",
                "no_qualified_technician" => "skills",
                "permit_not_ready" => "permits",
                "locked_window" | "double_booking" => "windows",
                _ => "prerequisites",
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_string)
            .collect();
        conflict_summary.push(CandidateConflictSummary {
            candidate_id: candidate.id,
            blocker_codes,
            blocker_dimensions,
            readiness_status: candidate.readiness_status.clone(),
            readiness_score: candidate.readiness_score,
        });
    }

    Ok(ScheduleBacklogSnapshot {
        as_of: chrono::Utc::now().to_rfc3339(),
        candidate_count: candidates.len() as i64,
        ready_count,
        blocked_count,
        candidates,
        conflict_summary,
        derivation_rules: vec![
            "Readiness score starts at 100 and decreases by 20 per blocked dimension (parts, skills, permits, windows, prerequisites).".to_string(),
            "Personnel availability is validated against explicit availability blocks and overlapping active work orders.".to_string(),
            "Conflicts are persisted in scheduling_conflicts so guardrails are enforced by backend workflows, not only the UI.".to_string(),
            "DI-to-WO causality is preserved through source_di_id on schedule_candidates and conflict details payloads.".to_string(),
        ],
    })
}

