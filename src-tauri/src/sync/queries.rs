use sea_orm::sqlx::error::ErrorKind;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    ApplySyncBatchInput, ApplySyncBatchResult, ListOutboxFilter, ReplaySyncFailuresInput, ReplaySyncFailuresResult,
    ResolveSyncConflictInput, StageOutboxItemInput, SyncCheckpoint, SyncConflictFilter, SyncConflictRecord, SyncHealthAlert,
    SyncHealthMetrics, SyncInboxItem, SyncObservabilityReport, SyncOutboxItem, SyncPushPayload, SyncRecoveryProof,
    SyncReplayRun, SyncRepairActionRecord, SyncRepairExecutionResult, SyncRepairPreview, SyncRepairPreviewInput,
    SyncStateSummary, SyncTypedRejection, ExecuteSyncRepairInput, SYNC_PROTOCOL_VERSION_V1,
};
use crate::audit::writer::{write_audit_event, AuditEventInput};

fn hash_payload(payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode sync field '{field}': {err}"))
}

fn validate_non_empty(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} is required."));
    }
}

fn validate_operation(operation: &str, errors: &mut Vec<String>) {
    match operation {
        "create" | "update" | "delete" | "upsert" | "archive" => {}
        _ => errors.push(format!(
            "operation must be one of: create, update, delete, upsert, archive. got '{operation}'."
        )),
    }
}

fn validate_protocol_version(version: &str) -> AppResult<()> {
    if version != SYNC_PROTOCOL_VERSION_V1 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Unsupported sync protocol version '{version}'. Supported: {SYNC_PROTOCOL_VERSION_V1}."
        )]));
    }
    Ok(())
}

fn conflict_policy(conflict_type: &str) -> (&'static str, bool, &'static str, &'static str) {
    match conflict_type {
        "NOT_FOUND" | "DUPLICATE_INBOUND" | "STALE_ACK" | "TRANSIENT_NETWORK" | "RATE_LIMIT" => {
            ("remote", false, "retry_later", "auto_retry_then_close")
        }
        "INVALID_INBOUND_PAYLOAD" => ("remote", true, "escalate", "manual_review_required"),
        "AUTHORITY_MISMATCH" => ("mixed", true, "merge_fields", "manual_review_required"),
        _ => ("mixed", true, "merge_fields", "manual_review_required"),
    }
}

fn map_resolution_to_status(action: &str) -> Option<&'static str> {
    match action {
        "accept_local" => Some("resolved_local"),
        "accept_remote" => Some("resolved_remote"),
        "merge_fields" => Some("merged"),
        "retry_later" => Some("triaged"),
        "escalate" => Some("escalated"),
        "dismiss" => Some("dismissed"),
        _ => None,
    }
}

fn parse_rfc3339(value: &str, field: &str) -> AppResult<()> {
    chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
        AppError::ValidationFailed(vec![format!("{field} must be a valid RFC3339 timestamp.")])
    })?;
    Ok(())
}

fn supported_repair_mode(mode: &str) -> bool {
    matches!(
        mode,
        "requeue_rejected_outbox" | "retry_operator_conflicts" | "checkpoint_realign"
    )
}

fn parse_checkpoint_sequence(token: &str) -> Option<i64> {
    let last = token.rsplit('-').next()?;
    last.parse::<i64>().ok()
}

fn is_stale_checkpoint(current: &str, incoming: &str) -> bool {
    match (parse_checkpoint_sequence(current), parse_checkpoint_sequence(incoming)) {
        (Some(current_seq), Some(incoming_seq)) => incoming_seq < current_seq,
        _ => false,
    }
}

async fn actor_id_for_audit(db: &DatabaseConnection, user_id: i64) -> AppResult<Option<i64>> {
    let exists: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query actor existence.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("actor_exists", e))?;
    Ok(if exists > 0 { Some(user_id) } else { None })
}

fn validate_outbox_input(input: &StageOutboxItemInput) -> AppResult<()> {
    let mut errors = Vec::new();
    validate_non_empty("idempotency_key", &input.idempotency_key, &mut errors);
    validate_non_empty("entity_type", &input.entity_type, &mut errors);
    validate_non_empty("entity_sync_id", &input.entity_sync_id, &mut errors);
    validate_non_empty("operation", &input.operation, &mut errors);
    validate_operation(&input.operation, &mut errors);
    if input.row_version < 0 {
        errors.push("row_version must be >= 0.".to_string());
    }
    if serde_json::from_str::<serde_json::Value>(&input.payload_json).is_err() {
        errors.push("payload_json must be a valid JSON document.".to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(errors))
    }
}

