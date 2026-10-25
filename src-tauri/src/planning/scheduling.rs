use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde_json::{json, Value};

use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit_event, NotificationEventInput};
use crate::{activity, notifications};

use super::break_in_approval::{self, ForcedCheck};
use super::domain::{
    AbsenceAccess, ApproverPassword, AbsenceDetail, AssigneeCapacityInput, CapacityRule, CapacityRuleFilter, CapacityWindow,
    CreateCapacityRuleInput, CreatePlanningWindowInput, CreateScheduleBreakInInput, CreateScheduleCommitmentInput,
    ExportPlanningGanttPdfInput, ExportedBinaryDocument, FreezeSchedulePeriodInput, NotifyTeamsInput,
    NotifyTeamsResult, PlanningAssigneeLane, PlanningCrewLane, PlanningGanttFilter, PlanningGanttSnapshot,
    PlanningWindow, PlanningWindowFilter, RescheduleCommitmentInput, ScheduleBreakIn, ScheduleBreakInFilter,
    ScheduleChangeLogEntry, ScheduleCommitment, ScheduleCommitmentFilter, TeamCapacityLoad, UpdateCapacityRuleInput,
    UpdatePlanningWindowInput,
};
use super::load;
use super::person_load;
use super::windows::{self, JobLocation, WINDOW_COLS};
use super::wo_sync::{self, WoSyncOutcome};

const CAPACITY_COLS: &str = "id,entity_id,team_id,effective_start,effective_end,available_hours_per_day,max_overtime_hours_per_day,row_version,created_at,updated_at";
const COMMITMENT_COLS: &str = "sc.id,sc.schedule_candidate_id,sc.source_type,sc.source_id,sc.schedule_period_start,sc.schedule_period_end,sc.committed_start,sc.committed_end,sc.assigned_team_id,sc.assigned_personnel_id,sc.committed_by_id,sc.frozen_at,sc.estimated_labor_cost,sc.budget_threshold,sc.cost_variance_warning,\
CASE WHEN sc.has_blocking_conflict = 1 OR EXISTS (SELECT 1 FROM scheduling_conflicts c \
     WHERE c.candidate_id = sc.schedule_candidate_id AND c.resolved_at IS NULL AND c.severity <> 'warning') THEN 1 ELSE 0 END AS has_blocking_conflict,\
(SELECT json_group_array(json_object('conflict_type', c.conflict_type, 'reason_code', c.reason_code, \
     'severity', c.severity, 'details_json', c.details_json)) \
   FROM scheduling_conflicts c WHERE c.candidate_id = sc.schedule_candidate_id AND c.resolved_at IS NULL) AS conflict_reasons_json,\
sc.nearest_feasible_window,sc.planned_labor_hours,sc.planned_person_hours,sc.status,sc.status_reason,sc.status_changed_at,sc.row_version,sc.created_at,sc.updated_at";
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
const BREAK_IN_COLS: &str = "sb.id,sb.schedule_commitment_id,sb.break_in_reason,sb.kind,sb.approved_by_user_id,sb.approved_by_personnel_id,sb.override_reason,sb.old_slot_start,sb.old_slot_end,sb.new_slot_start,sb.new_slot_end,sb.old_assignee_id,sb.new_assignee_id,sb.cost_impact_delta,sb.notification_dedupe_key,sb.row_version,sb.created_by_id,sb.created_at,sb.approver_signed_at,sb.forced_checks_json,sb.old_team_id,sb.new_team_id,sb.sync_refused_at,sb.sync_refused_reason";
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
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| {
        AppError::ValidationFailed(vec![format!(
            "GATE_PLAN_DATE_INVALID: {field} must be a valid RFC3339 timestamp."
        )])
    })?;
    Ok(parsed.with_timezone(&Utc))
}

