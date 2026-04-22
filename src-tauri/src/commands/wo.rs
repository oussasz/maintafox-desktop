//! WO IPC commands.
//!
//! Phase 2 – Sub-phase 05 – File 01 Sprint S2 + File 02 Sprint S2 + File 03 Sprint S2.
//!
//! Permission gates:
//!   ot.view    — list, get, list_labor, list_tasks, list_wo_parts, list_delay_segments,
//!                list_downtime_segments, get_cost_summary, list_wo_attachments,
//!                get_cost_posting_hook, get_wo_analytics_snapshot, get_wo_stats
//!   ot.create  — create work order
//!   ot.edit    — update draft, cancel, plan, assign, start, pause, resume, hold,
//!                complete_mechanically, add_labor, close_labor, add_part,
//!                record_part_usage, confirm_no_parts, add_task, complete_task,
//!                open_downtime, close_downtime, save_failure_detail, save_verification,
//!                close_wo, upload_wo_attachment, update_service_cost
//!   ot.admin   — reopen_wo, delete_wo_attachment

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::wo::{
    analytics, attachments, audit, closeout, costs, delay, execution, labor, parts, priorities, queries,
    stats, statuses, tasks,
};
use crate::wo::types;
use crate::{require_permission, require_session};

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use tauri::Manager;

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_wo — requires ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo(
    filter: queries::WoListFilter,
    state: State<'_, AppState>,
) -> AppResult<queries::WoListPage> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    queries::list_work_orders(&state.db, filter).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_wo — requires ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_wo(
    id: i64,
    state: State<'_, AppState>,
) -> AppResult<queries::WoGetResponse> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);

    let wo = queries::get_work_order(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: id.to_string(),
        })?;

    let transitions = queries::get_wo_transition_log(&state.db, id).await?;

    Ok(queries::WoGetResponse { wo, transitions })
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) create_wo — requires ot.create
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn create_wo(
    input: crate::wo::domain::WoCreateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.create", PermissionScope::Global);

    // ── Validate required fields ─────────────────────────────────────────
    let mut errors: Vec<String> = Vec::new();

    if input.title.trim().is_empty() {
        errors.push("Le titre est obligatoire.".into());
    }

    // Validate type_code resolves
    match types::resolve_work_order_type_id_by_code(&state.db, &input.type_code).await {
        Ok(_) => {}
        Err(AppError::ValidationFailed(mut errs)) => errors.append(&mut errs),
        Err(e) => return Err(e),
    }

    // Validate source_di_id resolves if provided
    if let Some(di_id) = input.source_di_id {
        let di_exists = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM intervention_requests WHERE id = ?",
                [di_id.into()],
            ))
            .await?;
        if di_exists.is_none() {
            errors.push(format!(
                "DI introuvable (source_di_id={di_id})."
            ));
        }
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    let wo = queries::create_work_order(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "created".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order created".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

#[tauri::command]
pub async fn list_work_order_types(
    state: State<'_, AppState>,
) -> AppResult<Vec<types::WorkOrderTypeOption>> {
    let user = require_session!(state);
    let has_ref_view = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "ref.view",
        &PermissionScope::Global,
    )
    .await?;
    let has_ot_view = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "ot.view",
        &PermissionScope::Global,
    )
    .await?;
    if !has_ref_view && !has_ot_view {
        require_permission!(state, &user, "ot.create", PermissionScope::Global);
    }
    types::list_work_order_types(&state.db).await
}

#[tauri::command]
pub async fn create_work_order_type(
    input: types::CreateWorkOrderTypeInput,
    state: State<'_, AppState>,
) -> AppResult<types::WorkOrderTypeOption> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    types::create_work_order_type(&state.db, input).await
}

#[tauri::command]
pub async fn update_work_order_type(
    id: i64,
    input: types::UpdateWorkOrderTypeInput,
    state: State<'_, AppState>,
) -> AppResult<types::WorkOrderTypeOption> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    types::update_work_order_type(&state.db, id, input).await
}

#[tauri::command]
pub async fn delete_work_order_type(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    types::delete_work_order_type(&state.db, id).await
}