fn to_sync_outbox_item(row: &sea_orm::QueryResult) -> AppResult<SyncOutboxItem> {
    Ok(SyncOutboxItem {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        idempotency_key: row
            .try_get("", "idempotency_key")
            .map_err(|e| decode_err("idempotency_key", e))?,
        entity_type: row
            .try_get("", "entity_type")
            .map_err(|e| decode_err("entity_type", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        operation: row.try_get("", "operation").map_err(|e| decode_err("operation", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        payload_json: row
            .try_get("", "payload_json")
            .map_err(|e| decode_err("payload_json", e))?,
        payload_hash: row
            .try_get("", "payload_hash")
            .map_err(|e| decode_err("payload_hash", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        acknowledged_at: row
            .try_get("", "acknowledged_at")
            .map_err(|e| decode_err("acknowledged_at", e))?,
        rejection_code: row
            .try_get("", "rejection_code")
            .map_err(|e| decode_err("rejection_code", e))?,
        rejection_message: row
            .try_get("", "rejection_message")
            .map_err(|e| decode_err("rejection_message", e))?,
        origin_machine_id: row
            .try_get("", "origin_machine_id")
            .map_err(|e| decode_err("origin_machine_id", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

fn to_sync_inbox_item(row: &sea_orm::QueryResult) -> AppResult<SyncInboxItem> {
    Ok(SyncInboxItem {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        server_batch_id: row
            .try_get("", "server_batch_id")
            .map_err(|e| decode_err("server_batch_id", e))?,
        checkpoint_token: row
            .try_get("", "checkpoint_token")
            .map_err(|e| decode_err("checkpoint_token", e))?,
        entity_type: row
            .try_get("", "entity_type")
            .map_err(|e| decode_err("entity_type", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        operation: row.try_get("", "operation").map_err(|e| decode_err("operation", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        payload_json: row
            .try_get("", "payload_json")
            .map_err(|e| decode_err("payload_json", e))?,
        payload_hash: row
            .try_get("", "payload_hash")
            .map_err(|e| decode_err("payload_hash", e))?,
        apply_status: row
            .try_get("", "apply_status")
            .map_err(|e| decode_err("apply_status", e))?,
        rejection_code: row
            .try_get("", "rejection_code")
            .map_err(|e| decode_err("rejection_code", e))?,
        rejection_message: row
            .try_get("", "rejection_message")
            .map_err(|e| decode_err("rejection_message", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

fn to_sync_conflict(row: &sea_orm::QueryResult) -> AppResult<SyncConflictRecord> {
    Ok(SyncConflictRecord {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        conflict_key: row
            .try_get("", "conflict_key")
            .map_err(|e| decode_err("conflict_key", e))?,
        source_scope: row
            .try_get("", "source_scope")
            .map_err(|e| decode_err("source_scope", e))?,
        source_batch_id: row
            .try_get("", "source_batch_id")
            .map_err(|e| decode_err("source_batch_id", e))?,
        linked_outbox_id: row
            .try_get("", "linked_outbox_id")
            .map_err(|e| decode_err("linked_outbox_id", e))?,
        linked_inbox_id: row
            .try_get("", "linked_inbox_id")
            .map_err(|e| decode_err("linked_inbox_id", e))?,
        entity_type: row
            .try_get("", "entity_type")
            .map_err(|e| decode_err("entity_type", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        operation: row.try_get("", "operation").map_err(|e| decode_err("operation", e))?,
        conflict_type: row
            .try_get("", "conflict_type")
            .map_err(|e| decode_err("conflict_type", e))?,
        local_payload_json: row
            .try_get("", "local_payload_json")
            .map_err(|e| decode_err("local_payload_json", e))?,
        inbound_payload_json: row
            .try_get("", "inbound_payload_json")
            .map_err(|e| decode_err("inbound_payload_json", e))?,
        authority_side: row
            .try_get("", "authority_side")
            .map_err(|e| decode_err("authority_side", e))?,
        checkpoint_token: row
            .try_get("", "checkpoint_token")
            .map_err(|e| decode_err("checkpoint_token", e))?,
        auto_resolution_policy: row
            .try_get("", "auto_resolution_policy")
            .map_err(|e| decode_err("auto_resolution_policy", e))?,
        requires_operator_review: row
            .try_get::<i64>("", "requires_operator_review")
            .map_err(|e| decode_err("requires_operator_review", e))?
            != 0,
        recommended_action: row
            .try_get("", "recommended_action")
            .map_err(|e| decode_err("recommended_action", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        resolution_action: row
            .try_get("", "resolution_action")
            .map_err(|e| decode_err("resolution_action", e))?,
        resolution_note: row
            .try_get("", "resolution_note")
            .map_err(|e| decode_err("resolution_note", e))?,
        resolved_by_id: row
            .try_get("", "resolved_by_id")
            .map_err(|e| decode_err("resolved_by_id", e))?,
        resolved_at: row
            .try_get("", "resolved_at")
            .map_err(|e| decode_err("resolved_at", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

fn to_sync_replay_run(row: &sea_orm::QueryResult) -> AppResult<SyncReplayRun> {
    Ok(SyncReplayRun {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        replay_key: row
            .try_get("", "replay_key")
            .map_err(|e| decode_err("replay_key", e))?,
        mode: row.try_get("", "mode").map_err(|e| decode_err("mode", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        reason: row.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
        requested_by_id: row
            .try_get("", "requested_by_id")
            .map_err(|e| decode_err("requested_by_id", e))?,
        scope_json: row
            .try_get("", "scope_json")
            .map_err(|e| decode_err("scope_json", e))?,
        pre_replay_checkpoint: row
            .try_get("", "pre_replay_checkpoint")
            .map_err(|e| decode_err("pre_replay_checkpoint", e))?,
        post_replay_checkpoint: row
            .try_get("", "post_replay_checkpoint")
            .map_err(|e| decode_err("post_replay_checkpoint", e))?,
        result_json: row
            .try_get("", "result_json")
            .map_err(|e| decode_err("result_json", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        started_at: row
            .try_get("", "started_at")
            .map_err(|e| decode_err("started_at", e))?,
        finished_at: row
            .try_get("", "finished_at")
            .map_err(|e| decode_err("finished_at", e))?,
    })
}

fn to_sync_repair_action(row: &sea_orm::QueryResult) -> AppResult<SyncRepairActionRecord> {
    Ok(SyncRepairActionRecord {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        plan_id: row.try_get("", "plan_id").map_err(|e| decode_err("plan_id", e))?,
        mode: row.try_get("", "mode").map_err(|e| decode_err("mode", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        reason: row.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
        created_by_id: row
            .try_get("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        executed_by_id: row
            .try_get("", "executed_by_id")
            .map_err(|e| decode_err("executed_by_id", e))?,
        scope_json: row
            .try_get("", "scope_json")
            .map_err(|e| decode_err("scope_json", e))?,
        preview_json: row
            .try_get("", "preview_json")
            .map_err(|e| decode_err("preview_json", e))?,
        result_json: row
            .try_get("", "result_json")
            .map_err(|e| decode_err("result_json", e))?,
        created_at: row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        executed_at: row
            .try_get("", "executed_at")
            .map_err(|e| decode_err("executed_at", e))?,
    })
}

fn is_unique_constraint_error(err: &sea_orm::DbErr) -> bool {
    match err {
        sea_orm::DbErr::Exec(sea_orm::RuntimeErr::SqlxError(sqlx_err))
        | sea_orm::DbErr::Query(sea_orm::RuntimeErr::SqlxError(sqlx_err)) => {
            if let Some(db_err) = sqlx_err.as_database_error() {
                return matches!(db_err.kind(), ErrorKind::UniqueViolation);
            }
            false
        }
        _ => false,
    }
}

pub async fn stage_outbox_item(db: &DatabaseConnection, input: StageOutboxItemInput) -> AppResult<SyncOutboxItem> {
    validate_outbox_input(&input)?;
    let payload_hash = hash_payload(&input.payload_json);
    let insert_result = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO sync_outbox (
                idempotency_key, entity_type, entity_sync_id, operation,
                row_version, payload_json, payload_hash, status, origin_machine_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
            [
                input.idempotency_key.clone().into(),
                input.entity_type.clone().into(),
                input.entity_sync_id.clone().into(),
                input.operation.clone().into(),
                input.row_version.into(),
                input.payload_json.clone().into(),
                payload_hash.clone().into(),
                input.origin_machine_id.clone().into(),
            ],
        ))
        .await;

    match insert_result {
        Ok(_) => {}
        Err(err) if is_unique_constraint_error(&err) => {
            // Idempotent staging: return existing matching envelope.
        }
        Err(err) => return Err(err.into()),
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, idempotency_key, entity_type, entity_sync_id, operation, row_version,
                    payload_json, payload_hash, status, acknowledged_at, rejection_code,
                    rejection_message, origin_machine_id, created_at, updated_at
             FROM sync_outbox
             WHERE idempotency_key = ?
               AND entity_type = ?
               AND entity_sync_id = ?
               AND operation = ?
               AND payload_hash = ?",
            [
                input.idempotency_key.into(),
                input.entity_type.into(),
                input.entity_sync_id.into(),
                input.operation.into(),
                payload_hash.into(),
            ],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to fetch staged outbox item.".to_string()))?;
    to_sync_outbox_item(&row)
}

pub async fn list_outbox_items(db: &DatabaseConnection, filter: ListOutboxFilter) -> AppResult<Vec<SyncOutboxItem>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let rows = if let Some(status) = filter.status {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, idempotency_key, entity_type, entity_sync_id, operation, row_version,
                    payload_json, payload_hash, status, acknowledged_at, rejection_code,
                    rejection_message, origin_machine_id, created_at, updated_at
             FROM sync_outbox
             WHERE status = ?
             ORDER BY id ASC
             LIMIT ?",
            [status.into(), limit.into()],
        ))
        .await?
    } else {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, idempotency_key, entity_type, entity_sync_id, operation, row_version,
                    payload_json, payload_hash, status, acknowledged_at, rejection_code,
                    rejection_message, origin_machine_id, created_at, updated_at
             FROM sync_outbox
             ORDER BY id ASC
             LIMIT ?",
            [limit.into()],
        ))
        .await?
    };

    rows.into_iter().map(|row| to_sync_outbox_item(&row)).collect()
}

pub async fn get_sync_push_payload(db: &DatabaseConnection, limit: Option<i64>) -> AppResult<SyncPushPayload> {
    let checkpoint_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
            [],
        ))
        .await?;
    let checkpoint_token = checkpoint_row
        .as_ref()
        .map(|row| row.try_get("", "checkpoint_token"))
        .transpose()
        .map_err(|e| decode_err("checkpoint_token", e))?;

    let outbox_batch = list_outbox_items(
        db,
        ListOutboxFilter {
            status: Some("pending".to_string()),
            limit,
        },
    )
    .await?;

    Ok(SyncPushPayload {
        protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
        checkpoint_token,
        outbox_batch,
    })
}

async fn upsert_sync_conflict(
    tx: &sea_orm::DatabaseTransaction,
    source_scope: &str,
    source_batch_id: Option<String>,
    linked_outbox_id: Option<i64>,
    linked_inbox_id: Option<i64>,
    entity_type: String,
    entity_sync_id: String,
    operation: String,
    conflict_type: String,
    local_payload_json: Option<String>,
    inbound_payload_json: Option<String>,
    checkpoint_token: Option<String>,
) -> AppResult<()> {
    let (authority_side, requires_operator_review, recommended_action, auto_resolution_policy) =
        conflict_policy(&conflict_type);
    let conflict_key = format!(
        "{}:{}:{}:{}:{}",
        source_scope,
        entity_type,
        entity_sync_id,
        operation,
        conflict_type
    );
    let initial_status = if requires_operator_review {
        "new"
    } else {
        "resolved_remote"
    };
    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_conflicts (
            conflict_key, source_scope, source_batch_id, linked_outbox_id, linked_inbox_id,
            entity_type, entity_sync_id, operation, conflict_type, local_payload_json,
            inbound_payload_json, authority_side, checkpoint_token, auto_resolution_policy,
            requires_operator_review, recommended_action, status, row_version, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1,
            strftime('%Y-%m-%dT%H:%M:%SZ','now'),
            strftime('%Y-%m-%dT%H:%M:%SZ','now')
        )
        ON CONFLICT(conflict_key) DO UPDATE SET
            source_batch_id = excluded.source_batch_id,
            linked_outbox_id = COALESCE(excluded.linked_outbox_id, sync_conflicts.linked_outbox_id),
            linked_inbox_id = COALESCE(excluded.linked_inbox_id, sync_conflicts.linked_inbox_id),
            local_payload_json = COALESCE(excluded.local_payload_json, sync_conflicts.local_payload_json),
            inbound_payload_json = COALESCE(excluded.inbound_payload_json, sync_conflicts.inbound_payload_json),
            authority_side = excluded.authority_side,
            checkpoint_token = excluded.checkpoint_token,
            auto_resolution_policy = excluded.auto_resolution_policy,
            requires_operator_review = excluded.requires_operator_review,
            recommended_action = excluded.recommended_action,
            status = CASE
                WHEN sync_conflicts.status IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed')
                    THEN sync_conflicts.status
                ELSE excluded.status
            END,
            row_version = sync_conflicts.row_version + 1,
            updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        [
            conflict_key.into(),
            source_scope.to_string().into(),
            source_batch_id.into(),
            linked_outbox_id.into(),
            linked_inbox_id.into(),
            entity_type.into(),
            entity_sync_id.into(),
            operation.into(),
            conflict_type.into(),
            local_payload_json.into(),
            inbound_payload_json.into(),
            authority_side.to_string().into(),
            checkpoint_token.into(),
            auto_resolution_policy.to_string().into(),
            (if requires_operator_review { 1_i64 } else { 0_i64 }).into(),
            recommended_action.to_string().into(),
            initial_status.to_string().into(),
        ],
    ))
    .await?;
    Ok(())
}

pub async fn apply_sync_batch(db: &DatabaseConnection, input: ApplySyncBatchInput) -> AppResult<ApplySyncBatchResult> {
    validate_protocol_version(&input.protocol_version)?;
    let mut validation_errors = Vec::new();
    validate_non_empty("server_batch_id", &input.server_batch_id, &mut validation_errors);
    validate_non_empty("checkpoint_token", &input.checkpoint_token, &mut validation_errors);
    for ack in &input.acknowledged_items {
        validate_non_empty("ack.idempotency_key", &ack.idempotency_key, &mut validation_errors);
        validate_non_empty("ack.entity_sync_id", &ack.entity_sync_id, &mut validation_errors);
        validate_operation(&ack.operation, &mut validation_errors);
    }
    for rejected in &input.rejected_items {
        validate_non_empty(
            "rejected.idempotency_key",
            &rejected.idempotency_key,
            &mut validation_errors,
        );
        validate_non_empty("rejected.entity_sync_id", &rejected.entity_sync_id, &mut validation_errors);
        validate_operation(&rejected.operation, &mut validation_errors);
        validate_non_empty("rejected.rejection_code", &rejected.rejection_code, &mut validation_errors);
        validate_non_empty(
            "rejected.rejection_message",
            &rejected.rejection_message,
            &mut validation_errors,
        );
    }
    if !validation_errors.is_empty() {
        return Err(AppError::ValidationFailed(validation_errors));
    }

    if let Some(current_checkpoint_row) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
            [],
        ))
        .await?
    {
        let current_checkpoint: Option<String> = current_checkpoint_row
            .try_get("", "checkpoint_token")
            .map_err(|e| decode_err("checkpoint_token", e))?;
        if let Some(current_checkpoint) = current_checkpoint {
            if input.checkpoint_token != current_checkpoint
                && is_stale_checkpoint(&current_checkpoint, &input.checkpoint_token)
            {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Stale checkpoint token '{}' rejected. Current token is '{}'.",
                    input.checkpoint_token, current_checkpoint
                )]));
            }
        }
    }

    let tx = db.begin().await?;

    let mut acknowledged_count: i64 = 0;
    for ack in &input.acknowledged_items {
        let result = tx
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_outbox
                 SET status = 'acked',
                     acknowledged_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE idempotency_key = ?
                   AND entity_sync_id = ?
                   AND operation = ?
                   AND status = 'pending'",
                [
                    ack.idempotency_key.clone().into(),
                    ack.entity_sync_id.clone().into(),
                    ack.operation.clone().into(),
                ],
            ))
            .await?;
        acknowledged_count += i64::try_from(result.rows_affected()).unwrap_or(0);
    }

    let mut rejected_count: i64 = 0;
    let mut typed_rejections: Vec<SyncTypedRejection> = Vec::new();
    for rejected in &input.rejected_items {
        let result = tx
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_outbox
                 SET status = 'rejected',
                     rejection_code = ?,
                     rejection_message = ?,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE idempotency_key = ?
                   AND entity_sync_id = ?
                   AND operation = ?
                   AND status IN ('pending', 'acked')",
                [
                    rejected.rejection_code.clone().into(),
                    rejected.rejection_message.clone().into(),
                    rejected.idempotency_key.clone().into(),
                    rejected.entity_sync_id.clone().into(),
                    rejected.operation.clone().into(),
                ],
            ))
            .await?;
        let affected = i64::try_from(result.rows_affected()).unwrap_or(0);
        rejected_count += affected;
        if affected > 0 {
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO sync_rejections (
                    source, linked_record_id, idempotency_key, entity_type, entity_sync_id, operation,
                    rejection_code, rejection_message
                )
                SELECT
                    'outbox', id, idempotency_key, entity_type, entity_sync_id, operation, ?, ?
                FROM sync_outbox
                WHERE idempotency_key = ?
                  AND entity_sync_id = ?
                  AND operation = ?",
                [
                    rejected.rejection_code.clone().into(),
                    rejected.rejection_message.clone().into(),
                    rejected.idempotency_key.clone().into(),
                    rejected.entity_sync_id.clone().into(),
                    rejected.operation.clone().into(),
                ],
            ))
            .await?;

            if let Some(outbox_row) = tx
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT id, entity_type, payload_json
                     FROM sync_outbox
                     WHERE idempotency_key = ?
                       AND entity_sync_id = ?
                       AND operation = ?",
                    [
                        rejected.idempotency_key.clone().into(),
                        rejected.entity_sync_id.clone().into(),
                        rejected.operation.clone().into(),
                    ],
                ))
                .await?
            {
                let linked_outbox_id: i64 = outbox_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
                let entity_type: String = outbox_row
                    .try_get("", "entity_type")
                    .map_err(|e| decode_err("entity_type", e))?;
                let local_payload_json: String = outbox_row
                    .try_get("", "payload_json")
                    .map_err(|e| decode_err("payload_json", e))?;
                upsert_sync_conflict(
                    &tx,
                    "outbox",
                    Some(input.server_batch_id.clone()),
                    Some(linked_outbox_id),
                    None,
                    entity_type,
                    rejected.entity_sync_id.clone(),
                    rejected.operation.clone(),
                    rejected.rejection_code.clone(),
                    Some(local_payload_json),
                    None,
                    Some(input.checkpoint_token.clone()),
                )
                .await?;
            }
        }
    }

    let mut inbound_applied_count: i64 = 0;
    let mut inbound_duplicate_count: i64 = 0;
    for inbound in &input.inbound_items {
        let mut inbound_errors = Vec::new();
        validate_non_empty("entity_type", &inbound.entity_type, &mut inbound_errors);
        validate_non_empty("entity_sync_id", &inbound.entity_sync_id, &mut inbound_errors);
        validate_non_empty("operation", &inbound.operation, &mut inbound_errors);
        validate_operation(&inbound.operation, &mut inbound_errors);
        if serde_json::from_str::<serde_json::Value>(&inbound.payload_json).is_err() {
            inbound_errors.push("payload_json must be a valid JSON document.".to_string());
        }
        if !inbound_errors.is_empty() {
            typed_rejections.push(SyncTypedRejection {
                scope: "inbound".to_string(),
                entity_sync_id: inbound.entity_sync_id.clone(),
                operation: inbound.operation.clone(),
                rejection_code: "INVALID_INBOUND_PAYLOAD".to_string(),
                rejection_message: inbound_errors.join(" "),
            });
            upsert_sync_conflict(
                &tx,
                "inbound",
                Some(input.server_batch_id.clone()),
                None,
                None,
                inbound.entity_type.clone(),
                inbound.entity_sync_id.clone(),
                inbound.operation.clone(),
                "INVALID_INBOUND_PAYLOAD".to_string(),
                None,
                Some(inbound.payload_json.clone()),
                Some(input.checkpoint_token.clone()),
            )
            .await?;
            continue;
        }

        let payload_hash = hash_payload(&inbound.payload_json);
        let insert_result = tx
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO sync_inbox (
                    server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                    row_version, payload_json, payload_hash, apply_status
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'applied')",
                [
                    input.server_batch_id.clone().into(),
                    input.checkpoint_token.clone().into(),
                    inbound.entity_type.clone().into(),
                    inbound.entity_sync_id.clone().into(),
                    inbound.operation.clone().into(),
                    inbound.row_version.into(),
                    inbound.payload_json.clone().into(),
                    payload_hash.into(),
                ],
            ))
            .await;

        match insert_result {
            Ok(_) => inbound_applied_count += 1,
            Err(err) if is_unique_constraint_error(&err) => inbound_duplicate_count += 1,
            Err(err) => return Err(err.into()),
        }
    }

    let checkpoint_advanced = typed_rejections.is_empty();
    if checkpoint_advanced {
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO sync_checkpoint (
                id, checkpoint_token, last_idempotency_key, protocol_version, policy_metadata_json,
                last_sync_at, updated_at
            ) VALUES (
                1, ?, NULL, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')
            )
            ON CONFLICT(id) DO UPDATE SET
                checkpoint_token = excluded.checkpoint_token,
                protocol_version = excluded.protocol_version,
                policy_metadata_json = excluded.policy_metadata_json,
                last_sync_at = excluded.last_sync_at,
                updated_at = excluded.updated_at",
            [
                input.checkpoint_token.clone().into(),
                input.protocol_version.clone().into(),
                input.policy_metadata_json.clone().into(),
            ],
        ))
        .await?;
    }

    tx.commit().await?;

    let checkpoint_token = if checkpoint_advanced {
        Some(input.checkpoint_token)
    } else {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
                [],
            ))
            .await?;
        row.as_ref()
            .map(|r| r.try_get("", "checkpoint_token"))
            .transpose()
            .map_err(|e| decode_err("checkpoint_token", e))?
    };

    Ok(ApplySyncBatchResult {
        protocol_version: input.protocol_version,
        checkpoint_token,
        checkpoint_advanced,
        acknowledged_count,
        rejected_count,
        inbound_applied_count,
        inbound_duplicate_count,
        typed_rejections,
    })
}

pub async fn list_inbox_items(
    db: &DatabaseConnection,
    apply_status: Option<String>,
    limit: Option<i64>,
) -> AppResult<Vec<SyncInboxItem>> {
    let page_size = limit.unwrap_or(200).clamp(1, 1000);
    let rows = if let Some(status) = apply_status {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                    row_version, payload_json, payload_hash, apply_status, rejection_code,
                    rejection_message, created_at, updated_at
             FROM sync_inbox
             WHERE apply_status = ?
             ORDER BY id ASC
             LIMIT ?",
            [status.into(), page_size.into()],
        ))
        .await?
    } else {
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                    row_version, payload_json, payload_hash, apply_status, rejection_code,
                    rejection_message, created_at, updated_at
             FROM sync_inbox
             ORDER BY id ASC
             LIMIT ?",
            [page_size.into()],
        ))
        .await?
    };

    rows.into_iter().map(|row| to_sync_inbox_item(&row)).collect()
}

pub async fn get_sync_state_summary(db: &DatabaseConnection) -> AppResult<SyncStateSummary> {
    let checkpoint = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, checkpoint_token, last_idempotency_key, protocol_version,
                    policy_metadata_json, last_sync_at, updated_at
             FROM sync_checkpoint
             WHERE id = 1",
            [],
        ))
        .await?
        .map(|row| -> AppResult<SyncCheckpoint> {
            Ok(SyncCheckpoint {
                id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
                checkpoint_token: row
                    .try_get("", "checkpoint_token")
                    .map_err(|e| decode_err("checkpoint_token", e))?,
                last_idempotency_key: row
                    .try_get("", "last_idempotency_key")
                    .map_err(|e| decode_err("last_idempotency_key", e))?,
                protocol_version: row
                    .try_get("", "protocol_version")
                    .map_err(|e| decode_err("protocol_version", e))?,
                policy_metadata_json: row
                    .try_get("", "policy_metadata_json")
                    .map_err(|e| decode_err("policy_metadata_json", e))?,
                last_sync_at: row
                    .try_get("", "last_sync_at")
                    .map_err(|e| decode_err("last_sync_at", e))?,
                updated_at: row
                    .try_get("", "updated_at")
                    .map_err(|e| decode_err("updated_at", e))?,
            })
        })
        .transpose()?;

    let pending_outbox_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'pending'",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query pending outbox count.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("pending_outbox_count", e))?;

    let rejected_outbox_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'rejected'",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query rejected outbox count.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("rejected_outbox_count", e))?;

    let inbox_error_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM sync_inbox WHERE apply_status = 'rejected'",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query inbox error count.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("inbox_error_count", e))?;

    Ok(SyncStateSummary {
        protocol_version: SYNC_PROTOCOL_VERSION_V1.to_string(),
        checkpoint,
        pending_outbox_count,
        rejected_outbox_count,
        inbox_error_count,
    })
}

pub async fn list_sync_conflicts(
    db: &DatabaseConnection,
    filter: SyncConflictFilter,
) -> AppResult<Vec<SyncConflictRecord>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, conflict_key, source_scope, source_batch_id, linked_outbox_id, linked_inbox_id,
                    entity_type, entity_sync_id, operation, conflict_type, local_payload_json,
                    inbound_payload_json, authority_side, checkpoint_token, auto_resolution_policy,
                    requires_operator_review, recommended_action, status, resolution_action,
                    resolution_note, resolved_by_id, resolved_at, row_version, created_at, updated_at
             FROM sync_conflicts
             WHERE (? IS NULL OR conflict_type = ?)
               AND (? IS NULL OR requires_operator_review = ?)
               AND (
                    ? IS NULL
                    OR status IN (
                        SELECT value FROM json_each(?)
                    )
               )
             ORDER BY
                CASE
                    WHEN status IN ('new', 'escalated') THEN 0
                    WHEN status = 'triaged' THEN 1
                    ELSE 2
                END,
                updated_at DESC,
                id DESC
             LIMIT ?",
            [
                filter.conflict_type.clone().into(),
                filter.conflict_type.into(),
                filter
                    .requires_operator_review
                    .map(|v| if v { 1_i64 } else { 0_i64 })
                    .into(),
                filter
                    .requires_operator_review
                    .map(|v| if v { 1_i64 } else { 0_i64 })
                    .into(),
                filter
                    .statuses
                    .as_ref()
                    .map(|_| "use_json_filter".to_string())
                    .into(),
                filter
                    .statuses
                    .map(|list| serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()))
                    .into(),
                limit.into(),
            ],
        ))
        .await?;

    rows.into_iter().map(|row| to_sync_conflict(&row)).collect()
}