fn validate_window(start: &str, end: &str, start_field: &str, end_field: &str) -> AppResult<()> {
    let start_dt = parse_rfc3339(start, start_field)?;
    let end_dt = parse_rfc3339(end, end_field)?;
    if end_dt <= start_dt {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_PLAN_DATE_ORDER: {end_field} must be strictly greater than {start_field}."
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
        equipment_id: row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?,
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
        scope_review_required: row
            .try_get("", "scope_review_required")
            .map_err(|e| decode_err("scope_review_required", e))?,
        entity_name: row
            .try_get("", "entity_name")
            .map_err(|e| decode_err("entity_name", e))?,
        equipment_code: row
            .try_get("", "equipment_code")
            .map_err(|e| decode_err("equipment_code", e))?,
        equipment_name: row
            .try_get("", "equipment_name")
            .map_err(|e| decode_err("equipment_name", e))?,
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
        conflict_reasons_json: row
            .try_get("", "conflict_reasons_json")
            .map_err(|e| decode_err("conflict_reasons_json", e))?,
        nearest_feasible_window: row
            .try_get("", "nearest_feasible_window")
            .map_err(|e| decode_err("nearest_feasible_window", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        status_reason: row
            .try_get("", "status_reason")
            .map_err(|e| decode_err("status_reason", e))?,
        status_changed_at: row
            .try_get("", "status_changed_at")
            .map_err(|e| decode_err("status_changed_at", e))?,
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
        kind: row.try_get("", "kind").map_err(|e| decode_err("kind", e))?,
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
        approver_signed_at: row
            .try_get("", "approver_signed_at")
            .map_err(|e| decode_err("approver_signed_at", e))?,
        forced_checks: row
            .try_get::<Option<String>>("", "forced_checks_json")
            .map_err(|e| decode_err("forced_checks_json", e))?
            .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
            .unwrap_or_default(),
        old_team_id: row.try_get("", "old_team_id").map_err(|e| decode_err("old_team_id", e))?,
        new_team_id: row.try_get("", "new_team_id").map_err(|e| decode_err("new_team_id", e))?,
        sync_refused_at: row
            .try_get("", "sync_refused_at")
            .map_err(|e| decode_err("sync_refused_at", e))?,
        sync_refused_reason: row
            .try_get("", "sync_refused_reason")
            .map_err(|e| decode_err("sync_refused_reason", e))?,
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

/// Records a commitment released or cancelled (`planning::lifecycle`) in the schedule change log.
pub(crate) async fn append_lifecycle_log(
    db: &impl ConnectionTrait,
    commitment_id: i64,
    actor_id: i64,
    to_status: &str,
    reason: &str,
) -> AppResult<()> {
    append_change_log(
        db,
        Some(commitment_id),
        if to_status == "released" {
            "release_commitment"
        } else {
            "cancel_commitment"
        },
        Some(actor_id),
        Some("status".to_string()),
        Some("committed".to_string()),
        Some(to_status.to_string()),
        Some(to_status.to_string()),
        Some(reason.to_string()),
        Some(reason.to_string()),
        None,
    )
    .await
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
        return Err(crate::errors::org_validation_failed(
            "PLANNING_CAPACITY_RULE_OVERLAP",
            "This crew already has an overtime ceiling over part of this period: end it first, or change its dates.",
            std::collections::HashMap::new(),
        ));
    }
    Ok(())
}

/// The first locked window over `[start, end)` that applies to the job (its site, a parent node,
/// its equipment, or plant-wide).
async fn find_locked_window(
    db: &impl ConnectionTrait,
    start: &str,
    end: &str,
    loc: JobLocation,
) -> AppResult<Option<PlanningWindow>> {
    let row = windows::find_locked_window_for(db, start, end, loc).await?;
    row.as_ref().map(map_planning_window).transpose()
}

fn locked_window_error(window: &PlanningWindow) -> AppError {
    crate::errors::org_validation_failed(
        "PLANNING_WINDOW_LOCKED",
        format!(
            "A locked planning window ({}, {} → {}) covers this job's site or equipment over the slot.",
            window.window_type, window.start_datetime, window.end_datetime
        ),
        std::collections::HashMap::from([
            ("window_type".to_string(), window.window_type.clone()),
            ("start".to_string(), window.start_datetime.clone()),
            ("end".to_string(), window.end_datetime.clone()),
            ("reason".to_string(), window.lock_reason.clone().unwrap_or_default()),
        ]),
    )
}

/// Work that needs the equipment stopped is committed only inside an open window of a
/// shutdown-work type (maintenance window, planned shutdown) that applies to it.
async fn enforce_shutdown_window(
    db: &impl ConnectionTrait,
    candidate_id: i64,
    start: &str,
    end: &str,
    loc: JobLocation,
) -> AppResult<()> {
    let requirement: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT shutdown_requirement FROM schedule_candidates WHERE id = ?",
            [candidate_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "shutdown_requirement"))
        .transpose()
        .map_err(|e| decode_err("shutdown_requirement", e))?
        .flatten();
    if !requirement.is_some_and(|r| r.eq_ignore_ascii_case("required")) {
        return Ok(());
    }
    if windows::has_shutdown_window_for(db, start, end, loc).await? {
        return Ok(());
    }
    Err(crate::errors::org_validation_failed(
        "PLANNING_NO_SHUTDOWN_WINDOW",
        "This job needs the equipment stopped: place it inside an open maintenance or shutdown window of its site or equipment.",
        std::collections::HashMap::new(),
    ))
}

async fn enforce_candidate_ready(
    db: &impl ConnectionTrait,
    candidate_id: i64,
    expected_row_version: Option<i64>,
) -> AppResult<(String, i64)> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT source_type, source_id, row_version
             FROM schedule_candidates
             WHERE id = ?",
            [candidate_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ScheduleCandidate".to_string(),
            id: candidate_id.to_string(),
        })?;

    let row_version: i64 = row
        .try_get("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    if let Some(expected) = expected_row_version {
        if expected != row_version {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: Schedule candidate was modified elsewhere (stale row_version)."
                    .to_string(),
            ]));
        }
    }
    // Judged on the current state: the conflicts are derived again from the sources now.
    super::queries::evaluate_candidate(db, candidate_id).await?;
    let blocking: Vec<String> = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT reason_code, severity FROM scheduling_conflicts
             WHERE candidate_id = ? AND resolved_at IS NULL
             ORDER BY id",
            [candidate_id.into()],
        ))
        .await?
        .iter()
        .map(|r| {
            Ok((
                r.try_get::<String>("", "reason_code")
                    .map_err(|e| decode_err("reason_code", e))?,
                r.try_get::<String>("", "severity")
                    .map_err(|e| decode_err("severity", e))?,
            ))
        })
        .collect::<AppResult<Vec<(String, String)>>>()?
        .into_iter()
        // A missing estimate is refused right after with its own message (`candidate_hours`).
        .filter(|(code, severity)| super::queries::is_blocking_severity(severity) && code != "NOT_ESTIMATED")
        .map(|(code, _)| code)
        .collect();
    if !blocking.is_empty() {
        return Err(crate::errors::org_validation_failed(
            "PLANNING_CANDIDATE_BLOCKED",
            format!(
                "This work cannot be committed yet: {} (see why on its card).",
                blocking.join(", ")
            ),
            std::collections::HashMap::from([("reasons".to_string(), blocking.join(", "))]),
        ));
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

/// A person's absence blocks touching `[start, end)`, as the planning board draws them, ordered by
/// start. `Full` (callers who may read the type, `personnel::absence_access`): `{start_at, end_at,
/// block_type, critical}` — `critical` is the block's own flag, the screen adds the type's tag from
/// the plant's list. `AvailabilityOnly`:
/// `{start_at, end_at, critical}` — `critical` when the absence type carries the `critical` tag in
/// Données de référence (`PERSONNEL.ABSENCE_TYPES`) or the block is marked critical; nothing else.
pub(crate) async fn absence_intervals(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    start: &str,
    end: &str,
    detail: AbsenceDetail,
) -> AppResult<Vec<Value>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT b.start_at, b.end_at, b.block_type, b.is_critical, {tag} AS tag
                 FROM personnel_availability_blocks b
                 WHERE b.personnel_id = ?
                   AND b.start_at < ?
                   AND b.end_at > ?
                 ORDER BY b.start_at ASC, b.id ASC",
                tag = crate::personnel::reference_lists::BLOCK_TAG_SQL
            ),
            [personnel_id.into(), end.to_string().into(), start.to_string().into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let start_at: String = row.try_get("", "start_at").map_err(|e| decode_err("start_at", e))?;
            let end_at: String = row.try_get("", "end_at").map_err(|e| decode_err("end_at", e))?;
            let block_critical = row
                .try_get::<i64>("", "is_critical")
                .map_err(|e| decode_err("is_critical", e))?
                == 1;
            Ok(match detail {
                AbsenceDetail::Full => json!({
                    "start_at": start_at,
                    "end_at": end_at,
                    "block_type": row.try_get::<String>("", "block_type").map_err(|e| decode_err("block_type", e))?,
                    "critical": block_critical,
                }),
                AbsenceDetail::AvailabilityOnly => {
                    let tag: Option<String> = row.try_get("", "tag").map_err(|e| decode_err("tag", e))?;
                    json!({
                        "start_at": start_at,
                        "end_at": end_at,
                        "critical": block_critical
                            || tag.as_deref() == Some(crate::personnel::reference_lists::ABSENCE_TAG_CRITICAL),
                    })
                }
            })
        })
        .collect()
}

