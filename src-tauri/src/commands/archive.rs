use sea_orm::{ConnectionTrait, DbBackend, Statement, Value};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::archive::integrity;
use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::settings;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

#[derive(Debug, Deserialize)]
pub struct ArchiveFilterInput {
    pub source_module: Option<String>,
    pub archive_class: Option<String>,
    pub legal_hold: Option<bool>,
    pub search_text: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ArchiveItemSummary {
    pub id: i64,
    pub source_module: String,
    pub source_record_id: String,
    pub archive_class: String,
    pub source_state: Option<String>,
    pub archive_reason_code: String,
    pub archived_at: String,
    pub archived_by_id: Option<i64>,
    pub retention_policy_id: Option<i64>,
    pub restore_policy: String,
    pub restore_until_at: Option<String>,
    pub legal_hold: bool,
    pub checksum_sha256: Option<String>,
    pub search_text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ArchivePayloadRow {
    pub id: i64,
    pub archive_item_id: i64,
    pub payload_json: serde_json::Value,
    pub workflow_history_json: Option<String>,
    pub attachment_manifest_json: Option<String>,
    pub config_version_refs_json: Option<String>,
    pub payload_size_bytes: i64,
}

#[derive(Debug, Serialize)]
pub struct ArchiveActionRow {
    pub id: i64,
    pub archive_item_id: i64,
    pub action: String,
    pub action_by_id: Option<i64>,
    pub action_at: String,
    pub reason_note: Option<String>,
    pub result_status: String,
}

#[derive(Debug, Serialize)]
pub struct RetentionPolicy {
    pub id: i64,
    pub module_code: String,
    pub archive_class: String,
    pub retention_years: i64,
    pub purge_mode: String,
    pub allow_restore: bool,
    pub allow_purge: bool,
    pub requires_legal_hold_check: bool,
}

#[derive(Debug, Serialize)]
pub struct ArchiveItemDetail {
    pub item: ArchiveItemSummary,
    pub payload: Option<ArchivePayloadRow>,
    pub actions: Vec<ArchiveActionRow>,
    pub retention_policy: Option<RetentionPolicy>,
    pub checksum_valid: bool,
}

#[derive(Debug, Deserialize)]
pub struct RestoreInput {
    pub archive_item_id: i64,
    pub reason_note: String,
}

#[derive(Debug, Serialize)]
pub struct ArchiveRestoreResult {
    pub archive_item_id: i64,
    pub restore_action_id: i64,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ExportInput {
    pub archive_item_ids: Vec<i64>,
    pub export_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExportedArchivePayload {
    pub archive_item_id: i64,
    pub source_module: String,
    pub source_record_id: String,
    pub archive_class: String,
    pub payload_json: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ExportPayload {
    pub items: Vec<ExportedArchivePayload>,
}

#[derive(Debug, Deserialize)]
pub struct PurgeInput {
    pub archive_item_ids: Vec<i64>,
    pub purge_reason: String,
}

#[derive(Debug, Serialize)]
pub struct PurgeBlockedItem {
    pub archive_item_id: i64,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct PurgeResult {
    pub strict_mode: bool,
    pub purged_item_ids: Vec<i64>,
    pub blocked_items: Vec<PurgeBlockedItem>,
}

#[derive(Debug, Deserialize)]
pub struct LegalHoldInput {
    pub archive_item_id: i64,
    pub enable: bool,
    pub reason_note: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRetentionInput {
    pub policy_id: i64,
    pub retention_years: Option<i64>,
    pub purge_mode: Option<String>,
    pub allow_restore: Option<bool>,
    pub allow_purge: Option<bool>,
    pub requires_legal_hold_check: Option<bool>,
}

#[tauri::command]
pub async fn list_archive_items(
    filter: Option<ArchiveFilterInput>,
    state: State<'_, AppState>,
) -> AppResult<Vec<ArchiveItemSummary>> {
    let user = require_session!(state);
    require_permission!(state, &user, "arc.view", PermissionScope::Global);

    let filter = filter.unwrap_or(ArchiveFilterInput {
        source_module: None,
        archive_class: None,
        legal_hold: None,
        search_text: None,
        date_from: None,
        date_to: None,
        limit: Some(50),
        offset: Some(0),
    });

    let limit = filter.limit.unwrap_or(50).clamp(1, 500);
    let offset = filter.offset.unwrap_or(0).max(0);

    let mut sql = String::from(
        "SELECT
            id, source_module, source_record_id, archive_class, source_state,
            archive_reason_code, archived_at, archived_by_id, retention_policy_id,
            restore_policy, restore_until_at, legal_hold, checksum_sha256, search_text
         FROM archive_items
         WHERE 1=1",
    );
    let mut params: Vec<Value> = Vec::new();

    if let Some(source_module) = filter.source_module {
        sql.push_str(" AND source_module = ?");
        params.push(source_module.into());
    }
    if let Some(archive_class) = filter.archive_class {
        sql.push_str(" AND archive_class = ?");
        params.push(archive_class.into());
    }
    if let Some(legal_hold) = filter.legal_hold {
        sql.push_str(" AND legal_hold = ?");
        params.push((if legal_hold { 1 } else { 0 }).into());
    }
    if let Some(search_text) = filter.search_text {
        sql.push_str(" AND search_text LIKE ?");
        params.push(format!("%{}%", search_text).into());
    }
    if let Some(date_from) = filter.date_from {
        sql.push_str(" AND archived_at >= ?");
        params.push(date_from.into());
    }
    if let Some(date_to) = filter.date_to {
        sql.push_str(" AND archived_at <= ?");
        params.push(date_to.into());
    }

    sql.push_str(" ORDER BY archived_at DESC LIMIT ? OFFSET ?");
    params.push(limit.into());
    params.push(offset.into());

    let rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            params,
        ))
        .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(parse_archive_item_summary(&row));
    }

    Ok(items)
}

#[tauri::command]
pub async fn get_archive_item(archive_item_id: i64, state: State<'_, AppState>) -> AppResult<ArchiveItemDetail> {
    let user = require_session!(state);
    require_permission!(state, &user, "arc.view", PermissionScope::Global);

    let item_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                id, source_module, source_record_id, archive_class, source_state,
                archive_reason_code, archived_at, archived_by_id, retention_policy_id,
                restore_policy, restore_until_at, legal_hold, checksum_sha256, search_text
             FROM archive_items
             WHERE id = ?
             LIMIT 1",
            [archive_item_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "archive_item".to_string(),
            id: archive_item_id.to_string(),
        })?;
    let item = parse_archive_item_summary(&item_row);

    let payload_row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                id, archive_item_id, payload_json_compressed,
                workflow_history_json, attachment_manifest_json, config_version_refs_json, payload_size_bytes
             FROM archive_payloads
             WHERE archive_item_id = ?
             LIMIT 1",
            [archive_item_id.into()],
        ))
        .await?;

    let payload = if let Some(row) = payload_row {
        let payload_bytes = row.try_get::<Vec<u8>>("", "payload_json_compressed")?;
        let payload_json = serde_json::from_slice::<serde_json::Value>(&payload_bytes)?;
        Some(ArchivePayloadRow {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            archive_item_id: row
                .try_get::<i64>("", "archive_item_id")
                .unwrap_or(archive_item_id),
            payload_json,
            workflow_history_json: row
                .try_get::<Option<String>>("", "workflow_history_json")
                .unwrap_or(None),
            attachment_manifest_json: row
                .try_get::<Option<String>>("", "attachment_manifest_json")
                .unwrap_or(None),
            config_version_refs_json: row
                .try_get::<Option<String>>("", "config_version_refs_json")
                .unwrap_or(None),
            payload_size_bytes: row.try_get::<i64>("", "payload_size_bytes").unwrap_or(0),
        })
    } else {
        None
    };

    let action_rows = state
        .db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, archive_item_id, action, action_by_id, action_at, reason_note, result_status
             FROM archive_actions
             WHERE archive_item_id = ?
             ORDER BY action_at DESC, id DESC",
            [archive_item_id.into()],
        ))
        .await?;

    let mut actions = Vec::with_capacity(action_rows.len());
    for row in action_rows {
        actions.push(ArchiveActionRow {
            id: row.try_get::<i64>("", "id").unwrap_or_default(),
            archive_item_id: row
                .try_get::<i64>("", "archive_item_id")
                .unwrap_or(archive_item_id),
            action: row.try_get::<String>("", "action").unwrap_or_default(),
            action_by_id: row.try_get::<Option<i64>>("", "action_by_id").unwrap_or(None),
            action_at: row.try_get::<String>("", "action_at").unwrap_or_default(),
            reason_note: row.try_get::<Option<String>>("", "reason_note").unwrap_or(None),
            result_status: row
                .try_get::<String>("", "result_status")
                .unwrap_or_else(|_| "success".to_string()),
        });
    }

    let retention_policy = if let Some(policy_id) = item.retention_policy_id {
        load_retention_policy_by_id(&state.db, policy_id).await?
    } else {
        None
    };

    let checksum_valid = integrity::verify_checksum(&state.db, archive_item_id).await?;

    Ok(ArchiveItemDetail {
        item,
        payload,
        actions,
        retention_policy,
        checksum_valid,
    })
}

