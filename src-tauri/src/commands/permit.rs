//! Work permit IPC (PRD §6.23).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::permit::domain::{
    LotoCardPrintInput, LotoCardPrintJob, LotoCardView, PermitComplianceKpi30d, PermitHandoverLog,
    PermitHandoverLogInput, PermitIsolation, PermitIsolationActionInput, PermitIsolationUpsertInput,
    PermitSuspendInput, PermitSuspension, PermitType, PermitTypeUpsertInput, WorkPermit, WorkPermitCreateInput,
    WorkPermitListFilter, WorkPermitStatusInput, WorkPermitUpdateInput,
};
use crate::permit::queries;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_permit_types(state: State<'_, AppState>) -> AppResult<Vec<PermitType>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::list_permit_types(&state.db).await
}

#[tauri::command]
pub async fn upsert_permit_type(input: PermitTypeUpsertInput, state: State<'_, AppState>) -> AppResult<PermitType> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    // Also the Données de référence catalog `PERMIT.TYPES`: one write path, same rules.
    crate::reference::audience::assert_module_catalog_editable(
        crate::reference::audience::MODULE_CATALOG_PERMIT_TYPES,
    )?;
    // Switching the energy-isolation rule off weakens the start gate: administrator + step-up
    // (the query refuses it otherwise, needs a reason and audits it; the LOTO type stays locked).
    let mut may_relax_isolation = false;
    if let Some(id) = input.id {
        if let Some(current) = queries::get_permit_type(&state.db, id).await? {
            if queries::relaxes_energy_isolation(&current, &input) {
                require_permission!(
                    state,
                    &user,
                    crate::rbac::permissions::ADM_SETTINGS,
                    PermissionScope::Global
                );
                crate::require_step_up!(state);
                may_relax_isolation = true;
            }
        }
    }
    queries::upsert_permit_type(
        &state.db,
        input,
        queries::PermitTypeWriter {
            actor_id: i64::from(user.user_id),
            may_relax_isolation,
        },
    )
    .await
}

#[tauri::command]
pub async fn list_work_permits(filter: WorkPermitListFilter, state: State<'_, AppState>) -> AppResult<Vec<WorkPermit>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::list_work_permits(&state.db, filter).await
}

#[tauri::command]
pub async fn get_work_permit(id: i64, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::get_work_permit(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: id.to_string(),
        })
}

#[tauri::command]
pub async fn create_work_permit(input: WorkPermitCreateInput, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    queries::create_work_permit(&state.db, input).await
}

#[tauri::command]
pub async fn update_work_permit(input: WorkPermitUpdateInput, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    queries::update_work_permit(&state.db, input).await
}

#[tauri::command]
pub async fn set_work_permit_status(input: WorkPermitStatusInput, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    match input.status.as_str() {
        "closed" | "handed_back" | "cancelled" => {
            require_permission!(
                state,
                &user,
                crate::rbac::permissions::PTW_CLOSE,
                PermissionScope::Global
            );
        }
        _ => {
            require_permission!(
                state,
                &user,
                crate::rbac::permissions::PTW_ISSUE,
                PermissionScope::Global
            );
        }
    }
    queries::set_work_permit_status(&state.db, input, i64::from(user.user_id)).await
}

/// Choices of the isolation form (energy types, the plant's isolation methods). Read with the
/// permit view right: a technician recording a LOTO needs no reference-data permission.
#[tauri::command]
pub async fn list_permit_isolation_options(
    state: State<'_, AppState>,
) -> AppResult<crate::permit::energy_reference::IsolationOptions> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    crate::permit::energy_reference::list_isolation_options(&state.db).await
}

#[tauri::command]
pub async fn list_permit_isolations(permit_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PermitIsolation>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::list_permit_isolations(&state.db, permit_id).await
}

#[tauri::command]
pub async fn upsert_permit_isolation(
    input: PermitIsolationUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    queries::upsert_permit_isolation(&state.db, input).await
}

/// Records the session user as the person who applied the isolation (lock fitted).
#[tauri::command]
pub async fn apply_permit_isolation(
    input: PermitIsolationActionInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    queries::apply_permit_isolation(&state.db, input, i64::from(user.user_id)).await
}

/// Records the session user as verifier; rejected when they also applied it (two-person rule).
#[tauri::command]
pub async fn verify_permit_isolation(
    input: PermitIsolationActionInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    queries::verify_permit_isolation(&state.db, input, i64::from(user.user_id)).await
}

/// Records verified lock removal by the session user (part of closing a permit).
#[tauri::command]
pub async fn verify_permit_isolation_removal(
    input: PermitIsolationActionInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_CLOSE,
        PermissionScope::Global
    );
    queries::verify_permit_isolation_removal(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn suspend_work_permit(
    input: PermitSuspendInput,
    state: State<'_, AppState>,
) -> AppResult<(WorkPermit, PermitSuspension)> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    queries::suspend_work_permit(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn append_permit_handover_log(
    input: PermitHandoverLogInput,
    state: State<'_, AppState>,
) -> AppResult<PermitHandoverLog> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_CLOSE,
        PermissionScope::Global
    );
    queries::append_permit_handover_log(&state.db, input).await
}

#[tauri::command]
pub async fn list_permit_suspensions(permit_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PermitSuspension>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::list_permit_suspensions(&state.db, permit_id).await
}

#[tauri::command]
pub async fn list_permit_handover_logs(
    permit_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PermitHandoverLog>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::list_permit_handover_logs(&state.db, permit_id).await
}

#[tauri::command]
pub async fn get_loto_card_view(
    permit_id: i64,
    isolation_id: i64,
    state: State<'_, AppState>,
) -> AppResult<LotoCardView> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::get_loto_card_view(&state.db, permit_id, isolation_id).await
}

#[tauri::command]
pub async fn record_loto_card_print(
    input: LotoCardPrintInput,
    state: State<'_, AppState>,
) -> AppResult<LotoCardPrintJob> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    queries::record_loto_card_print(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn list_open_permits_report(state: State<'_, AppState>) -> AppResult<Vec<WorkPermit>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::list_open_permits_report(&state.db).await
}

#[tauri::command]
pub async fn permit_compliance_kpi_30d(state: State<'_, AppState>) -> AppResult<PermitComplianceKpi30d> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::permit_compliance_kpi_30d(&state.db).await
}
