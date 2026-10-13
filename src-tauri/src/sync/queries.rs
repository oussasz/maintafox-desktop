use sea_orm::sqlx::error::ErrorKind;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use sha2::{Digest, Sha256};

use crate::audit::writer::{write_audit_event, AuditEventInput};
use crate::commands::product_license::tenant_config_sync_payload;
use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    ApplySyncBatchInput, ApplySyncBatchResult, ExecuteSyncRepairInput, ListOutboxFilter, ReplaySyncFailuresInput,
    ReplaySyncFailuresResult, ResolveSyncConflictInput, StageOutboxItemInput, SyncCheckpoint, SyncConflictFilter,
    SyncConflictRecord, SyncHealthAlert, SyncHealthMetrics, SyncInboundItemInput, SyncInboxItem, SyncMediaManifestItem,
    SyncObservabilityReport, SyncOutboxItem, SyncPushPayload, SyncRecoveryProof, SyncRepairActionRecord,
    SyncRepairExecutionResult, SyncRepairPreview, SyncRepairPreviewInput, SyncReplayRun, SyncStateSummary,
    SyncTypedRejection, SYNC_ENTITY_MEDIA_ATTACHMENTS, SYNC_PROTOCOL_VERSION_V1, SYNC_PROTOCOL_VERSION_V2,
};
use crate::sync::registry::{self, SyncScope};

/// After this many failed deliveries reported by the transport (`record_outbox_delivery_failure`)
/// an outbox item becomes `dead_letter` and is listed as a conflict for a person. It is never
/// dropped: replay puts it back to `pending`.
pub const OUTBOX_MAX_DELIVERY_ATTEMPTS: i64 = 25;

/// Outbox statuses of a change that is still on its way to the server.
const OUTBOX_UNSENT_STATUSES_SQL: &str = "('pending', 'in_flight')";

const OUTBOX_COLUMNS: &str = "id, idempotency_key, entity_type, entity_sync_id, operation, row_version,
    payload_json, payload_hash, status, acknowledged_at, rejection_code, rejection_message,
    origin_machine_id, base_row_version, owner_user_sync_id, attempt_count, last_attempt_at, last_error,
    created_at, updated_at";

const INBOX_COLUMNS: &str = "id, server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
    row_version, payload_json, payload_hash, apply_status, rejection_code, rejection_message,
    server_seq, owner_user_sync_id, attempt_count, last_attempt_at, created_at, updated_at";

pub(crate) const CONFLICT_COLUMNS: &str = "id, conflict_key, source_scope, source_batch_id, linked_outbox_id,
    linked_inbox_id, entity_type, entity_sync_id, operation, conflict_type, local_payload_json,
    inbound_payload_json, authority_side, checkpoint_token, auto_resolution_policy,
    requires_operator_review, recommended_action, status, resolution_action, resolution_note,
    resolved_by_id, resolved_at, row_version, local_row_version, inbound_row_version, created_at, updated_at";

pub(crate) fn hash_payload(payload: &str) -> String {
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
    if version != SYNC_PROTOCOL_VERSION_V1 && version != SYNC_PROTOCOL_VERSION_V2 {
        return Err(AppError::ValidationFailed(vec![format!(
            "Unsupported sync protocol version '{version}'. Supported: {SYNC_PROTOCOL_VERSION_V1}, {SYNC_PROTOCOL_VERSION_V2}."
        )]));
    }
    Ok(())
}

/// `(authority_side, requires_operator_review, recommended_action, auto_resolution_policy)`.
///
/// Every conflict that touches data needs a person (owner decision: no silent last-write-wins).
/// `recommended_action` is either a resolution action or `review` when only a person can tell.
fn conflict_policy(conflict_type: &str) -> (&'static str, bool, &'static str, &'static str) {
    match conflict_type {
        "NOT_FOUND" | "DUPLICATE_INBOUND" | "STALE_ACK" | "TRANSIENT_NETWORK" | "RATE_LIMIT" => {
            ("remote", false, "retry_later", "auto_retry_then_close")
        }
        "INVALID_INBOUND_PAYLOAD" | "MATERIALIZE_ERROR" => ("remote", true, "escalate", "manual_review_required"),
        "AUTHORITY_MISMATCH" => ("remote", true, "accept_remote", "manual_review_required"),
        "STALE_INBOUND" => ("local", true, "accept_local", "manual_review_required"),
        "CODE_CLASH" => ("mixed", true, "renumber_local", "manual_review_required"),
        "OUTBOX_DEAD_LETTER" => ("local", true, "retry_later", "manual_review_required"),
        _ => ("mixed", true, "review", "manual_review_required"),
    }
}

fn map_resolution_to_status(action: &str) -> Option<&'static str> {
    match action {
        "accept_local" => Some("resolved_local"),
        "accept_remote" => Some("resolved_remote"),
        "retry_later" => Some("triaged"),
        "escalate" => Some("escalated"),
        "dismiss" => Some("dismissed"),
        // Number clash: the local record got a new number and the received one was applied.
        "renumber_local" => Some("merged"),
        _ => None,
    }
}

fn parse_rfc3339(value: &str, field: &str) -> AppResult<()> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be a valid RFC3339 timestamp.")]))?;
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

