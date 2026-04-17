use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait,
};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit_event, NotificationEventInput};
use crate::{activity, notifications};

use super::domain::{
    CapacityRule, CapacityRuleFilter, CreateCapacityRuleInput, CreatePlanningWindowInput,
    CreateScheduleBreakInInput, CreateScheduleCommitmentInput, ExportPlanningGanttPdfInput,
    ExportedBinaryDocument, FreezeSchedulePeriodInput, NotifyTeamsInput, NotifyTeamsResult,
    PlanningAssigneeLane, PlanningGanttFilter, PlanningGanttSnapshot, PlanningWindow,
    PlanningWindowFilter, RescheduleCommitmentInput, ScheduleBreakIn, ScheduleBreakInFilter,
    ScheduleChangeLogEntry, ScheduleCommitment, ScheduleCommitmentFilter, TeamCapacityLoad,
    UpdateCapacityRuleInput, UpdatePlanningWindowInput,
};

const CAPACITY_COLS: &str = "id,entity_id,team_id,effective_start,effective_end,available_hours_per_day,max_overtime_hours_per_day,row_version,created_at,updated_at";
const WINDOW_COLS: &str =
    "id,entity_id,window_type,start_datetime,end_datetime,is_locked,lock_reason,row_version,created_at,updated_at";
const COMMITMENT_COLS: &str = "id,schedule_candidate_id,source_type,source_id,schedule_period_start,schedule_period_end,committed_start,committed_end,assigned_team_id,assigned_personnel_id,committed_by_id,frozen_at,estimated_labor_cost,budget_threshold,cost_variance_warning,has_blocking_conflict,nearest_feasible_window,row_version,created_at,updated_at";
const CHANGE_LOG_COLS: &str = "id,commitment_id,action_type,actor_id,field_changed,old_value,new_value,reason_code,reason_note,reason,details_json,created_at";
const BREAK_IN_COLS: &str = "id,schedule_commitment_id,break_in_reason,approved_by_user_id,approved_by_personnel_id,override_reason,old_slot_start,old_slot_end,new_slot_start,new_slot_end,old_assignee_id,new_assignee_id,cost_impact_delta,notification_dedupe_key,row_version,created_by_id,created_at";

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "planning scheduling decode failed for column '{column}': {e}"
    ))
}

fn parse_rfc3339(value: &str, field: &str) -> AppResult<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| {
        AppError::ValidationFailed(vec![format!("{field} must be a valid RFC3339 timestamp.")])
    })?;
    Ok(parsed.with_timezone(&Utc))
}

fn validate_window(start: &str, end: &str, start_field: &str, end_field: &str) -> AppResult<()> {
    let start_dt = parse_rfc3339(start, start_field)?;
    let end_dt = parse_rfc3339(end, end_field)?;
    if end_dt <= start_dt {
        return Err(AppError::ValidationFailed(vec![format!(
            "{end_field} must be strictly greater than {start_field}."
        )]));
    }
    Ok(())
}

fn hours_between(start: &str, end: &str) -> AppResult<f64> {
    let start_dt = parse_rfc3339(start, "committed_start")?;
    let end_dt = parse_rfc3339(end, "committed_end")?;
    Ok((end_dt - start_dt).num_minutes() as f64 / 60.0)
}