pub async fn resolve_sync_conflict(
    db: &DatabaseConnection,
    user_id: i64,
    input: ResolveSyncConflictInput,
) -> AppResult<SyncConflictRecord> {
    let mut errors = Vec::new();
    if input.conflict_id <= 0 {
        errors.push("conflict_id must be > 0.".to_string());
    }
    if input.expected_row_version <= 0 {
        errors.push("expected_row_version must be > 0.".to_string());
    }
    if map_resolution_to_status(&input.action).is_none() {
        errors.push(
            "action must be one of: accept_local, accept_remote, merge_fields, retry_later, escalate, dismiss."
                .to_string(),
        );
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    let next_status = map_resolution_to_status(&input.action)
        .ok_or_else(|| AppError::SyncError("Invalid conflict action mapping.".to_string()))?;

    let tx = db.begin().await?;
    let current = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT row_version, status
             FROM sync_conflicts
             WHERE id = ?",
            [input.conflict_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "sync_conflict".to_string(),
            id: input.conflict_id.to_string(),
        })?;
    let current_row_version: i64 = current
        .try_get("", "row_version")
        .map_err(|e| decode_err("row_version", e))?;
    if current_row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Sync conflict was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_conflicts
         SET status = ?,
             resolution_action = ?,
             resolution_note = ?,
             resolved_by_id = ?,
             resolved_at = CASE WHEN ? IN ('resolved_local', 'resolved_remote', 'merged', 'dismissed')
                                THEN strftime('%Y-%m-%dT%H:%M:%SZ','now')
                                ELSE NULL END,
             row_version = row_version + 1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ? AND row_version = ?",
        [
            next_status.to_string().into(),
            input.action.clone().into(),
            input.resolution_note.clone().into(),
            user_id.into(),
            next_status.to_string().into(),
            input.conflict_id.into(),
            input.expected_row_version.into(),
        ],
    ))
    .await?;
    tx.commit().await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, conflict_key, source_scope, source_batch_id, linked_outbox_id, linked_inbox_id,
                    entity_type, entity_sync_id, operation, conflict_type, local_payload_json,
                    inbound_payload_json, authority_side, checkpoint_token, auto_resolution_policy,
                    requires_operator_review, recommended_action, status, resolution_action,
                    resolution_note, resolved_by_id, resolved_at, row_version, created_at, updated_at
             FROM sync_conflicts
             WHERE id = ?",
            [input.conflict_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "sync_conflict".to_string(),
            id: input.conflict_id.to_string(),
        })?;
    to_sync_conflict(&row)
}