/// v1 tokens end in `-<sequence>`. v2 tokens (`v2.<payload>.<signature>`) are opaque and signed;
/// the server checks them, so they are never compared here.
fn is_stale_checkpoint(current: &str, incoming: &str) -> bool {
    if current.starts_with("v2.") || incoming.starts_with("v2.") {
        return false;
    }
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
    validate_non_empty("entity_type", &input.entity_type, &mut errors);
    validate_non_empty("entity_sync_id", &input.entity_sync_id, &mut errors);
    validate_non_empty("operation", &input.operation, &mut errors);
    validate_operation(&input.operation, &mut errors);
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
        base_row_version: row
            .try_get("", "base_row_version")
            .map_err(|e| decode_err("base_row_version", e))?,
        owner_user_sync_id: row
            .try_get("", "owner_user_sync_id")
            .map_err(|e| decode_err("owner_user_sync_id", e))?,
        attempt_count: row
            .try_get("", "attempt_count")
            .map_err(|e| decode_err("attempt_count", e))?,
        last_attempt_at: row
            .try_get("", "last_attempt_at")
            .map_err(|e| decode_err("last_attempt_at", e))?,
        last_error: row.try_get("", "last_error").map_err(|e| decode_err("last_error", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
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
        server_seq: row.try_get("", "server_seq").map_err(|e| decode_err("server_seq", e))?,
        owner_user_sync_id: row
            .try_get("", "owner_user_sync_id")
            .map_err(|e| decode_err("owner_user_sync_id", e))?,
        attempt_count: row
            .try_get("", "attempt_count")
            .map_err(|e| decode_err("attempt_count", e))?,
        last_attempt_at: row
            .try_get("", "last_attempt_at")
            .map_err(|e| decode_err("last_attempt_at", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

pub(crate) fn to_sync_conflict(row: &sea_orm::QueryResult) -> AppResult<SyncConflictRecord> {
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
        local_row_version: row
            .try_get("", "local_row_version")
            .map_err(|e| decode_err("local_row_version", e))?,
        inbound_row_version: row
            .try_get("", "inbound_row_version")
            .map_err(|e| decode_err("inbound_row_version", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
    })
}

fn to_sync_replay_run(row: &sea_orm::QueryResult) -> AppResult<SyncReplayRun> {
    Ok(SyncReplayRun {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        replay_key: row.try_get("", "replay_key").map_err(|e| decode_err("replay_key", e))?,
        mode: row.try_get("", "mode").map_err(|e| decode_err("mode", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        reason: row.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
        requested_by_id: row
            .try_get("", "requested_by_id")
            .map_err(|e| decode_err("requested_by_id", e))?,
        scope_json: row.try_get("", "scope_json").map_err(|e| decode_err("scope_json", e))?,
        pre_replay_checkpoint: row
            .try_get("", "pre_replay_checkpoint")
            .map_err(|e| decode_err("pre_replay_checkpoint", e))?,
        post_replay_checkpoint: row
            .try_get("", "post_replay_checkpoint")
            .map_err(|e| decode_err("post_replay_checkpoint", e))?,
        result_json: row
            .try_get("", "result_json")
            .map_err(|e| decode_err("result_json", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
        started_at: row.try_get("", "started_at").map_err(|e| decode_err("started_at", e))?,
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
        scope_json: row.try_get("", "scope_json").map_err(|e| decode_err("scope_json", e))?,
        preview_json: row
            .try_get("", "preview_json")
            .map_err(|e| decode_err("preview_json", e))?,
        result_json: row
            .try_get("", "result_json")
            .map_err(|e| decode_err("result_json", e))?,
        created_at: row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?,
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

/// Stable id of this local database, recorded as `origin_machine_id` on every staged change.
pub async fn local_replica_id(db: &impl ConnectionTrait) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT replica_id FROM sync_replica_identity WHERE id = 1",
            [],
        ))
        .await?
        .ok_or_else(|| {
            AppError::SyncError("sync_replica_identity is missing; database migrations are incomplete.".into())
        })?;
    row.try_get("", "replica_id").map_err(|e| decode_err("replica_id", e))
}

/// Recreate the one-row sync control records after the local tenant data was wiped
/// (`reset_local_tenant_runtime_data` deletes every table outside its keep-list): a new replica id
/// (the wiped database is a new replica) and an empty transport state. On a new database the
/// migrations (160, 161) create them.
pub async fn ensure_sync_control_rows(db: &impl ConnectionTrait) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT OR IGNORE INTO sync_replica_identity (id, replica_id) VALUES (1, ?)",
        [uuid::Uuid::new_v4().to_string().into()],
    ))
    .await?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT OR IGNORE INTO sync_transport_state (id) VALUES (1)",
        [],
    ))
    .await?;
    Ok(())
}

/// Last row version the server is known to hold for an entity (acknowledged or applied inbound).
pub(crate) async fn synced_row_version(
    db: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<(i64, String)>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT synced_row_version, synced_payload_hash FROM sync_entity_state
             WHERE entity_type = ? AND entity_sync_id = ?",
            [entity_type.into(), entity_sync_id.into()],
        ))
        .await?;
    row.map(|r| -> AppResult<(i64, String)> {
        Ok((
            r.try_get("", "synced_row_version")
                .map_err(|e| decode_err("synced_row_version", e))?,
            r.try_get("", "synced_payload_hash")
                .map_err(|e| decode_err("synced_payload_hash", e))?,
        ))
    })
    .transpose()
}

/// Record the version the server now holds for an entity. Never moves backwards.
pub(crate) async fn record_synced_state(
    db: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    row_version: i64,
    payload_hash: &str,
    source: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_entity_state
            (entity_type, entity_sync_id, synced_row_version, synced_payload_hash, source, updated_at)
         VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(entity_type, entity_sync_id) DO UPDATE SET
            synced_row_version = excluded.synced_row_version,
            synced_payload_hash = excluded.synced_payload_hash,
            source = excluded.source,
            updated_at = excluded.updated_at
         WHERE excluded.synced_row_version >= sync_entity_state.synced_row_version",
        [
            entity_type.into(),
            entity_sync_id.into(),
            row_version.into(),
            payload_hash.into(),
            source.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Replace the known server version for an entity (used when a person resolves a conflict with the
/// server's version as the new base).
pub(crate) async fn overwrite_synced_state(
    db: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    row_version: i64,
    payload_hash: &str,
    source: &str,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_entity_state
            (entity_type, entity_sync_id, synced_row_version, synced_payload_hash, source, updated_at)
         VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(entity_type, entity_sync_id) DO UPDATE SET
            synced_row_version = excluded.synced_row_version,
            synced_payload_hash = excluded.synced_payload_hash,
            source = excluded.source,
            updated_at = excluded.updated_at",
        [
            entity_type.into(),
            entity_sync_id.into(),
            row_version.into(),
            payload_hash.into(),
            source.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn owner_user_sync_id_for(
    db: &impl ConnectionTrait,
    def: &registry::SyncEntityDef,
    entity_sync_id: &str,
) -> AppResult<Option<String>> {
    if def.scope == SyncScope::Tenant {
        return Ok(None);
    }
    let column = def.owner_user_column.ok_or_else(|| {
        AppError::SyncError(format!(
            "User-scoped sync entity '{}' has no owner column in the registry.",
            def.entity_type
        ))
    })?;
    // Table and column names come from the static registry, never from input.
    let sql = format!(
        "SELECT u.entity_sync_id AS owner FROM {table} t
         JOIN user_accounts u ON u.id = t.{column}
         WHERE t.entity_sync_id = ?",
        table = def.table
    );
    let owner: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [entity_sync_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "owner"))
        .transpose()
        .map_err(|e| decode_err("owner_user_sync_id", e))?
        .flatten()
        .filter(|v| !v.trim().is_empty());
    owner.map(Some).ok_or_else(|| {
        AppError::SyncError(format!(
            "{} {entity_sync_id} is user-scoped but has no owning user with a sync id.",
            def.entity_type
        ))
    })
}

async fn latest_unsent_outbox_item(
    db: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<SyncOutboxItem>> {
    let sql = format!(
        "SELECT {OUTBOX_COLUMNS} FROM sync_outbox
         WHERE entity_type = ? AND entity_sync_id = ? AND status IN {OUTBOX_UNSENT_STATUSES_SQL}
         ORDER BY id DESC LIMIT 1"
    );
    db.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        &sql,
        [entity_type.into(), entity_sync_id.into()],
    ))
    .await?
    .map(|row| to_sync_outbox_item(&row))
    .transpose()
}

/// Stage a change in the outbox. Call it with the same connection or transaction as the business
/// write, so a failed stage fails the write.
///
/// Versioning (protocol v2, `SYNC_PROTOCOL_V2.md` §3.1):
/// - `base_row_version` is the version of the previous unsent change of the same entity, else the
///   version the server is known to hold (`sync_entity_state`), else `NULL` (the row is created
///   here);
/// - the envelope `row_version` is `base_row_version + 1`, or `1` for a create;
/// - restaging an unchanged payload while the previous change is still unsent returns that change;
/// - every new envelope gets a fresh UUID idempotency key, unique per tenant forever.
pub async fn stage_outbox_item(db: &impl ConnectionTrait, input: StageOutboxItemInput) -> AppResult<SyncOutboxItem> {
    stage_outbox_item_inner(db, input, None).await
}

/// [`stage_outbox_item`] with the owner of a user-scoped entity given by the caller (the generic
/// capture knows it from the row, including for a deleted row and for natural-key rows that have
/// no `entity_sync_id` column to look the owner up by).
pub(crate) async fn stage_outbox_item_with_owner(
    db: &impl ConnectionTrait,
    input: StageOutboxItemInput,
    owner_user_sync_id: Option<String>,
) -> AppResult<SyncOutboxItem> {
    stage_outbox_item_inner(db, input, Some(owner_user_sync_id)).await
}

async fn stage_outbox_item_inner(
    db: &impl ConnectionTrait,
    input: StageOutboxItemInput,
    owner_override: Option<Option<String>>,
) -> AppResult<SyncOutboxItem> {
    validate_outbox_input(&input)?;
    let def = registry::require_entity(&input.entity_type)?;
    let payload_hash = hash_payload(&input.payload_json);

    let previous = latest_unsent_outbox_item(db, &input.entity_type, &input.entity_sync_id).await?;
    if let Some(prev) = &previous {
        if prev.payload_hash == payload_hash && prev.operation == input.operation {
            return Ok(prev.clone());
        }
    }
    let base_row_version = match &previous {
        Some(prev) => Some(prev.row_version),
        None => synced_row_version(db, &input.entity_type, &input.entity_sync_id)
            .await?
            .map(|(version, _)| version),
    };
    let row_version = base_row_version.map_or(1, |base| base + 1);
    let owner_user_sync_id = match owner_override {
        Some(owner) => match def.scope {
            SyncScope::Tenant => None,
            SyncScope::User => Some(owner.filter(|o| !o.trim().is_empty()).ok_or_else(|| {
                AppError::SyncError(format!(
                    "{} {} is user-scoped but has no owning user.",
                    def.entity_type, input.entity_sync_id
                ))
            })?),
        },
        None => owner_user_sync_id_for(db, def, &input.entity_sync_id).await?,
    };
    let origin_machine_id = local_replica_id(db).await?;
    let idempotency_key = uuid::Uuid::new_v4().to_string();

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_outbox (
            idempotency_key, entity_type, entity_sync_id, operation, row_version, base_row_version,
            owner_user_sync_id, payload_json, payload_hash, status, origin_machine_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
        [
            idempotency_key.clone().into(),
            input.entity_type.into(),
            input.entity_sync_id.into(),
            input.operation.into(),
            row_version.into(),
            base_row_version.into(),
            owner_user_sync_id.into(),
            input.payload_json.into(),
            payload_hash.into(),
            origin_machine_id.into(),
        ],
    ))
    .await?;

    let sql = format!("SELECT {OUTBOX_COLUMNS} FROM sync_outbox WHERE idempotency_key = ?");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [idempotency_key.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to fetch staged outbox item.".to_string()))?;
    to_sync_outbox_item(&row)
}

/// Report that delivering outbox items failed for a reason other than a server answer (the
/// transport calls this). Items go back to `pending`; after `OUTBOX_MAX_DELIVERY_ATTEMPTS` they
/// become `dead_letter` and a conflict is recorded for a person.
pub async fn record_outbox_delivery_failure(
    db: &DatabaseConnection,
    outbox_ids: &[i64],
    error: &str,
) -> AppResult<i64> {
    let tx = db.begin().await?;
    let mut dead_lettered = 0_i64;
    for id in outbox_ids {
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM sync_outbox WHERE id = ? AND status = 'in_flight'");
        let Some(row) = tx
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                &sql,
                [(*id).into()],
            ))
            .await?
        else {
            continue;
        };
        let item = to_sync_outbox_item(&row)?;
        if item.attempt_count >= OUTBOX_MAX_DELIVERY_ATTEMPTS {
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_outbox
                 SET status = 'dead_letter', last_error = ?,
                     dead_lettered_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE id = ?",
                [error.into(), (*id).into()],
            ))
            .await?;
            record_sync_conflict(
                &tx,
                NewSyncConflict {
                    source_scope: "outbox",
                    source_batch_id: None,
                    linked_outbox_id: Some(item.id),
                    linked_inbox_id: None,
                    entity_type: item.entity_type.clone(),
                    entity_sync_id: item.entity_sync_id.clone(),
                    operation: item.operation.clone(),
                    conflict_type: "OUTBOX_DEAD_LETTER".to_string(),
                    local_payload_json: Some(item.payload_json.clone()),
                    inbound_payload_json: None,
                    checkpoint_token: None,
                    local_row_version: Some(item.row_version),
                    inbound_row_version: None,
                },
            )
            .await?;
            dead_lettered += 1;
        } else {
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_outbox
                 SET status = 'pending', last_error = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE id = ?",
                [error.into(), (*id).into()],
            ))
            .await?;
        }
    }
    tx.commit().await?;
    Ok(dead_lettered)
}