fn map_capacity_rule(row: &QueryResult) -> AppResult<CapacityRule> {
    Ok(CapacityRule {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_id: row
            .try_get("", "entity_id")
            .map_err(|e| decode_err("entity_id", e))?,
        team_id: row.try_get("", "team_id").map_err(|e| decode_err("team_id", e))?,
        effective_start: row
            .try_get("", "effective_start")
            .map_err(|e| decode_err("effective_start", e))?,
        effective_end: row
            .try_get("", "effective_end")
            .map_err(|e| decode_err("effective_end", e))?,
        available_hours_per_day: row
            .try_get("", "available_hours_per_day")
            .map_err(|e| decode_err("available_hours_per_day", e))?,
        max_overtime_hours_per_day: row
            .try_get("", "max_overtime_hours_per_day")
            .map_err(|e| decode_err("max_overtime_hours_per_day", e))?,
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

fn map_planning_window(row: &QueryResult) -> AppResult<PlanningWindow> {
    Ok(PlanningWindow {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_id: row
            .try_get("", "entity_id")
            .map_err(|e| decode_err("entity_id", e))?,
        window_type: row
            .try_get("", "window_type")
            .map_err(|e| decode_err("window_type", e))?,
        start_datetime: row
            .try_get("", "start_datetime")
            .map_err(|e| decode_err("start_datetime", e))?,
        end_datetime: row
            .try_get("", "end_datetime")
            .map_err(|e| decode_err("end_datetime", e))?,
        is_locked: row
            .try_get("", "is_locked")
            .map_err(|e| decode_err("is_locked", e))?,
        lock_reason: row
            .try_get("", "lock_reason")
            .map_err(|e| decode_err("lock_reason", e))?,
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

fn map_commitment(row: &QueryResult) -> AppResult<ScheduleCommitment> {
    Ok(ScheduleCommitment {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        schedule_candidate_id: row
            .try_get("", "schedule_candidate_id")
            .map_err(|e| decode_err("schedule_candidate_id", e))?,
        source_type: row
            .try_get("", "source_type")
            .map_err(|e| decode_err("source_type", e))?,
        source_id: row
            .try_get("", "source_id")
            .map_err(|e| decode_err("source_id", e))?,
        schedule_period_start: row
            .try_get("", "schedule_period_start")
            .map_err(|e| decode_err("schedule_period_start", e))?,
        schedule_period_end: row
            .try_get("", "schedule_period_end")
            .map_err(|e| decode_err("schedule_period_end", e))?,
        committed_start: row
            .try_get("", "committed_start")
            .map_err(|e| decode_err("committed_start", e))?,
        committed_end: row
            .try_get("", "committed_end")
            .map_err(|e| decode_err("committed_end", e))?,
        assigned_team_id: row
            .try_get("", "assigned_team_id")
            .map_err(|e| decode_err("assigned_team_id", e))?,
        assigned_personnel_id: row
            .try_get("", "assigned_personnel_id")
            .map_err(|e| decode_err("assigned_personnel_id", e))?,
        committed_by_id: row
            .try_get("", "committed_by_id")
            .map_err(|e| decode_err("committed_by_id", e))?,
        frozen_at: row
            .try_get("", "frozen_at")
            .map_err(|e| decode_err("frozen_at", e))?,
        estimated_labor_cost: row
            .try_get("", "estimated_labor_cost")
            .map_err(|e| decode_err("estimated_labor_cost", e))?,
        budget_threshold: row
            .try_get("", "budget_threshold")
            .map_err(|e| decode_err("budget_threshold", e))?,
        cost_variance_warning: row
            .try_get("", "cost_variance_warning")
            .map_err(|e| decode_err("cost_variance_warning", e))?,
        has_blocking_conflict: row
            .try_get("", "has_blocking_conflict")
            .map_err(|e| decode_err("has_blocking_conflict", e))?,
        nearest_feasible_window: row
            .try_get("", "nearest_feasible_window")
            .map_err(|e| decode_err("nearest_feasible_window", e))?,
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

fn map_change_log(row: &QueryResult) -> AppResult<ScheduleChangeLogEntry> {
    Ok(ScheduleChangeLogEntry {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        commitment_id: row
            .try_get("", "commitment_id")
            .map_err(|e| decode_err("commitment_id", e))?,
        action_type: row
            .try_get("", "action_type")
            .map_err(|e| decode_err("action_type", e))?,
        actor_id: row
            .try_get("", "actor_id")
            .map_err(|e| decode_err("actor_id", e))?,
        field_changed: row
            .try_get("", "field_changed")
            .map_err(|e| decode_err("field_changed", e))?,
        old_value: row
            .try_get("", "old_value")
            .map_err(|e| decode_err("old_value", e))?,
        new_value: row
            .try_get("", "new_value")
            .map_err(|e| decode_err("new_value", e))?,
        reason_code: row
            .try_get("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        reason_note: row
            .try_get("", "reason_note")
            .map_err(|e| decode_err("reason_note", e))?,
        reason: row
            .try_get("", "reason")
            .map_err(|e| decode_err("reason", e))?,
        details_json: row
            .try_get("", "details_json")
            .map_err(|e| decode_err("details_json", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

fn map_schedule_break_in(row: &QueryResult) -> AppResult<ScheduleBreakIn> {
    Ok(ScheduleBreakIn {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        schedule_commitment_id: row
            .try_get("", "schedule_commitment_id")
            .map_err(|e| decode_err("schedule_commitment_id", e))?,
        break_in_reason: row
            .try_get("", "break_in_reason")
            .map_err(|e| decode_err("break_in_reason", e))?,
        approved_by_user_id: row
            .try_get("", "approved_by_user_id")
            .map_err(|e| decode_err("approved_by_user_id", e))?,
        approved_by_personnel_id: row
            .try_get("", "approved_by_personnel_id")
            .map_err(|e| decode_err("approved_by_personnel_id", e))?,
        override_reason: row
            .try_get("", "override_reason")
            .map_err(|e| decode_err("override_reason", e))?,
        old_slot_start: row
            .try_get("", "old_slot_start")
            .map_err(|e| decode_err("old_slot_start", e))?,
        old_slot_end: row
            .try_get("", "old_slot_end")
            .map_err(|e| decode_err("old_slot_end", e))?,
        new_slot_start: row
            .try_get("", "new_slot_start")
            .map_err(|e| decode_err("new_slot_start", e))?,
        new_slot_end: row
            .try_get("", "new_slot_end")
            .map_err(|e| decode_err("new_slot_end", e))?,
        old_assignee_id: row
            .try_get("", "old_assignee_id")
            .map_err(|e| decode_err("old_assignee_id", e))?,
        new_assignee_id: row
            .try_get("", "new_assignee_id")
            .map_err(|e| decode_err("new_assignee_id", e))?,
        cost_impact_delta: row
            .try_get("", "cost_impact_delta")
            .map_err(|e| decode_err("cost_impact_delta", e))?,
        notification_dedupe_key: row
            .try_get("", "notification_dedupe_key")
            .map_err(|e| decode_err("notification_dedupe_key", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_by_id: row
            .try_get("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

async fn get_commitment(
    db: &impl ConnectionTrait,
    commitment_id: i64,
) -> AppResult<ScheduleCommitment> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {COMMITMENT_COLS} FROM schedule_commitments WHERE id = ?"),
            [commitment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleCommitment".to_string(),
            id: commitment_id.to_string(),
        })?;
    map_commitment(&row)
}

async fn append_change_log(
    db: &impl ConnectionTrait,
    commitment_id: Option<i64>,
    action_type: &str,
    actor_id: Option<i64>,
    field_changed: Option<String>,
    old_value: Option<String>,
    new_value: Option<String>,
    reason_code: Option<String>,
    reason_note: Option<String>,
    reason: Option<String>,
    details_json: Option<String>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO schedule_change_log
            (commitment_id, action_type, actor_id, field_changed, old_value, new_value, reason_code, reason_note, reason, details_json)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            commitment_id.into(),
            action_type.to_string().into(),
            actor_id.into(),
            field_changed.into(),
            old_value.into(),
            new_value.into(),
            reason_code.into(),
            reason_note.into(),
            reason.into(),
            details_json.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn validate_capacity_overlap(
    db: &impl ConnectionTrait,
    team_id: i64,
    entity_id: Option<i64>,
    effective_start: &str,
    effective_end: Option<&str>,
    exclude_id: Option<i64>,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM capacity_rules cr
             WHERE cr.team_id = ?
               AND COALESCE(cr.entity_id, -1) = COALESCE(?, -1)
               AND (? IS NULL OR cr.id <> ?)
               AND COALESCE(cr.effective_end, '9999-12-31') >= ?
               AND COALESCE(?, '9999-12-31') >= cr.effective_start",
            [
                team_id.into(),
                entity_id.into(),
                exclude_id.into(),
                exclude_id.into(),
                effective_start.to_string().into(),
                effective_end.map(str::to_string).into(),
            ],
        ))
        .await?;
    let overlaps: i64 = row.and_then(|r| r.try_get("", "c").ok()).unwrap_or_default();
    if overlaps > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Capacity rule overlaps an existing effective period for this team/entity.".to_string(),
        ]));
    }
    Ok(())
}

async fn find_locked_window(
    db: &impl ConnectionTrait,
    start: &str,
    end: &str,
) -> AppResult<Option<PlanningWindow>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {WINDOW_COLS}
                 FROM planning_windows pw
                 WHERE pw.is_locked = 1
                   AND pw.start_datetime < ?
                   AND pw.end_datetime > ?
                 ORDER BY pw.start_datetime ASC
                 LIMIT 1"
            ),
            [end.to_string().into(), start.to_string().into()],
        ))
        .await?;
    row.as_ref().map(map_planning_window).transpose()
}

async fn enforce_candidate_ready(
    db: &impl ConnectionTrait,
    candidate_id: i64,
    expected_row_version: Option<i64>,
) -> AppResult<(String, i64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT source_type, source_id, readiness_status, row_version
             FROM schedule_candidates
             WHERE id = ?",
            [candidate_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleCandidate".to_string(),
            id: candidate_id.to_string(),
        })?;

    let readiness_status: String = row
        .try_get("", "readiness_status")
        .map_err(|e| decode_err("readiness_status", e))?;
    let row_version: i64 = row
        .try_get("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    if let Some(expected) = expected_row_version {
        if expected != row_version {
            return Err(AppError::ValidationFailed(vec![
                "Schedule candidate was modified elsewhere (stale row_version).".to_string(),
            ]));
        }
    }
    if readiness_status != "ready" {
        return Err(AppError::ValidationFailed(vec![
            "Readiness gate failed: candidate is not ready for commitment.".to_string(),
        ]));
    }

    let unresolved: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM scheduling_conflicts
             WHERE candidate_id = ?
               AND resolved_at IS NULL",
            [candidate_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or_default();
    if unresolved > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Readiness gate failed: unresolved scheduling conflicts remain.".to_string(),
        ]));
    }

    let source_type: String = row
        .try_get("", "source_type")
        .map_err(|e| decode_err("source_type", e))?;
    let source_id: i64 = row
        .try_get("", "source_id")
        .map_err(|e| decode_err("source_id", e))?;
    Ok((source_type, source_id))
}

async fn compute_capacity_gate(
    db: &impl ConnectionTrait,
    team_id: i64,
    start: &str,
    end: &str,
    exclude_commitment_id: Option<i64>,
) -> AppResult<(bool, Option<String>)> {
    let start_dt = parse_rfc3339(start, "committed_start")?;
    let work_date = start_dt.date_naive().to_string();

    let rule_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT available_hours_per_day, max_overtime_hours_per_day
             FROM capacity_rules
             WHERE team_id = ?
               AND effective_start <= ?
               AND (effective_end IS NULL OR effective_end >= ?)
             ORDER BY effective_start DESC
             LIMIT 1",
            [team_id.into(), work_date.clone().into(), work_date.clone().into()],
        ))
        .await?;
    let available_hours = rule_row
        .as_ref()
        .and_then(|r| r.try_get::<f64>("", "available_hours_per_day").ok())
        .unwrap_or(8.0);
    let max_overtime = rule_row
        .as_ref()
        .and_then(|r| r.try_get::<f64>("", "max_overtime_hours_per_day").ok())
        .unwrap_or(0.0);

    let committed_hours: f64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM((julianday(committed_end) - julianday(committed_start)) * 24.0), 0.0) AS h
             FROM schedule_commitments
             WHERE assigned_team_id = ?
               AND schedule_period_start = ?
               AND (? IS NULL OR id <> ?)",
            [
                team_id.into(),
                work_date.into(),
                exclude_commitment_id.into(),
                exclude_commitment_id.into(),
            ],
        ))
        .await?
        .and_then(|r| r.try_get("", "h").ok())
        .unwrap_or(0.0);
    let requested_hours = hours_between(start, end)?;
    let total = committed_hours + requested_hours;
    let cap = available_hours + max_overtime;
    if total > cap {
        return Ok((false, Some(start_dt.to_rfc3339())));
    }
    Ok((true, None))
}

async fn enforce_assignee_availability(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    start: &str,
    end: &str,
) -> AppResult<()> {
    let blocks = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT start_at, end_at, block_type, is_critical
             FROM personnel_availability_blocks
             WHERE personnel_id = ?
               AND start_at < ?
               AND end_at > ?",
            [personnel_id.into(), end.to_string().into(), start.to_string().into()],
        ))
        .await?;
    if !blocks.is_empty() {
        let payload: Vec<Value> = blocks
            .into_iter()
            .map(|row| {
                json!({
                    "start_at": row.try_get::<String>("", "start_at").unwrap_or_default(),
                    "end_at": row.try_get::<String>("", "end_at").unwrap_or_default(),
                    "block_type": row.try_get::<String>("", "block_type").unwrap_or_default(),
                    "critical": row.try_get::<i64>("", "is_critical").unwrap_or_default() == 1
                })
            })
            .collect();
        return Err(AppError::ValidationFailed(vec![format!(
            "ASSIGNEE_UNAVAILABLE {}",
            json!({
                "personnel_id": personnel_id,
                "intervals": payload
            })
        )]));
    }

    Ok(())
}

