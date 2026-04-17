//! PM delete IPC commands.
//!
//! Restricted to `pm.delete` and guarded by optimistic concurrency checks.

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};
use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn delete_pm_plan_version(
    version_id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "pm.delete", PermissionScope::Global);

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, pm_plan_id, status, row_version FROM pm_plan_versions WHERE id = ?",
            [version_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan_version".to_string(),
            id: version_id.to_string(),
        })?;

    let current_row_version: i64 = row.try_get("", "row_version")?;
    if current_row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "PM version delete failed (stale row_version).".to_string(),
        ]));
    }

    let status: String = row.try_get("", "status")?;
    if status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "Only draft PM plan versions can be deleted.".to_string(),
        ]));
    }

    let linked_occurrence_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences WHERE plan_version_id = ?",
            [version_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm occurrence count missing")))?;
    let linked_occurrence_count: i64 = linked_occurrence_row.try_get("", "cnt")?;
    if linked_occurrence_count > 0 {
        return Err(AppError::ValidationFailed(vec![
            "A PM version with generated occurrences cannot be deleted.".to_string(),
        ]));
    }

    let plan_id: i64 = row.try_get("", "pm_plan_id")?;
    let tx = state.db.begin().await?;

    let delete_result = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM pm_plan_versions WHERE id = ? AND row_version = ?",
            [version_id.into(), expected_row_version.into()],
        ))
        .await?;
    if delete_result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "PM version delete failed.".to_string(),
        ]));
    }

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE pm_plans SET current_version_id = NULL, row_version = row_version + 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ? AND current_version_id = ?",
        [plan_id.into(), version_id.into()],
    ))
    .await?;

    tx.commit().await?;
    Ok(())
}

#[tauri::command]
pub async fn delete_pm_plan(
    plan_id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "pm.delete", PermissionScope::Global);

    let plan_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, lifecycle_status, row_version FROM pm_plans WHERE id = ?",
            [plan_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "pm_plan".to_string(),
            id: plan_id.to_string(),
        })?;

    let current_row_version: i64 = plan_row.try_get("", "row_version")?;
    if current_row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "PM plan delete failed (stale row_version).".to_string(),
        ]));
    }

    let lifecycle_status: String = plan_row.try_get("", "lifecycle_status")?;
    if lifecycle_status != "draft" {
        return Err(AppError::ValidationFailed(vec![
            "Only draft PM plans can be deleted.".to_string(),
        ]));
    }

    let version_count_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM pm_plan_versions WHERE pm_plan_id = ?",
            [plan_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm version count missing")))?;
    let version_count: i64 = version_count_row.try_get("", "cnt")?;
    if version_count > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Delete PM versions first. A PM plan with versions cannot be deleted.".to_string(),
        ]));
    }

    let occurrence_count_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM pm_occurrences WHERE pm_plan_id = ?",
            [plan_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("pm occurrence count missing")))?;
    let occurrence_count: i64 = occurrence_count_row.try_get("", "cnt")?;
    if occurrence_count > 0 {
        return Err(AppError::ValidationFailed(vec![
            "A PM plan with generated occurrences cannot be deleted.".to_string(),
        ]));
    }

    let delete_result = state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM pm_plans WHERE id = ? AND row_version = ? AND lifecycle_status = 'draft'",
            [plan_id.into(), expected_row_version.into()],
        ))
        .await?;
    if delete_result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "PM plan delete failed.".to_string(),
        ]));
    }

    Ok(())
}