pub async fn list_outbox_items(db: &DatabaseConnection, filter: ListOutboxFilter) -> AppResult<Vec<SyncOutboxItem>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let rows = if let Some(status) = filter.status {
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM sync_outbox WHERE status = ? ORDER BY id ASC LIMIT ?");
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [status.into(), limit.into()],
        ))
        .await?
    } else {
        let sql = format!("SELECT {OUTBOX_COLUMNS} FROM sync_outbox ORDER BY id ASC LIMIT ?");
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [limit.into()],
        ))
        .await?
    };

    rows.into_iter().map(|row| to_sync_outbox_item(&row)).collect()
}

/// Users signed in or trusted on this device; the server delivers their personal rows here.
pub(crate) async fn current_user_sync_ids(db: &impl ConnectionTrait) -> AppResult<Vec<String>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT DISTINCT u.entity_sync_id AS sid
             FROM user_accounts u
             WHERE u.entity_sync_id IS NOT NULL AND trim(u.entity_sync_id) <> ''
               AND u.id IN (
                   SELECT user_id FROM trusted_devices WHERE is_revoked = 0
                   UNION
                   SELECT user_id FROM app_sessions WHERE is_revoked = 0
               )
             ORDER BY u.entity_sync_id",
            [],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            r.try_get::<String>("", "sid")
                .map_err(|e| decode_err("current_user_sync_id", e))
        })
        .collect()
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

    // Unsent changes in staging order. An entity whose earlier change is waiting for a person
    // (`conflict`) is held back: its later changes were built on the refused one.
    let page_size = limit.unwrap_or(200).clamp(1, 1000);
    let tx = db.begin().await?;
    // Changes captured by the table triggers become outbox envelopes first.
    crate::sync::capture::drain_capture_journal(&tx).await?;
    let sql = format!(
        "SELECT {OUTBOX_COLUMNS} FROM sync_outbox o
         WHERE o.status IN {OUTBOX_UNSENT_STATUSES_SQL}
           AND NOT EXISTS (
               SELECT 1 FROM sync_outbox c
               WHERE c.entity_type = o.entity_type
                 AND c.entity_sync_id = o.entity_sync_id
                 AND c.status = 'conflict'
           )
         ORDER BY o.id ASC
         LIMIT ?"
    );
    let rows = tx
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [page_size.into()],
        ))
        .await?;
    let ids: Vec<i64> = rows
        .iter()
        .map(|row| row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e)))
        .collect::<AppResult<Vec<_>>>()?;
    let mut outbox_batch = Vec::with_capacity(ids.len());
    if !ids.is_empty() {
        let ids_json = serde_json::to_string(&ids)?;
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_outbox
             SET status = 'in_flight',
                 attempt_count = attempt_count + 1,
                 last_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id IN (SELECT value FROM json_each(?))",
            [ids_json.clone().into()],
        ))
        .await?;
        let sql = format!(
            "SELECT {OUTBOX_COLUMNS} FROM sync_outbox WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id ASC"
        );
        let refreshed = tx
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                &sql,
                [ids_json.into()],
            ))
            .await?;
        outbox_batch = refreshed
            .iter()
            .map(to_sync_outbox_item)
            .collect::<AppResult<Vec<_>>>()?;
    }
    let current_user_sync_ids = current_user_sync_ids(&tx).await?;
    tx.commit().await?;

    let tenant_config = tenant_config_sync_payload(db).await?;
    let media_manifest = build_media_manifest_from_outbox(&outbox_batch);

    Ok(SyncPushPayload {
        protocol_version: SYNC_PROTOCOL_VERSION_V2.to_string(),
        checkpoint_token,
        outbox_batch,
        tenant_config,
        device_class: Some("desktop".to_string()),
        media_manifest,
        current_user_sync_ids,
    })
}

