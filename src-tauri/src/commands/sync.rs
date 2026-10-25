use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::state::AppState;
use crate::sync::domain::{
    ExecuteSyncRepairInput, ListOutboxFilter, ReplaySyncFailuresInput, ReplaySyncFailuresResult,
    ResolveSyncConflictInput, StageOutboxItemInput, SyncConflictFilter, SyncConflictRecord, SyncInboxItem,
    SyncObservabilityReport, SyncOutboxItem, SyncRepairActionRecord, SyncRepairExecutionResult, SyncRepairPreview,
    SyncRepairPreviewInput, SyncReplayRun, SyncStateSummary,
};
use crate::sync::queries;
use crate::sync::transport::{self, SyncCommand, SyncWorkerStatus};
use crate::{require_permission_allowing_system_admin, require_session, require_step_up};

#[tauri::command]
pub async fn stage_outbox_item(input: StageOutboxItemInput, state: State<'_, AppState>) -> AppResult<SyncOutboxItem> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    let result = queries::stage_outbox_item(&state.db, input).await?;
    tracing::info!(
        event = "desktop_sync_stage_outbox",
        user_id = user.user_id,
        outbox_id = result.id,
        status = result.status.as_str(),
        "Outbox item staged"
    );
    Ok(result)
}

#[tauri::command]
pub async fn list_outbox_items(filter: ListOutboxFilter, state: State<'_, AppState>) -> AppResult<Vec<SyncOutboxItem>> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    queries::list_outbox_items(&state.db, filter).await
}

#[tauri::command]
pub async fn list_inbox_items(
    apply_status: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SyncInboxItem>> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    queries::list_inbox_items(&state.db, apply_status, limit).await
}

/// The received rows that wait, each with what it waits for, and every waiting row counted by
/// that (`sync::waiting`).
#[tauri::command]
pub async fn list_waiting_inbox_rows(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<crate::sync::waiting::WaitingInboxReport> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    crate::sync::waiting::list_waiting_inbox_rows(&state.db, limit).await
}

#[tauri::command]
pub async fn get_sync_state_summary(state: State<'_, AppState>) -> AppResult<SyncStateSummary> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    queries::get_sync_state_summary(&state.db).await
}

#[tauri::command]
pub async fn list_sync_conflicts(
    filter: SyncConflictFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<SyncConflictRecord>> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    queries::list_sync_conflicts(&state.db, filter).await
}

#[tauri::command]
pub async fn resolve_sync_conflict(
    input: ResolveSyncConflictInput,
    state: State<'_, AppState>,
) -> AppResult<SyncConflictRecord> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_RESOLVE,
        PermissionScope::Global
    );
    queries::resolve_sync_conflict(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn replay_sync_failures(
    input: ReplaySyncFailuresInput,
    state: State<'_, AppState>,
) -> AppResult<ReplaySyncFailuresResult> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_REPLAY,
        PermissionScope::Global
    );
    let result = queries::replay_sync_failures(&state.db, i64::from(user.user_id), input).await?;
    tracing::info!(
        event = "desktop_sync_replay_failures",
        user_id = user.user_id,
        requeued_outbox = result.requeued_outbox_count,
        transitioned_conflicts = result.transitioned_conflict_count,
        "Sync replay run completed"
    );
    Ok(result)
}

#[tauri::command]
pub async fn list_sync_replay_runs(limit: Option<i64>, state: State<'_, AppState>) -> AppResult<Vec<SyncReplayRun>> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    queries::list_sync_replay_runs(&state.db, limit).await
}

#[tauri::command]
pub async fn preview_sync_repair(
    input: SyncRepairPreviewInput,
    state: State<'_, AppState>,
) -> AppResult<SyncRepairPreview> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_REPAIR,
        PermissionScope::Global
    );
    require_step_up!(state);
    queries::preview_sync_repair(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn execute_sync_repair(
    input: ExecuteSyncRepairInput,
    state: State<'_, AppState>,
) -> AppResult<SyncRepairExecutionResult> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_REPAIR,
        PermissionScope::Global
    );
    require_step_up!(state);
    let result = queries::execute_sync_repair(&state.db, i64::from(user.user_id), input).await?;
    tracing::info!(
        event = "desktop_sync_execute_repair",
        user_id = user.user_id,
        mode = result.mode.as_str(),
        requeued_outbox = result.requeued_outbox_count,
        transitioned_conflicts = result.transitioned_conflict_count,
        plan_id = result.plan_id.as_str(),
        status = result.status.as_str(),
        "Sync repair executed"
    );
    Ok(result)
}

