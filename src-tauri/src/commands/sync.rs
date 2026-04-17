use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::state::AppState;
use crate::sync::domain::{
    ApplySyncBatchInput, ApplySyncBatchResult, ExecuteSyncRepairInput, ListOutboxFilter, ReplaySyncFailuresInput,
    ReplaySyncFailuresResult, ResolveSyncConflictInput, StageOutboxItemInput, SyncConflictFilter, SyncConflictRecord,
    SyncInboxItem, SyncObservabilityReport, SyncOutboxItem, SyncPushPayload, SyncReplayRun, SyncRepairActionRecord,
    SyncRepairExecutionResult, SyncRepairPreview, SyncRepairPreviewInput, SyncStateSummary,
};
use crate::sync::queries;
use crate::{require_permission, require_session, require_step_up};

#[tauri::command]
pub async fn stage_outbox_item(
    input: StageOutboxItemInput,
    state: State<'_, AppState>,
) -> AppResult<SyncOutboxItem> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.manage", PermissionScope::Global);
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
pub async fn list_outbox_items(
    filter: ListOutboxFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<SyncOutboxItem>> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
    queries::list_outbox_items(&state.db, filter).await
}

#[tauri::command]
pub async fn get_sync_push_payload(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<SyncPushPayload> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
    queries::get_sync_push_payload(&state.db, limit).await
}

#[tauri::command]
pub async fn apply_sync_batch(
    input: ApplySyncBatchInput,
    state: State<'_, AppState>,
) -> AppResult<ApplySyncBatchResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.manage", PermissionScope::Global);
    let result = queries::apply_sync_batch(&state.db, input).await?;
    tracing::info!(
        event = "desktop_sync_apply_batch",
        user_id = user.user_id,
        acknowledged = result.acknowledged_count,
        inbound_applied = result.inbound_applied_count,
        rejected = result.rejected_count,
        checkpoint = result.checkpoint_token.as_deref().unwrap_or(""),
        "Sync batch applied"
    );
    Ok(result)
}

#[tauri::command]
pub async fn list_inbox_items(
    apply_status: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SyncInboxItem>> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
    queries::list_inbox_items(&state.db, apply_status, limit).await
}

#[tauri::command]
pub async fn get_sync_state_summary(state: State<'_, AppState>) -> AppResult<SyncStateSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
    queries::get_sync_state_summary(&state.db).await
}

#[tauri::command]
pub async fn list_sync_conflicts(
    filter: SyncConflictFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<SyncConflictRecord>> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
    queries::list_sync_conflicts(&state.db, filter).await
}

#[tauri::command]
pub async fn resolve_sync_conflict(
    input: ResolveSyncConflictInput,
    state: State<'_, AppState>,
) -> AppResult<SyncConflictRecord> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.resolve", PermissionScope::Global);
    queries::resolve_sync_conflict(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn replay_sync_failures(
    input: ReplaySyncFailuresInput,
    state: State<'_, AppState>,
) -> AppResult<ReplaySyncFailuresResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.replay", PermissionScope::Global);
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
pub async fn list_sync_replay_runs(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SyncReplayRun>> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
    queries::list_sync_replay_runs(&state.db, limit).await
}

#[tauri::command]
pub async fn preview_sync_repair(
    input: SyncRepairPreviewInput,
    state: State<'_, AppState>,
) -> AppResult<SyncRepairPreview> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.repair", PermissionScope::Global);
    require_step_up!(state);
    queries::preview_sync_repair(&state.db, i64::from(user.user_id), input).await
}

#[tauri::command]
pub async fn execute_sync_repair(
    input: ExecuteSyncRepairInput,
    state: State<'_, AppState>,
) -> AppResult<SyncRepairExecutionResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.repair", PermissionScope::Global);
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
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
    queries::list_sync_repair_actions(&state.db, limit).await
}

#[tauri::command]
pub async fn get_sync_observability_report(
    state: State<'_, AppState>,
) -> AppResult<SyncObservabilityReport> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.view", PermissionScope::Global);
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