fn build_media_manifest_from_outbox(outbox_batch: &[SyncOutboxItem]) -> Option<Vec<SyncMediaManifestItem>> {
    let mut items = Vec::new();
    for item in outbox_batch {
        if item.entity_type != SYNC_ENTITY_MEDIA_ATTACHMENTS {
            continue;
        }
        if item.operation == "delete" || item.operation == "archive" {
            continue;
        }
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(&item.payload_json) else {
            continue;
        };
        let content_sha256 = payload
            .get("content_sha256")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let object_key = payload
            .get("object_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let byte_size = payload.get("byte_size").and_then(|v| v.as_i64()).unwrap_or(0);
        if content_sha256.is_empty() || object_key.is_empty() {
            continue;
        }
        items.push(SyncMediaManifestItem {
            entity_sync_id: item.entity_sync_id.clone(),
            content_sha256,
            object_key,
            byte_size,
        });
    }
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// A conflict to record for a person.
pub(crate) struct NewSyncConflict {
    pub source_scope: &'static str,
    pub source_batch_id: Option<String>,
    pub linked_outbox_id: Option<i64>,
    pub linked_inbox_id: Option<i64>,
    pub entity_type: String,
    pub entity_sync_id: String,
    pub operation: String,
    pub conflict_type: String,
    pub local_payload_json: Option<String>,
    pub inbound_payload_json: Option<String>,
    pub checkpoint_token: Option<String>,
    pub local_row_version: Option<i64>,
    pub inbound_row_version: Option<i64>,
}

/// Insert or refresh a conflict. The key identifies the concrete outbox/inbox row (or inbound
/// version), so a new conflict on an entity after an earlier one was resolved is a new record.
pub(crate) async fn record_sync_conflict(conn: &impl ConnectionTrait, c: NewSyncConflict) -> AppResult<()> {
    let (authority_side, requires_operator_review, recommended_action, auto_resolution_policy) =
        conflict_policy(&c.conflict_type);
    let discriminator = match (c.linked_outbox_id, c.linked_inbox_id, c.inbound_row_version) {
        (Some(outbox_id), _, _) => format!("o{outbox_id}"),
        (None, Some(inbox_id), _) => format!("i{inbox_id}"),
        (None, None, Some(version)) => format!("v{version}"),
        (None, None, None) => "-".to_string(),
    };
    let conflict_key = format!(
        "{}:{}:{}:{}:{}:{}",
        c.source_scope, c.entity_type, c.entity_sync_id, c.operation, c.conflict_type, discriminator
    );
    let initial_status = if requires_operator_review {
        "new"
    } else {
        "resolved_remote"
    };
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO sync_conflicts (
            conflict_key, source_scope, source_batch_id, linked_outbox_id, linked_inbox_id,
            entity_type, entity_sync_id, operation, conflict_type, local_payload_json,
            inbound_payload_json, authority_side, checkpoint_token, auto_resolution_policy,
            requires_operator_review, recommended_action, status, row_version,
            local_row_version, inbound_row_version, created_at, updated_at
        ) VALUES (
            ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?,
            strftime('%Y-%m-%dT%H:%M:%SZ','now'),
            strftime('%Y-%m-%dT%H:%M:%SZ','now')
        )
        ON CONFLICT(conflict_key) DO UPDATE SET
            source_batch_id = excluded.source_batch_id,
            linked_outbox_id = COALESCE(excluded.linked_outbox_id, sync_conflicts.linked_outbox_id),
            linked_inbox_id = COALESCE(excluded.linked_inbox_id, sync_conflicts.linked_inbox_id),
            local_payload_json = COALESCE(excluded.local_payload_json, sync_conflicts.local_payload_json),
            inbound_payload_json = COALESCE(excluded.inbound_payload_json, sync_conflicts.inbound_payload_json),
            local_row_version = COALESCE(excluded.local_row_version, sync_conflicts.local_row_version),
            inbound_row_version = COALESCE(excluded.inbound_row_version, sync_conflicts.inbound_row_version),
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
            c.source_scope.to_string().into(),
            c.source_batch_id.into(),
            c.linked_outbox_id.into(),
            c.linked_inbox_id.into(),
            c.entity_type.into(),
            c.entity_sync_id.into(),
            c.operation.into(),
            c.conflict_type.into(),
            c.local_payload_json.into(),
            c.inbound_payload_json.into(),
            authority_side.to_string().into(),
            c.checkpoint_token.into(),
            auto_resolution_policy.to_string().into(),
            (if requires_operator_review { 1_i64 } else { 0_i64 }).into(),
            recommended_action.to_string().into(),
            initial_status.to_string().into(),
            c.local_row_version.into(),
            c.inbound_row_version.into(),
        ],
    ))
    .await?;
    Ok(())
}