async fn check_double_booking(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    start: &str,
    end: &str,
    exclude_commitment_id: Option<i64>,
) -> AppResult<i64> {
    let overlap_count = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM schedule_commitments
             WHERE assigned_personnel_id = ?
               AND committed_start < ?
               AND committed_end > ?
               AND (? IS NULL OR id <> ?)",
            [
                personnel_id.into(),
                end.to_string().into(),
                start.to_string().into(),
                exclude_commitment_id.into(),
                exclude_commitment_id.into(),
            ],
        ))
        .await?
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or_default();
    Ok(overlap_count)
}

fn is_valid_break_in_reason(reason: &str) -> bool {
    matches!(
        reason,
        "emergency" | "safety" | "production_loss" | "regulatory" | "other"
    )
}

async fn resolve_user_personnel_id(
    db: &impl ConnectionTrait,
    user_id: i64,
) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i64>("", "personnel_id").ok()))
}

async fn user_has_plan_approval_scope(db: &impl ConnectionTrait, user_id: i64) -> AppResult<bool> {
    let has: i64 = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c
             FROM user_scope_assignments usa
             JOIN role_permissions rp ON rp.role_id = usa.role_id
             JOIN permissions p ON p.id = rp.permission_id
             WHERE usa.user_id = ?
               AND usa.deleted_at IS NULL
               AND (usa.valid_to IS NULL OR usa.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))
               AND p.name IN ('plan.confirm', 'plan.windows')",
            [user_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or_default();
    Ok(has > 0)
}

async fn assignee_matches_candidate_skills(
    db: &impl ConnectionTrait,
    candidate_id: i64,
    personnel_id: i64,
) -> AppResult<bool> {
    let skill_json = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT required_skill_set_json
             FROM schedule_candidates
             WHERE id = ?",
            [candidate_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<String>("", "required_skill_set_json").ok());
    let Some(skill_json) = skill_json else {
        return Ok(true);
    };
    let required: Vec<String> = serde_json::from_str::<Vec<String>>(&skill_json)
        .unwrap_or_default()
        .into_iter()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .collect();
    if required.is_empty() {
        return Ok(true);
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lower(sd.code) AS code
             FROM personnel_skills ps
             JOIN skill_domains sd ON sd.id = ps.skill_domain_id
             WHERE ps.personnel_id = ?
               AND ps.status = 'active'",
            [personnel_id.into()],
        ))
        .await?;
    let mut have = BTreeSet::new();
    for row in rows {
        if let Ok(code) = row.try_get::<String>("", "code") {
            have.insert(code);
        }
    }
    Ok(required.into_iter().all(|code| have.contains(&code)))
}

async fn emit_planning_notification(
    db: &notifications::SqlitePool,
    event_code: &str,
    severity: &str,
    dedupe_key: Option<String>,
    source_record_id: String,
    payload: Value,
    title: String,
    body: String,
) {
    let _ = emit_event(
        db,
        NotificationEventInput {
            source_module: "planning".to_string(),
            source_record_id: Some(source_record_id),
            event_code: event_code.to_string(),
            category_code: "planning.schedule".to_string(),
            severity: severity.to_string(),
            dedupe_key,
            payload_json: Some(payload.to_string()),
            title,
            body: Some(body),
            action_url: Some("/planning".to_string()),
        },
    )
    .await;
}

async fn estimate_labor_cost(
    db: &impl ConnectionTrait,
    personnel_id: Option<i64>,
    committed_start: &str,
    committed_end: &str,
) -> AppResult<Option<f64>> {
    let Some(personnel_id) = personnel_id else {
        return Ok(None);
    };
    let rate_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT labor_rate
             FROM personnel_rate_cards
             WHERE personnel_id = ?
             ORDER BY effective_from DESC
             LIMIT 1",
            [personnel_id.into()],
        ))
        .await?;
    let Some(rate_row) = rate_row else {
        return Ok(None);
    };
    let rate: f64 = rate_row.try_get("", "labor_rate").unwrap_or(0.0);
    let duration = hours_between(committed_start, committed_end)?;
    Ok(Some((duration * rate * 100.0).round() / 100.0))
}

struct CommitmentGateResult {
    source_type: String,
    source_id: i64,
    nearest_feasible_window: Option<String>,
    estimated_labor_cost: Option<f64>,
    cost_variance_warning: i64,
}

async fn run_commitment_gates(
    tx: &impl ConnectionTrait,
    candidate_id: i64,
    expected_candidate_row_version: Option<i64>,
    committed_start: &str,
    committed_end: &str,
    assigned_team_id: i64,
    assigned_personnel_id: Option<i64>,
    allow_double_booking_override: bool,
    override_reason: Option<String>,
    budget_threshold: Option<f64>,
    actor_id: i64,
    exclude_commitment_id: Option<i64>,
) -> AppResult<CommitmentGateResult> {
    validate_window(
        committed_start,
        committed_end,
        "committed_start",
        "committed_end",
    )?;
    let (source_type, source_id) =
        enforce_candidate_ready(tx, candidate_id, expected_candidate_row_version).await?;

    if let Some(window) = find_locked_window(tx, committed_start, committed_end).await? {
        return Err(AppError::ValidationFailed(vec![format!(
            "Lock-window gate failed: planning window '{}' is locked for this range.",
            window.window_type
        )]));
    }

    let (capacity_ok, nearest_feasible_window) =
        compute_capacity_gate(tx, assigned_team_id, committed_start, committed_end, exclude_commitment_id)
            .await?;
    if !capacity_ok {
        return Err(AppError::ValidationFailed(vec![
            "Capacity gate failed: team load exceeds capacity + overtime for selected period.".to_string(),
        ]));
    }

    if let Some(personnel_id) = assigned_personnel_id {
        enforce_assignee_availability(
            tx,
            personnel_id,
            committed_start,
            committed_end,
        )
        .await?;

        let overlaps = check_double_booking(
            tx,
            personnel_id,
            committed_start,
            committed_end,
            exclude_commitment_id,
        )
        .await?;
        if overlaps > 0 && !allow_double_booking_override {
            return Err(AppError::ValidationFailed(vec![
                "Double-booking detected. Provide explicit override reason.".to_string(),
            ]));
        }
        if overlaps > 0 && allow_double_booking_override {
            let reason = override_reason
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    AppError::ValidationFailed(vec![
                        "Override reason is required when bypassing double-booking prevention."
                            .to_string(),
                    ])
                })?;
            append_change_log(
                tx,
                exclude_commitment_id,
                "override_double_booking",
                Some(actor_id),
                Some("assignment_window".to_string()),
                None,
                Some(format!("{committed_start} -> {committed_end}")),
                Some("double_booking".to_string()),
                Some(reason.clone()),
                Some(reason),
                Some(
                    json!({
                        "personnel_id": personnel_id,
                        "committed_start": committed_start,
                        "committed_end": committed_end
                    })
                    .to_string(),
                ),
            )
            .await?;
        }
    }

    let estimated_labor_cost =
        estimate_labor_cost(tx, assigned_personnel_id, committed_start, committed_end).await?;
    let cost_variance_warning = if let (Some(cost), Some(threshold)) =
        (estimated_labor_cost, budget_threshold)
    {
        i64::from(cost > threshold)
    } else {
        0
    };

    Ok(CommitmentGateResult {
        source_type,
        source_id,
        nearest_feasible_window,
        estimated_labor_cost,
        cost_variance_warning,
    })
}