#[tauri::command]
pub async fn restore_archive_item(
    payload: RestoreInput,
    state: State<'_, AppState>,
) -> AppResult<ArchiveRestoreResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "arc.restore", PermissionScope::Global);
    require_step_up!(state);

    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, restore_policy, restore_until_at, legal_hold, retention_policy_id
             FROM archive_items
             WHERE id = ?
             LIMIT 1",
            [payload.archive_item_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "archive_item".to_string(),
            id: payload.archive_item_id.to_string(),
        })?;

    let restore_policy = row
        .try_get::<String>("", "restore_policy")
        .unwrap_or_else(|_| "not_allowed".to_string());
    if !matches!(restore_policy.as_str(), "admin_only" | "until_date") {
        let _ = write_archive_action(
            &state.db,
            payload.archive_item_id,
            "restore",
            Some(i64::from(user.user_id)),
            Some("restore blocked: restore_policy is not allowed"),
            "failed",
        )
        .await;
        emit_archive_activity_event(
            &state.db,
            payload.archive_item_id,
            i64::from(user.user_id),
            "archive.restore_blocked",
            "blocked",
            "restore",
        )
        .await;
        return Err(AppError::PermissionDenied(
            "Restore blocked: restore_policy is not allowed for this item".to_string(),
        ));
    }

    if restore_policy == "until_date" {
        let restore_until_at = row.try_get::<Option<String>>("", "restore_until_at").unwrap_or(None);
        let Some(until) = restore_until_at else {
            let _ = write_archive_action(
                &state.db,
                payload.archive_item_id,
                "restore",
                Some(i64::from(user.user_id)),
                Some("restore blocked: restore_until_at is missing for until_date policy"),
                "failed",
            )
            .await;
            emit_archive_activity_event(
                &state.db,
                payload.archive_item_id,
                i64::from(user.user_id),
                "archive.restore_blocked",
                "blocked",
                "restore",
            )
            .await;
            return Err(AppError::ValidationFailed(vec![
                "Restore blocked: restore_until_at is required for until_date policy".to_string(),
            ]));
        };
        let is_still_valid = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT CASE
                    WHEN datetime(?) >= datetime('now')
                    THEN 1 ELSE 0 END AS is_valid",
                [until.into()],
            ))
            .await?
            .and_then(|r| r.try_get::<i32>("", "is_valid").ok())
            .unwrap_or(0)
            == 1;
        if !is_still_valid {
            let _ = write_archive_action(
                &state.db,
                payload.archive_item_id,
                "restore",
                Some(i64::from(user.user_id)),
                Some("restore blocked: restore window expired"),
                "failed",
            )
            .await;
            emit_archive_activity_event(
                &state.db,
                payload.archive_item_id,
                i64::from(user.user_id),
                "archive.restore_blocked",
                "blocked",
                "restore",
            )
            .await;
            return Err(AppError::PermissionDenied(
                "Restore blocked: restore window has expired".to_string(),
            ));
        }
    }

    let legal_hold = row.try_get::<i32>("", "legal_hold").unwrap_or(0) == 1;
    if legal_hold {
        let _ = write_archive_action(
            &state.db,
            payload.archive_item_id,
            "restore",
            Some(i64::from(user.user_id)),
            Some("restore blocked: legal hold is enabled"),
            "failed",
        )
        .await;
        emit_archive_activity_event(
            &state.db,
            payload.archive_item_id,
            i64::from(user.user_id),
            "archive.restore_blocked",
            "blocked",
            "restore",
        )
        .await;
        return Err(AppError::PermissionDenied(
            "Restore blocked: item is under legal hold".to_string(),
        ));
    }

    let retention_policy_id = row.try_get::<Option<i64>>("", "retention_policy_id").unwrap_or(None);
    if let Some(policy_id) = retention_policy_id {
        if let Some(policy) = load_retention_policy_by_id(&state.db, policy_id).await? {
            if !policy.allow_restore {
                let _ = write_archive_action(
                    &state.db,
                    payload.archive_item_id,
                    "restore",
                    Some(i64::from(user.user_id)),
                    Some("restore blocked: retention policy disallows restore"),
                    "failed",
                )
                .await;
                emit_archive_activity_event(
                    &state.db,
                    payload.archive_item_id,
                    i64::from(user.user_id),
                    "archive.restore_blocked",
                    "blocked",
                    "restore",
                )
                .await;
                return Err(AppError::PermissionDenied(
                    "Restore blocked: retention policy does not allow restore".to_string(),
                ));
            }
        }
    }

    let restore_action_id = write_archive_action(
        &state.db,
        payload.archive_item_id,
        "restore",
        Some(i64::from(user.user_id)),
        Some(&payload.reason_note),
        "success",
    )
    .await?;

    emit_archive_activity_event(
        &state.db,
        payload.archive_item_id,
        i64::from(user.user_id),
        "archive.restore_requested",
        "success",
        "restore",
    )
    .await;

    Ok(ArchiveRestoreResult {
        archive_item_id: payload.archive_item_id,
        restore_action_id,
        message: "Archive restore recorded; module-specific replay is handled in later phases".to_string(),
    })
}