pub async fn replay_sync_failures(
    db: &DatabaseConnection,
    user_id: i64,
    input: ReplaySyncFailuresInput,
) -> AppResult<ReplaySyncFailuresResult> {
    let mut validation_errors = Vec::new();
    validate_non_empty("replay_key", &input.replay_key, &mut validation_errors);
    validate_non_empty("mode", &input.mode, &mut validation_errors);
    validate_non_empty("reason", &input.reason, &mut validation_errors);
    match input.mode.as_str() {
        "single_item" | "batch" | "window" | "checkpoint_rollback" => {}
        _ => validation_errors.push(
            "mode must be one of: single_item, batch, window, checkpoint_rollback.".to_string(),
        ),
    }
    if input.mode == "single_item" && input.outbox_id.is_none() && input.conflict_id.is_none() {
        validation_errors.push(
            "single_item replay requires outbox_id or conflict_id scope.".to_string(),
        );
    }
    if input.mode == "batch" {
        validate_non_empty(
            "server_batch_id",
            input.server_batch_id.as_deref().unwrap_or_default(),
            &mut validation_errors,
        );
    }
    if input.mode == "window" {
        if let Some(start) = &input.window_start {
            parse_rfc3339(start, "window_start")?;
        } else {
            validation_errors.push("window_start is required for window mode.".to_string());
        }
        if let Some(end) = &input.window_end {
            parse_rfc3339(end, "window_end")?;
        } else {
            validation_errors.push("window_end is required for window mode.".to_string());
        }
    }
    if input.mode == "checkpoint_rollback" {
        validate_non_empty(
            "checkpoint_token",
            input.checkpoint_token.as_deref().unwrap_or_default(),
            &mut validation_errors,
        );
    }
    if !validation_errors.is_empty() {
        return Err(AppError::ValidationFailed(validation_errors));
    }

    if let Some(existing) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, replay_key, mode, status, reason, requested_by_id, scope_json,
                    pre_replay_checkpoint, post_replay_checkpoint, result_json, created_at, started_at, finished_at
             FROM sync_replay_runs
             WHERE replay_key = ?",
            [input.replay_key.clone().into()],
        ))
        .await?
    {
        return Ok(ReplaySyncFailuresResult {
            run: to_sync_replay_run(&existing)?,
            requeued_outbox_count: 0,
            transitioned_conflict_count: 0,
            checkpoint_token_after: None,
            guard_applied: false,
        });
    }

    let unresolved_operator_conflicts: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count
             FROM sync_conflicts
             WHERE requires_operator_review = 1
               AND status IN ('new', 'triaged', 'escalated')",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to count unresolved operator conflicts.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("unresolved_operator_conflicts", e))?;
    let guard_applied = input.mode == "checkpoint_rollback" || input.mode == "window";
    if guard_applied && unresolved_operator_conflicts > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Replay blocked by checkpoint safety guard: unresolved operator-required conflicts exist.".to_string(),
        ]));
    }

    let pre_checkpoint: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
            [],
        ))
        .await?
        .as_ref()
        .map(|r| r.try_get("", "checkpoint_token"))
        .transpose()
        .map_err(|e| decode_err("checkpoint_token", e))?;

    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_replay_runs (
            replay_key, mode, status, reason, requested_by_id, scope_json, pre_replay_checkpoint,
            post_replay_checkpoint, result_json, created_at, started_at
        ) VALUES (
            ?, ?, 'running', ?, ?, ?, ?, NULL, NULL,
            strftime('%Y-%m-%dT%H:%M:%SZ','now'),
            strftime('%Y-%m-%dT%H:%M:%SZ','now')
        )",
        [
            input.replay_key.clone().into(),
            input.mode.clone().into(),
            input.reason.clone().into(),
            user_id.into(),
            serde_json::to_string(&input)
                .map_err(AppError::Serialization)?
                .into(),
            pre_checkpoint.clone().into(),
        ],
    ))
    .await?;
    let run_id: i64 = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id FROM sync_replay_runs WHERE replay_key = ?",
            [input.replay_key.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to resolve replay run id.".to_string()))?
        .try_get("", "id")
        .map_err(|e| decode_err("id", e))?;

    let mut requeued_outbox_count = 0_i64;
    let mut transitioned_conflict_count = 0_i64;
    match input.mode.as_str() {
        "single_item" => {
            if let Some(outbox_id) = input.outbox_id {
                let result = tx
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE sync_outbox
                         SET status = 'pending',
                             rejection_code = NULL,
                             rejection_message = NULL,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE id = ?
                           AND status = 'rejected'",
                        [outbox_id.into()],
                    ))
                    .await?;
                requeued_outbox_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            }
            if let Some(conflict_id) = input.conflict_id {
                let result = tx
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE sync_conflicts
                         SET status = 'triaged',
                             resolution_action = 'retry_later',
                             resolution_note = 'Requeued by replay workflow.',
                             resolved_by_id = ?,
                             resolved_at = NULL,
                             row_version = row_version + 1,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE id = ?",
                        [user_id.into(), conflict_id.into()],
                    ))
                    .await?;
                transitioned_conflict_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            }
        }
        "batch" => {
            let batch_id = input.server_batch_id.unwrap_or_default();
            let result = tx
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sync_outbox
                     SET status = 'pending',
                         rejection_code = NULL,
                         rejection_message = NULL,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE id IN (
                        SELECT linked_outbox_id
                        FROM sync_conflicts
                        WHERE source_batch_id = ?
                          AND linked_outbox_id IS NOT NULL
                     )
                       AND status = 'rejected'",
                    [batch_id.clone().into()],
                ))
                .await?;
            requeued_outbox_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            let result = tx
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sync_conflicts
                     SET status = 'triaged',
                         resolution_action = 'retry_later',
                         resolution_note = 'Batch replay requested by operator.',
                         resolved_by_id = ?,
                         resolved_at = NULL,
                         row_version = row_version + 1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE source_batch_id = ?
                       AND status IN ('new', 'escalated', 'triaged')",
                    [user_id.into(), batch_id.into()],
                ))
                .await?;
            transitioned_conflict_count += i64::try_from(result.rows_affected()).unwrap_or(0);
        }
        "window" => {
            let start = input.window_start.unwrap_or_default();
            let end = input.window_end.unwrap_or_default();
            let result = tx
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sync_outbox
                     SET status = 'pending',
                         rejection_code = NULL,
                         rejection_message = NULL,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE status = 'rejected'
                       AND updated_at >= ?
                       AND updated_at <= ?",
                    [start.clone().into(), end.clone().into()],
                ))
                .await?;
            requeued_outbox_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            let result = tx
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE sync_conflicts
                     SET status = 'triaged',
                         resolution_action = 'retry_later',
                         resolution_note = 'Window replay requested by operator.',
                         resolved_by_id = ?,
                         resolved_at = NULL,
                         row_version = row_version + 1,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE updated_at >= ?
                       AND updated_at <= ?
                       AND status IN ('new', 'escalated', 'triaged')",
                    [user_id.into(), start.into(), end.into()],
                ))
                .await?;
            transitioned_conflict_count += i64::try_from(result.rows_affected()).unwrap_or(0);
        }
        "checkpoint_rollback" => {
            let checkpoint_token = input.checkpoint_token.unwrap_or_default();
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO sync_checkpoint (
                    id, checkpoint_token, last_idempotency_key, protocol_version, policy_metadata_json,
                    last_sync_at, updated_at
                ) VALUES (
                    1, ?, NULL, ?, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now')
                )
                ON CONFLICT(id) DO UPDATE SET
                    checkpoint_token = excluded.checkpoint_token,
                    protocol_version = excluded.protocol_version,
                    updated_at = excluded.updated_at",
                [
                    checkpoint_token.into(),
                    SYNC_PROTOCOL_VERSION_V1.to_string().into(),
                ],
            ))
            .await?;
        }
        _ => {}
    }

    let post_checkpoint: Option<String> = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
            [],
        ))
        .await?
        .as_ref()
        .map(|r| r.try_get("", "checkpoint_token"))
        .transpose()
        .map_err(|e| decode_err("post_checkpoint", e))?;

    let result_json = serde_json::json!({
        "requeued_outbox_count": requeued_outbox_count,
        "transitioned_conflict_count": transitioned_conflict_count,
        "guard_applied": guard_applied
    })
    .to_string();

    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_replay_runs
         SET status = 'completed',
             post_replay_checkpoint = ?,
             result_json = ?,
             finished_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [post_checkpoint.clone().into(), result_json.into(), run_id.into()],
    ))
    .await?;
    tx.commit().await?;

    let run_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, replay_key, mode, status, reason, requested_by_id, scope_json,
                    pre_replay_checkpoint, post_replay_checkpoint, result_json, created_at, started_at, finished_at
             FROM sync_replay_runs
             WHERE id = ?",
            [run_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "sync_replay_run".to_string(),
            id: run_id.to_string(),
        })?;

    Ok(ReplaySyncFailuresResult {
        run: to_sync_replay_run(&run_row)?,
        requeued_outbox_count,
        transitioned_conflict_count,
        checkpoint_token_after: post_checkpoint,
        guard_applied,
    })
}