#[tauri::command]
pub async fn list_work_order_priorities(
    state: State<'_, AppState>,
) -> AppResult<Vec<priorities::WorkOrderPriorityOption>> {
    let user = require_session!(state);
    let has_ref_view = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "ref.view",
        &PermissionScope::Global,
    )
    .await?;
    let has_ot_view = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "ot.view",
        &PermissionScope::Global,
    )
    .await?;
    if !has_ref_view && !has_ot_view {
        require_permission!(state, &user, "ot.create", PermissionScope::Global);
    }
    priorities::list_work_order_priorities(&state.db).await
}

#[tauri::command]
pub async fn update_work_order_priority(
    id: i64,
    input: priorities::UpdateWorkOrderPriorityInput,
    state: State<'_, AppState>,
) -> AppResult<priorities::WorkOrderPriorityOption> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    priorities::update_work_order_priority(&state.db, id, input).await
}

#[tauri::command]
pub async fn list_work_order_statuses(
    state: State<'_, AppState>,
) -> AppResult<Vec<statuses::WorkOrderStatusOption>> {
    let user = require_session!(state);
    let has_ref_view = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "ref.view",
        &PermissionScope::Global,
    )
    .await?;
    let has_ot_view = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "ot.view",
        &PermissionScope::Global,
    )
    .await?;
    if !has_ref_view && !has_ot_view {
        require_permission!(state, &user, "ot.create", PermissionScope::Global);
    }
    statuses::list_work_order_statuses(&state.db).await
}