#[tauri::command]
pub async fn export_archive_items(
    payload: ExportInput,
    state: State<'_, AppState>,
) -> AppResult<ExportPayload> {
    let user = require_session!(state);
    require_permission!(state, &user, "arc.export", PermissionScope::Global);
    // Explicitly enforce arc.view too as requested.
    require_permission!(state, &user, "arc.view", PermissionScope::Global);

    if payload.archive_item_ids.is_empty() {
        return Ok(ExportPayload { items: Vec::new() });
    }

    let mut exported = Vec::with_capacity(payload.archive_item_ids.len());
    for archive_item_id in payload.archive_item_ids {
        let row = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT
                    ai.id,
                    ai.source_module,
                    ai.source_record_id,
                    ai.archive_class,
                    ap.payload_json_compressed
                 FROM archive_items ai
                 JOIN archive_payloads ap ON ap.archive_item_id = ai.id
                 WHERE ai.id = ?
                 LIMIT 1",
                [archive_item_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "archive_item".to_string(),
                id: archive_item_id.to_string(),
            })?;

        let payload_bytes = row.try_get::<Vec<u8>>("", "payload_json_compressed")?;
        let payload_json = serde_json::from_slice::<serde_json::Value>(&payload_bytes)?;

        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT INTO archive_actions
                    (archive_item_id, action, action_by_id, reason_note, result_status)
                 VALUES (?, 'export', ?, ?, 'success')",
                [
                    archive_item_id.into(),
                    user.user_id.into(),
                    payload.export_reason.clone().into(),
                ],
            ))
            .await?;

        exported.push(ExportedArchivePayload {
            archive_item_id,
            source_module: row.try_get::<String>("", "source_module").unwrap_or_default(),
            source_record_id: row
                .try_get::<String>("", "source_record_id")
                .unwrap_or_default(),
            archive_class: row.try_get::<String>("", "archive_class").unwrap_or_default(),
            payload_json,
        });
    }

    Ok(ExportPayload { items: exported })
}