pub async fn list_sync_replay_runs(db: &DatabaseConnection, limit: Option<i64>) -> AppResult<Vec<SyncReplayRun>> {
    let page_size = limit.unwrap_or(100).clamp(1, 500);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, replay_key, mode, status, reason, requested_by_id, scope_json,
                    pre_replay_checkpoint, post_replay_checkpoint, result_json, created_at, started_at, finished_at
             FROM sync_replay_runs
             ORDER BY id DESC
             LIMIT ?",
            [page_size.into()],
        ))
        .await?;
    rows.into_iter().map(|row| to_sync_replay_run(&row)).collect()
}

pub async fn preview_sync_repair(
    db: &DatabaseConnection,
    user_id: i64,
    input: SyncRepairPreviewInput,
) -> AppResult<SyncRepairPreview> {
    let mut validation_errors = Vec::new();
    validate_non_empty("mode", &input.mode, &mut validation_errors);
    validate_non_empty("reason", &input.reason, &mut validation_errors);
    if !supported_repair_mode(&input.mode) {
        validation_errors.push(
            "Unsupported repair mode. Allowed: requeue_rejected_outbox, retry_operator_conflicts, checkpoint_realign."
                .to_string(),
        );
    }
    if input.mode.contains("reset") {
        validation_errors.push(
            "Destructive reset is disabled. Use scoped repair modes only.".to_string(),
        );
    }
    if input.mode == "checkpoint_realign" {
        validate_non_empty(
            "checkpoint_token",
            input.checkpoint_token.as_deref().unwrap_or_default(),
            &mut validation_errors,
        );
    }
    if !validation_errors.is_empty() {
        return Err(AppError::ValidationFailed(validation_errors));
    }

    let plan_id = format!("repair-{}", chrono::Utc::now().timestamp_millis());
    let mut affected_outbox_count = 0_i64;
    let mut affected_conflict_count = 0_i64;
    let mut warnings = Vec::new();
    let projected_checkpoint_token: Option<String>;

    match input.mode.as_str() {
        "requeue_rejected_outbox" => {
            if let Some(ids) = &input.outbox_ids {
                if ids.is_empty() {
                    warnings.push("No outbox_ids supplied; no rows will be affected.".to_string());
                } else {
                    for outbox_id in ids {
                        let count: i64 = db
                            .query_one(Statement::from_sql_and_values(
                                DatabaseBackend::Sqlite,
                                "SELECT COUNT(*) AS count
                                 FROM sync_outbox
                                 WHERE id = ? AND status = 'rejected'",
                                [(*outbox_id).into()],
                            ))
                            .await?
                            .ok_or_else(|| AppError::SyncError("Failed to count rejected outbox rows.".to_string()))?
                            .try_get("", "count")
                            .map_err(|e| decode_err("count", e))?;
                        affected_outbox_count += count;
                    }
                }
            } else if let Some(server_batch_id) = &input.server_batch_id {
                affected_outbox_count = db
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "SELECT COUNT(*) AS count
                         FROM sync_outbox
                         WHERE status = 'rejected'
                           AND id IN (
                                SELECT linked_outbox_id
                                FROM sync_conflicts
                                WHERE source_batch_id = ?
                                  AND linked_outbox_id IS NOT NULL
                           )",
                        [server_batch_id.clone().into()],
                    ))
                    .await?
                    .ok_or_else(|| AppError::SyncError("Failed to count batch-scoped outbox rows.".to_string()))?
                    .try_get("", "count")
                    .map_err(|e| decode_err("count", e))?;
            } else {
                affected_outbox_count = db
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'rejected'",
                        [],
                    ))
                    .await?
                    .ok_or_else(|| AppError::SyncError("Failed to count rejected outbox rows.".to_string()))?
                    .try_get("", "count")
                    .map_err(|e| decode_err("count", e))?;
            }
            projected_checkpoint_token = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
                    [],
                ))
                .await?
                .as_ref()
                .map(|r| r.try_get("", "checkpoint_token"))
                .transpose()
                .map_err(|e| decode_err("checkpoint_token", e))?;
        }
        "retry_operator_conflicts" => {
            if let Some(conflict_ids) = &input.conflict_ids {
                if conflict_ids.is_empty() {
                    warnings.push("No conflict_ids supplied; no rows will be affected.".to_string());
                } else {
                    for conflict_id in conflict_ids {
                        let count: i64 = db
                            .query_one(Statement::from_sql_and_values(
                                DatabaseBackend::Sqlite,
                                "SELECT COUNT(*) AS count
                                 FROM sync_conflicts
                                 WHERE id = ?
                                   AND status IN ('new', 'triaged', 'escalated')",
                                [(*conflict_id).into()],
                            ))
                            .await?
                            .ok_or_else(|| AppError::SyncError("Failed to count scoped conflicts.".to_string()))?
                            .try_get("", "count")
                            .map_err(|e| decode_err("count", e))?;
                        affected_conflict_count += count;
                    }
                }
            } else if let Some(server_batch_id) = &input.server_batch_id {
                affected_conflict_count = db
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "SELECT COUNT(*) AS count
                         FROM sync_conflicts
                         WHERE source_batch_id = ?
                           AND status IN ('new', 'triaged', 'escalated')",
                        [server_batch_id.clone().into()],
                    ))
                    .await?
                    .ok_or_else(|| AppError::SyncError("Failed to count batch-scoped conflicts.".to_string()))?
                    .try_get("", "count")
                    .map_err(|e| decode_err("count", e))?;
            } else {
                affected_conflict_count = db
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "SELECT COUNT(*) AS count
                         FROM sync_conflicts
                         WHERE status IN ('new', 'triaged', 'escalated')",
                        [],
                    ))
                    .await?
                    .ok_or_else(|| AppError::SyncError("Failed to count conflicts.".to_string()))?
                    .try_get("", "count")
                    .map_err(|e| decode_err("count", e))?;
            }
            projected_checkpoint_token = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
                    [],
                ))
                .await?
                .as_ref()
                .map(|r| r.try_get("", "checkpoint_token"))
                .transpose()
                .map_err(|e| decode_err("checkpoint_token", e))?;
        }
        "checkpoint_realign" => {
            projected_checkpoint_token = input.checkpoint_token.clone();
            warnings.push("Checkpoint realign updates token pointer only; data rows are untouched.".to_string());
        }
        _ => {
            return Err(AppError::ValidationFailed(vec![
                "Unsupported repair mode.".to_string(),
            ]));
        }
    }

    if affected_outbox_count == 0 && affected_conflict_count == 0 && input.mode != "checkpoint_realign" {
        warnings.push("Repair preview has zero affected rows.".to_string());
    }
    let risk_level = if input.mode == "checkpoint_realign" {
        "high".to_string()
    } else if affected_outbox_count + affected_conflict_count > 100 {
        "high".to_string()
    } else if affected_outbox_count + affected_conflict_count > 20 {
        "medium".to_string()
    } else {
        "low".to_string()
    };

    let preview = SyncRepairPreview {
        plan_id: plan_id.clone(),
        mode: input.mode.clone(),
        reason: input.reason.clone(),
        affected_outbox_count,
        affected_conflict_count,
        projected_checkpoint_token: projected_checkpoint_token.clone(),
        warnings,
        requires_confirmation: true,
        risk_level,
    };

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_repair_actions (
            plan_id, mode, status, reason, created_by_id, executed_by_id, scope_json,
            preview_json, result_json, created_at, executed_at
         ) VALUES (
            ?, ?, 'previewed', ?, ?, NULL, ?, ?, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), NULL
         )",
        [
            plan_id.into(),
            input.mode.clone().into(),
            input.reason.clone().into(),
            user_id.into(),
            serde_json::to_string(&input).map_err(AppError::Serialization)?.into(),
            serde_json::to_string(&preview).map_err(AppError::Serialization)?.into(),
        ],
    ))
    .await?;

    let audit_actor_id = actor_id_for_audit(db, user_id).await?;
    write_audit_event(
        db,
        AuditEventInput {
            action_code: "sync.repair.preview".to_string(),
            target_type: Some("sync_repair_actions".to_string()),
            target_id: Some(preview.plan_id.clone()),
            actor_id: audit_actor_id,
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "operations".to_string(),
            details_json: Some(serde_json::json!({
                "mode": preview.mode,
                "reason": preview.reason,
                "affected_outbox_count": preview.affected_outbox_count,
                "affected_conflict_count": preview.affected_conflict_count,
                "projected_checkpoint_token": preview.projected_checkpoint_token
            })),
        },
    )
    .await?;

    Ok(preview)
}

