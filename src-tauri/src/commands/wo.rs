//! WO IPC commands.
//!
//! Phase 2 – Sub-phase 05 – File 01 – Sprint S2.
//!
//! Permission gates:
//!   ot.view    — list, get
//!   ot.create  — create work order
//!   ot.edit    — update draft, cancel (step-up if in_progress or later)

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::wo::queries;
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