#[tauri::command]
pub async fn purge_archive_items(
    payload: PurgeInput,
    state: State<'_, AppState>,
) -> AppResult<PurgeResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "arc.purge", PermissionScope::Global);
    require_step_up!(state);

    let strict_mode = load_archive_purge_strict_mode(&state).await?;

    let mut eligible_ids: Vec<i64> = Vec::new();
    let mut blocked_items: Vec<PurgeBlockedItem> = Vec::new();

    for archive_item_id in payload.archive_item_ids {
        match evaluate_purge_eligibility(&state, archive_item_id).await? {
            None => eligible_ids.push(archive_item_id),
            Some(reason) => {
                let log_reason = format!("purge blocked: {reason}");
                let _ = write_archive_action(
                    &state.db,
                    archive_item_id,
                    "purge",
                    Some(i64::from(user.user_id)),
                    Some(&log_reason),
                    "failed",
                )
                .await;
                emit_archive_activity_event(
                    &state.db,
                    archive_item_id,
                    i64::from(user.user_id),
                    "archive.purge_blocked",
                    "blocked",
                    "purge",
                )
                .await;
                blocked_items.push(PurgeBlockedItem {
                    archive_item_id,
                    reason,
                });
            }
        }
    }

    if strict_mode && !blocked_items.is_empty() {
        return Ok(PurgeResult {
            strict_mode,
            purged_item_ids: Vec::new(),
            blocked_items,
        });
    }

    let mut purged_item_ids = Vec::new();
    for archive_item_id in &eligible_ids {
        write_archive_action(
            &state.db,
            *archive_item_id,
            "purge",
            Some(i64::from(user.user_id)),
            Some(&payload.purge_reason),
            "success",
        )
        .await?;

        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM archive_payloads WHERE archive_item_id = ?",
                [(*archive_item_id).into()],
            ))
            .await?;

        state
            .db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM archive_items WHERE id = ?",
                [(*archive_item_id).into()],
            ))
            .await?;

        emit_archive_activity_event(
            &state.db,
            *archive_item_id,
            i64::from(user.user_id),
            "archive.purged",
            "success",
            "purge",
        )
        .await;

        purged_item_ids.push(*archive_item_id);
    }

    Ok(PurgeResult {
        strict_mode,
        purged_item_ids,
        blocked_items,
    })
}