async fn enforce_assignee_availability(
    db: &impl ConnectionTrait,
    personnel_id: i64,
    start: &str,
    end: &str,
) -> AppResult<()> {
    // The refusal names when the person is away and whether it is critical for planning, never the
    // absence type: it reaches any planner (and the logs), whatever their personnel rights.
    let intervals = absence_intervals(db, personnel_id, start, end, AbsenceDetail::AvailabilityOnly).await?;
    if !intervals.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "ASSIGNEE_UNAVAILABLE {}",
            json!({
                "personnel_id": personnel_id,
                "intervals": intervals
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
    clear_assignee: bool,
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
        clear_assignee,
    )
    .await?;
    Ok(Some(outcome))
}

/// The assignee of a work-order commitment must hold what the work order requires
/// (`qualification::gate`); an authorised supervisor's override, recorded beforehand with
/// `record_qualification_override`, lets them through. Other sources have no requirements.
/// Refuses a named technician who lacks a qualification the job asks for — work order, PM
/// occurrence or request (`planning::qualifications`) — valid on the slot's first day.
pub(crate) async fn enforce_assignee_qualifications(
    db: &impl ConnectionTrait,
    source_type: &str,
    source_id: i64,
    personnel_id: i64,
    slot_start: &str,
) -> AppResult<()> {
    let work_day = crate::wo::shifts::planned_instant(slot_start)
        .map(|t| t.with_timezone(&chrono::Local).date_naive())
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![format!(
                "GATE_PLAN_SLOT_START_INVALID: The slot start '{slot_start}' is not a date and time."
            )])
        })?;
    match super::qualifications::check_assignee(db, source_type, source_id, personnel_id, work_day).await? {
        Some(check) if !check.qualified => Err(crate::qualification::gate::blocked_error(&check)),
        _ => Ok(()),
    }
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