#[tauri::command]
pub async fn update_work_order_status(
    id: i64,
    input: statuses::UpdateWorkOrderStatusInput,
    state: State<'_, AppState>,
) -> AppResult<statuses::WorkOrderStatusOption> {
    let user = require_session!(state);
    require_permission!(state, &user, "ref.manage", PermissionScope::Global);
    statuses::update_work_order_status(&state.db, id, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) update_wo_draft — requires ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_wo_draft(
    input: crate::wo::domain::WoDraftUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    queries::update_wo_draft_fields(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) cancel_wo — requires ot.edit + step-up if in_progress or later
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn cancel_wo(
    input: crate::wo::domain::WoCancelInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);

    // Check if the WO is in_progress or later — require step-up
    let current = queries::get_work_order(&state.db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.id.to_string(),
        })?;

    let status_code = current.status_code.as_deref().unwrap_or("unknown");
    let status = crate::wo::domain::WoStatus::try_from_str(status_code).map_err(|e| {
        AppError::Internal(anyhow::anyhow!("Stored WO has invalid status: {e}"))
    })?;

    // Step-up required for executing states or completion states
    if status.is_executing()
        || matches!(
            status,
            crate::wo::domain::WoStatus::MechanicallyComplete
                | crate::wo::domain::WoStatus::TechnicallyVerified
        )
    {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_wo_change_event(&state.db, audit::WoAuditInput {
                wo_id: Some(input.id),
                action: "cancelled".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Cancellation blocked: step-up verification failed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "blocked".into(),
            }).await;
            return Err(AppError::StepUpRequired);
        }
    }

    let wo = queries::cancel_work_order(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "cancelled".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order cancelled".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) plan_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn plan_wo(
    input: execution::WoPlanInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo = execution::plan_wo(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "planned".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order planned".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) assign_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn assign_wo(
    input: execution::WoAssignInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo = execution::assign_wo(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "assigned".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order assigned".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// H) start_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn start_wo(
    input: execution::WoStartInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo = execution::start_wo(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "started".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order started".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// I) pause_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn pause_wo(
    input: execution::WoPauseInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo = execution::pause_wo(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "paused".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order paused".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// J) resume_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn resume_wo(
    input: execution::WoResumeInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo = execution::resume_wo(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "resumed".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order resumed".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// K) hold_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn hold_wo(
    input: execution::WoHoldInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo = execution::set_waiting_for_prerequisite(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "held".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order put on hold".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// L) complete_wo_mechanically — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn complete_wo_mechanically(
    input: execution::WoMechCompleteInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo = execution::complete_wo_mechanically(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "mechanically_completed".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order mechanically completed".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// M) add_labor — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn add_labor(
    input: labor::AddLaborInput,
    state: State<'_, AppState>,
) -> AppResult<labor::WoIntervener> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    labor::add_labor_entry(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// N) close_labor — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_labor(
    intervener_id: i64,
    ended_at: String,
    actor_id: i64,
    state: State<'_, AppState>,
) -> AppResult<labor::WoIntervener> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    labor::close_labor_entry(&state.db, intervener_id, ended_at, actor_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// O) list_labor — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_labor(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<labor::WoIntervener>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    labor::list_labor_entries(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// P) add_part — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn add_part(
    input: parts::AddPartInput,
    state: State<'_, AppState>,
) -> AppResult<parts::WoPart> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    parts::add_planned_part(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Q) record_part_usage — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn record_part_usage(
    wo_part_id: i64,
    quantity_used: f64,
    unit_cost: Option<f64>,
    state: State<'_, AppState>,
) -> AppResult<parts::WoPart> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    parts::record_actual_usage(&state.db, wo_part_id, quantity_used, unit_cost).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// R) confirm_no_parts — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn confirm_no_parts(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    parts::confirm_no_parts_used(&state.db, wo_id, user.user_id.into()).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// R2) list_wo_parts — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo_parts(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<parts::WoPart>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    parts::list_wo_parts(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// S) add_task — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn add_task(
    input: tasks::AddTaskInput,
    state: State<'_, AppState>,
) -> AppResult<tasks::WoTask> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    tasks::add_task(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// T) complete_task — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn complete_task(
    task_id: i64,
    actor_id: i64,
    result_code: String,
    notes: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<tasks::WoTask> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    tasks::complete_task(&state.db, task_id, actor_id, result_code, notes).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// U) list_tasks — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_tasks(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<tasks::WoTask>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    tasks::list_tasks(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// V) open_downtime — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn open_downtime(
    wo_id: i64,
    downtime_type: String,
    comment: Option<String>,
    actor_id: i64,
    state: State<'_, AppState>,
) -> AppResult<delay::WoDowntimeSegment> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    delay::open_downtime_segment(
        &state.db,
        delay::OpenDowntimeInput {
            wo_id,
            downtime_type,
            comment,
            actor_id,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// W) close_downtime — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_downtime(
    segment_id: i64,
    ended_at: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<delay::WoDowntimeSegment> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    delay::close_downtime_segment(&state.db, segment_id, ended_at).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// X) list_delay_segments — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_delay_segments(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<delay::WoDelaySegment>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    delay::list_delay_segments(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Y) list_downtime_segments — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_downtime_segments(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<delay::WoDowntimeSegment>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    delay::list_downtime_segments(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// File 03 — Close-Out, Verification, Cost, Attachments, Analytics
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Z) save_failure_detail — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn save_failure_detail(
    input: closeout::SaveFailureDetailInput,
    state: State<'_, AppState>,
) -> AppResult<closeout::WoFailureDetail> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo_id = input.wo_id;
    let detail = closeout::save_failure_detail(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo_id),
        action: "failure_detail_saved".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Failure detail recorded".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(detail)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AA) save_verification — ot.edit + step-up
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn save_verification(
    input: closeout::SaveVerificationInput,
    state: State<'_, AppState>,
) -> AppResult<(closeout::WoVerification, crate::wo::domain::WorkOrder)> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);

    // Manual step-up check to record blocked event
    {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_wo_change_event(&state.db, audit::WoAuditInput {
                wo_id: Some(input.wo_id),
                action: "verification_saved".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Verification blocked: step-up verification failed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "blocked".into(),
            }).await;
            return Err(AppError::StepUpRequired);
        }
    }

    let result = closeout::save_verification(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(result.1.id),
        action: "verification_saved".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Technical verification recorded".into()),
        details_json: None,
        requires_step_up: true,
        apply_result: "applied".into(),
    }).await;
    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AB) close_wo — ot.edit + step-up
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_wo(
    input: closeout::WoCloseInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);

    // Manual step-up check to record blocked event
    {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_wo_change_event(&state.db, audit::WoAuditInput {
                wo_id: Some(input.wo_id),
                action: "closed".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Close blocked: step-up verification failed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "blocked".into(),
            }).await;
            return Err(AppError::StepUpRequired);
        }
    }

    match closeout::close_wo(&state.db, input).await {
        Ok(wo) => {
            audit::record_wo_change_event(&state.db, audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "closed".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Work order closed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            }).await;
            Ok(wo)
        }
        Err(AppError::ValidationFailed(ref errs)) => {
            let details = serde_json::json!({ "quality_gate_errors": errs }).to_string();
            audit::record_wo_change_event(&state.db, audit::WoAuditInput {
                wo_id: None,
                action: "closed".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Close blocked: quality gate failed".into()),
                details_json: Some(details),
                requires_step_up: true,
                apply_result: "blocked".into(),
            }).await;
            Err(AppError::ValidationFailed(errs.clone()))
        }
        Err(other) => Err(other),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AC) reopen_wo — ot.admin + step-up
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn reopen_wo(
    input: closeout::WoReopenInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.admin", PermissionScope::Global);

    // Manual step-up check to record blocked event
    {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_wo_change_event(&state.db, audit::WoAuditInput {
                wo_id: Some(input.wo_id),
                action: "reopened".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Reopen blocked: step-up verification failed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "blocked".into(),
            }).await;
            return Err(AppError::StepUpRequired);
        }
    }

    let wo = closeout::reopen_wo(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo.id),
        action: "reopened".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Work order reopened".into()),
        details_json: None,
        requires_step_up: true,
        apply_result: "applied".into(),
    }).await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AC-2) update_wo_rca — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_wo_rca(
    input: closeout::UpdateWoRcaInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    let wo_id = input.wo_id;
    closeout::update_wo_rca(&state.db, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo_id),
        action: "rca_updated".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Root cause analysis updated".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// AD) upload_wo_attachment — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn upload_wo_attachment(
    app: tauri::AppHandle,
    wo_id: i64,
    file_name: String,
    file_bytes: Vec<u8>,
    mime_type: String,
    notes: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<attachments::WoAttachment> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;

    let input = attachments::WoAttachmentInput {
        wo_id,
        file_name,
        file_bytes,
        mime_type,
        notes,
        uploaded_by_id: i64::from(user.user_id),
    };

    let attachment = attachments::save_wo_attachment(&state.db, &app_data_dir, input).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo_id),
        action: "attachment_uploaded".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Attachment uploaded".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(attachment)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AE) list_wo_attachments — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo_attachments(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<attachments::WoAttachment>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    attachments::list_wo_attachments(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AF) delete_wo_attachment — ot.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn delete_wo_attachment(
    attachment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.admin", PermissionScope::Global);
    attachments::delete_wo_attachment_record(&state.db, attachment_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AG) get_cost_summary — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_cost_summary(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<costs::WoCostSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    costs::get_cost_summary(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AH) update_service_cost — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_service_cost(
    wo_id: i64,
    service_cost: f64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.edit", PermissionScope::Global);
    costs::update_service_cost(&state.db, wo_id, service_cost, i64::from(user.user_id)).await?;
    audit::record_wo_change_event(&state.db, audit::WoAuditInput {
        wo_id: Some(wo_id),
        action: "service_cost_updated".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some(format!("Service cost updated to {service_cost}")),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// AI) get_cost_posting_hook — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_cost_posting_hook(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<costs::CostPostingHook> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    costs::get_cost_posting_hook(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AJ) get_wo_analytics_snapshot — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_wo_analytics_snapshot(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<analytics::WoAnalyticsSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    analytics::get_wo_analytics_snapshot(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AK) get_wo_stats — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_wo_stats(
    state: State<'_, AppState>,
) -> AppResult<stats::WoStatsPayload> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    stats::get_wo_stats(&state.db).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AL) list_wo_change_events — ot.view (per-WO audit timeline)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo_change_events(
    wo_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::WoChangeEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.view", PermissionScope::Global);
    audit::list_wo_change_events(&state.db, wo_id, limit.unwrap_or(50)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AM) list_all_wo_change_events — ot.admin (global audit log)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_all_wo_change_events(
    filter: audit::WoAuditFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::WoChangeEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ot.admin", PermissionScope::Global);
    audit::list_all_wo_change_events(&state.db, filter).await
}
