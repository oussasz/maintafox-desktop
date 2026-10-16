use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit_event, NotificationEventInput};
use crate::{activity, notifications};

use super::domain::{
    AssigneeCapacityInput, CapacityRule, CapacityRuleFilter, CapacityWindow, CreateCapacityRuleInput,
    CreatePlanningWindowInput, CreateScheduleBreakInInput, CreateScheduleCommitmentInput, ExportPlanningGanttPdfInput,
    ExportedBinaryDocument, FreezeSchedulePeriodInput, NotifyTeamsInput, NotifyTeamsResult, PlanningAssigneeLane,
    PlanningCrewLane, PlanningGanttFilter, PlanningGanttSnapshot, PlanningWindow, PlanningWindowFilter,
    RescheduleCommitmentInput, ScheduleBreakIn, ScheduleBreakInFilter, ScheduleChangeLogEntry, ScheduleCommitment,
    ScheduleCommitmentFilter, TeamCapacityLoad, UpdateCapacityRuleInput, UpdatePlanningWindowInput,
};
use super::load;
use super::person_load;
use super::wo_sync::{self, WoSyncOutcome};

const CAPACITY_COLS: &str = "id,entity_id,team_id,effective_start,effective_end,available_hours_per_day,max_overtime_hours_per_day,row_version,created_at,updated_at";
const WINDOW_COLS: &str =
    "id,entity_id,window_type,start_datetime,end_datetime,is_locked,lock_reason,row_version,created_at,updated_at";
const COMMITMENT_COLS: &str = "sc.id,sc.schedule_candidate_id,sc.source_type,sc.source_id,sc.schedule_period_start,sc.schedule_period_end,sc.committed_start,sc.committed_end,sc.assigned_team_id,sc.assigned_personnel_id,sc.committed_by_id,sc.frozen_at,sc.estimated_labor_cost,sc.budget_threshold,sc.cost_variance_warning,sc.has_blocking_conflict,sc.nearest_feasible_window,sc.planned_labor_hours,sc.planned_person_hours,sc.row_version,sc.created_at,sc.updated_at";
const COMMITMENT_JOIN_COLS: &str = "\
    CASE sc.source_type \
        WHEN 'work_order' THEN wo.code \
        WHEN 'pm_occurrence' THEN COALESCE(pp.code, 'PM-' || CAST(sc.source_id AS TEXT)) \
        WHEN 'inspection_follow_up' THEN ir.code \
        ELSE NULL \
    END AS source_code, \
    CASE sc.source_type \
        WHEN 'work_order' THEN wo.title \
        WHEN 'pm_occurrence' THEN COALESCE(pp.title, po.status) \
        WHEN 'inspection_follow_up' THEN ir.title \
        ELSE NULL \
    END AS source_title, \
    CASE WHEN t.id IS NOT NULL THEN t.name || ' (' || t.code || ')' ELSE NULL END AS assigned_team_label, \
    p.full_name AS assigned_personnel_name";
const COMMITMENT_JOINS: &str = "\
    LEFT JOIN work_orders wo ON sc.source_type = 'work_order' AND wo.id = sc.source_id \
    LEFT JOIN pm_occurrences po ON sc.source_type = 'pm_occurrence' AND po.id = sc.source_id \
    LEFT JOIN pm_plans pp ON pp.id = po.pm_plan_id \
    LEFT JOIN intervention_requests ir ON sc.source_type = 'inspection_follow_up' AND ir.id = sc.source_id \
    LEFT JOIN teams t ON t.id = sc.assigned_team_id \
    LEFT JOIN personnel p ON p.id = sc.assigned_personnel_id";
const CHANGE_LOG_COLS: &str = "scl.id,scl.commitment_id,scl.action_type,scl.actor_id,scl.field_changed,scl.old_value,scl.new_value,scl.reason_code,scl.reason_note,scl.reason,scl.details_json,scl.created_at";
const CHANGE_LOG_JOIN_COLS: &str = "COALESCE(ua.display_name, ua.username) AS actor_display_name";
const CHANGE_LOG_JOINS: &str = "LEFT JOIN user_accounts ua ON ua.id = scl.actor_id";
const BREAK_IN_COLS: &str = "sb.id,sb.schedule_commitment_id,sb.break_in_reason,sb.approved_by_user_id,sb.approved_by_personnel_id,sb.override_reason,sb.old_slot_start,sb.old_slot_end,sb.new_slot_start,sb.new_slot_end,sb.old_assignee_id,sb.new_assignee_id,sb.cost_impact_delta,sb.notification_dedupe_key,sb.row_version,sb.created_by_id,sb.created_at";
const BREAK_IN_JOIN_COLS: &str = "\
    CASE cmt.source_type \
        WHEN 'work_order' THEN wo.code \
        WHEN 'pm_occurrence' THEN COALESCE(pp.code, 'PM-' || CAST(cmt.source_id AS TEXT)) \
        WHEN 'inspection_follow_up' THEN ir.code \
        ELSE NULL \
    END AS commitment_source_code, \
    COALESCE(approver.display_name, approver.username) AS approved_by_display_name, \
    old_p.full_name AS old_assignee_name, \
    new_p.full_name AS new_assignee_name";