pub async fn list_capacity_rules(
    db: &DatabaseConnection,
    filter: CapacityRuleFilter,
) -> AppResult<Vec<CapacityRule>> {
    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(entity_id) = filter.entity_id {
        where_sql.push("entity_id = ?".to_string());
        values.push(entity_id.into());
    }
    if let Some(team_id) = filter.team_id {
        where_sql.push("team_id = ?".to_string());
        values.push(team_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {CAPACITY_COLS}
                 FROM capacity_rules
                 WHERE {}
                 ORDER BY team_id ASC, effective_start DESC, id DESC",
                where_sql.join(" AND ")
            ),
            values,
        ))
        .await?;
    rows.iter().map(map_capacity_rule).collect()
}

pub async fn create_capacity_rule(
    db: &DatabaseConnection,
    input: CreateCapacityRuleInput,
) -> AppResult<CapacityRule> {
    if input.available_hours_per_day < 0.0 || input.max_overtime_hours_per_day < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Capacity rule hours must be non-negative.".to_string(),
        ]));
    }
    if let Some(end) = input.effective_end.as_deref() {
        if end < input.effective_start.as_str() {
            return Err(AppError::ValidationFailed(vec![
                "effective_end must be >= effective_start.".to_string(),
            ]));
        }
    }
    validate_capacity_overlap(
        db,
        input.team_id,
        input.entity_id,
        &input.effective_start,
        input.effective_end.as_deref(),
        None,
    )
    .await?;

    let inserted = db
        .execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO capacity_rules
            (entity_id, team_id, effective_start, effective_end, available_hours_per_day, max_overtime_hours_per_day)
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            input.entity_id.into(),
            input.team_id.into(),
            input.effective_start.into(),
            input.effective_end.into(),
            input.available_hours_per_day.into(),
            input.max_overtime_hours_per_day.into(),
        ],
    ))
        .await?;
    let id = i64::try_from(inserted.last_insert_id()).map_err(|_| {
        AppError::Internal(anyhow::anyhow!(
            "schedule_commitments last_insert_id does not fit i64"
        ))
    })?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {CAPACITY_COLS} FROM capacity_rules WHERE id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "CapacityRule".to_string(),
            id: id.to_string(),
        })?;
    map_capacity_rule(&row)
}

pub async fn update_capacity_rule(
    db: &DatabaseConnection,
    rule_id: i64,
    expected_row_version: i64,
    input: UpdateCapacityRuleInput,
) -> AppResult<CapacityRule> {
    let current = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {CAPACITY_COLS} FROM capacity_rules WHERE id = ?"),
            [rule_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "CapacityRule".to_string(),
            id: rule_id.to_string(),
        })?;
    let current = map_capacity_rule(&current)?;
    if current.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Capacity rule was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    let effective_start = input
        .effective_start
        .unwrap_or_else(|| current.effective_start.clone());
    let effective_end = input.effective_end.or(current.effective_end.clone());
    let available_hours = input
        .available_hours_per_day
        .unwrap_or(current.available_hours_per_day);
    let overtime_hours = input
        .max_overtime_hours_per_day
        .unwrap_or(current.max_overtime_hours_per_day);
    if available_hours < 0.0 || overtime_hours < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Capacity rule hours must be non-negative.".to_string(),
        ]));
    }
    validate_capacity_overlap(
        db,
        current.team_id,
        current.entity_id,
        &effective_start,
        effective_end.as_deref(),
        Some(rule_id),
    )
    .await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE capacity_rules
         SET effective_start = ?, effective_end = ?, available_hours_per_day = ?, max_overtime_hours_per_day = ?,
             row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        [
            effective_start.into(),
            effective_end.into(),
            available_hours.into(),
            overtime_hours.into(),
            rule_id.into(),
            expected_row_version.into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {CAPACITY_COLS} FROM capacity_rules WHERE id = ?"),
            [rule_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "CapacityRule".to_string(),
            id: rule_id.to_string(),
        })?;
    map_capacity_rule(&row)
}

pub async fn list_planning_windows(
    db: &DatabaseConnection,
    filter: PlanningWindowFilter,
) -> AppResult<Vec<PlanningWindow>> {
    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(entity_id) = filter.entity_id {
        where_sql.push("entity_id = ?".to_string());
        values.push(entity_id.into());
    }
    if let Some(window_type) = filter.window_type {
        where_sql.push("window_type = ?".to_string());
        values.push(window_type.into());
    }
    if !filter.include_locked.unwrap_or(true) {
        where_sql.push("is_locked = 0".to_string());
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {WINDOW_COLS}
                 FROM planning_windows
                 WHERE {}
                 ORDER BY start_datetime ASC, id ASC",
                where_sql.join(" AND ")
            ),
            values,
        ))
        .await?;
    rows.iter().map(map_planning_window).collect()
}

pub async fn create_planning_window(
    db: &DatabaseConnection,
    input: CreatePlanningWindowInput,
) -> AppResult<PlanningWindow> {
    validate_window(
        &input.start_datetime,
        &input.end_datetime,
        "start_datetime",
        "end_datetime",
    )?;
    let inserted = db
        .execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO planning_windows
            (entity_id, window_type, start_datetime, end_datetime, is_locked, lock_reason)
         VALUES (?, ?, ?, ?, ?, ?)",
        [
            input.entity_id.into(),
            input.window_type.into(),
            input.start_datetime.into(),
            input.end_datetime.into(),
            i64::from(input.is_locked.unwrap_or(false)).into(),
            input.lock_reason.into(),
        ],
    ))
        .await?;
    let id = i64::try_from(inserted.last_insert_id()).map_err(|_| {
        AppError::Internal(anyhow::anyhow!(
            "schedule_commitments last_insert_id does not fit i64"
        ))
    })?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {WINDOW_COLS} FROM planning_windows WHERE id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PlanningWindow".to_string(),
            id: id.to_string(),
        })?;
    map_planning_window(&row)
}