pub async fn execute_sync_repair(
    db: &DatabaseConnection,
    user_id: i64,
    input: ExecuteSyncRepairInput,
) -> AppResult<SyncRepairExecutionResult> {
    let mut validation_errors = Vec::new();
    validate_non_empty("plan_id", &input.plan_id, &mut validation_errors);
    if input.confirm_phrase != "CONFIRM_SYNC_REPAIR" {
        validation_errors.push("confirm_phrase must be exactly 'CONFIRM_SYNC_REPAIR'.".to_string());
    }
    if !validation_errors.is_empty() {
        return Err(AppError::ValidationFailed(validation_errors));
    }

    let plan_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, plan_id, mode, status, reason, scope_json
             FROM sync_repair_actions
             WHERE plan_id = ?",
            [input.plan_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "sync_repair_action".to_string(),
            id: input.plan_id.clone(),
        })?;
    let status: String = plan_row
        .try_get("", "status")
        .map_err(|e| decode_err("status", e))?;
    if status != "previewed" {
        return Err(AppError::ValidationFailed(vec![
            "Repair plan is no longer executable (must be in previewed status).".to_string(),
        ]));
    }
    let mode: String = plan_row.try_get("", "mode").map_err(|e| decode_err("mode", e))?;
    if !supported_repair_mode(&mode) || mode.contains("reset") {
        return Err(AppError::ValidationFailed(vec![
            "Destructive or unsupported repair mode blocked.".to_string(),
        ]));
    }
    let scope_json: String = plan_row
        .try_get("", "scope_json")
        .map_err(|e| decode_err("scope_json", e))?;
    let preview_scope: SyncRepairPreviewInput =
        serde_json::from_str(&scope_json).map_err(AppError::Serialization)?;

    let tx = db.begin().await?;
    let mut requeued_outbox_count = 0_i64;
    let mut transitioned_conflict_count = 0_i64;

    match mode.as_str() {
        "requeue_rejected_outbox" => {
            if let Some(outbox_ids) = preview_scope.outbox_ids {
                for outbox_id in outbox_ids {
                    let result = tx
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Sqlite,
                            "UPDATE sync_outbox
                             SET status = 'pending',
                                 rejection_code = NULL,
                                 rejection_message = NULL,
                                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                             WHERE id = ?
                               AND status = 'rejected'",
                            [outbox_id.into()],
                        ))
                        .await?;
                    requeued_outbox_count += i64::try_from(result.rows_affected()).unwrap_or(0);
                }
            } else if let Some(server_batch_id) = preview_scope.server_batch_id {
                let result = tx
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE sync_outbox
                         SET status = 'pending',
                             rejection_code = NULL,
                             rejection_message = NULL,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE status = 'rejected'
                           AND id IN (
                                SELECT linked_outbox_id
                                FROM sync_conflicts
                                WHERE source_batch_id = ?
                                  AND linked_outbox_id IS NOT NULL
                           )",
                        [server_batch_id.into()],
                    ))
                    .await?;
                requeued_outbox_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            } else {
                let result = tx
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE sync_outbox
                         SET status = 'pending',
                             rejection_code = NULL,
                             rejection_message = NULL,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE status = 'rejected'",
                        [],
                    ))
                    .await?;
                requeued_outbox_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            }
        }
        "retry_operator_conflicts" => {
            if let Some(conflict_ids) = preview_scope.conflict_ids {
                for conflict_id in conflict_ids {
                    let result = tx
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Sqlite,
                            "UPDATE sync_conflicts
                             SET status = 'triaged',
                                 resolution_action = 'retry_later',
                                 resolution_note = 'Retry queued from repair workflow.',
                                 resolved_by_id = ?,
                                 resolved_at = NULL,
                                 row_version = row_version + 1,
                                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                             WHERE id = ?
                               AND status IN ('new', 'triaged', 'escalated')",
                            [user_id.into(), conflict_id.into()],
                        ))
                        .await?;
                    transitioned_conflict_count += i64::try_from(result.rows_affected()).unwrap_or(0);
                }
            } else if let Some(server_batch_id) = preview_scope.server_batch_id {
                let result = tx
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE sync_conflicts
                         SET status = 'triaged',
                             resolution_action = 'retry_later',
                             resolution_note = 'Retry queued from batch-scoped repair workflow.',
                             resolved_by_id = ?,
                             resolved_at = NULL,
                             row_version = row_version + 1,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE source_batch_id = ?
                           AND status IN ('new', 'triaged', 'escalated')",
                        [user_id.into(), server_batch_id.into()],
                    ))
                    .await?;
                transitioned_conflict_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            } else {
                let result = tx
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Sqlite,
                        "UPDATE sync_conflicts
                         SET status = 'triaged',
                             resolution_action = 'retry_later',
                             resolution_note = 'Retry queued from global repair workflow.',
                             resolved_by_id = ?,
                             resolved_at = NULL,
                             row_version = row_version + 1,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE status IN ('new', 'triaged', 'escalated')",
                        [user_id.into()],
                    ))
                    .await?;
                transitioned_conflict_count += i64::try_from(result.rows_affected()).unwrap_or(0);
            }
        }
        "checkpoint_realign" => {
            let checkpoint_token = preview_scope
                .checkpoint_token
                .ok_or_else(|| AppError::ValidationFailed(vec!["checkpoint_token is required.".to_string()]))?;
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO sync_checkpoint (
                    id, checkpoint_token, last_idempotency_key, protocol_version, policy_metadata_json,
                    last_sync_at, updated_at
                ) VALUES (
                    1, ?, NULL, ?, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now')
                )
                ON CONFLICT(id) DO UPDATE SET
                    checkpoint_token = excluded.checkpoint_token,
                    protocol_version = excluded.protocol_version,
                    updated_at = excluded.updated_at",
                [checkpoint_token.into(), SYNC_PROTOCOL_VERSION_V1.to_string().into()],
            ))
            .await?;
        }
        _ => {
            return Err(AppError::ValidationFailed(vec![
                "Unsupported repair mode.".to_string(),
            ]));
        }
    }

    let checkpoint_token_after: Option<String> = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
            [],
        ))
        .await?
        .as_ref()
        .map(|r| r.try_get("", "checkpoint_token"))
        .transpose()
        .map_err(|e| decode_err("checkpoint_token_after", e))?;
    let executed_at = chrono::Utc::now().to_rfc3339();
    let result_json = serde_json::json!({
        "requeued_outbox_count": requeued_outbox_count,
        "transitioned_conflict_count": transitioned_conflict_count,
        "checkpoint_token_after": checkpoint_token_after
    })
    .to_string();
    tx.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_repair_actions
         SET status = 'executed',
             executed_by_id = ?,
             result_json = ?,
             executed_at = ?
         WHERE plan_id = ?",
        [
            user_id.into(),
            result_json.into(),
            executed_at.clone().into(),
            input.plan_id.clone().into(),
        ],
    ))
    .await?;
    tx.commit().await?;

    let audit_actor_id = actor_id_for_audit(db, user_id).await?;
    write_audit_event(
        db,
        AuditEventInput {
            action_code: "sync.repair.execute".to_string(),
            target_type: Some("sync_repair_actions".to_string()),
            target_id: Some(input.plan_id.clone()),
            actor_id: audit_actor_id,
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "operations".to_string(),
            details_json: Some(serde_json::json!({
                "mode": mode,
                "requeued_outbox_count": requeued_outbox_count,
                "transitioned_conflict_count": transitioned_conflict_count,
                "checkpoint_token_after": checkpoint_token_after
            })),
        },
    )
    .await?;

    Ok(SyncRepairExecutionResult {
        plan_id: input.plan_id,
        mode,
        status: "executed".to_string(),
        requeued_outbox_count,
        transitioned_conflict_count,
        checkpoint_token_after,
        executed_at,
    })
}