const BREAK_IN_JOINS: &str = "\
    LEFT JOIN schedule_commitments cmt ON cmt.id = sb.schedule_commitment_id \
    LEFT JOIN work_orders wo ON cmt.source_type = 'work_order' AND wo.id = cmt.source_id \
    LEFT JOIN pm_occurrences po ON cmt.source_type = 'pm_occurrence' AND po.id = cmt.source_id \
    LEFT JOIN pm_plans pp ON pp.id = po.pm_plan_id \
    LEFT JOIN intervention_requests ir ON cmt.source_type = 'inspection_follow_up' AND ir.id = cmt.source_id \
    LEFT JOIN user_accounts approver ON approver.id = sb.approved_by_user_id \
    LEFT JOIN personnel old_p ON old_p.id = sb.old_assignee_id \
    LEFT JOIN personnel new_p ON new_p.id = sb.new_assignee_id";

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "planning scheduling decode failed for column '{column}': {e}"
    ))
}

fn parse_rfc3339(value: &str, field: &str) -> AppResult<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be a valid RFC3339 timestamp.")]))?;
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
        entity_id: row.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?,
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
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

fn map_planning_window(row: &QueryResult) -> AppResult<PlanningWindow> {
    Ok(PlanningWindow {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_id: row.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?,
        window_type: row
            .try_get("", "window_type")
            .map_err(|e| decode_err("window_type", e))?,
        start_datetime: row
            .try_get("", "start_datetime")
            .map_err(|e| decode_err("start_datetime", e))?,
        end_datetime: row
            .try_get("", "end_datetime")
            .map_err(|e| decode_err("end_datetime", e))?,
        is_locked: row.try_get("", "is_locked").map_err(|e| decode_err("is_locked", e))?,
        lock_reason: row
            .try_get("", "lock_reason")
            .map_err(|e| decode_err("lock_reason", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
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
        source_id: row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?,
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
        frozen_at: row.try_get("", "frozen_at").map_err(|e| decode_err("frozen_at", e))?,
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
        planned_labor_hours: row
            .try_get("", "planned_labor_hours")
            .map_err(|e| decode_err("planned_labor_hours", e))?,
        planned_person_hours: row
            .try_get("", "planned_person_hours")
            .map_err(|e| decode_err("planned_person_hours", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
        source_code: row.try_get::<Option<String>>("", "source_code").unwrap_or(None),
        source_title: row.try_get::<Option<String>>("", "source_title").unwrap_or(None),
        assigned_team_label: row.try_get::<Option<String>>("", "assigned_team_label").unwrap_or(None),
        assigned_personnel_name: row
            .try_get::<Option<String>>("", "assigned_personnel_name")
            .unwrap_or(None),
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
        actor_id: row.try_get("", "actor_id").map_err(|e| decode_err("actor_id", e))?,
        field_changed: row
            .try_get("", "field_changed")
            .map_err(|e| decode_err("field_changed", e))?,
        old_value: row.try_get("", "old_value").map_err(|e| decode_err("old_value", e))?,
        new_value: row.try_get("", "new_value").map_err(|e| decode_err("new_value", e))?,
        reason_code: row
            .try_get("", "reason_code")
            .map_err(|e| decode_err("reason_code", e))?,
        reason_note: row
            .try_get("", "reason_note")
            .map_err(|e| decode_err("reason_note", e))?,
        reason: row.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
        details_json: row
            .try_get("", "details_json")
            .map_err(|e| decode_err("details_json", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        actor_display_name: row.try_get::<Option<String>>("", "actor_display_name").unwrap_or(None),
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
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        commitment_source_code: row
            .try_get::<Option<String>>("", "commitment_source_code")
            .unwrap_or(None),
        approved_by_display_name: row
            .try_get::<Option<String>>("", "approved_by_display_name")
            .unwrap_or(None),
        old_assignee_name: row.try_get::<Option<String>>("", "old_assignee_name").unwrap_or(None),
        new_assignee_name: row.try_get::<Option<String>>("", "new_assignee_name").unwrap_or(None),
    })
}

async fn get_commitment(db: &impl ConnectionTrait, commitment_id: i64) -> AppResult<ScheduleCommitment> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {COMMITMENT_COLS}, {COMMITMENT_JOIN_COLS}
                 FROM schedule_commitments sc
                 {COMMITMENT_JOINS}
                 WHERE sc.id = ?"
            ),
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

async fn find_locked_window(db: &impl ConnectionTrait, start: &str, end: &str) -> AppResult<Option<PlanningWindow>> {
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
    let source_id: i64 = row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?;
    Ok((source_type, source_id))
}

/// Capacity gate over every day of the slot (`planning::load`): the work's labour hours, spread over
/// the slot's working days, plus the team's committed load that day, must fit the team's hours plus
/// the planner's overtime ceiling. Returns the first overloaded day when it does not fit.
async fn compute_capacity_gate(
    db: &impl ConnectionTrait,
    team_id: i64,
    start: &str,
    end: &str,
    labour_hours: f64,
    exclude_commitment_id: Option<i64>,
    exclude_work_order: Option<i64>,
) -> AppResult<(bool, Option<String>)> {
    let days = load::slot_days(start, end)?;
    let (Some(first), Some(last)) = (days.first().map(|d| d.0), days.last().map(|d| d.0)) else {
        return Ok((true, None));
    };
    // From the day before: a night shift that started then may run into the slot.
    let team = load::team_days(db, team_id, first - chrono::Duration::days(1), last).await?;
    // A team with no timetable at all over the slot: say so rather than call it overloaded.
    if team
        .range(first..=last)
        .all(|(_, d)| d.basis == "no_schedule" || d.basis == "no_members")
    {
        return Err(crate::errors::org_validation_failed(
            "TEAM_NO_SCHEDULE",
            "The team has no working hours that day: link it to a Classe horaire (or give its members one).",
            std::collections::HashMap::from([("day".to_string(), first.to_string())]),
        ));
    }
    let hours: BTreeMap<chrono::NaiveDate, load::DayHours> = team.iter().map(|(d, t)| (*d, t.hours.clone())).collect();
    let spread = load::spread(labour_hours, start, end, &hours)?;
    let (Some(spread_first), Some(spread_last)) = (spread.keys().next().copied(), spread.keys().last().copied()) else {
        return Ok((true, None));
    };
    let committed = load::team_day_loads(
        db,
        Some(&[team_id]),
        spread_first,
        spread_last,
        exclude_commitment_id,
        exclude_work_order,
    )
    .await?;
    for (day, share) in spread {
        let (cap, overtime) = team.get(&day).map(|t| (t.available, t.overtime)).unwrap_or((0.0, 0.0));
        let already = committed.get(&(team_id, day)).copied().unwrap_or(0.0);
        // A hundredth of an hour of tolerance for the spread's rounding.
        if already + share > cap + overtime + 0.01 {
            return Ok((false, Some(day.to_string())));
        }
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

async fn resolve_user_personnel_id(db: &impl ConnectionTrait, user_id: i64) -> AppResult<Option<i64>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get::<i64>("", "personnel_id").ok()))
}

async fn maybe_sync_work_order(
    tx: &impl ConnectionTrait,
    actor_id: i64,
    schedule_candidate_id: i64,
    source_type: &str,
    source_id: i64,
    committed_start: &str,
    committed_end: &str,
    assigned_team_id: i64,
    assigned_personnel_id: Option<i64>,
) -> AppResult<Option<WoSyncOutcome>> {
    if source_type != "work_order" {
        return Ok(None);
    }
    let outcome = wo_sync::sync_work_order_from_commitment(
        tx,
        actor_id,
        schedule_candidate_id,
        source_id,
        committed_start,
        committed_end,
        assigned_team_id,
        assigned_personnel_id,
    )
    .await?;
    Ok(Some(outcome))
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
               AND p.name IN (?, ?)",
            [
                user_id.into(),
                crate::rbac::permissions::PLAN_CONFIRM.into(),
                crate::rbac::permissions::PLAN_WINDOWS.into(),
            ],
        ))
        .await?
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or_default();
    Ok(has > 0)
}

/// The assignee of a work-order commitment must hold what the work order requires
/// (`qualification::gate`); an authorised supervisor's override, recorded beforehand with
/// `record_qualification_override`, lets them through. Other sources have no requirements.
pub(crate) async fn enforce_assignee_qualifications(
    db: &impl ConnectionTrait,
    source_type: &str,
    source_id: i64,
    personnel_id: i64,
) -> AppResult<()> {
    if source_type != "work_order" {
        return Ok(());
    }
    crate::qualification::gate::enforce_work_order_assignment(db, personnel_id, source_id, None).await?;
    Ok(())
}

/// Hours a team can work on `day` and why (`personnel::capacity`): its members' Classes horaires
/// on the site's working days minus absences, × utilisation (`roster`), or none (`rest_day`,
/// `holiday`, `no_schedule`, `no_members`). The planner's capacity rule only adds its overtime
/// ceiling. No default hours.
pub struct TeamDayHours {
    pub available: f64,
    pub overtime: f64,
    pub basis: String,
}

pub async fn team_day_hours(
    db: &impl ConnectionTrait,
    team_id: i64,
    day: chrono::NaiveDate,
) -> AppResult<TeamDayHours> {
    Ok(team_hours_from(
        crate::personnel::capacity::team_capacity_hours(db, team_id, day).await?,
    ))
}

/// Timetable hours, the rule's overtime ceiling, and why.
fn team_hours_from(roster: crate::personnel::capacity::TeamDayCapacity) -> TeamDayHours {
    TeamDayHours {
        available: roster.available_hours,
        overtime: roster.rule_overtime_hours.unwrap_or(0.0),
        basis: roster.basis,
    }
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

/// Labour cost of the hours the work books (not of the slot's wall-clock length).
async fn estimate_labor_cost(
    db: &impl ConnectionTrait,
    personnel_id: Option<i64>,
    labour_hours: f64,
) -> AppResult<Option<f64>> {
    let Some(personnel_id) = personnel_id else {
        return Ok(None);
    };
    let Some(rate) = crate::personnel::queries::resolve_active_labor_rate(db, personnel_id).await? else {
        return Ok(None);
    };
    Ok(Some((labour_hours * rate * 100.0).round() / 100.0))
}

/// Hours a candidate books: the crew's labour (estimated duration × planned crew size) and each
/// person's hours (the duration). Work without both cannot be committed: no duration or crew is
/// ever assumed.
async fn candidate_hours(db: &impl ConnectionTrait, candidate_id: i64) -> AppResult<(f64, f64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT estimated_duration_hours, planned_crew_size FROM schedule_candidates WHERE id = ?",
            [candidate_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleCandidate".to_string(),
            id: candidate_id.to_string(),
        })?;
    // A value that cannot be read is an error, never "not estimated".
    let estimate: Option<f64> = row
        .try_get("", "estimated_duration_hours")
        .map_err(|e| decode_err("estimated_duration_hours", e))?;
    let crew: Option<i64> = row
        .try_get("", "planned_crew_size")
        .map_err(|e| decode_err("planned_crew_size", e))?;
    match (estimate, crew) {
        (Some(hours), Some(people)) if hours > 0.0 && people >= 1 => Ok((hours * people as f64, hours)),
        _ => Err(crate::errors::org_validation_failed(
            "PLANNING_NOT_ESTIMATED",
            "This work is not estimated: enter its duration and crew size before committing it to the schedule.",
            std::collections::HashMap::new(),
        )),
    }
}

struct CommitmentGateResult {
    source_type: String,
    source_id: i64,
    nearest_feasible_window: Option<String>,
    estimated_labor_cost: Option<f64>,
    cost_variance_warning: i64,
    labour_hours: f64,
    /// Hours the assigned technician works on the job (its duration).
    person_hours: f64,
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
    acknowledged_assignee_issues: &[String],
    budget_threshold: Option<f64>,
    actor_id: i64,
    exclude_commitment_id: Option<i64>,
) -> AppResult<CommitmentGateResult> {
    validate_window(committed_start, committed_end, "committed_start", "committed_end")?;
    load::validate_slot_length(committed_start, committed_end)?;
    let (source_type, source_id) = enforce_candidate_ready(tx, candidate_id, expected_candidate_row_version).await?;

    if let Some(window) = find_locked_window(tx, committed_start, committed_end).await? {
        return Err(AppError::ValidationFailed(vec![format!(
            "Lock-window gate failed: planning window '{}' is locked for this range.",
            window.window_type
        )]));
    }

    let (labour_hours, person_hours) = candidate_hours(tx, candidate_id).await?;
    // The OT behind the job: not counted twice against the crew or the technician.
    let own_work_order = person_load::source_work_order(tx, &source_type, source_id).await?;
    let (capacity_ok, overloaded_day) = compute_capacity_gate(
        tx,
        assigned_team_id,
        committed_start,
        committed_end,
        labour_hours,
        exclude_commitment_id,
        own_work_order,
    )
    .await?;
    if !capacity_ok {
        return Err(AppError::ValidationFailed(vec![format!(
            "Capacity gate failed: team load exceeds capacity + overtime on {}.",
            overloaded_day.unwrap_or_default()
        )]));
    }
    let nearest_feasible_window: Option<String> = None;

    if let Some(personnel_id) = assigned_personnel_id {
        enforce_assignee_availability(tx, personnel_id, committed_start, committed_end).await?;
        enforce_assignee_qualifications(tx, &source_type, source_id, personnel_id).await?;

        // The technician's own day (`planning::person_load`): in the crew, working during the slot,
        // enough hours left each day net of all their committed work.
        let issues = person_load::assignee_issues(
            tx,
            assigned_team_id,
            personnel_id,
            committed_start,
            committed_end,
            Some(person_hours),
            exclude_commitment_id,
            own_work_order,
        )
        .await?;
        let overlaps =
            check_double_booking(tx, personnel_id, committed_start, committed_end, exclude_commitment_id).await?;
        // A refusal is overridden only when the planner saw it: each code must be acknowledged.
        let all_acknowledged = issues
            .iter()
            .all(|i| acknowledged_assignee_issues.iter().any(|a| a == &i.code));
        if !issues.is_empty() && (!allow_double_booking_override || !all_acknowledged) {
            return Err(crate::errors::org_validation_failed_issues(issues));
        }
        if overlaps > 0 && !allow_double_booking_override {
            return Err(AppError::ValidationFailed(vec![
                "Double-booking detected. Provide explicit override reason.".to_string(),
            ]));
        }
        if (overlaps > 0 || !issues.is_empty()) && allow_double_booking_override {
            let reason = override_reason
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    AppError::ValidationFailed(vec![
                        "Override reason is required when bypassing the assignee checks.".to_string()
                    ])
                })?;
            let mut overridden: Vec<String> = issues.iter().map(|i| i.code.clone()).collect();
            if overlaps > 0 {
                overridden.push("DOUBLE_BOOKING".to_string());
            }
            append_change_log(
                tx,
                exclude_commitment_id,
                if issues.is_empty() {
                    "override_double_booking"
                } else {
                    "override_assignee"
                },
                Some(actor_id),
                Some("assignment_window".to_string()),
                None,
                Some(format!("{committed_start} -> {committed_end}")),
                Some(overridden.join(",")),
                Some(reason.clone()),
                Some(reason),
                Some(
                    json!({
                        "personnel_id": personnel_id,
                        "committed_start": committed_start,
                        "committed_end": committed_end,
                        "overridden": overridden,
                        "issues": issues,
                    })
                    .to_string(),
                ),
            )
            .await?;
        }
    }

    let estimated_labor_cost = estimate_labor_cost(tx, assigned_personnel_id, labour_hours).await?;
    let cost_variance_warning = if let (Some(cost), Some(threshold)) = (estimated_labor_cost, budget_threshold) {
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
        labour_hours,
        person_hours,
    })
}