fn strip_hash_prefix(hash: &str) -> &str {
    hash.strip_prefix("sha256:").unwrap_or(hash)
}

/// Store inbound envelopes in the inbox as `pending` (materialized later), in dependency order.
/// An invalid envelope is recorded as a conflict for a person and returned as a typed rejection.
/// Returns `(inserted, duplicates)`.
async fn ingest_inbound_items(
    tx: &impl ConnectionTrait,
    server_batch_id: &str,
    checkpoint_label: &str,
    items: &[SyncInboundItemInput],
    typed_rejections: &mut Vec<SyncTypedRejection>,
) -> AppResult<(i64, i64)> {
    let mut inbound_applied_count: i64 = 0;
    let mut inbound_duplicate_count: i64 = 0;
    let mut inbound_work: Vec<SyncInboundItemInput> = items.to_vec();
    // Stable sort: dependency order first, server order within an entity type.
    inbound_work.sort_by_key(|item| registry::apply_priority(&item.entity_type));
    for inbound in &inbound_work {
        let mut inbound_errors = Vec::new();
        validate_non_empty("entity_type", &inbound.entity_type, &mut inbound_errors);
        validate_non_empty("entity_sync_id", &inbound.entity_sync_id, &mut inbound_errors);
        validate_non_empty("operation", &inbound.operation, &mut inbound_errors);
        validate_operation(&inbound.operation, &mut inbound_errors);
        if serde_json::from_str::<serde_json::Value>(&inbound.payload_json).is_err() {
            inbound_errors.push("payload_json must be a valid JSON document.".to_string());
        }
        let payload_hash = hash_payload(&inbound.payload_json);
        if let Some(server_hash) = &inbound.payload_hash {
            if !strip_hash_prefix(server_hash).eq_ignore_ascii_case(&payload_hash) {
                inbound_errors.push("payload_hash does not match payload_json.".to_string());
            }
        }
        if !inbound_errors.is_empty() {
            typed_rejections.push(SyncTypedRejection {
                scope: "inbound".to_string(),
                entity_sync_id: inbound.entity_sync_id.clone(),
                operation: inbound.operation.clone(),
                rejection_code: "INVALID_INBOUND_PAYLOAD".to_string(),
                rejection_message: inbound_errors.join(" "),
            });
            record_sync_conflict(
                tx,
                NewSyncConflict {
                    source_scope: "inbound",
                    source_batch_id: Some(server_batch_id.to_string()),
                    linked_outbox_id: None,
                    linked_inbox_id: None,
                    entity_type: inbound.entity_type.clone(),
                    entity_sync_id: inbound.entity_sync_id.clone(),
                    operation: inbound.operation.clone(),
                    conflict_type: "INVALID_INBOUND_PAYLOAD".to_string(),
                    local_payload_json: None,
                    inbound_payload_json: Some(inbound.payload_json.clone()),
                    checkpoint_token: Some(checkpoint_label.to_string()),
                    local_row_version: None,
                    inbound_row_version: Some(inbound.row_version),
                },
            )
            .await?;
            continue;
        }

        let operation = if inbound.deleted {
            "delete".to_string()
        } else {
            inbound.operation.clone()
        };
        let insert_result = tx
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "INSERT INTO sync_inbox (
                    server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                    row_version, payload_json, payload_hash, apply_status, server_seq, owner_user_sync_id
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
                [
                    server_batch_id.to_string().into(),
                    checkpoint_label.to_string().into(),
                    inbound.entity_type.clone().into(),
                    inbound.entity_sync_id.clone().into(),
                    operation.into(),
                    inbound.row_version.into(),
                    inbound.payload_json.clone().into(),
                    payload_hash.into(),
                    inbound.server_seq.into(),
                    inbound.owner_user_sync_id.clone().into(),
                ],
            ))
            .await;

        match insert_result {
            Ok(_) => inbound_applied_count += 1,
            Err(err) if is_unique_constraint_error(&err) => inbound_duplicate_count += 1,
            Err(err) => return Err(err.into()),
        }
    }

    Ok((inbound_applied_count, inbound_duplicate_count))
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
        validate_non_empty(
            "rejected.entity_sync_id",
            &rejected.entity_sync_id,
            &mut validation_errors,
        );
        validate_operation(&rejected.operation, &mut validation_errors);
        validate_non_empty(
            "rejected.rejection_code",
            &rejected.rejection_code,
            &mut validation_errors,
        );
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
                     last_error = NULL,
                     server_seq = ?,
                     ack_duplicate = ?,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE idempotency_key = ?
                   AND entity_sync_id = ?
                   AND operation = ?
                   AND status IN ('pending', 'in_flight')",
                [
                    ack.server_seq.into(),
                    i64::from(ack.duplicate).into(),
                    ack.idempotency_key.clone().into(),
                    ack.entity_sync_id.clone().into(),
                    ack.operation.clone().into(),
                ],
            ))
            .await?;
        let affected = i64::try_from(result.rows_affected()).unwrap_or(0);
        acknowledged_count += affected;
        if affected > 0 {
            let row = tx
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT entity_type, row_version, payload_hash FROM sync_outbox WHERE idempotency_key = ?",
                    [ack.idempotency_key.clone().into()],
                ))
                .await?
                .ok_or_else(|| AppError::SyncError("Acknowledged outbox item disappeared.".into()))?;
            let entity_type: String = row
                .try_get("", "entity_type")
                .map_err(|e| decode_err("entity_type", e))?;
            let row_version: i64 = row
                .try_get("", "row_version")
                .map_err(|e| decode_err("row_version", e))?;
            let payload_hash: String = row
                .try_get("", "payload_hash")
                .map_err(|e| decode_err("payload_hash", e))?;
            record_synced_state(
                &tx,
                &entity_type,
                &ack.entity_sync_id,
                row_version,
                &payload_hash,
                "outbox_ack",
            )
            .await?;
        }
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
                   AND status IN ('pending', 'in_flight', 'acked')",
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
                    "SELECT id, entity_type, payload_json, row_version
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
                let local_row_version: i64 = outbox_row
                    .try_get("", "row_version")
                    .map_err(|e| decode_err("row_version", e))?;
                record_sync_conflict(
                    &tx,
                    NewSyncConflict {
                        source_scope: "outbox",
                        source_batch_id: Some(input.server_batch_id.clone()),
                        linked_outbox_id: Some(linked_outbox_id),
                        linked_inbox_id: None,
                        entity_type,
                        entity_sync_id: rejected.entity_sync_id.clone(),
                        operation: rejected.operation.clone(),
                        conflict_type: rejected.rejection_code.clone(),
                        local_payload_json: Some(local_payload_json),
                        inbound_payload_json: None,
                        checkpoint_token: Some(input.checkpoint_token.clone()),
                        local_row_version: Some(local_row_version),
                        inbound_row_version: None,
                    },
                )
                .await?;
            }
        }
    }

    let mut conflict_count: i64 = 0;
    for conflict in &input.conflict_items {
        let outbox_row = tx
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT id, payload_json FROM sync_outbox
                 WHERE idempotency_key = ? AND entity_type = ? AND entity_sync_id = ?",
                [
                    conflict.idempotency_key.clone().into(),
                    conflict.entity_type.clone().into(),
                    conflict.entity_sync_id.clone().into(),
                ],
            ))
            .await?;
        let (linked_outbox_id, local_payload_json) = match &outbox_row {
            Some(row) => (
                Some(row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?),
                Some(
                    row.try_get::<String>("", "payload_json")
                        .map_err(|e| decode_err("payload_json", e))?,
                ),
            ),
            None => (None, None),
        };
        if let Some(outbox_id) = linked_outbox_id {
            let base_label = conflict
                .local_base_row_version
                .map_or_else(|| "a new row".to_string(), |v| format!("version {v}"));
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_outbox
                 SET status = 'conflict', rejection_code = 'SERVER_CONFLICT',
                     rejection_message = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE id = ?",
                [
                    format!(
                        "Server holds version {} (this change was made on {base_label}).",
                        conflict.server_row_version
                    )
                    .into(),
                    outbox_id.into(),
                ],
            ))
            .await?;
        }
        record_sync_conflict(
            &tx,
            NewSyncConflict {
                source_scope: "outbox",
                source_batch_id: Some(input.server_batch_id.clone()),
                linked_outbox_id,
                linked_inbox_id: None,
                entity_type: conflict.entity_type.clone(),
                entity_sync_id: conflict.entity_sync_id.clone(),
                operation: if conflict.server_deleted {
                    "delete".to_string()
                } else {
                    conflict.server_operation.clone()
                },
                conflict_type: "SERVER_CONFLICT".to_string(),
                local_payload_json,
                inbound_payload_json: conflict.server_payload_json.clone(),
                checkpoint_token: Some(input.checkpoint_token.clone()),
                local_row_version: Some(conflict.local_row_version),
                inbound_row_version: Some(conflict.server_row_version),
            },
        )
        .await?;
        conflict_count += 1;
    }

    let (inbound_applied_count, inbound_duplicate_count) = ingest_inbound_items(
        &tx,
        &input.server_batch_id,
        &input.checkpoint_token,
        &input.inbound_items,
        &mut typed_rejections,
    )
    .await?;

    // v1: an invalid inbound envelope holds the checkpoint so it is fetched again. v2: item failures
    // never block the cursor (SYNC_PROTOCOL_V2.md §3.1); the envelope is already a conflict for a
    // person, and holding the token would make the worker fetch the same page forever.
    let checkpoint_advanced = input.protocol_version == SYNC_PROTOCOL_VERSION_V2 || typed_rejections.is_empty();
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
        conflict_count,
        typed_rejections,
    })
}