pub(crate) async fn emit_planning_notification(
    db: &notifications::SqlitePool,
    event_code: &str,
    severity: &str,
    dedupe_key: Option<String>,
    source_record_id: String,
    payload: Value,
    title: String,
    body: String,
) {
    if let Err(err) = emit_event(
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
    .await
    {
        tracing::error!(event_code, error = %err, "planning notification not raised");
    }
}

/// A slot can be moved or broken into only while it is held (`committed`): a released,
/// cancelled or closed commitment is history.
fn ensure_held(current: &ScheduleCommitment) -> AppResult<()> {
    if current.status == super::lifecycle::STATUS_COMMITTED {
        return Ok(());
    }
    Err(crate::errors::org_validation_failed(
        "PLANNING_COMMITMENT_NOT_ACTIVE",
        format!("This commitment is already {}.", current.status),
        std::collections::HashMap::from([("status".to_string(), current.status.clone())]),
    ))
}

/// Tells the crew and the activity feed about a break-in (a slot moved, or new work entering a
/// frozen week) once its transaction is committed.
#[allow(clippy::too_many_arguments)]
async fn announce_break_in(
    db: &DatabaseConnection,
    actor_id: i64,
    commitment_id: i64,
    break_in_id: i64,
    dedupe_key: String,
    payload: Value,
    dangerous: bool,
    body: String,
) -> AppResult<()> {
    emit_planning_notification(
        db,
        "planning.break_in.created",
        "high",
        Some(dedupe_key),
        commitment_id.to_string(),
        payload.clone(),
        "Break-in schedule override".to_string(),
        body,
    )
    .await;
    // Correlates the events of one break-in on every device: by sync id, not the local id.
    let correlation = crate::sync::identity::optional_entity_sync_id(db, "schedule_break_ins", break_in_id)
        .await?
        // Every row gets a sync id at insert; a row without one keeps a device-marked key.
        .unwrap_or_else(|| format!("local-{break_in_id}"));
    if let Err(err) = activity::emitter::emit_activity_event(
        db,
        activity::emitter::ActivityEventInput {
            event_class: "operational".to_string(),
            event_code: "planning.break_in".to_string(),
            source_module: "planning".to_string(),
            source_record_type: Some("schedule_commitment".to_string()),
            source_record_id: Some(commitment_id.to_string()),
            entity_scope_id: None,
            actor_id: Some(actor_id),
            severity: if dangerous {
                "warning".to_string()
            } else {
                "info".to_string()
            },
            summary_json: Some(payload),
            correlation_id: Some(format!("planning-breakin-{correlation}")),
            visibility_scope: "entity".to_string(),
        },
    )
    .await
    {
        tracing::error!(break_in_id, error = %err, "planning break-in activity not recorded");
    }
    Ok(())
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
    // A job holds one slot at a time: release (or cancel) it before committing it again.
    if exclude_commitment_id.is_none() && super::lifecycle::candidate_holds_a_slot(tx, candidate_id).await? {
        return Err(crate::errors::org_validation_failed(
            "PLANNING_ALREADY_COMMITTED",
            "This job already holds a slot in the schedule: reschedule it, or release its slot first.",
            std::collections::HashMap::new(),
        ));
    }
    let (source_type, source_id) = enforce_candidate_ready(tx, candidate_id, expected_candidate_row_version).await?;

    let location = windows::job_location(tx, &source_type, source_id).await?;
    if let Some(window) = find_locked_window(tx, committed_start, committed_end, location).await? {
        return Err(locked_window_error(&window));
    }
    enforce_shutdown_window(tx, candidate_id, committed_start, committed_end, location).await?;

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
        let day = overloaded_day.unwrap_or_default();
        return Err(crate::errors::org_validation_failed(
            "PLANNING_CAPACITY_EXCEEDED",
            format!("The crew's load would exceed its capacity plus its overtime ceiling on {day}."),
            std::collections::HashMap::from([("day".to_string(), day.to_string())]),
        ));
    }
    let nearest_feasible_window: Option<String> = None;

    if let Some(personnel_id) = assigned_personnel_id {
        enforce_assignee_availability(tx, personnel_id, committed_start, committed_end).await?;
        enforce_assignee_qualifications(tx, &source_type, source_id, personnel_id, committed_start).await?;

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
            return Err(crate::errors::org_validation_failed(
                "PLANNING_DOUBLE_BOOKING",
                "The technician already holds another job at this time: choose someone else, move the slot or override with a reason.",
                std::collections::HashMap::new(),
            ));
        }
        if (overlaps > 0 || !issues.is_empty()) && allow_double_booking_override {
            let reason = override_reason
                .as_ref()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    crate::errors::org_validation_failed(
                        "PLANNING_OVERRIDE_REASON_REQUIRED",
                        "Give the reason for overriding the checks on the technician.",
                        std::collections::HashMap::new(),
                    )
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

/// Hours of a capacity rule are never negative.
fn check_capacity_rule_hours(available: f64, overtime: f64) -> AppResult<()> {
    if available < 0.0 || overtime < 0.0 || !available.is_finite() || !overtime.is_finite() {
        return Err(crate::errors::org_validation_failed(
            "PLANNING_CAPACITY_RULE_HOURS_NEGATIVE",
            "The overtime ceiling is a number of hours, zero or more.",
            std::collections::HashMap::new(),
        ));
    }
    Ok(())
}

/// A capacity rule ends on or after the day it starts.
fn check_capacity_rule_period(start: &str, end: Option<&str>) -> AppResult<()> {
    if end.is_some_and(|end| end < start) {
        return Err(crate::errors::org_validation_failed(
            "PLANNING_CAPACITY_RULE_END_BEFORE_START",
            "The rule cannot end before it starts.",
            std::collections::HashMap::from([
                ("start".to_string(), start.to_string()),
                ("end".to_string(), end.unwrap_or_default().to_string()),
            ]),
        ));
    }
    Ok(())
}

pub async fn create_capacity_rule(db: &DatabaseConnection, input: CreateCapacityRuleInput) -> AppResult<CapacityRule> {
    check_capacity_rule_hours(
        input.available_hours_per_day.unwrap_or(0.0),
        input.max_overtime_hours_per_day,
    )?;
    check_capacity_rule_period(&input.effective_start, input.effective_end.as_deref())?;
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

/// The rule was changed elsewhere since it was read: refused, never applied over it.
fn capacity_rule_stale(rule_id: i64) -> AppError {
    crate::errors::org_validation_failed(
        "PLANNING_CAPACITY_RULE_STALE",
        "This capacity rule was changed elsewhere: reload it and try again.",
        std::collections::HashMap::from([("rule_id".to_string(), rule_id.to_string())]),
    )
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
        return Err(capacity_rule_stale(rule_id));
    }
    let effective_start = input.effective_start.unwrap_or_else(|| current.effective_start.clone());
    let effective_end = if input.clear_effective_end {
        None
    } else {
        input.effective_end.or(current.effective_end.clone())
    };
    let available_hours = input.available_hours_per_day.unwrap_or(current.available_hours_per_day);
    let overtime_hours = input
        .max_overtime_hours_per_day
        .unwrap_or(current.max_overtime_hours_per_day);
    check_capacity_rule_hours(available_hours, overtime_hours)?;
    check_capacity_rule_period(&effective_start, effective_end.as_deref())?;
    validate_capacity_overlap(
        db,
        current.team_id,
        current.entity_id,
        &effective_start,
        effective_end.as_deref(),
        Some(rule_id),
    )
    .await?;
    let updated = db
        .execute(Statement::from_sql_and_values(
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
    // Changed elsewhere between the read and the write: refused, never applied over it.
    if updated.rows_affected() == 0 {
        return Err(capacity_rule_stale(rule_id));
    }
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
        where_sql.push("pw.entity_id = ?".to_string());
        values.push(entity_id.into());
    }
    if let Some(equipment_id) = filter.equipment_id {
        where_sql.push("pw.equipment_id = ?".to_string());
        values.push(equipment_id.into());
    }
    if let Some(window_type) = filter.window_type {
        where_sql.push("pw.window_type = ?".to_string());
        values.push(window_type.into());
    }
    if !filter.include_locked.unwrap_or(true) {
        where_sql.push("pw.is_locked = 0".to_string());
    }

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT {WINDOW_COLS}
                 FROM planning_windows pw
                 WHERE {}
                 ORDER BY pw.start_datetime ASC, pw.id ASC",
                where_sql.join(" AND ")
            ),
            values,
        ))
        .await?;
    rows.iter().map(map_planning_window).collect()
}