pub async fn list_capacity_rules(db: &DatabaseConnection, filter: CapacityRuleFilter) -> AppResult<Vec<CapacityRule>> {
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

pub async fn create_capacity_rule(db: &DatabaseConnection, input: CreateCapacityRuleInput) -> AppResult<CapacityRule> {
    if input.available_hours_per_day.is_some_and(|h| h < 0.0) || input.max_overtime_hours_per_day < 0.0 {
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
    crate::repository::team_repository::assert_active_crew_exists(db, input.team_id).await?;

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
                // Explicit: capacity no longer reads this column, never its old 8 h default.
                input.available_hours_per_day.unwrap_or(0.0).into(),
                input.max_overtime_hours_per_day.into(),
            ],
        ))
        .await?;
    let id = i64::try_from(inserted.last_insert_id())
        .map_err(|_| AppError::Internal(anyhow::anyhow!("schedule_commitments last_insert_id does not fit i64")))?;
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
    let effective_start = input.effective_start.unwrap_or_else(|| current.effective_start.clone());
    let effective_end = input.effective_end.or(current.effective_end.clone());
    let available_hours = input.available_hours_per_day.unwrap_or(current.available_hours_per_day);
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
    let id = i64::try_from(inserted.last_insert_id())
        .map_err(|_| AppError::Internal(anyhow::anyhow!("schedule_commitments last_insert_id does not fit i64")))?;
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
    let start_datetime = input.start_datetime.unwrap_or(current.start_datetime.clone());
    let end_datetime = input.end_datetime.unwrap_or(current.end_datetime.clone());
    validate_window(&start_datetime, &end_datetime, "start_datetime", "end_datetime")?;

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
        where_sql.push("sc.committed_end >= ?".to_string());
        values.push(start.into());
    }
    if let Some(end) = filter.period_end {
        where_sql.push("sc.committed_start <= ?".to_string());
        values.push(end.into());
    }
    if let Some(team_id) = filter.team_id {
        where_sql.push("sc.assigned_team_id = ?".to_string());
        values.push(team_id.into());
    }
    if let Some(personnel_id) = filter.personnel_id {
        where_sql.push("sc.assigned_personnel_id = ?".to_string());
        values.push(personnel_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {COMMITMENT_COLS}, {COMMITMENT_JOIN_COLS}
                 FROM schedule_commitments sc
                 {COMMITMENT_JOINS}
                 WHERE {}
                 ORDER BY sc.committed_start ASC, sc.id ASC",
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
        where_sql.push("scl.commitment_id = ?".to_string());
        values.push(commitment_id.into());
    }
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {CHANGE_LOG_COLS}, {CHANGE_LOG_JOIN_COLS}
                 FROM schedule_change_log scl
                 {CHANGE_LOG_JOINS}
                 WHERE {}
                 ORDER BY scl.created_at DESC, scl.id DESC",
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
                "SELECT {BREAK_IN_COLS}, {BREAK_IN_JOIN_COLS}
                 FROM schedule_break_ins sb
                 {BREAK_IN_JOINS}
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
    load::validate_slot_length(&input.new_slot_start, &input.new_slot_end)?;
    let reason = input.break_in_reason.trim().to_ascii_lowercase();
    if !is_valid_break_in_reason(&reason) {
        return Err(AppError::ValidationFailed(vec![
            "break_in_reason must be one of: emergency, safety, production_loss, regulatory, other.".to_string(),
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
            "Emergency and safety break-ins require approver evidence or dangerous_override_reason.".to_string(),
        ]));
    }
    if approved_by_user_id.is_none() && dangerous_override_reason.is_some() {
        approved_by_user_id = Some(actor_id);
    }

    let next_team_id = input.new_assigned_team_id.unwrap_or(current.assigned_team_id);
    // "Unassigned" chosen explicitly clears the technician; otherwise the current one stays.
    let next_personnel_id = if input.clear_assignee.unwrap_or(false) {
        None
    } else {
        input.new_assigned_personnel_id.or(current.assigned_personnel_id)
    };
    // The hours this commitment books (its slot length for one from before migration 205).
    let labour_hours = match current.planned_labor_hours {
        Some(hours) => hours,
        None => hours_between(&current.committed_start, &current.committed_end)?,
    };
    let bypass_availability = input.bypass_availability.unwrap_or(false);
    let override_reason = input
        .override_reason
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    // The OT behind the job: not counted twice against the crew or the technician.
    let own_work_order = person_load::source_work_order(&tx, &current.source_type, current.source_id).await?;

    if let Some(personnel_id) = next_personnel_id {
        if let Err(err) =
            enforce_assignee_availability(&tx, personnel_id, &input.new_slot_start, &input.new_slot_end).await
        {
            if !bypass_availability || override_reason.is_none() {
                return Err(err);
            }
        }
        // Qualifications are not bypassed here: a supervisor override (trn.certify, step-up,
        // audited) is recorded first with `record_qualification_override`.
        enforce_assignee_qualifications(&tx, &current.source_type, current.source_id, personnel_id).await?;
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
        // The technician's own day: in the crew, working, enough hours left. Each refusal is
        // overridden only when acknowledged, with the break-in's reason, and logged.
        let person_hours = commitment_person_hours(&tx, &current).await?;
        let issues = person_load::assignee_issues(
            &tx,
            next_team_id,
            personnel_id,
            &input.new_slot_start,
            &input.new_slot_end,
            person_hours,
            Some(current.id),
            own_work_order,
        )
        .await?;
        if !issues.is_empty() {
            let all_acknowledged = issues
                .iter()
                .all(|i| input.acknowledged_assignee_issues.iter().any(|a| a == &i.code));
            let Some(reason) = override_reason.clone().filter(|_| all_acknowledged) else {
                return Err(crate::errors::org_validation_failed_issues(issues));
            };
            let overridden: Vec<String> = issues.iter().map(|i| i.code.clone()).collect();
            append_change_log(
                &tx,
                Some(current.id),
                "override_assignee",
                Some(actor_id),
                Some("assignment_window".to_string()),
                None,
                Some(format!("{} -> {}", input.new_slot_start, input.new_slot_end)),
                Some(overridden.join(",")),
                Some(reason.clone()),
                Some(reason),
                Some(
                    json!({
                        "personnel_id": personnel_id,
                        "committed_start": input.new_slot_start,
                        "committed_end": input.new_slot_end,
                        "overridden": overridden,
                        "issues": issues,
                        "break_in": true,
                    })
                    .to_string(),
                ),
            )
            .await?;
        }
    }

    // The first overloaded day is reported by the error; nothing is stored as a "feasible window".
    let (capacity_ok, _overloaded_day) = compute_capacity_gate(
        &tx,
        next_team_id,
        &input.new_slot_start,
        &input.new_slot_end,
        labour_hours,
        Some(current.id),
        own_work_order,
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
            "Break-in intersects a locked window and requires dangerous_override_reason.".to_string(),
        ]));
    }

    let next_cost = estimate_labor_cost(&tx, next_personnel_id, labour_hours).await?;
    let old_cost = current.estimated_labor_cost.unwrap_or(0.0);
    let cost_delta = next_cost.map(|v| ((v - old_cost) * 100.0).round() / 100.0);
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
            None::<String>.into(),
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
    let break_in_id = i64::try_from(inserted.last_insert_id())
        .map_err(|_| AppError::Internal(anyhow::anyhow!("schedule_break_ins last_insert_id does not fit i64")))?;

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
                "cost_impact_delta": cost_delta
            })
            .to_string(),
        ),
    )
    .await?;

    let wo_sync = maybe_sync_work_order(
        &tx,
        actor_id,
        current.schedule_candidate_id,
        &current.source_type,
        current.source_id,
        &input.new_slot_start,
        &input.new_slot_end,
        next_team_id,
        next_personnel_id,
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
    if let Some(outcome) = wo_sync {
        wo_sync::record_wo_sync_audit(db, actor_id, outcome.wo_id, "break_in").await;
    }
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
            payload.get("reason_code").and_then(Value::as_str).unwrap_or("other")
        ),
    )
    .await;
    // Correlates the events of one break-in on every device: by sync id, not the local id.
    let correlation = crate::sync::identity::optional_entity_sync_id(db, "schedule_break_ins", break_in_id)
        .await?
        // Every row gets a sync id at insert; a row without one keeps a device-marked key.
        .unwrap_or_else(|| format!("local-{break_in_id}"));
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
            correlation_id: Some(format!("planning-breakin-{correlation}")),
            visibility_scope: "entity".to_string(),
        },
    )
    .await;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {BREAK_IN_COLS}, {BREAK_IN_JOIN_COLS}
                 FROM schedule_break_ins sb
                 {BREAK_IN_JOINS}
                 WHERE sb.id = ?"
            ),
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
        &input.acknowledged_assignee_issues,
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
             has_blocking_conflict, nearest_feasible_window, planned_labor_hours, planned_person_hours)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?)",
        [
            input.schedule_candidate_id.into(),
            gate.source_type.clone().into(),
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
            gate.labour_hours.into(),
            gate.person_hours.into(),
        ],
    ))
        .await?;
    let id = i64::try_from(inserted.last_insert_id())
        .map_err(|_| AppError::Internal(anyhow::anyhow!("schedule_commitments last_insert_id does not fit i64")))?;
    // The candidate is derived on each device; the commitment depends on it, so it travels now.
    crate::sync::capture::capture_row_change(&tx, "schedule_candidates", input.schedule_candidate_id).await?;
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

    let wo_sync = maybe_sync_work_order(
        &tx,
        actor_id,
        input.schedule_candidate_id,
        &gate.source_type,
        gate.source_id,
        &input.committed_start,
        &input.committed_end,
        input.assigned_team_id,
        input.assigned_personnel_id,
    )
    .await?;

    tx.commit().await?;
    if let Some(outcome) = wo_sync {
        wo_sync::record_wo_sync_audit(db, actor_id, outcome.wo_id, "create").await;
    }
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
        let payload = json!({
            "commitment_id": current.id,
            "team_id": current.assigned_team_id,
            "impacted_assignee_id": current.assigned_personnel_id,
            "old_slot_start": current.committed_start,
            "old_slot_end": current.committed_end,
            "attempted_slot_start": input.committed_start,
            "attempted_slot_end": input.committed_end
        });
        // Release the open SQLite transaction before emitting — same connection cannot
        // write notification_events while the reschedule txn is held.
        tx.rollback().await.ok();
        emit_planning_notification(
            db,
            "planning.freeze_breach.blocked",
            "warning",
            Some(format!("planning.freeze-breach.{period_key}")),
            current.id.to_string(),
            payload,
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
        &input.acknowledged_assignee_issues,
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
                 planned_labor_hours = ?, planned_person_hours = ?,
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
                gate.labour_hours.into(),
                gate.person_hours.into(),
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

    let wo_sync = maybe_sync_work_order(
        &tx,
        actor_id,
        current.schedule_candidate_id,
        &current.source_type,
        current.source_id,
        &input.committed_start,
        &input.committed_end,
        input.assigned_team_id,
        input.assigned_personnel_id,
    )
    .await?;

    tx.commit().await?;
    if let Some(outcome) = wo_sync {
        wo_sync::record_wo_sync_audit(db, actor_id, outcome.wo_id, "reschedule").await;
    }
    get_commitment(db, input.commitment_id).await
}