pub async fn update_planning_window(
    db: &DatabaseConnection,
    window_id: i64,
    expected_row_version: i64,
    input: UpdatePlanningWindowInput,
) -> AppResult<PlanningWindow> {
    let current = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {WINDOW_COLS} FROM planning_windows WHERE id = ?"),
            [window_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PlanningWindow".to_string(),
            id: window_id.to_string(),
        })?;
    let current = map_planning_window(&current)?;
    if current.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Planning window was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    let window_type = input.window_type.unwrap_or(current.window_type.clone());
    let start_datetime = input
        .start_datetime
        .unwrap_or(current.start_datetime.clone());
    let end_datetime = input.end_datetime.unwrap_or(current.end_datetime.clone());
    validate_window(
        &start_datetime,
        &end_datetime,
        "start_datetime",
        "end_datetime",
    )?;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE planning_windows
         SET window_type = ?, start_datetime = ?, end_datetime = ?, is_locked = ?, lock_reason = ?,
             row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        [
            window_type.into(),
            start_datetime.into(),
            end_datetime.into(),
            i64::from(input.is_locked.unwrap_or(current.is_locked == 1)).into(),
            input.lock_reason.or(current.lock_reason).into(),
            window_id.into(),
            expected_row_version.into(),
        ],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {WINDOW_COLS} FROM planning_windows WHERE id = ?"),
            [window_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PlanningWindow".to_string(),
            id: window_id.to_string(),
        })?;
    map_planning_window(&row)
}

pub async fn list_schedule_commitments(
    db: &DatabaseConnection,
    filter: ScheduleCommitmentFilter,
) -> AppResult<Vec<ScheduleCommitment>> {
    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(start) = filter.period_start {
        where_sql.push("committed_end >= ?".to_string());
        values.push(start.into());
    }
    if let Some(end) = filter.period_end {
        where_sql.push("committed_start <= ?".to_string());
        values.push(end.into());
    }
    if let Some(team_id) = filter.team_id {
        where_sql.push("assigned_team_id = ?".to_string());
        values.push(team_id.into());
    }
    if let Some(personnel_id) = filter.personnel_id {
        where_sql.push("assigned_personnel_id = ?".to_string());
        values.push(personnel_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {COMMITMENT_COLS}
                 FROM schedule_commitments
                 WHERE {}
                 ORDER BY committed_start ASC, id ASC",
                where_sql.join(" AND ")
            ),
            values,
        ))
        .await?;
    rows.iter().map(map_commitment).collect()
}

pub async fn list_schedule_change_log(
    db: &DatabaseConnection,
    commitment_id: Option<i64>,
) -> AppResult<Vec<ScheduleChangeLogEntry>> {
    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(commitment_id) = commitment_id {
        where_sql.push("commitment_id = ?".to_string());
        values.push(commitment_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {CHANGE_LOG_COLS}
                 FROM schedule_change_log
                 WHERE {}
                 ORDER BY created_at DESC, id DESC",
                where_sql.join(" AND ")
            ),
            values,
        ))
        .await?;
    rows.iter().map(map_change_log).collect()
}

pub async fn list_schedule_break_ins(
    db: &DatabaseConnection,
    filter: ScheduleBreakInFilter,
) -> AppResult<Vec<ScheduleBreakIn>> {
    let mut where_sql = vec!["1 = 1".to_string()];
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(start) = filter.period_start {
        where_sql.push("sb.new_slot_end >= ?".to_string());
        values.push(start.into());
    }
    if let Some(end) = filter.period_end {
        where_sql.push("sb.new_slot_start <= ?".to_string());
        values.push(end.into());
    }
    if let Some(reason) = filter.break_in_reason {
        where_sql.push("sb.break_in_reason = ?".to_string());
        values.push(reason.into());
    }
    if let Some(approver) = filter.approved_by_user_id {
        where_sql.push("sb.approved_by_user_id = ?".to_string());
        values.push(approver.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {BREAK_IN_COLS}
                 FROM schedule_break_ins sb
                 WHERE {}
                 ORDER BY sb.created_at DESC, sb.id DESC",
                where_sql.join(" AND ")
            ),
            values,
        ))
        .await?;
    rows.iter().map(map_schedule_break_in).collect()
}