#[tauri::command]
pub async fn list_sync_repair_actions(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SyncRepairActionRecord>> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    queries::list_sync_repair_actions(&state.db, limit).await
}

#[tauri::command]
pub async fn get_sync_observability_report(state: State<'_, AppState>) -> AppResult<SyncObservabilityReport> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    let result = queries::get_sync_observability_report(&state.db).await?;
    tracing::info!(
        event = "desktop_sync_observability_report",
        user_id = user.user_id,
        pending_outbox_count = result.metrics.pending_outbox_count,
        rejected_outbox_count = result.metrics.rejected_outbox_count,
        unresolved_conflict_count = result.metrics.unresolved_conflict_count,
        "Sync observability report generated"
    );
    Ok(result)
}

// ── Sync worker (protocol v2 transport, background task `sync`) ─────────────────────────────

/// Current worker status. Changes are also pushed as the `sync-worker-status` event.
#[tauri::command]
pub async fn get_sync_worker_status(state: State<'_, AppState>) -> AppResult<SyncWorkerStatus> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    Ok(state.sync_worker.status())
}

/// Sync now (manual run): same permission as the former manual exchange (`sync.view`).
#[tauri::command]
pub async fn run_sync_now(state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_VIEW,
        PermissionScope::Global
    );
    state.sync_worker.send(SyncCommand::RunNow);
    tracing::info!(
        event = "desktop_sync_run_now",
        user_id = user.user_id,
        "Sync requested by operator"
    );
    Ok(())
}

/// Pause sync for this device for `minutes` (1..=1440). Returns the end of the pause.
#[tauri::command]
pub async fn pause_sync(minutes: u32, state: State<'_, AppState>) -> AppResult<String> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    let until = transport::pause_sync(&state.db, &state.sync_worker, minutes).await?;
    tracing::info!(
        event = "desktop_sync_paused",
        user_id = user.user_id,
        until = until.as_str(),
        "Sync paused"
    );
    Ok(until)
}

/// End a pause and sync now.
#[tauri::command]
pub async fn resume_sync(state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission_allowing_system_admin!(
        state,
        &user,
        crate::rbac::permissions::SYNC_MANAGE,
        PermissionScope::Global
    );
    transport::resume_sync(&state.db, &state.sync_worker).await?;
    tracing::info!(event = "desktop_sync_resumed", user_id = user.user_id, "Sync resumed");
    Ok(())
}

/// The webview saw the network come back (`online` event). Only wakes an offline worker (never
/// sooner than `SYNC_NETWORK_WAKE_MIN_SPACING` after its last attempt); carries no data, so it needs
/// no session (the window may be locked).
#[tauri::command]
pub async fn report_network_online(state: State<'_, AppState>) -> AppResult<()> {
    state.sync_worker.send(SyncCommand::NetworkOnline);
    Ok(())
}

/// Most file states asked for in one call (an attachment list or a photo gallery).
const MAX_MEDIA_FILE_STATES: usize = 500;

fn media_root(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))
}

/// Whether the files of attachment rows are on this device (`available`), still coming from
/// another device (`pending`), given up (`failed`, with the reason) or unknown (`missing`). Only a
/// session is needed: the answer names no content, and every list that shows the paths already
/// checked the module permission.
#[tauri::command]
pub async fn get_media_file_states(
    relative_paths: Vec<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::sync::media::MediaFileState>> {
    let _user = require_session!(state);
    if relative_paths.len() > MAX_MEDIA_FILE_STATES {
        return Err(crate::errors::AppError::ValidationFailed(vec![format!(
            "GATE_CMD_SYNC_MEDIA_BATCH_LARGE:max={MAX_MEDIA_FILE_STATES}: At most {MAX_MEDIA_FILE_STATES} files can be checked at once."
        )]));
    }
    crate::sync::media::media_file_states(&state.db, &media_root(&app)?, &relative_paths).await
}

/// Queue the download of a file received from another device again (after it failed), then wake
/// the sync worker.
#[tauri::command]
pub async fn retry_media_download(relative_path: String, state: State<'_, AppState>) -> AppResult<u64> {
    let user = require_session!(state);
    let queued = crate::sync::media::retry_media_download(&state.db, &relative_path).await?;
    if queued > 0 {
        state.sync_worker.send(SyncCommand::RunNow);
    }
    tracing::info!(
        event = "desktop_sync_media_download_retry",
        user_id = user.user_id,
        queued,
        "Media download retry requested"
    );
    Ok(queued)
}