pub async fn freeze_schedule_period(
    db: &DatabaseConnection,
    actor_id: i64,
    input: FreezeSchedulePeriodInput,
) -> AppResult<i64> {
    validate_window(&input.period_start, &input.period_end, "period_start", "period_end")?;
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
            [input.period_end.clone().into(), input.period_start.clone().into()],
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
    validate_window(&filter.period_start, &filter.period_end, "period_start", "period_end")?;
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
    // Every team-day of the period gets its capacity, not only the days that already hold
    // commitments: the board must never assume hours a team does not have.
    let parse_day =
        |raw: &str| chrono::NaiveDate::parse_from_str(&raw.chars().take(10).collect::<String>(), "%Y-%m-%d");
    if let (Ok(first), Ok(last)) = (parse_day(&filter.period_start), parse_day(&filter.period_end)) {
        if last >= first && (last - first).num_days() <= 366 {
            let team_ids: Vec<i64> = match filter.team_id {
                Some(team_id) => vec![team_id],
                None => db
                    .query_all(Statement::from_string(
                        DbBackend::Sqlite,
                        "SELECT id FROM teams WHERE status = 'active' AND deleted_at IS NULL ORDER BY id".to_string(),
                    ))
                    .await?
                    .iter()
                    .filter_map(|r| r.try_get::<i64>("", "id").ok())
                    .collect(),
            };
            for team_id in team_ids {
                for day in first.iter_days().take_while(|d| *d <= last) {
                    capacity_by_team_day
                        .entry((team_id, day.to_string()))
                        .or_insert(TeamCapacityLoad {
                            team_id,
                            work_date: day.to_string(),
                            available_hours: 0.0,
                            overtime_hours: 0.0,
                            committed_hours: 0.0,
                            utilization_ratio: None,
                            basis: String::new(),
                            work_windows: Vec::new(),
                            unwindowed_hours: 0.0,
                        });
                }
            }
        }
    }
    // One roster computation per team over the days it needs.
    let mut days_by_team: BTreeMap<i64, (chrono::NaiveDate, chrono::NaiveDate)> = BTreeMap::new();
    for (team_id, day) in capacity_by_team_day.keys() {
        let day = parse_day(day).map_err(|e| AppError::Internal(anyhow::anyhow!("capacity day '{day}': {e}")))?;
        let span = days_by_team.entry(*team_id).or_insert((day, day));
        span.0 = span.0.min(day);
        span.1 = span.1.max(day);
    }
    let mut roster: BTreeMap<(i64, String), load::TeamDay> = BTreeMap::new();
    for (team_id, (from, to)) in days_by_team {
        for (day, team_day) in load::team_days(db, team_id, from, to).await? {
            roster.insert((team_id, day.to_string()), team_day);
        }
    }
    for ((team_id, day), row) in &mut capacity_by_team_day {
        let key = (*team_id, day.chars().take(10).collect::<String>());
        let hours = roster
            .remove(&key)
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("capacity missing for team {team_id} on {day}")))?;
        row.available_hours = hours.available;
        row.overtime_hours = hours.overtime;
        row.basis = hours.basis;
        row.work_windows = hours
            .hours
            .windows
            .iter()
            .map(|w| CapacityWindow {
                start: w.start.format("%Y-%m-%dT%H:%M:%S").to_string(),
                end: w.end.format("%Y-%m-%dT%H:%M:%S").to_string(),
                hours: w.hours,
            })
            .collect();
        row.unwindowed_hours = hours.hours.unwindowed;
    }
    // Committed load per team-day (`planning::load`): labour hours spread over the slots' working
    // days, finished work excluded.
    if let (Ok(first), Ok(last)) = (parse_day(&filter.period_start), parse_day(&filter.period_end)) {
        let teams: Vec<i64> = capacity_by_team_day
            .keys()
            .map(|(t, _)| *t)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let loads = load::team_day_loads(db, Some(&teams), first, last, None, None).await?;
        for ((team_id, day), row) in &mut capacity_by_team_day {
            if let Ok(d) = parse_day(day) {
                row.committed_hours = loads.get(&(*team_id, d)).copied().unwrap_or(0.0);
            }
        }
    }
    for row in capacity_by_team_day.values_mut() {
        let denom = row.available_hours + row.overtime_hours;
        row.utilization_ratio = (denom > 0.0).then(|| row.committed_hours / denom);
    }

    let mut personnel_ids = BTreeSet::new();
    let mut assignee_lanes: Vec<PlanningAssigneeLane> = Vec::new();
    let mut crew_lanes: Vec<PlanningCrewLane> = Vec::new();

    // Single-crew drill-in: seed technician lanes from roster + people with work.
    // Plant-wide All Crews: crew-level lanes only (no person roster).
    if let Some(team_id) = filter.team_id {
        let roster_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT p.id AS personnel_id
                 FROM personnel_team_assignments pta
                 INNER JOIN personnel p ON p.id = pta.personnel_id
                 WHERE pta.team_id = ?
                   AND (pta.valid_to IS NULL OR pta.valid_to >= strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                   AND p.employment_status = 'active'
                 ORDER BY pta.is_lead DESC, p.full_name ASC, pta.id ASC",
                [team_id.into()],
            ))
            .await?;
        for row in roster_rows {
            if let Ok(personnel_id) = row.try_get::<i64>("", "personnel_id") {
                personnel_ids.insert(personnel_id);
            }
        }
        for commitment in &commitments {
            if let Some(personnel_id) = commitment.assigned_personnel_id {
                personnel_ids.insert(personnel_id);
            }
        }
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
    } else {
        let crew_rows = db
            .query_all(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, code, name
                 FROM teams
                 WHERE deleted_at IS NULL
                   AND status = 'active'
                 ORDER BY name ASC, code ASC, id ASC",
                [],
            ))
            .await?;
        for row in crew_rows {
            let team_id: i64 = row.try_get("", "id").map_err(|e| decode_err("team_id", e))?;
            let team_code: String = row.try_get("", "code").unwrap_or_default();
            let team_name: String = row.try_get("", "name").unwrap_or_default();
            let lane_commitments: Vec<Value> = commitments
                .iter()
                .filter(|c| c.assigned_team_id == team_id)
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
            crew_lanes.push(PlanningCrewLane {
                team_id,
                team_code,
                team_name,
                commitments_json: Value::Array(lane_commitments).to_string(),
            });
        }
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
    let locked_windows: Vec<PlanningWindow> = locked_rows.iter().map(map_planning_window).collect::<AppResult<_>>()?;

    Ok(PlanningGanttSnapshot {
        period_start: filter.period_start,
        period_end: filter.period_end,
        commitments,
        locked_windows,
        capacity: capacity_by_team_day.into_values().collect(),
        assignee_lanes,
        crew_lanes,
    })
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
    let paper_size = input.paper_size.unwrap_or_else(|| "A4".to_string());
    let font = crate::reports::pdf::load_system_font()?;
    let mut doc = crate::reports::pdf::PdfDocument::new(
        &font,
        crate::reports::pdf::PaperSize::from_code(&paper_size, true),
        "Maintafox — Planning",
    )?;
    doc.heading("Planning — Gantt");
    doc.paragraph(&format!(
        "{} → {} · {} commitment(s)",
        snapshot.period_start,
        snapshot.period_end,
        snapshot.commitments.len()
    ));
    let rows: Vec<Vec<String>> = snapshot
        .commitments
        .iter()
        .map(|c| {
            vec![
                c.source_code.clone().unwrap_or_else(|| c.source_type.clone()),
                c.source_title.clone().unwrap_or_default(),
                c.committed_start.clone(),
                c.committed_end.clone(),
                c.assigned_team_label.clone().unwrap_or_default(),
                c.assigned_personnel_name.clone().unwrap_or_else(|| "—".to_string()),
            ]
        })
        .collect();
    doc.table(
        &["Ref.", "Title", "Start", "End", "Crew", "Assignee"],
        &rows,
        &[1.2, 3.0, 1.6, 1.6, 1.5, 1.6],
    );
    let bytes = doc.finish()?;
    Ok(ExportedBinaryDocument {
        file_name: format!("planning-gantt-{}-{}.pdf", snapshot.period_start, snapshot.period_end),
        mime_type: "application/pdf".to_string(),
        bytes,
    })
}