pub async fn create_schedule_break_in(
    db: &DatabaseConnection,
    actor_id: i64,
    input: CreateScheduleBreakInInput,
) -> AppResult<ScheduleBreakIn> {
    validate_window(
        &input.new_slot_start,
        &input.new_slot_end,
        "new_slot_start",
        "new_slot_end",
    )?;
    let reason = input.break_in_reason.trim().to_ascii_lowercase();
    if !is_valid_break_in_reason(&reason) {
        return Err(AppError::ValidationFailed(vec![
            "break_in_reason must be one of: emergency, safety, production_loss, regulatory, other."
                .to_string(),
        ]));
    }
    let dangerous_override_reason = input
        .dangerous_override_reason
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let tx = db.begin().await?;
    let current = get_commitment(&tx, input.schedule_commitment_id).await?;
    if current.row_version != input.expected_commitment_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Schedule commitment was modified elsewhere (stale row_version).".to_string(),
        ]));
    }

    let mut approved_by_user_id = input.approved_by_user_id;
    let mut approved_by_personnel_id = None;
    if let Some(approver_user_id) = approved_by_user_id {
        if !user_has_plan_approval_scope(&tx, approver_user_id).await? {
            return Err(AppError::ValidationFailed(vec![
                "approved_by_user_id must have plan.confirm or plan.windows permission.".to_string(),
            ]));
        }
        approved_by_personnel_id = resolve_user_personnel_id(&tx, approver_user_id).await?;
        if approved_by_personnel_id.is_none() {
            return Err(AppError::ValidationFailed(vec![
                "approved_by_user_id must be linked to a personnel record.".to_string(),
            ]));
        }
    }
    if matches!(reason.as_str(), "emergency" | "safety")
        && approved_by_user_id.is_none()
        && dangerous_override_reason.is_none()
    {
        return Err(AppError::ValidationFailed(vec![
            "Emergency and safety break-ins require approver evidence or dangerous_override_reason."
                .to_string(),
        ]));
    }
    if approved_by_user_id.is_none() && dangerous_override_reason.is_some() {
        approved_by_user_id = Some(actor_id);
    }

    let next_team_id = input.new_assigned_team_id.unwrap_or(current.assigned_team_id);
    let next_personnel_id = input.new_assigned_personnel_id.or(current.assigned_personnel_id);
    let bypass_availability = input.bypass_availability.unwrap_or(false);
    let bypass_qualification = input.bypass_qualification.unwrap_or(false);
    let override_reason = input
        .override_reason
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    if let Some(personnel_id) = next_personnel_id {
        if let Err(err) = enforce_assignee_availability(
            &tx,
            personnel_id,
            &input.new_slot_start,
            &input.new_slot_end,
        )
        .await
        {
            if !bypass_availability || override_reason.is_none() {
                return Err(err);
            }
        }
        let qualified =
            assignee_matches_candidate_skills(&tx, current.schedule_candidate_id, personnel_id).await?;
        if !qualified && (!bypass_qualification || override_reason.is_none()) {
            return Err(AppError::ValidationFailed(vec![
                "Break-in assignee does not match required qualifications; override reason is required."
                    .to_string(),
            ]));
        }
        let overlaps = check_double_booking(
            &tx,
            personnel_id,
            &input.new_slot_start,
            &input.new_slot_end,
            Some(current.id),
        )
        .await?;
        if overlaps > 0 && override_reason.is_none() {
            return Err(AppError::ValidationFailed(vec![
                "Break-in creates double-booking; override_reason is required.".to_string(),
            ]));
        }
    }

    let (capacity_ok, nearest_window) = compute_capacity_gate(
        &tx,
        next_team_id,
        &input.new_slot_start,
        &input.new_slot_end,
        Some(current.id),
    )
    .await?;
    if !capacity_ok && dangerous_override_reason.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Break-in exceeds team capacity; dangerous_override_reason is required.".to_string(),
        ]));
    }

    let locked_window = find_locked_window(&tx, &input.new_slot_start, &input.new_slot_end).await?;
    if locked_window.is_some() && dangerous_override_reason.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Break-in intersects a locked window and requires dangerous_override_reason."
                .to_string(),
        ]));
    }

    let next_cost =
        estimate_labor_cost(&tx, next_personnel_id, &input.new_slot_start, &input.new_slot_end).await?;
    let old_cost = current.estimated_labor_cost.unwrap_or(0.0);
    let cost_delta = next_cost.map(|v| (v - old_cost).round() * 100.0 / 100.0);
    let next_period_start = parse_rfc3339(&input.new_slot_start, "new_slot_start")?
        .date_naive()
        .to_string();
    let next_period_end = parse_rfc3339(&input.new_slot_end, "new_slot_end")?
        .date_naive()
        .to_string();

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE schedule_commitments
         SET schedule_period_start = ?, schedule_period_end = ?,
             committed_start = ?, committed_end = ?,
             assigned_team_id = ?, assigned_personnel_id = ?,
             estimated_labor_cost = ?, nearest_feasible_window = ?,
             has_blocking_conflict = 0,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        [
            next_period_start.clone().into(),
            next_period_end.clone().into(),
            input.new_slot_start.clone().into(),
            input.new_slot_end.clone().into(),
            next_team_id.into(),
            next_personnel_id.into(),
            next_cost.into(),
            nearest_window.into(),
            current.id.into(),
            input.expected_commitment_row_version.into(),
        ],
    ))
    .await?;

    let dedupe_key = format!(
        "planning.breakin.{}.{}.{}",
        current.id, next_period_start, next_period_end
    );
    let inserted = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO schedule_break_ins
                (schedule_commitment_id, break_in_reason, approved_by_user_id, approved_by_personnel_id, override_reason,
                 old_slot_start, old_slot_end, new_slot_start, new_slot_end, old_assignee_id, new_assignee_id,
                 cost_impact_delta, notification_dedupe_key, created_by_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                current.id.into(),
                reason.clone().into(),
                approved_by_user_id.into(),
                approved_by_personnel_id.into(),
                override_reason.clone().into(),
                current.committed_start.clone().into(),
                current.committed_end.clone().into(),
                input.new_slot_start.clone().into(),
                input.new_slot_end.clone().into(),
                current.assigned_personnel_id.into(),
                next_personnel_id.into(),
                cost_delta.into(),
                Some(dedupe_key.clone()).into(),
                Some(actor_id).into(),
            ],
        ))
        .await?;
    let break_in_id = i64::try_from(inserted.last_insert_id()).map_err(|_| {
        AppError::Internal(anyhow::anyhow!(
            "schedule_break_ins last_insert_id does not fit i64"
        ))
    })?;

    let reason_note = override_reason
        .clone()
        .or(dangerous_override_reason.clone())
        .or_else(|| Some("approved break-in".to_string()));
    append_change_log(
        &tx,
        Some(current.id),
        "break_in_override",
        Some(actor_id),
        Some("commitment_slot".to_string()),
        Some(format!("{} -> {}", current.committed_start, current.committed_end)),
        Some(format!("{} -> {}", input.new_slot_start, input.new_slot_end)),
        Some(reason.clone()),
        reason_note.clone(),
        Some(reason.clone()),
        Some(
            json!({
                "break_in_id": break_in_id,
                "approved_by_user_id": approved_by_user_id,
                "approved_by_personnel_id": approved_by_personnel_id,
                "dangerous_override": dangerous_override_reason.is_some(),
                "bypass_availability": bypass_availability,
                "bypass_qualification": bypass_qualification,
                "cost_impact_delta": cost_delta
            })
            .to_string(),
        ),
    )
    .await?;

    let payload = json!({
        "break_in_id": break_in_id,
        "schedule_commitment_id": current.id,
        "reason_code": reason,
        "impacted_assignee_id": next_personnel_id,
        "old_slot_start": current.committed_start,
        "old_slot_end": current.committed_end,
        "new_slot_start": input.new_slot_start,
        "new_slot_end": input.new_slot_end,
        "slot_delta": {
            "from": [current.committed_start, current.committed_end],
            "to": [input.new_slot_start, input.new_slot_end]
        },
        "cost_impact_estimate": cost_delta,
        "team_id": next_team_id
    });
    tx.commit().await?;
    emit_planning_notification(
        db,
        "planning.break_in.created",
        "high",
        Some(dedupe_key.clone()),
        current.id.to_string(),
        payload.clone(),
        "Break-in schedule override".to_string(),
        format!(
            "Break-in moved commitment #{} (reason: {}).",
            current.id,
            payload
                .get("reason_code")
                .and_then(Value::as_str)
                .unwrap_or("other")
        ),
    )
    .await;
    let _ = activity::emitter::emit_activity_event(
        db,
        activity::emitter::ActivityEventInput {
            event_class: "operational".to_string(),
            event_code: "planning.break_in".to_string(),
            source_module: "planning".to_string(),
            source_record_type: Some("schedule_commitment".to_string()),
            source_record_id: Some(current.id.to_string()),
            entity_scope_id: None,
            actor_id: Some(actor_id),
            severity: if dangerous_override_reason.is_some() {
                "warning".to_string()
            } else {
                "info".to_string()
            },
            summary_json: Some(payload),
            correlation_id: Some(format!("planning-breakin-{break_in_id}")),
            visibility_scope: "entity".to_string(),
        },
    )
    .await;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {BREAK_IN_COLS} FROM schedule_break_ins WHERE id = ?"),
            [break_in_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleBreakIn".to_string(),
            id: break_in_id.to_string(),
        })?;
    map_schedule_break_in(&row)
}

pub async fn create_schedule_commitment(
    db: &DatabaseConnection,
    actor_id: i64,
    input: CreateScheduleCommitmentInput,
) -> AppResult<ScheduleCommitment> {
    let tx = db.begin().await?;
    let gate = run_commitment_gates(
        &tx,
        input.schedule_candidate_id,
        input.expected_candidate_row_version,
        &input.committed_start,
        &input.committed_end,
        input.assigned_team_id,
        input.assigned_personnel_id,
        input.allow_double_booking_override.unwrap_or(false),
        input.override_reason.clone(),
        input.budget_threshold,
        actor_id,
        None,
    )
    .await?;

    let period_start = parse_rfc3339(&input.committed_start, "committed_start")?
        .date_naive()
        .to_string();
    let period_end = parse_rfc3339(&input.committed_end, "committed_end")?
        .date_naive()
        .to_string();

    let inserted = tx
        .execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO schedule_commitments
            (schedule_candidate_id, source_type, source_id, schedule_period_start, schedule_period_end, committed_start, committed_end,
             assigned_team_id, assigned_personnel_id, committed_by_id, estimated_labor_cost, budget_threshold, cost_variance_warning,
             has_blocking_conflict, nearest_feasible_window)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?)",
        [
            input.schedule_candidate_id.into(),
            gate.source_type.into(),
            gate.source_id.into(),
            period_start.into(),
            period_end.into(),
            input.committed_start.clone().into(),
            input.committed_end.clone().into(),
            input.assigned_team_id.into(),
            input.assigned_personnel_id.into(),
            Some(actor_id).into(),
            gate.estimated_labor_cost.into(),
            input.budget_threshold.into(),
            gate.cost_variance_warning.into(),
            gate.nearest_feasible_window.into(),
        ],
    ))
        .await?;
    let id = i64::try_from(inserted.last_insert_id()).map_err(|_| {
        AppError::Internal(anyhow::anyhow!(
            "schedule_commitments last_insert_id does not fit i64"
        ))
    })?;
    append_change_log(
        &tx,
        Some(id),
        "create_commitment",
        Some(actor_id),
        Some("commitment_slot".to_string()),
        None,
        Some(format!("{} -> {}", input.committed_start, input.committed_end)),
        Some("commitment_create".to_string()),
        input.override_reason.clone(),
        input.override_reason,
        Some(
            json!({
                "schedule_candidate_id": input.schedule_candidate_id,
                "committed_start": input.committed_start,
                "committed_end": input.committed_end
            })
            .to_string(),
        ),
    )
    .await?;
    tx.commit().await?;
    get_commitment(db, id).await
}