pub async fn list_sync_repair_actions(
    db: &DatabaseConnection,
    limit: Option<i64>,
) -> AppResult<Vec<SyncRepairActionRecord>> {
    let page_size = limit.unwrap_or(100).clamp(1, 500);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, plan_id, mode, status, reason, created_by_id, executed_by_id,
                    scope_json, preview_json, result_json, created_at, executed_at
             FROM sync_repair_actions
             ORDER BY id DESC
             LIMIT ?",
            [page_size.into()],
        ))
        .await?;
    rows.into_iter().map(|row| to_sync_repair_action(&row)).collect()
}

pub async fn get_sync_observability_report(db: &DatabaseConnection) -> AppResult<SyncObservabilityReport> {
    let generated_at = chrono::Utc::now().to_rfc3339();
    let pending_outbox_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'pending'",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query pending outbox count.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("pending_outbox_count", e))?;
    let rejected_outbox_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'rejected'",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query rejected outbox count.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("rejected_outbox_count", e))?;
    let unresolved_conflict_count: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count
             FROM sync_conflicts
             WHERE status IN ('new', 'triaged', 'escalated')",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query unresolved conflicts.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("unresolved_conflict_count", e))?;
    let replay_runs_last_24h: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count
             FROM sync_replay_runs
             WHERE created_at >= datetime('now', '-1 day')",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query replay runs.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("replay_runs_last_24h", e))?;
    let repair_runs_last_24h: i64 = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count
             FROM sync_repair_actions
             WHERE created_at >= datetime('now', '-1 day')",
            [],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to query repair runs.".to_string()))?
        .try_get("", "count")
        .map_err(|e| decode_err("repair_runs_last_24h", e))?;
    let checkpoint_token: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT checkpoint_token FROM sync_checkpoint WHERE id = 1",
            [],
        ))
        .await?
        .as_ref()
        .map(|r| r.try_get("", "checkpoint_token"))
        .transpose()
        .map_err(|e| decode_err("checkpoint_token", e))?;

    let metrics = SyncHealthMetrics {
        generated_at: generated_at.clone(),
        pending_outbox_count,
        rejected_outbox_count,
        unresolved_conflict_count,
        replay_runs_last_24h,
        repair_runs_last_24h,
        checkpoint_token,
    };

    let mut alerts = Vec::new();
    if rejected_outbox_count > 0 {
        alerts.push(SyncHealthAlert {
            code: "SYNC_REJECTED_OUTBOX".to_string(),
            severity: "warning".to_string(),
            message: format!("{rejected_outbox_count} rejected outbox records require remediation."),
            runbook_url: "https://docs.maintafox.com/runbooks/sync/rejected-outbox".to_string(),
        });
    }
    if unresolved_conflict_count > 0 {
        alerts.push(SyncHealthAlert {
            code: "SYNC_CONFLICT_BACKLOG".to_string(),
            severity: "warning".to_string(),
            message: format!("{unresolved_conflict_count} unresolved sync conflicts are open."),
            runbook_url: "https://docs.maintafox.com/runbooks/sync/conflict-review".to_string(),
        });
    }
    if pending_outbox_count > 500 {
        alerts.push(SyncHealthAlert {
            code: "SYNC_BACKLOG_HIGH".to_string(),
            severity: "critical".to_string(),
            message: format!("Pending outbox backlog is {pending_outbox_count}, above safe threshold."),
            runbook_url: "https://docs.maintafox.com/runbooks/sync/backlog-drain".to_string(),
        });
    }
    if alerts.is_empty() {
        alerts.push(SyncHealthAlert {
            code: "SYNC_HEALTHY".to_string(),
            severity: "info".to_string(),
            message: "No active sync reliability alerts.".to_string(),
            runbook_url: "https://docs.maintafox.com/runbooks/sync/overview".to_string(),
        });
    }

    let mut recovery_proofs = Vec::new();
    let resolved_conflicts = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, created_at, resolved_at
             FROM sync_conflicts
             WHERE resolved_at IS NOT NULL
             ORDER BY resolved_at DESC
             LIMIT 20",
            [],
        ))
        .await?;
    for row in resolved_conflicts {
        let id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let created_at: String = row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?;
        let resolved_at: String = row
            .try_get("", "resolved_at")
            .map_err(|e| decode_err("resolved_at", e))?;
        let created_dt = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map_err(|e| AppError::SyncError(format!("Invalid created_at timestamp: {e}")))?;
        let resolved_dt = chrono::DateTime::parse_from_rfc3339(&resolved_at)
            .map_err(|e| AppError::SyncError(format!("Invalid resolved_at timestamp: {e}")))?;
        let duration_seconds = (resolved_dt - created_dt).num_seconds().max(0);
        recovery_proofs.push(SyncRecoveryProof {
            workflow: "conflict_resolution".to_string(),
            reference_id: format!("conflict:{id}"),
            failure_at: created_at,
            recovered_at: resolved_at,
            duration_seconds,
        });
    }

    let executed_repairs = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT plan_id, created_at, executed_at
             FROM sync_repair_actions
             WHERE status = 'executed'
               AND executed_at IS NOT NULL
             ORDER BY executed_at DESC
             LIMIT 20",
            [],
        ))
        .await?;
    for row in executed_repairs {
        let plan_id: String = row
            .try_get("", "plan_id")
            .map_err(|e| decode_err("plan_id", e))?;
        let created_at: String = row
            .try_get("", "created_at")
            .map_err(|e| decode_err("created_at", e))?;
        let executed_at: String = row
            .try_get("", "executed_at")
            .map_err(|e| decode_err("executed_at", e))?;
        let created_dt = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map_err(|e| AppError::SyncError(format!("Invalid repair created_at timestamp: {e}")))?;
        let executed_dt = chrono::DateTime::parse_from_rfc3339(&executed_at)
            .map_err(|e| AppError::SyncError(format!("Invalid repair executed_at timestamp: {e}")))?;
        let duration_seconds = (executed_dt - created_dt).num_seconds().max(0);
        recovery_proofs.push(SyncRecoveryProof {
            workflow: "repair_workflow".to_string(),
            reference_id: format!("repair:{plan_id}"),
            failure_at: created_at,
            recovered_at: executed_at,
            duration_seconds,
        });
    }

    Ok(SyncObservabilityReport {
        metrics,
        alerts,
        recovery_proofs,
        diagnostics_links: vec![
            "https://docs.maintafox.com/runbooks/sync/overview".to_string(),
            "https://docs.maintafox.com/runbooks/sync/recovery-proof".to_string(),
            "https://docs.maintafox.com/runbooks/sync/operator-repair".to_string(),
        ],
    })
}
