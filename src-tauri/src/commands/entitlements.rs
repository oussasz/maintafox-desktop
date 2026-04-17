use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::entitlements::domain::{
    EntitlementCapabilityCheck, EntitlementDiagnostics, EntitlementRefreshResult, EntitlementSummary,
    EntitlementEnvelopeInput,
};
use crate::entitlements::queries;
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[tauri::command]
pub async fn apply_entitlement_envelope(
    input: EntitlementEnvelopeInput,
    state: State<'_, AppState>,
) -> AppResult<EntitlementRefreshResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "ent.manage", PermissionScope::Global);
    require_step_up!(state);
    queries::apply_entitlement_envelope(&state.db, input).await
}

#[tauri::command]
pub async fn get_entitlement_summary(state: State<'_, AppState>) -> AppResult<EntitlementSummary> {
    let _user = require_session!(state);
    queries::get_entitlement_summary(&state.db).await
}

#[tauri::command]
pub async fn check_entitlement_capability(
    capability: String,
    state: State<'_, AppState>,
) -> AppResult<EntitlementCapabilityCheck> {
    let _user = require_session!(state);
    queries::check_entitlement_capability(&state.db, capability).await
}

#[tauri::command]
pub async fn get_entitlement_diagnostics(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<EntitlementDiagnostics> {
    let user = require_session!(state);
    require_permission!(state, &user, "ent.view", PermissionScope::Global);
    queries::get_entitlement_diagnostics(&state.db, limit).await
}