pub async fn reschedule_schedule_commitment(
    db: &DatabaseConnection,
    actor_id: i64,
    input: RescheduleCommitmentInput,
) -> AppResult<ScheduleCommitment> {
    let tx = db.begin().await?;
    let current = get_commitment(&tx, input.commitment_id).await?;
    if current.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Schedule commitment was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    if current.frozen_at.is_some() {
        let period_key = format!(
            "{}:{}:{}",
            current.id, current.schedule_period_start, current.schedule_period_end
        );
        emit_planning_notification(
            db,
            "planning.freeze_breach.blocked",
            "warning",
            Some(format!("planning.freeze-breach.{period_key}")),
            current.id.to_string(),
            json!({
                "commitment_id": current.id,
                "team_id": current.assigned_team_id,
                "impacted_assignee_id": current.assigned_personnel_id,
                "old_slot_start": current.committed_start,
                "old_slot_end": current.committed_end,
                "attempted_slot_start": input.committed_start,
                "attempted_slot_end": input.committed_end
            }),
            "Freeze breach blocked".to_string(),
            "Attempted reschedule was rejected because the commitment is frozen.".to_string(),
        )
        .await;
        return Err(AppError::ValidationFailed(vec![
            "Commitment is frozen and cannot be rescheduled.".to_string(),
        ]));
    }

    let gate = run_commitment_gates(
        &tx,
        current.schedule_candidate_id,
        None,
        &input.committed_start,
        &input.committed_end,
        input.assigned_team_id,
        input.assigned_personnel_id,
        input.allow_double_booking_override.unwrap_or(false),
        input.override_reason.clone(),
        input.budget_threshold,
        actor_id,
        Some(input.commitment_id),
    )
    .await?;

    let period_start = parse_rfc3339(&input.committed_start, "committed_start")?
        .date_naive()
        .to_string();
    let period_end = parse_rfc3339(&input.committed_end, "committed_end")?
        .date_naive()
        .to_string();

    let res = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE schedule_commitments
             SET schedule_period_start = ?, schedule_period_end = ?, committed_start = ?, committed_end = ?,
                 assigned_team_id = ?, assigned_personnel_id = ?, estimated_labor_cost = ?, budget_threshold = ?,
                 cost_variance_warning = ?, has_blocking_conflict = 0, nearest_feasible_window = ?,
                 row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                period_start.into(),
                period_end.into(),
                input.committed_start.clone().into(),
                input.committed_end.clone().into(),
                input.assigned_team_id.into(),
                input.assigned_personnel_id.into(),
                gate.estimated_labor_cost.into(),
                input.budget_threshold.into(),
                gate.cost_variance_warning.into(),
                gate.nearest_feasible_window.into(),
                input.commitment_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Schedule commitment was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    append_change_log(
        &tx,
        Some(input.commitment_id),
        "reschedule_commitment",
        Some(actor_id),
        Some("commitment_slot".to_string()),
        Some(format!("{} -> {}", current.committed_start, current.committed_end)),
        Some(format!("{} -> {}", input.committed_start, input.committed_end)),
        Some("reschedule".to_string()),
        input.override_reason.clone(),
        input.override_reason,
        Some(
            json!({
                "previous_start": current.committed_start,
                "previous_end": current.committed_end,
                "next_start": input.committed_start,
                "next_end": input.committed_end
            })
            .to_string(),
        ),
    )
    .await?;
    tx.commit().await?;
    get_commitment(db, input.commitment_id).await
}

pub async fn freeze_schedule_period(
    db: &DatabaseConnection,
    actor_id: i64,
    input: FreezeSchedulePeriodInput,
) -> AppResult<i64> {
    validate_window(
        &input.period_start,
        &input.period_end,
        "period_start",
        "period_end",
    )?;
    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO planning_windows (entity_id, window_type, start_datetime, end_datetime, is_locked, lock_reason)
         VALUES (NULL, 'freeze', ?, ?, 1, ?)",
        [
            input.period_start.clone().into(),
            input.period_end.clone().into(),
            input.reason.clone().into(),
        ],
    ))
    .await?;
    let updated = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE schedule_commitments
             SET frozen_at = COALESCE(frozen_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE committed_start < ?
               AND committed_end > ?",
            [
                input.period_end.clone().into(),
                input.period_start.clone().into(),
            ],
        ))
        .await?;
    append_change_log(
        &tx,
        None,
        "freeze_schedule_period",
        Some(actor_id),
        Some("freeze_window".to_string()),
        None,
        Some(format!("{} -> {}", input.period_start, input.period_end)),
        Some("freeze".to_string()),
        input.reason.clone(),
        input.reason,
        Some(
            json!({
                "period_start": input.period_start,
                "period_end": input.period_end,
                "frozen_commitments": updated.rows_affected()
            })
            .to_string(),
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(updated.rows_affected() as i64)
}

pub async fn get_planning_gantt_snapshot(
    db: &DatabaseConnection,
    filter: PlanningGanttFilter,
) -> AppResult<PlanningGanttSnapshot> {
    validate_window(
        &filter.period_start,
        &filter.period_end,
        "period_start",
        "period_end",
    )?;
    let commitments = list_schedule_commitments(
        db,
        ScheduleCommitmentFilter {
            period_start: Some(filter.period_start.clone()),
            period_end: Some(filter.period_end.clone()),
            team_id: filter.team_id,
            personnel_id: None,
        },
    )
    .await?;

    let mut capacity_by_team_day: BTreeMap<(i64, String), TeamCapacityLoad> = BTreeMap::new();
    for commitment in &commitments {
        let day = commitment.schedule_period_start.clone();
        let hours = hours_between(&commitment.committed_start, &commitment.committed_end)?;
        let row = capacity_by_team_day
            .entry((commitment.assigned_team_id, day.clone()))
            .or_insert(TeamCapacityLoad {
                team_id: commitment.assigned_team_id,
                work_date: day.clone(),
                available_hours: 8.0,
                overtime_hours: 0.0,
                committed_hours: 0.0,
                utilization_ratio: 0.0,
            });
        row.committed_hours += hours;
    }
    for ((team_id, day), row) in &mut capacity_by_team_day {
        let cap_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT available_hours_per_day, max_overtime_hours_per_day
                 FROM capacity_rules
                 WHERE team_id = ?
                   AND effective_start <= ?
                   AND (effective_end IS NULL OR effective_end >= ?)
                 ORDER BY effective_start DESC
                 LIMIT 1",
                [
                    (*team_id).into(),
                    day.clone().into(),
                    day.clone().into(),
                ],
            ))
            .await?;
        if let Some(cap_row) = cap_row {
            row.available_hours = cap_row
                .try_get("", "available_hours_per_day")
                .unwrap_or(8.0);
            row.overtime_hours = cap_row
                .try_get("", "max_overtime_hours_per_day")
                .unwrap_or(0.0);
        }
        let denom = row.available_hours + row.overtime_hours;
        row.utilization_ratio = if denom <= 0.0 {
            1.0
        } else {
            row.committed_hours / denom
        };
    }

    let mut personnel_ids = BTreeSet::new();
    for commitment in &commitments {
        if let Some(personnel_id) = commitment.assigned_personnel_id {
            personnel_ids.insert(personnel_id);
        }
    }
    let mut assignee_lanes: Vec<PlanningAssigneeLane> = Vec::new();
    for personnel_id in personnel_ids {
        let full_name = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT full_name FROM personnel WHERE id = ?",
                [personnel_id.into()],
            ))
            .await?
            .and_then(|r| r.try_get::<String>("", "full_name").ok())
            .unwrap_or_else(|| format!("Personnel #{personnel_id}"));

        let blocked_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT start_at, end_at, block_type, is_critical
                 FROM personnel_availability_blocks
                 WHERE personnel_id = ?
                   AND start_at < ?
                   AND end_at > ?",
                [
                    personnel_id.into(),
                    filter.period_end.clone().into(),
                    filter.period_start.clone().into(),
                ],
            ))
            .await?;
        let blocked: Vec<Value> = blocked_rows
            .into_iter()
            .map(|row| {
                json!({
                    "start_at": row.try_get::<String>("", "start_at").unwrap_or_default(),
                    "end_at": row.try_get::<String>("", "end_at").unwrap_or_default(),
                    "block_type": row.try_get::<String>("", "block_type").unwrap_or_default(),
                    "critical": row.try_get::<i64>("", "is_critical").unwrap_or_default() == 1
                })
            })
            .collect();
        let lane_commitments: Vec<Value> = commitments
            .iter()
            .filter(|c| c.assigned_personnel_id == Some(personnel_id))
            .map(|c| {
                json!({
                    "id": c.id,
                    "start": c.committed_start,
                    "end": c.committed_end,
                    "source_type": c.source_type,
                    "source_id": c.source_id,
                })
            })
            .collect();
        assignee_lanes.push(PlanningAssigneeLane {
            personnel_id,
            full_name,
            blocked_intervals_json: Value::Array(blocked).to_string(),
            commitments_json: Value::Array(lane_commitments).to_string(),
        });
    }

    let locked_rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {WINDOW_COLS}
                 FROM planning_windows
                 WHERE is_locked = 1
                   AND start_datetime < ?
                   AND end_datetime > ?
                 ORDER BY start_datetime ASC"
            ),
            [filter.period_end.clone().into(), filter.period_start.clone().into()],
        ))
        .await?;
    let locked_windows: Vec<PlanningWindow> =
        locked_rows.iter().map(map_planning_window).collect::<AppResult<_>>()?;

    Ok(PlanningGanttSnapshot {
        period_start: filter.period_start,
        period_end: filter.period_end,
        commitments,
        locked_windows,
        capacity: capacity_by_team_day.into_values().collect(),
        assignee_lanes,
    })
}

