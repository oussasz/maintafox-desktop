//! WO IPC commands.
//!
//! Phase 2 – Sub-phase 05 – File 01 Sprint S2 + File 02 Sprint S2.
//!
//! Permission gates:
//!   ot.view    — list, get, list_labor, list_tasks, list_delay_segments, list_downtime_segments
//!   ot.create  — create work order
//!   ot.edit    — update draft, cancel, plan, assign, start, pause, resume, hold,
//!                complete_mechanically, add_labor, close_labor, add_part,
//!                record_part_usage, confirm_no_parts, add_task, complete_task,
//!                open_downtime, close_downtime

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::wo::{delay, execution, labor, parts, queries, tasks};
use crate::{require_permission, require_session, require_step_up};

use sea_orm::{ConnectionTrait, DbBackend, Statement};

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

    // Validate type_id resolves
    let type_exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_order_types WHERE id = ?",
            [input.type_id.into()],
        ))
        .await?;
    if type_exists.is_none() {
        errors.push(format!(
            "Type d'OT introuvable (type_id={}).",
            input.type_id
        ));
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

    queries::create_work_order(&state.db, input).await
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
        require_step_up!(state);
    }

    queries::cancel_work_order(&state.db, input).await
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
    execution::plan_wo(&state.db, input).await
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
    execution::assign_wo(&state.db, input).await
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
    execution::start_wo(&state.db, input).await
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
    execution::pause_wo(&state.db, input).await
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
    execution::resume_wo(&state.db, input).await
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
    execution::set_waiting_for_prerequisite(&state.db, input).await
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
    execution::complete_wo_mechanically(&state.db, input).await
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