async fn get_planning_window(db: &impl ConnectionTrait, window_id: i64) -> AppResult<PlanningWindow> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {WINDOW_COLS} FROM planning_windows pw WHERE pw.id = ?"),
            [window_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PlanningWindow".to_string(),
            id: window_id.to_string(),
        })?;
    map_planning_window(&row)
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
    let window_type = input.window_type.trim().to_string();
    windows::validate_window_type(db, &window_type).await?;
    let entity_id = windows::validate_window_scope(db, input.entity_id, input.equipment_id).await?;
    let inserted = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO planning_windows
            (entity_id, equipment_id, window_type, start_datetime, end_datetime, is_locked, lock_reason)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
            [
                entity_id.into(),
                input.equipment_id.into(),
                window_type.into(),
                input.start_datetime.into(),
                input.end_datetime.into(),
                i64::from(input.is_locked.unwrap_or(false)).into(),
                input.lock_reason.into(),
            ],
        ))
        .await?;
    let id = i64::try_from(inserted.last_insert_id())
        .map_err(|_| AppError::Internal(anyhow::anyhow!("planning_windows last_insert_id does not fit i64")))?;
    get_planning_window(db, id).await
}

pub async fn update_planning_window(
    db: &DatabaseConnection,
    window_id: i64,
    expected_row_version: i64,
    input: UpdatePlanningWindowInput,
) -> AppResult<PlanningWindow> {
    let current = get_planning_window(db, window_id).await?;
    if current.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Planning window was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    let window_type = input
        .window_type
        .map(|t| t.trim().to_string())
        .unwrap_or(current.window_type.clone());
    if window_type != current.window_type {
        windows::validate_window_type(db, &window_type).await?;
    }
    let start_datetime = input.start_datetime.unwrap_or(current.start_datetime.clone());
    let end_datetime = input.end_datetime.unwrap_or(current.end_datetime.clone());
    validate_window(&start_datetime, &end_datetime, "start_datetime", "end_datetime")?;
    // Re-entering the site or the equipment settles a scope flagged for review.
    let scope_review_required = if input.entity_id.is_some() || input.equipment_id.is_some() {
        0
    } else {
        current.scope_review_required
    };
    let equipment_id = input.equipment_id.unwrap_or(current.equipment_id);
    let entity_id =
        windows::validate_window_scope(db, input.entity_id.unwrap_or(current.entity_id), equipment_id).await?;

    let updated = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE planning_windows
         SET entity_id = ?, equipment_id = ?, window_type = ?, start_datetime = ?, end_datetime = ?,
             is_locked = ?, lock_reason = ?, scope_review_required = ?,
             row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
            [
                entity_id.into(),
                equipment_id.into(),
                window_type.into(),
                start_datetime.into(),
                end_datetime.into(),
                i64::from(input.is_locked.unwrap_or(current.is_locked == 1)).into(),
                input.lock_reason.or(current.lock_reason).into(),
                scope_review_required.into(),
                window_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "GATE_RECORD_CHANGED_ELSEWHERE: Planning window was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    get_planning_window(db, window_id).await
}

pub async fn list_schedule_commitments(
    db: &DatabaseConnection,
    filter: ScheduleCommitmentFilter,
) -> AppResult<Vec<ScheduleCommitment>> {
    // The board shows slots held and work done; released or cancelled slots are history only.
    let mut where_sql = vec!["sc.status IN ('committed', 'closed')".to_string()];
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
    // A reason of the COMPANY list PLANNING.BREAK_IN_REASONS; an urgent one needs evidence.
    let break_in_reason = super::break_in_reasons::resolve_break_in_reason(db, &input.break_in_reason).await?;
    let reason = break_in_reason.code.clone();
    let dangerous_override_reason = input
        .dangerous_override_reason
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    // The approver's password counts toward their lockout (`auth::second_signer`): read before the
    // transaction, the outcome recorded after it, refused or not.
    let typed = input.approver_password.as_ref().map(ApproverPassword::expose);
    let signer = break_in_approval::signer_account(input.approved_by_user_id, typed);
    let password = break_in_approval::signer_password_state(db, signer, typed).await?;
    let mut password_checked = None;
    let tx = db.begin().await?;
    let in_tx = async {
        let current = get_commitment(&tx, input.schedule_commitment_id).await?;
        if current.row_version != input.expected_commitment_row_version {
            return Err(AppError::ValidationFailed(vec![
                "GATE_RECORD_CHANGED_ELSEWHERE: Schedule commitment was modified elsewhere (stale row_version)."
                    .to_string(),
            ]));
        }
        ensure_held(&current)?;

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
            enforce_assignee_qualifications(
                &tx,
                &current.source_type,
                current.source_id,
                personnel_id,
                &input.new_slot_start,
            )
            .await?;
            let overlaps = check_double_booking(
                &tx,
                personnel_id,
                &input.new_slot_start,
                &input.new_slot_end,
                Some(current.id),
            )
            .await?;
            if overlaps > 0 && override_reason.is_none() {
                return Err(crate::errors::org_validation_failed(
                "PLANNING_DOUBLE_BOOKING",
                "The technician already holds another job at this time: give the override reason to break in anyway.",
                std::collections::HashMap::new(),
            ));
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
        // A blocking check is forced only with a recorded dangerous-override reason and the signature
        // of a second planner (CMMS Advisor answer Q1, `break_in_approval`).
        let mut forced: Vec<ForcedCheck> = Vec::new();
        if !capacity_ok {
            if dangerous_override_reason.is_none() {
                return Err(crate::errors::org_validation_failed(
                    "PLANNING_BREAK_IN_CAPACITY_OVERRIDE_REQUIRED",
                    "This break-in exceeds the crew's capacity: give a dangerous-override reason to proceed.",
                    std::collections::HashMap::new(),
                ));
            }
            forced.push(ForcedCheck::CrewCapacity);
        }

        let location = windows::job_location(&tx, &current.source_type, current.source_id).await?;
        let locked_window = find_locked_window(&tx, &input.new_slot_start, &input.new_slot_end, location).await?;
        if locked_window.is_some() {
            if dangerous_override_reason.is_none() {
                return Err(AppError::ValidationFailed(vec![
                "GATE_PLAN_BREAKIN_LOCKED_WINDOW: Break-in intersects a locked window and requires dangerous_override_reason.".to_string(),
            ]));
            }
            forced.push(ForcedCheck::LockedWindow);
        }

        // The same blocking checks as a commitment (the job's blocking conflicts, re-derived now, and
        // the shutdown window over the new slot). A break-in passes them only with the recorded
        // dangerous-override reason and the signature, and then stays critical on the board.
        let readiness = enforce_candidate_ready(&tx, current.schedule_candidate_id, None)
            .await
            .map(|_| ());
        let shutdown = enforce_shutdown_window(
            &tx,
            current.schedule_candidate_id,
            &input.new_slot_start,
            &input.new_slot_end,
            location,
        )
        .await;
        for (check, code) in [
            (readiness, ForcedCheck::Readiness),
            (shutdown, ForcedCheck::ShutdownWindow),
        ] {
            if let Err(err) = check {
                if dangerous_override_reason.is_none() {
                    return Err(err);
                }
                forced.push(code);
            }
        }
        let forced_blocking = !forced.is_empty();
        // An urgent reason or a forced check: signed by a second planner holding plan.confirm at the
        // crew's site, with their password (never the requester; no fallback).
        let approval = break_in_approval::validate_break_in_approval(
            &tx,
            &break_in_approval::ApprovalRequest {
                actor_id,
                team_id: next_team_id,
                // Work taken from another crew: the approver holds the right at its site too.
                from_team_id: Some(current.assigned_team_id).filter(|t| *t != next_team_id),
                reason_code: &reason,
                urgent: break_in_reason.urgent,
                forced: &forced,
                approver: input.approved_by_user_id,
                password,
            },
            &mut password_checked,
        )
        .await?;

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
             has_blocking_conflict = ?,
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
                // Forced over a blocking check (capacity, locked window, readiness, shutdown window):
                // the bar stays critical.
                i64::from(forced_blocking).into(),
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
                (schedule_commitment_id, break_in_reason, kind, approved_by_user_id, approved_by_personnel_id, override_reason,
                 old_slot_start, old_slot_end, new_slot_start, new_slot_end, old_assignee_id, new_assignee_id,
                 cost_impact_delta, notification_dedupe_key, created_by_id, approver_signed_at, forced_checks_json,
                 old_team_id, new_team_id)
             VALUES (?, ?, 'move', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            [
                current.id.into(),
                reason.clone().into(),
                approval.approver.into(),
                approval.approver_personnel.into(),
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
                approval.signed_at.clone().into(),
                break_in_approval::forced_checks_json(&forced).into(),
                current.assigned_team_id.into(),
                next_team_id.into(),
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
                    // Both names: who asked for the break-in and who signed it.
                    "requested_by_user_id": actor_id,
                    "approved_by_user_id": approval.approver,
                    "approved_by_personnel_id": approval.approver_personnel,
                    "approved_by_display_name": approval.approver_name,
                    "approver_signed": approval.signed_at.is_some(),
                    "dangerous_override": dangerous_override_reason.is_some(),
                    "dangerous_override_reason": dangerous_override_reason,
                    // A blocking check (capacity, locked window, readiness, shutdown) was forced.
                    "forced_blocking": forced_blocking,
                    "forced_checks": forced.iter().map(|c| c.code()).collect::<Vec<_>>(),
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
            // Only the explicit "Unassigned" removes the OT's responsible.
            input.clear_assignee.unwrap_or(false),
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
        Ok::<_, AppError>((current, break_in_id, dedupe_key, payload, wo_sync, approval, forced))
    };
    let (current, break_in_id, dedupe_key, payload, wo_sync, approval, forced) = match in_tx.await {
        Ok(done) => done,
        Err(e) => {
            tx.rollback().await?;
            break_in_approval::record_signer_outcome(
                db,
                actor_id,
                signer,
                password_checked,
                Some(input.schedule_commitment_id),
            )
            .await?;
            return Err(e);
        }
    };
    tx.commit().await?;
    // Committed: a failure to count the password outcome is logged, never turned into an error.
    if let Err(e) =
        break_in_approval::record_signer_outcome(db, actor_id, signer, password_checked, Some(current.id)).await
    {
        tracing::error!(error = %e, commitment_id = current.id, "break-in: approver password outcome not recorded");
    }
    break_in_approval::audit_signed_break_in(
        db,
        actor_id,
        current.id,
        break_in_id,
        "move",
        &reason,
        &approval,
        &forced,
    )
    .await;
    if let Some(outcome) = wo_sync {
        wo_sync::record_wo_sync_audit(db, actor_id, outcome.wo_id, "break_in").await;
    }
    let body = format!(
        "Break-in moved commitment #{} (reason: {}).",
        current.id,
        payload.get("reason_code").and_then(Value::as_str).unwrap_or("other")
    );
    announce_break_in(
        db,
        actor_id,
        current.id,
        break_in_id,
        dedupe_key.clone(),
        payload,
        dangerous_override_reason.is_some(),
        body,
    )
    .await?;
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
    // The approver of a break-in into a frozen week signs with their password, counted toward their
    // lockout (`auth::second_signer`): read before the transaction, recorded after it, refused or not.
    let (break_in_approver, break_in_password) = input
        .frozen_week_break_in
        .as_ref()
        .map(|b| (b.approved_by_user_id, b.approver_password.as_ref().map(ApproverPassword::expose)))
        .unwrap_or((None, None));
    let signer = break_in_approval::signer_account(break_in_approver, break_in_password);
    let password = break_in_approval::signer_password_state(db, signer, break_in_password).await?;
    let mut password_checked = None;
    let tx = db.begin().await?;
    let in_tx = async {
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

        // New work enters a frozen week only as a recorded break-in.
        let frozen_week = super::weekly_schedule::frozen_week_for_slot(
            &tx,
            input.assigned_team_id,
            &input.committed_start,
            &input.committed_end,
            chrono::Utc::now(),
        )
        .await?;
        let break_in = match (&frozen_week, &input.frozen_week_break_in) {
            (None, _) => None,
            (Some(week), None) => {
                return Err(crate::errors::org_validation_failed(
                    "PLANNING_WEEK_FROZEN",
                    format!(
                        "The week of {} is frozen: new work enters it only as a break-in, with its reason.",
                        week.week_start
                    ),
                    std::collections::HashMap::from([("week_start".to_string(), week.week_start.clone())]),
                ))
            }
            (Some(_), Some(b)) => {
                let reason = super::break_in_reasons::resolve_break_in_reason(&tx, &b.reason).await?;
                let dangerous = b
                    .dangerous_override_reason
                    .as_ref()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty());
                // New work passes every blocking check of a commitment (`run_commitment_gates`): nothing
                // is forced. An urgent reason is signed by a second planner holding plan.confirm at the
                // crew's site (a dangerous-override reason no longer stands in for the signature).
                let approval = break_in_approval::validate_break_in_approval(
                    &tx,
                    &break_in_approval::ApprovalRequest {
                        actor_id,
                        team_id: input.assigned_team_id,
                        from_team_id: None,
                        reason_code: &reason.code,
                        urgent: reason.urgent,
                        forced: &[],
                        approver: b.approved_by_user_id,
                        password,
                    },
                    &mut password_checked,
                )
                .await?;
                Some((reason, approval, dangerous))
            }
        };

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
        let mut inserted_break_in = None;
        if let Some((reason, approval, dangerous)) = &break_in {
            // Part of the frozen week from now on (not of its baseline: unplanned work), recorded as a
            // break-in of kind `insert`: no old slot, and the approver only when someone signed it.
            tx.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE schedule_commitments SET frozen_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
                [id.into()],
            ))
            .await?;
            let dedupe_key = format!(
                "planning.breakin.{id}.{}.{}",
                input.committed_start, input.committed_end
            );
            let inserted = tx
                .execute(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "INSERT INTO schedule_break_ins
                    (schedule_commitment_id, break_in_reason, kind, approved_by_user_id, approved_by_personnel_id,
                     override_reason, old_slot_start, old_slot_end, new_slot_start, new_slot_end, old_assignee_id,
                     new_assignee_id, cost_impact_delta, notification_dedupe_key, created_by_id, approver_signed_at,
                     forced_checks_json, old_team_id, new_team_id)
                 VALUES (?, ?, 'insert', ?, ?, ?, NULL, NULL, ?, ?, NULL, ?, NULL, ?, ?, ?, NULL, NULL, ?)",
                    [
                        id.into(),
                        reason.code.clone().into(),
                        approval.approver.into(),
                        approval.approver_personnel.into(),
                        dangerous.clone().into(),
                        input.committed_start.clone().into(),
                        input.committed_end.clone().into(),
                        input.assigned_personnel_id.into(),
                        Some(dedupe_key.clone()).into(),
                        Some(actor_id).into(),
                        approval.signed_at.clone().into(),
                        input.assigned_team_id.into(),
                    ],
                ))
                .await?;
            let break_in_id = i64::try_from(inserted.last_insert_id()).map_err(|_| {
                AppError::Internal(anyhow::anyhow!("schedule_break_ins last_insert_id does not fit i64"))
            })?;
            let payload = json!({
                "break_in_id": break_in_id,
                "kind": "insert",
                "schedule_commitment_id": id,
                "reason_code": reason.code,
                "impacted_assignee_id": input.assigned_personnel_id,
                "new_slot_start": input.committed_start,
                "new_slot_end": input.committed_end,
                "frozen_week": frozen_week.as_ref().map(|w| w.week_start.clone()),
                "team_id": input.assigned_team_id
            });
            inserted_break_in = Some((
                break_in_id,
                dedupe_key,
                payload,
                dangerous.is_some(),
                reason.code.clone(),
            ));
            append_change_log(
                &tx,
                Some(id),
                "break_in_insert",
                Some(actor_id),
                Some("commitment_slot".to_string()),
                None,
                Some(format!("{} -> {}", input.committed_start, input.committed_end)),
                Some(reason.code.clone()),
                dangerous.clone(),
                Some(reason.code.clone()),
                Some(
                    json!({
                        "break_in_id": break_in_id,
                        // Both names: who asked for the break-in and who signed it.
                        "requested_by_user_id": actor_id,
                        "approved_by_user_id": approval.approver,
                        "approved_by_display_name": approval.approver_name,
                        "approver_signed": approval.signed_at.is_some(),
                        "dangerous_override_reason": dangerous,
                        "frozen_week": frozen_week.as_ref().map(|w| w.week_start.clone()),
                    })
                    .to_string(),
                ),
            )
            .await?;
        }
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
            false,
        )
        .await?;
        let approval = break_in.map(|(_, approval, _)| approval).unwrap_or_default();
        Ok::<_, AppError>((id, wo_sync, inserted_break_in, approval))
    };
    let (id, wo_sync, inserted_break_in, approval) = match in_tx.await {
        Ok(done) => done,
        Err(e) => {
            tx.rollback().await?;
            break_in_approval::record_signer_outcome(db, actor_id, signer, password_checked, None).await?;
            return Err(e);
        }
    };
    tx.commit().await?;
    // Committed: a failure to count the password outcome is logged, never turned into an error.
    if let Err(e) = break_in_approval::record_signer_outcome(db, actor_id, signer, password_checked, Some(id)).await {
        tracing::error!(error = %e, commitment_id = id, "break-in: approver password outcome not recorded");
    }
    if let Some(outcome) = wo_sync {
        wo_sync::record_wo_sync_audit(db, actor_id, outcome.wo_id, "create").await;
    }
    if let Some((break_in_id, dedupe_key, payload, dangerous, reason)) = inserted_break_in {
        break_in_approval::audit_signed_break_in(db, actor_id, id, break_in_id, "insert", &reason, &approval, &[])
            .await;
        let body = format!("Break-in added commitment #{id} to a frozen week (reason: {reason}).");
        announce_break_in(db, actor_id, id, break_in_id, dedupe_key, payload, dangerous, body).await?;
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
            "GATE_RECORD_CHANGED_ELSEWHERE: Schedule commitment was modified elsewhere (stale row_version)."
                .to_string(),
        ]));
    }
    ensure_held(&current)?;
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
            "GATE_PLAN_COMMITMENT_FROZEN: Commitment is frozen and cannot be rescheduled.".to_string(),
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
    // Moving work into a frozen week is a break-in (recorded with its reason), not a reschedule.
    if let Some(week) = super::weekly_schedule::frozen_week_for_slot(
        &tx,
        input.assigned_team_id,
        &input.committed_start,
        &input.committed_end,
        chrono::Utc::now(),
    )
    .await?
    {
        return Err(crate::errors::org_validation_failed(
            "PLANNING_WEEK_FROZEN",
            format!(
                "The week of {} is frozen: move work into it with a break-in, with its reason.",
                week.week_start
            ),
            std::collections::HashMap::from([("week_start".to_string(), week.week_start.clone())]),
        ));
    }

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
            "GATE_RECORD_CHANGED_ELSEWHERE: Schedule commitment was modified elsewhere (stale row_version)."
                .to_string(),
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
        false,
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