#[tauri::command]
pub async fn set_legal_hold(payload: LegalHoldInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "arc.purge", PermissionScope::Global);
    require_step_up!(state);

    let update_res = state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE archive_items SET legal_hold = ? WHERE id = ?",
            [
                (if payload.enable { 1 } else { 0 }).into(),
                payload.archive_item_id.into(),
            ],
        ))
        .await?;
    if update_res.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "archive_item".to_string(),
            id: payload.archive_item_id.to_string(),
        });
    }

    let action = if payload.enable {
        "legal_hold_on"
    } else {
        "legal_hold_off"
    };
    let sql = format!(
        "INSERT INTO archive_actions
            (archive_item_id, action, action_by_id, reason_note, result_status)
         VALUES (?, '{}', ?, ?, 'success')",
        action
    );
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                payload.archive_item_id.into(),
                user.user_id.into(),
                payload.reason_note.into(),
            ],
        ))
        .await?;

    Ok(())
}

#[tauri::command]
pub async fn list_retention_policies(state: State<'_, AppState>) -> AppResult<Vec<RetentionPolicy>> {
    let user = require_session!(state);
    require_permission!(state, &user, "arc.view", PermissionScope::Global);

    let rows = state
        .db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                id, module_code, archive_class, retention_years, purge_mode,
                allow_restore, allow_purge, requires_legal_hold_check
             FROM retention_policies
             ORDER BY module_code, archive_class"
                .to_string(),
        ))
        .await?;

    let mut policies = Vec::with_capacity(rows.len());
    for row in rows {
        policies.push(parse_retention_policy(&row));
    }

    Ok(policies)
}