/// Ingest one page of a protocol v2 snapshot (`POST /api/v1/sync/snapshot`). Rows go to the inbox
/// as `pending`, like exchange inbound items, under `snapshot_id` (one id per snapshot run, so a
/// restarted snapshot is applied again instead of being dropped as a duplicate).
///
/// `checkpoint_token` is given only for the last page of a scope `all` snapshot; it is stored in
/// the same transaction as the page, so the delta never starts before the snapshot is in the inbox.
pub async fn apply_snapshot_page(
    db: &DatabaseConnection,
    snapshot_id: &str,
    items: &[SyncInboundItemInput],
    checkpoint_token: Option<&str>,
) -> AppResult<ApplySyncBatchResult> {
    if snapshot_id.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["snapshot_id is required.".to_string()]));
    }
    let tx = db.begin().await?;
    let mut typed_rejections = Vec::new();
    let (inbound_applied_count, inbound_duplicate_count) =
        ingest_inbound_items(&tx, snapshot_id, snapshot_id, items, &mut typed_rejections).await?;
    let checkpoint = checkpoint_token.map(str::trim).filter(|t| !t.is_empty());
    if let Some(token) = checkpoint {
        tx.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO sync_checkpoint (
                id, checkpoint_token, last_idempotency_key, protocol_version, policy_metadata_json,
                last_sync_at, updated_at
            ) VALUES (
                1, ?, NULL, ?, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now')
            )
            ON CONFLICT(id) DO UPDATE SET
                checkpoint_token = excluded.checkpoint_token,
                protocol_version = excluded.protocol_version,
                policy_metadata_json = NULL,
                last_sync_at = excluded.last_sync_at,
                updated_at = excluded.updated_at",
            [token.into(), SYNC_PROTOCOL_VERSION_V2.into()],
        ))
        .await?;
    }
    tx.commit().await?;
    Ok(ApplySyncBatchResult {
        protocol_version: SYNC_PROTOCOL_VERSION_V2.to_string(),
        checkpoint_token: checkpoint.map(str::to_string),
        checkpoint_advanced: checkpoint.is_some(),
        acknowledged_count: 0,
        rejected_count: 0,
        inbound_applied_count,
        inbound_duplicate_count,
        conflict_count: 0,
        typed_rejections,
    })
}