/// The planning board over a period. `access` is what the caller may read of each assignee's
/// absences (`personnel::absence_access`, loaded by the IPC command).
pub async fn get_planning_gantt_snapshot(
    db: &DatabaseConnection,
    filter: PlanningGanttFilter,
    access: &AbsenceAccess,
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

            let blocked = absence_intervals(
                db,
                personnel_id,
                &filter.period_start,
                &filter.period_end,
                access.detail_for(personnel_id),
            )
            .await?;
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
                 FROM planning_windows pw
                 WHERE pw.is_locked = 1
                   AND julianday(pw.start_datetime) < julianday(?)
                   AND julianday(pw.end_datetime) > julianday(?)
                 ORDER BY pw.start_datetime ASC"
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

/// An instant stored in UTC, printed in this PC's local time in the report language; text that is
/// not a timestamp is printed as it is.
pub(crate) fn local_pdf_instant(locale: crate::reports::export::ReportLocale, value: &str) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value) else {
        return value.to_string();
    };
    let local = parsed.with_timezone(&chrono::Local);
    match locale {
        crate::reports::export::ReportLocale::Fr => local.format("%d/%m/%Y %H:%M").to_string(),
        crate::reports::export::ReportLocale::En => local.format("%Y-%m-%d %H:%M").to_string(),
    }
}

