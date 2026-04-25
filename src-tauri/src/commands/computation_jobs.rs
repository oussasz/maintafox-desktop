//! Async computation job IPC (Phase 5 — job orchestration).

use tauri::{AppHandle, State};

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::jobs::domain::ComputationJob;
use crate::jobs::queries;
use crate::reliability::domain::RefreshReliabilityKpiSnapshotInput;
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn submit_reliability_kpi_computation_job(
    input: RefreshReliabilityKpiSnapshotInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<i64> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    state
        .computation_jobs
        .spawn_reliability_kpi_refresh(state.db.clone(), app, input)
        .await
}

#[tauri::command]
pub async fn cancel_computation_job(job_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.manage", PermissionScope::Global);
    state.computation_jobs.cancel_job(job_id).await;
    Ok(())
}

#[tauri::command]
pub async fn get_computation_job(job_id: i64, state: State<'_, AppState>) -> AppResult<Option<ComputationJob>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::get_computation_job(&state.db, job_id).await
}

#[tauri::command]
pub async fn list_computation_jobs(limit: Option<i64>, state: State<'_, AppState>) -> AppResult<Vec<ComputationJob>> {
    let user = require_session!(state);
    require_permission!(state, &user, "ram.view", PermissionScope::Global);
    queries::list_computation_jobs(&state.db, limit.unwrap_or(50)).await
}