/// Forget the delta checkpoint (the server refused it: `invalid_checkpoint_token` or
/// `checkpoint_ahead_of_server`). The next sync bootstraps with a snapshot.
pub async fn clear_sync_checkpoint(db: &DatabaseConnection) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_checkpoint
         SET checkpoint_token = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = 1",
        [],
    ))
    .await?;
    Ok(())
}

/// Put in-flight items back to `pending` without counting a delivery attempt: the request never
/// reached the server (no connection) or the server refused it before looking at the items
/// (rate limit, activation, checkpoint). Delivery attempts that may have reached the server go
/// through `record_outbox_delivery_failure` instead.
pub async fn return_unsent_outbox_items(db: &DatabaseConnection, outbox_ids: &[i64]) -> AppResult<()> {
    if outbox_ids.is_empty() {
        return Ok(());
    }
    let ids_json = serde_json::to_string(outbox_ids)?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_outbox
         SET status = 'pending',
             attempt_count = MAX(attempt_count - 1, 0),
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE status = 'in_flight' AND id IN (SELECT value FROM json_each(?))",
        [ids_json.into()],
    ))
    .await?;
    Ok(())
}

/// Mark the items of a stored exchange request as being sent again (same key, same body): they
/// count one more delivery attempt, exactly like `get_sync_push_payload` does for a new request.
pub async fn mark_outbox_items_resent(db: &DatabaseConnection, outbox_ids: &[i64]) -> AppResult<()> {
    if outbox_ids.is_empty() {
        return Ok(());
    }
    let ids_json = serde_json::to_string(outbox_ids)?;
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_outbox
         SET status = 'in_flight',
             attempt_count = attempt_count + 1,
             last_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE status IN ('pending', 'in_flight') AND id IN (SELECT value FROM json_each(?))",
        [ids_json.into()],
    ))
    .await?;
    Ok(())
}

pub async fn list_inbox_items(
    db: &DatabaseConnection,
    apply_status: Option<String>,
    limit: Option<i64>,
) -> AppResult<Vec<SyncInboxItem>> {
    let page_size = limit.unwrap_or(200).clamp(1, 1000);
    let rows = if let Some(status) = apply_status {
        let sql = format!("SELECT {INBOX_COLUMNS} FROM sync_inbox WHERE apply_status = ? ORDER BY id ASC LIMIT ?");
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [status.into(), page_size.into()],
        ))
        .await?
    } else {
        let sql = format!("SELECT {INBOX_COLUMNS} FROM sync_inbox ORDER BY id ASC LIMIT ?");
        db.query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [page_size.into()],
        ))
        .await?
    };

    rows.into_iter().map(|row| to_sync_inbox_item(&row)).collect()
}

