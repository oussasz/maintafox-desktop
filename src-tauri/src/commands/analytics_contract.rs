//! Frozen analytics contract registry (gap 06 sprint 03).

use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use crate::analytics_contract::{
    get_contract_version_by_id, insert_contract_version, list_contract_versions,
    stage_analytics_contract_version_sync, AnalyticsContractVersionRow,
};
use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterAnalyticsContractVersionInput {
    pub contract_id: String,
    pub version_semver: String,
    pub content_sha256: String,
}

#[tauri::command]
pub async fn list_analytics_contract_versions(
    state: State<'_, AppState>,
) -> AppResult<Vec<AnalyticsContractVersionRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "sync.manage", PermissionScope::Global);
    require_permission!(state, &user, "integrity.repair", PermissionScope::Global);
    list_contract_versions(&state.db).await
}

#[tauri::command]
pub async fn register_analytics_contract_version(
    input: RegisterAnalyticsContractVersionInput,
    state: State<'_, AppState>,
) -> AppResult<AnalyticsContractVersionRow> {
    let u = require_session!(state);
    require_permission!(state, &u, "sync.manage", PermissionScope::Global);
    require_step_up!(state);

    if input.contract_id.trim().is_empty()
        || input.version_semver.trim().is_empty()
        || input.content_sha256.len() != 64
    {
        return Err(crate::errors::AppError::ValidationFailed(vec![
            "contract_id, version_semver, and 64-char content_sha256 required".into(),
        ]));
    }

    let esid = format!("{}:{}", input.contract_id.trim(), Uuid::new_v4());
    let id = insert_contract_version(
        &state.db,
        &esid,
        input.contract_id.trim(),
        input.version_semver.trim(),
        input.content_sha256.trim(),
    )
    .await?;
    let row = get_contract_version_by_id(&state.db, id)
        .await?
        .ok_or_else(|| crate::errors::AppError::Internal(anyhow::anyhow!("contract version row")))?;
    stage_analytics_contract_version_sync(&state.db, &row).await?;
    Ok(row)
}