#[tauri::command]
pub async fn update_retention_policy(
    payload: UpdateRetentionInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "adm.settings", PermissionScope::Global);
    require_step_up!(state);

    let existing = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, archive_class, retention_years, purge_mode, allow_restore, allow_purge, requires_legal_hold_check
             FROM retention_policies
             WHERE id = ?
             LIMIT 1",
            [payload.policy_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "retention_policy".to_string(),
            id: payload.policy_id.to_string(),
        })?;

    let current_archive_class = existing
        .try_get::<String>("", "archive_class")
        .unwrap_or_default();
    let current_retention_years = existing.try_get::<i64>("", "retention_years").unwrap_or(7);
    let current_purge_mode = existing
        .try_get::<String>("", "purge_mode")
        .unwrap_or_else(|_| "manual_approval".to_string());
    let current_allow_purge = existing.try_get::<i32>("", "allow_purge").unwrap_or(0) == 1;

    let target_retention_years = payload.retention_years.unwrap_or(current_retention_years);
    let target_purge_mode = payload
        .purge_mode
        .clone()
        .unwrap_or_else(|| current_purge_mode.clone());
    let target_allow_purge = payload.allow_purge.unwrap_or(current_allow_purge);

    if target_allow_purge && target_purge_mode == "never" {
        return Err(AppError::ValidationFailed(vec![
            "Cannot enable allow_purge when purge_mode is 'never'".to_string(),
        ]));
    }
    if current_archive_class == "operational_history" && target_retention_years < current_retention_years {
        return Err(AppError::ValidationFailed(vec![
            "Cannot reduce retention_years for operational_history policies".to_string(),
        ]));
    }

    let mut set_parts: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();

    if let Some(retention_years) = payload.retention_years {
        if retention_years < 0 {
            return Err(AppError::ValidationFailed(vec![
                "retention_years must be >= 0".to_string(),
            ]));
        }
        set_parts.push("retention_years = ?".to_string());
        values.push(retention_years.into());
    }
    if let Some(purge_mode) = payload.purge_mode {
        if !matches!(purge_mode.as_str(), "manual_approval" | "scheduled" | "never") {
            return Err(AppError::ValidationFailed(vec![
                "purge_mode must be one of: manual_approval, scheduled, never".to_string(),
            ]));
        }
        set_parts.push("purge_mode = ?".to_string());
        values.push(purge_mode.into());
    }
    if let Some(allow_restore) = payload.allow_restore {
        set_parts.push("allow_restore = ?".to_string());
        values.push((if allow_restore { 1 } else { 0 }).into());
    }
    if let Some(allow_purge) = payload.allow_purge {
        set_parts.push("allow_purge = ?".to_string());
        values.push((if allow_purge { 1 } else { 0 }).into());
    }
    if let Some(requires_legal_hold_check) = payload.requires_legal_hold_check {
        set_parts.push("requires_legal_hold_check = ?".to_string());
        values.push((if requires_legal_hold_check { 1 } else { 0 }).into());
    }

    if set_parts.is_empty() {
        return Ok(());
    }

    values.push(payload.policy_id.into());
    let sql = format!(
        "UPDATE retention_policies SET {} WHERE id = ?",
        set_parts.join(", ")
    );
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            values,
        ))
        .await?;

    Ok(())
}