async fn count(db: &impl ConnectionTrait, sql: &str, field: &str) -> AppResult<i64> {
    db.query_one(Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, []))
        .await?
        .ok_or_else(|| AppError::SyncError(format!("Failed to query {field}.")))?
        .try_get("", "count")
        .map_err(|e| decode_err(field, e))
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
                updated_at: row.try_get("", "updated_at").map_err(|e| decode_err("updated_at", e))?,
            })
        })
        .transpose()?;

    let pending_outbox_count = count(
        db,
        // Captured changes not drained into the outbox yet are pending too (one per entity).
        "SELECT (SELECT COUNT(*) FROM sync_outbox WHERE status = 'pending')
              + (SELECT COUNT(DISTINCT entity_type || ':' || COALESCE(entity_sync_id, natural_key_json))
                   FROM sync_capture_journal) AS count",
        "pending_outbox_count",
    )
    .await?;
    let in_flight_outbox_count = count(
        db,
        "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'in_flight'",
        "in_flight_outbox_count",
    )
    .await?;
    let rejected_outbox_count = count(
        db,
        "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'rejected'",
        "rejected_outbox_count",
    )
    .await?;
    let dead_letter_outbox_count = count(
        db,
        "SELECT COUNT(*) AS count FROM sync_outbox WHERE status = 'dead_letter'",
        "dead_letter_outbox_count",
    )
    .await?;
    let inbox_error_count = count(
        db,
        "SELECT COUNT(*) AS count FROM sync_inbox WHERE apply_status = 'rejected'",
        "inbox_error_count",
    )
    .await?;
    let deferred_inbox_count = count(
        db,
        "SELECT COUNT(*) AS count FROM sync_inbox WHERE apply_status = 'deferred'",
        "deferred_inbox_count",
    )
    .await?;
    let inbox_conflict_count = count(
        db,
        "SELECT COUNT(*) AS count FROM sync_inbox WHERE apply_status = 'conflict'",
        "inbox_conflict_count",
    )
    .await?;
    let unresolved_conflict_count = count(
        db,
        "SELECT COUNT(*) AS count FROM sync_conflicts
         WHERE requires_operator_review = 1 AND status IN ('new', 'triaged', 'escalated')",
        "unresolved_conflict_count",
    )
    .await?;

    Ok(SyncStateSummary {
        protocol_version: SYNC_PROTOCOL_VERSION_V2.to_string(),
        checkpoint,
        pending_outbox_count,
        rejected_outbox_count,
        inbox_error_count,
        in_flight_outbox_count,
        dead_letter_outbox_count,
        deferred_inbox_count,
        inbox_conflict_count,
        unresolved_conflict_count,
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
            format!(
                "SELECT {CONFLICT_COLUMNS}
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
             LIMIT ?"
            ),
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
                filter.statuses.as_ref().map(|_| "use_json_filter".to_string()).into(),
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
            "action must be one of: accept_local, accept_remote, renumber_local, retry_later, escalate, dismiss."
                .to_string(),
        );
    }
    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }
    let next_status = map_resolution_to_status(&input.action)
        .ok_or_else(|| AppError::SyncError("Invalid conflict action mapping.".to_string()))?;

    let tx = db.begin().await?;
    let sql = format!("SELECT {CONFLICT_COLUMNS} FROM sync_conflicts WHERE id = ?");
    let current = tx
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [input.conflict_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "sync_conflict".to_string(),
            id: input.conflict_id.to_string(),
        })?;
    let conflict = to_sync_conflict(&current)?;
    if conflict.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Sync conflict was modified elsewhere (stale row_version).".to_string(),
        ]));
    }
    if matches!(
        conflict.status.as_str(),
        "resolved_local" | "resolved_remote" | "merged" | "dismissed"
    ) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Sync conflict {} is already closed ({}).",
            conflict.id, conflict.status
        )]));
    }

    // The person's choice is applied to the data in the same transaction as the status change.
    let mut renumbered = None;
    match input.action.as_str() {
        "accept_local" => crate::sync::conflicts::keep_local_version(&tx, &conflict).await?,
        "accept_remote" => crate::sync::conflicts::take_remote_version(&tx, &conflict).await?,
        "renumber_local" => renumbered = crate::sync::code_clash::renumber_local_record(&tx, &conflict).await?,
        _ => {}
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

    if let Some(change) = renumbered {
        // History of the renumbering (the record's own change travels through the outbox).
        crate::audit::writer::write_audit_event(
            db,
            crate::audit::writer::AuditEventInput {
                action_code: "sync.code_clash.renumbered".to_string(),
                target_type: Some("sync_conflict".to_string()),
                target_id: Some(input.conflict_id.to_string()),
                actor_id: Some(user_id),
                auth_context: "session".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "operations".to_string(),
                details_json: Some(serde_json::json!({
                    "entity_type": change.entity_type,
                    "entity_sync_id": change.local_entity_sync_id,
                    "old_number": change.old_number,
                    "new_number": change.new_number,
                })),
            },
        )
        .await?;
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
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
        _ => {
            validation_errors.push("mode must be one of: single_item, batch, window, checkpoint_rollback.".to_string())
        }
    }
    if input.mode == "single_item" && input.outbox_id.is_none() && input.conflict_id.is_none() {
        validation_errors.push("single_item replay requires outbox_id or conflict_id scope.".to_string());
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
            serde_json::to_string(&input).map_err(AppError::Serialization)?.into(),
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
                             attempt_count = 0,
                             dead_lettered_at = NULL,
                             rejection_code = NULL,
                             rejection_message = NULL,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                         WHERE id = ?
                           AND status IN ('rejected', 'dead_letter')",
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
                         attempt_count = 0,
                         dead_lettered_at = NULL,
                         rejection_code = NULL,
                         rejection_message = NULL,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE id IN (
                        SELECT linked_outbox_id
                        FROM sync_conflicts
                        WHERE source_batch_id = ?
                          AND linked_outbox_id IS NOT NULL
                     )
                       AND status IN ('rejected', 'dead_letter')",
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
                         attempt_count = 0,
                         dead_lettered_at = NULL,
                         rejection_code = NULL,
                         rejection_message = NULL,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                     WHERE status IN ('rejected', 'dead_letter')
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
                [checkpoint_token.into(), SYNC_PROTOCOL_VERSION_V1.to_string().into()],
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
        validation_errors.push("Destructive reset is disabled. Use scoped repair modes only.".to_string());
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
            return Err(AppError::ValidationFailed(vec!["Unsupported repair mode.".to_string()]));
        }
    }

    if affected_outbox_count == 0 && affected_conflict_count == 0 && input.mode != "checkpoint_realign" {
        warnings.push("Repair preview has zero affected rows.".to_string());
    }
    let risk_level = if input.mode == "checkpoint_realign" || affected_outbox_count + affected_conflict_count > 100 {
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
    let status: String = plan_row.try_get("", "status").map_err(|e| decode_err("status", e))?;
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
    let preview_scope: SyncRepairPreviewInput = serde_json::from_str(&scope_json).map_err(AppError::Serialization)?;

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
                                 attempt_count = 0,
                                 dead_lettered_at = NULL,
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
                             attempt_count = 0,
                             dead_lettered_at = NULL,
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
                             attempt_count = 0,
                             dead_lettered_at = NULL,
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
            return Err(AppError::ValidationFailed(vec!["Unsupported repair mode.".to_string()]));
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
        let created_at: String = row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?;
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
        let plan_id: String = row.try_get("", "plan_id").map_err(|e| decode_err("plan_id", e))?;
        let created_at: String = row.try_get("", "created_at").map_err(|e| decode_err("created_at", e))?;
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