/// Hours the assigned technician works on a commitment: recorded at commit time (migration 210),
/// or for an older commitment the crew's labour ÷ its crew size; `None` when neither is known (the
/// hours are then reported as unknown — the slot's length is never assumed).
async fn commitment_person_hours(db: &impl ConnectionTrait, commitment: &ScheduleCommitment) -> AppResult<Option<f64>> {
    if let Some(hours) = commitment.planned_person_hours {
        return Ok(Some(hours));
    }
    let Some(labour) = commitment.planned_labor_hours else {
        return Ok(None);
    };
    let crew: Option<i64> = match db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT planned_crew_size FROM schedule_candidates WHERE id = ?",
            [commitment.schedule_candidate_id.into()],
        ))
        .await?
    {
        Some(row) => row
            .try_get("", "planned_crew_size")
            .map_err(|e| decode_err("planned_crew_size", e))?,
        None => None,
    };
    Ok(crew.filter(|n| *n >= 1).map(|n| labour / n as f64))
}

/// Each crew member over a slot: shift, hours left each day net of all their committed work, and
/// what the commit gate would refuse for the job (`planning::person_load`).
pub async fn list_assignee_capacity(
    db: &DatabaseConnection,
    input: AssigneeCapacityInput,
) -> AppResult<Vec<person_load::AssigneeCapacity>> {
    validate_window(&input.start, &input.end, "start", "end")?;
    load::validate_slot_length(&input.start, &input.end)?;
    // The job's hours per person and the work order behind it (not counted twice).
    let (person_hours, own_work_order) = match (input.commitment_id, input.candidate_id) {
        (Some(commitment_id), _) => {
            let current = get_commitment(db, commitment_id).await?;
            (
                commitment_person_hours(db, &current).await?,
                person_load::source_work_order(db, &current.source_type, current.source_id).await?,
            )
        }
        (None, Some(candidate_id)) => {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT source_type, source_id, estimated_duration_hours FROM schedule_candidates WHERE id = ?",
                    [candidate_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "ScheduleCandidate".to_string(),
                    id: candidate_id.to_string(),
                })?;
            let source_type: String = row
                .try_get("", "source_type")
                .map_err(|e| decode_err("source_type", e))?;
            let source_id: i64 = row.try_get("", "source_id").map_err(|e| decode_err("source_id", e))?;
            // A job without a duration shows the hours left, without refusals (it cannot be committed).
            let duration: Option<f64> = row
                .try_get("", "estimated_duration_hours")
                .map_err(|e| decode_err("estimated_duration_hours", e))?;
            (
                duration.filter(|h| *h > 0.0),
                person_load::source_work_order(db, &source_type, source_id).await?,
            )
        }
        (None, None) => (None, None),
    };
    person_load::crew_assignee_capacity(
        db,
        input.team_id,
        &input.start,
        &input.end,
        person_hours,
        input.commitment_id,
        own_work_order,
    )
    .await
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
    validate_window(&input.period_start, &input.period_end, "period_start", "period_end")?;
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