fn parse_archive_item_summary(row: &sea_orm::QueryResult) -> ArchiveItemSummary {
    ArchiveItemSummary {
        id: row.try_get::<i64>("", "id").unwrap_or_default(),
        source_module: row.try_get::<String>("", "source_module").unwrap_or_default(),
        source_record_id: row
            .try_get::<String>("", "source_record_id")
            .unwrap_or_default(),
        archive_class: row.try_get::<String>("", "archive_class").unwrap_or_default(),
        source_state: row.try_get::<Option<String>>("", "source_state").unwrap_or(None),
        archive_reason_code: row
            .try_get::<String>("", "archive_reason_code")
            .unwrap_or_default(),
        archived_at: row.try_get::<String>("", "archived_at").unwrap_or_default(),
        archived_by_id: row.try_get::<Option<i64>>("", "archived_by_id").unwrap_or(None),
        retention_policy_id: row
            .try_get::<Option<i64>>("", "retention_policy_id")
            .unwrap_or(None),
        restore_policy: row
            .try_get::<String>("", "restore_policy")
            .unwrap_or_else(|_| "not_allowed".to_string()),
        restore_until_at: row
            .try_get::<Option<String>>("", "restore_until_at")
            .unwrap_or(None),
        legal_hold: row.try_get::<i32>("", "legal_hold").unwrap_or(0) == 1,
        checksum_sha256: row
            .try_get::<Option<String>>("", "checksum_sha256")
            .unwrap_or(None),
        search_text: row.try_get::<Option<String>>("", "search_text").unwrap_or(None),
    }
}

fn parse_retention_policy(row: &sea_orm::QueryResult) -> RetentionPolicy {
    RetentionPolicy {
        id: row.try_get::<i64>("", "id").unwrap_or_default(),
        module_code: row.try_get::<String>("", "module_code").unwrap_or_default(),
        archive_class: row.try_get::<String>("", "archive_class").unwrap_or_default(),
        retention_years: row.try_get::<i64>("", "retention_years").unwrap_or(7),
        purge_mode: row
            .try_get::<String>("", "purge_mode")
            .unwrap_or_else(|_| "manual_approval".to_string()),
        allow_restore: row.try_get::<i32>("", "allow_restore").unwrap_or(0) == 1,
        allow_purge: row.try_get::<i32>("", "allow_purge").unwrap_or(0) == 1,
        requires_legal_hold_check: row
            .try_get::<i32>("", "requires_legal_hold_check")
            .unwrap_or(1)
            == 1,
    }
}

async fn load_retention_policy_by_id(
    db: &sea_orm::DatabaseConnection,
    policy_id: i64,
) -> AppResult<Option<RetentionPolicy>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                id, module_code, archive_class, retention_years, purge_mode,
                allow_restore, allow_purge, requires_legal_hold_check
             FROM retention_policies
             WHERE id = ?
             LIMIT 1",
            [policy_id.into()],
        ))
        .await?;
    Ok(row.map(|r| parse_retention_policy(&r)))
}

/// Fire-and-log an activity event for a completed (success) archive restore.
async fn emit_archive_activity_event(
    db: &sea_orm::DatabaseConnection,
    archive_item_id: i64,
    actor_id: i64,
    event_code: &str,
    result: &str,
    action: &str,
) {
    let input = crate::activity::emitter::ActivityEventInput {
        event_class: "operational".to_string(),
        event_code: event_code.to_string(),
        source_module: "archive".to_string(),
        source_record_type: Some("archive_item".to_string()),
        source_record_id: Some(archive_item_id.to_string()),
        entity_scope_id: None,
        actor_id: Some(actor_id),
        severity: if result == "blocked" { "warning".to_string() } else { "info".to_string() },
        summary_json: Some(serde_json::json!({
            "archive_item_id": archive_item_id,
            "action": action,
            "result": result,
        })),
        correlation_id: None,
        visibility_scope: "global".to_string(),
    };
    // Fire-and-log: failure must never break the archive command
    let _ = crate::activity::emitter::emit_activity_event(db, input).await;
}