/// Label for a commitment that has no code (its source type), in the report language.
pub(crate) fn pdf_source_type_label(locale: crate::reports::export::ReportLocale, source_type: &str) -> String {
    match source_type {
        "work_order" => locale.pick("Ordre de travail", "Work order").to_string(),
        "pm_occurrence" => locale.pick("Occurrence PM", "PM occurrence").to_string(),
        other => other.to_string(),
    }
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
        // The PDF prints commitments and capacity only, never absences.
        &AbsenceAccess::neutral(),
    )
    .await?;
    let paper_size = input.paper_size.unwrap_or_else(|| "A4".to_string());
    // The PDF follows the app language and prints times in this PC's local time.
    let locale = crate::reports::export::ReportLocale::resolve(db, None).await;
    let font = crate::reports::pdf::load_system_font()?;
    let mut doc = crate::reports::pdf::PdfDocument::new(
        &font,
        crate::reports::pdf::PaperSize::from_code(&paper_size, true),
        "Maintafox — Planning",
    )?;
    doc.heading("Planning — Gantt");
    doc.paragraph(&format!(
        "{} → {} · {} {} · {}",
        local_pdf_instant(locale, &snapshot.period_start),
        local_pdf_instant(locale, &snapshot.period_end),
        snapshot.commitments.len(),
        locale.pick("engagement(s)", "commitment(s)"),
        locale.pick("heures locales", "local time"),
    ));
    let rows: Vec<Vec<String>> = snapshot
        .commitments
        .iter()
        .map(|c| {
            vec![
                c.source_code
                    .clone()
                    .unwrap_or_else(|| pdf_source_type_label(locale, &c.source_type)),
                c.source_title.clone().unwrap_or_default(),
                local_pdf_instant(locale, &c.committed_start),
                local_pdf_instant(locale, &c.committed_end),
                c.assigned_team_label.clone().unwrap_or_default(),
                c.assigned_personnel_name.clone().unwrap_or_else(|| "—".to_string()),
            ]
        })
        .collect();
    doc.table(
        &[
            locale.pick("Réf.", "Ref."),
            locale.pick("Titre", "Title"),
            locale.pick("Début", "Start"),
            locale.pick("Fin", "End"),
            locale.pick("Équipe", "Crew"),
            locale.pick("Intervenant", "Assignee"),
        ],
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
        // Only the capacity is returned.
        &AbsenceAccess::neutral(),
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