fn pdf_escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn build_simple_pdf(lines: &[String], paper_size: &str) -> Vec<u8> {
    let (w, h) = if paper_size.eq_ignore_ascii_case("A3") {
        (1191, 842)
    } else {
        (842, 595)
    };
    let mut content = String::from("BT /F1 11 Tf 40 ");
    content.push_str(&(h - 50).to_string());
    content.push_str(" Td ");
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            content.push_str("T* ");
        }
        content.push('(');
        content.push_str(&pdf_escape(line));
        content.push_str(") Tj ");
    }
    content.push_str("ET");
    let stream = content.into_bytes();

    let mut out = Vec::<u8>::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = vec![0_usize];
    let push_obj = |buf: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(buf.len());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\n");
    };

    push_obj(
        &mut out,
        &mut offsets,
        b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj",
    );
    push_obj(
        &mut out,
        &mut offsets,
        b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj",
    );
    let page_obj = format!(
        "3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj"
    );
    push_obj(&mut out, &mut offsets, page_obj.as_bytes());
    let stream_head = format!("4 0 obj << /Length {} >> stream\n", stream.len());
    offsets.push(out.len());
    out.extend_from_slice(stream_head.as_bytes());
    out.extend_from_slice(&stream);
    out.extend_from_slice(b"\nendstream endobj\n");
    push_obj(
        &mut out,
        &mut offsets,
        b"5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj",
    );

    let xref_pos = out.len();
    let object_count = offsets.len();
    out.extend_from_slice(format!("xref\n0 {}\n", object_count).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            object_count, xref_pos
        )
        .as_bytes(),
    );
    out
}

pub async fn export_planning_gantt_pdf(
    db: &DatabaseConnection,
    input: ExportPlanningGanttPdfInput,
) -> AppResult<ExportedBinaryDocument> {
    let snapshot = get_planning_gantt_snapshot(
        db,
        PlanningGanttFilter {
            period_start: input.period_start.clone(),
            period_end: input.period_end.clone(),
            team_id: input.team_id,
        },
    )
    .await?;
    let mut lines = vec![
        "Maintafox - Planning Gantt Export".to_string(),
        format!("Period: {} -> {}", snapshot.period_start, snapshot.period_end),
        format!("Commitments: {}", snapshot.commitments.len()),
    ];
    for commitment in snapshot.commitments.iter().take(40) {
        lines.push(format!(
            "#{} {}:{} {} -> {} team={} personnel={}",
            commitment.id,
            commitment.source_type,
            commitment.source_id,
            commitment.committed_start,
            commitment.committed_end,
            commitment.assigned_team_id,
            commitment
                .assigned_personnel_id
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        ));
    }
    let paper_size = input.paper_size.unwrap_or_else(|| "A4".to_string());
    let bytes = build_simple_pdf(&lines, &paper_size);
    Ok(ExportedBinaryDocument {
        file_name: format!(
            "planning-gantt-{}-{}.pdf",
            snapshot.period_start, snapshot.period_end
        ),
        mime_type: "application/pdf".to_string(),
        bytes,
    })
}

pub async fn list_team_capacity_load(
    db: &DatabaseConnection,
    period_start: String,
    period_end: String,
    team_id: Option<i64>,
) -> AppResult<Vec<TeamCapacityLoad>> {
    let snapshot = get_planning_gantt_snapshot(
        db,
        PlanningGanttFilter {
            period_start,
            period_end,
            team_id,
        },
    )
    .await?;
    Ok(snapshot.capacity)
}

pub async fn notify_schedule_teams(
    db: &DatabaseConnection,
    actor_id: i64,
    input: NotifyTeamsInput,
) -> AppResult<NotifyTeamsResult> {
    validate_window(
        &input.period_start,
        &input.period_end,
        "period_start",
        "period_end",
    )?;
    let commitments = list_schedule_commitments(
        db,
        ScheduleCommitmentFilter {
            period_start: Some(input.period_start.clone()),
            period_end: Some(input.period_end.clone()),
            team_id: input.team_id,
            personnel_id: None,
        },
    )
    .await?;
    let mut emitted = 0_i64;
    for commitment in &commitments {
        let break_ins = if input.include_break_ins.unwrap_or(true) {
            list_schedule_break_ins(
                db,
                ScheduleBreakInFilter {
                    period_start: Some(commitment.schedule_period_start.clone()),
                    period_end: Some(commitment.schedule_period_end.clone()),
                    break_in_reason: None,
                    approved_by_user_id: None,
                },
            )
            .await?
            .into_iter()
            .filter(|row| row.schedule_commitment_id == commitment.id)
            .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let payload = json!({
            "commitment_id": commitment.id,
            "team_id": commitment.assigned_team_id,
            "assignee_id": commitment.assigned_personnel_id,
            "start": commitment.committed_start,
            "end": commitment.committed_end,
            "source_type": commitment.source_type,
            "source_id": commitment.source_id,
            "break_in_modifications": break_ins.len(),
            "cost_impact_estimate": commitment.estimated_labor_cost
        });
        emit_planning_notification(
            db,
            "planning.team_notification",
            "info",
            Some(format!(
                "planning.notify.{}.{}",
                commitment.id, commitment.schedule_period_start
            )),
            commitment.id.to_string(),
            payload,
            "Planning schedule notification".to_string(),
            format!(
                "Commitment #{} starts at {}.",
                commitment.id, commitment.committed_start
            ),
        )
        .await;
        emitted += 1;
    }

    append_change_log(
        db,
        None,
        "notify_teams",
        Some(actor_id),
        Some("planning_period".to_string()),
        None,
        Some(format!("{} -> {}", input.period_start, input.period_end)),
        Some("notify_teams".to_string()),
        Some("planning notify-teams action".to_string()),
        Some("notify_teams".to_string()),
        Some(
            json!({
                "emitted_count": emitted,
                "team_id": input.team_id,
                "include_break_ins": input.include_break_ins.unwrap_or(true)
            })
            .to_string(),
        ),
    )
    .await?;

    Ok(NotifyTeamsResult {
        emitted_count: emitted,
        skipped_count: 0,
    })
}