async fn load_archive_purge_strict_mode(state: &State<'_, AppState>) -> AppResult<bool> {
    let setting = settings::get_setting(&state.db, "archive_purge_strict_mode", "tenant").await?;
    let Some(setting) = setting else {
        return Ok(true);
    };
    let parsed = serde_json::from_str::<serde_json::Value>(&setting.setting_value_json).ok();
    if let Some(value) = parsed {
        if let Some(flag) = value.as_bool() {
            return Ok(flag);
        }
        if let Some(obj) = value.as_object() {
            if let Some(flag) = obj.get("enabled").and_then(serde_json::Value::as_bool) {
                return Ok(flag);
            }
        }
    }
    Ok(true)
}

/// Shared purge gate logic (retention, legal hold, policy flags). Used by
/// [`purge_archive_items`] and by cross-module integration tests.
pub(crate) async fn evaluate_purge_eligibility_db(
    db: &sea_orm::DatabaseConnection,
    archive_item_id: i64,
) -> AppResult<Option<String>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT
                ai.id,
                ai.legal_hold,
                ai.archived_at,
                rp.retention_years,
                rp.purge_mode,
                rp.allow_purge
             FROM archive_items ai
             LEFT JOIN retention_policies rp ON rp.id = ai.retention_policy_id
             WHERE ai.id = ?
             LIMIT 1",
            [archive_item_id.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Ok(Some("archive item not found".to_string()));
    };

    let legal_hold = row.try_get::<i32>("", "legal_hold").unwrap_or(0) == 1;
    if legal_hold {
        return Ok(Some("blocked: legal hold is enabled".to_string()));
    }

    let allow_purge = row.try_get::<Option<i32>>("", "allow_purge").unwrap_or(None) == Some(1);
    if !allow_purge {
        return Ok(Some(
            "blocked: retention policy does not allow purge".to_string(),
        ));
    }

    let purge_mode = row
        .try_get::<Option<String>>("", "purge_mode")
        .unwrap_or(None)
        .unwrap_or_else(|| "manual_approval".to_string());
    if purge_mode == "never" {
        return Ok(Some("blocked: purge_mode is never".to_string()));
    }

    let retention_years = row.try_get::<Option<i64>>("", "retention_years").unwrap_or(None);
    let Some(retention_years) = retention_years else {
        return Ok(Some(
            "blocked: retention policy is missing retention_years".to_string(),
        ));
    };

    let archived_at = row.try_get::<String>("", "archived_at").unwrap_or_default();
    let elapsed = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT CASE
                WHEN datetime(?) <= datetime('now', printf('-%d years', ?))
                THEN 1 ELSE 0 END AS elapsed",
            [archived_at.into(), retention_years.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<i32>("", "elapsed").ok())
        .unwrap_or(0)
        == 1;
    if !elapsed {
        return Ok(Some(
            "blocked: retention period has not elapsed".to_string(),
        ));
    }

    Ok(None)
}

async fn evaluate_purge_eligibility(
    state: &State<'_, AppState>,
    archive_item_id: i64,
) -> AppResult<Option<String>> {
    evaluate_purge_eligibility_db(&state.db, archive_item_id).await
}

async fn write_archive_action(
    db: &sea_orm::DatabaseConnection,
    archive_item_id: i64,
    action: &str,
    action_by_id: Option<i64>,
    reason_note: Option<&str>,
    result_status: &str,
) -> AppResult<i64> {
    let sql = format!(
        "INSERT INTO archive_actions
            (archive_item_id, action, action_by_id, reason_note, result_status)
         VALUES (?, '{}', ?, ?, ?)",
        action
    );
    let insert = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                archive_item_id.into(),
                action_by_id.into(),
                reason_note.map(std::string::ToString::to_string).into(),
                result_status.into(),
            ],
        ))
        .await?;
    Ok(insert.last_insert_id() as i64)
}
