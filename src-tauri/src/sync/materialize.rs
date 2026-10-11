//! Inbox materialization — apply pending sync_inbox rows to domain tables.
//!
//! Ingest (`apply_sync_batch`) stores envelopes as `pending`. This module applies them in
//! dependency order (`sync::registry::apply_priority`) and marks each row:
//! - `applied`: written to the domain table (or already current here);
//! - `deferred`: waiting for a parent row from a later batch, or for a materializer (entity types
//!   that are staged but not yet applied inbound); retried on every run;
//! - `conflict`: needs a person; a `sync_conflicts` record is written in the same transaction.
//!
//! A row is never marked `applied` unless it was applied or is already current.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use crate::errors::{AppError, AppResult};
use crate::sync::queries::{record_sync_conflict, record_synced_state, synced_row_version, NewSyncConflict};
use crate::sync::registry::{self, ConflictPolicy};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaterializeInboxResult {
    pub processed: i64,
    pub applied: i64,
    pub conflicts: i64,
    /// Already current here (replayed row or this device's own change sent back).
    pub skipped: i64,
    /// Waiting for a parent row or a materializer; retried on the next run.
    pub deferred: i64,
    pub errors: Vec<String>,
}

/// Deferral code for a row whose parent has not been applied yet.
pub(crate) const DEFER_MISSING_PARENT: &str = "MISSING_PARENT";
/// Deferral code for an entity type that is staged by other devices but not applied here yet.
pub(crate) const DEFER_NO_MATERIALIZER: &str = "NO_MATERIALIZER";
/// Deferral code for an entity type this version of the app does not know.
pub(crate) const DEFER_UNKNOWN_ENTITY_TYPE: &str = "UNKNOWN_ENTITY_TYPE";

#[derive(Debug, Clone)]
struct InboxRow {
    id: i64,
    server_batch_id: String,
    checkpoint_token: String,
    entity_type: String,
    entity_sync_id: String,
    operation: String,
    row_version: i64,
    payload_json: String,
    payload_hash: String,
}

fn decode(field: &str, e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("inbox {field}: {e}"))
}

/// Apply pending inbox rows, plus deferred rows whose entity type can now be applied.
pub async fn materialize_inbox_batch(db: &DatabaseConnection, limit: Option<i64>) -> AppResult<MaterializeInboxResult> {
    let lim = limit.unwrap_or(200).clamp(1, 2000);
    let materializable = serde_json::to_string(&registry::materializable_entity_types())?;
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT id, server_batch_id, checkpoint_token, entity_type, entity_sync_id, operation,
                    row_version, payload_json, payload_hash
             FROM sync_inbox
             WHERE apply_status = 'pending'
                OR (apply_status = 'deferred' AND entity_type IN (SELECT value FROM json_each(?)))
             ORDER BY id ASC
             LIMIT ?",
            [materializable.into(), lim.into()],
        ))
        .await?;

    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        items.push(InboxRow {
            id: row.try_get("", "id").map_err(|e| decode("id", e))?,
            server_batch_id: row
                .try_get("", "server_batch_id")
                .map_err(|e| decode("server_batch_id", e))?,
            checkpoint_token: row
                .try_get("", "checkpoint_token")
                .map_err(|e| decode("checkpoint_token", e))?,
            entity_type: row.try_get("", "entity_type").map_err(|e| decode("entity_type", e))?,
            entity_sync_id: row
                .try_get("", "entity_sync_id")
                .map_err(|e| decode("entity_sync_id", e))?,
            operation: row.try_get("", "operation").map_err(|e| decode("operation", e))?,
            row_version: row.try_get("", "row_version").map_err(|e| decode("row_version", e))?,
            payload_json: row.try_get("", "payload_json").map_err(|e| decode("payload_json", e))?,
            payload_hash: row.try_get("", "payload_hash").map_err(|e| decode("payload_hash", e))?,
        });
    }
    // Creates and updates go parents first; deletes go after them, children first, so a parent
    // is never removed while a row received in the same run still points to it.
    items.sort_by(|a, b| {
        let a_delete = is_delete_operation(&a.operation);
        let b_delete = is_delete_operation(&b.operation);
        a_delete
            .cmp(&b_delete)
            .then_with(|| {
                let (pa, pb) = (registry::apply_priority(&a.entity_type), registry::apply_priority(&b.entity_type));
                if a_delete {
                    pb.cmp(&pa)
                } else {
                    pa.cmp(&pb)
                }
            })
            .then(a.id.cmp(&b.id))
    });

    let mut result = MaterializeInboxResult {
        processed: i64::try_from(items.len()).unwrap_or(i64::MAX),
        applied: 0,
        conflicts: 0,
        skipped: 0,
        deferred: 0,
        errors: Vec::new(),
    };

    // Rows waiting for a parent are retried in the same run as long as other rows were applied,
    // so a child that arrived before its parent in the same run does not wait for the next one.
    let mut queue = items;
    loop {
        let mut waiting_for_parent = Vec::new();
        let mut progressed = false;
        for item in queue {
            match apply_inbox_row(db, &item).await? {
                ApplyOutcome::Applied => {
                    result.applied += 1;
                    progressed = true;
                }
                ApplyOutcome::Unchanged => {
                    result.skipped += 1;
                    progressed = true;
                }
                ApplyOutcome::Deferred { code, .. } if code == DEFER_MISSING_PARENT => {
                    waiting_for_parent.push(item);
                }
                ApplyOutcome::Deferred { .. } => result.deferred += 1,
                ApplyOutcome::Conflict { code, message } => {
                    result.conflicts += 1;
                    if code == "MATERIALIZE_ERROR" {
                        result
                            .errors
                            .push(format!("{}/{}: {message}", item.entity_type, item.entity_sync_id));
                    }
                }
            }
        }
        if waiting_for_parent.is_empty() {
            break;
        }
        if !progressed {
            result.deferred += i64::try_from(waiting_for_parent.len()).unwrap_or(i64::MAX);
            break;
        }
        queue = waiting_for_parent;
    }

    Ok(result)
}

#[derive(Debug, Clone)]
pub(crate) enum ApplyOutcome {
    /// Written to the domain table.
    Applied,
    /// Already current locally; nothing to write.
    Unchanged,
    /// Cannot be applied yet; kept for a later run.
    Deferred { code: String, message: String },
    /// Needs a person.
    Conflict { code: String, message: String },
}

fn is_delete_operation(operation: &str) -> bool {
    operation == "delete" || operation == "archive"
}

/// Apply one inbox row in its own transaction and record the outcome.
async fn apply_inbox_row(db: &DatabaseConnection, item: &InboxRow) -> AppResult<ApplyOutcome> {
    let tx = db.begin().await?;
    // Local changes captured since the last drain must be in the outbox before the conflict check.
    crate::sync::capture::drain_capture_journal(&tx).await?;
    let evaluated = evaluate_and_apply(&tx, item).await;
    let outcome = match evaluated {
        Ok(outcome @ (ApplyOutcome::Applied | ApplyOutcome::Unchanged)) => {
            mark_inbox(&tx, item.id, "applied", None, None).await?;
            crate::sync::code_clash::close_outdated_clashes(&tx, &item.entity_type, &item.entity_sync_id, item.row_version)
                .await?;
            record_synced_state(
                &tx,
                &item.entity_type,
                &item.entity_sync_id,
                item.row_version,
                &item.payload_hash,
                "inbound_apply",
            )
            .await?;
            tx.commit().await?;
            return Ok(outcome);
        }
        Ok(other) => {
            tx.rollback().await?;
            other
        }
        Err(e) => {
            tx.rollback().await?;
            ApplyOutcome::Conflict {
                code: "MATERIALIZE_ERROR".into(),
                message: e.to_string(),
            }
        }
    };

    // Nothing from the attempt is kept; record why the row was not applied.
    let tx = db.begin().await?;
    match &outcome {
        ApplyOutcome::Deferred { code, message } => {
            tx.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sync_inbox
                 SET apply_status = 'deferred', rejection_code = ?, rejection_message = ?,
                     attempt_count = attempt_count + 1,
                     last_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE id = ?",
                [code.as_str().into(), message.as_str().into(), item.id.into()],
            ))
            .await?;
        }
        ApplyOutcome::Conflict { code, message } => {
            mark_inbox(&tx, item.id, "conflict", Some(code), Some(message)).await?;
            let local = latest_local_change(&tx, &item.entity_type, &item.entity_sync_id).await?;
            record_sync_conflict(
                &tx,
                NewSyncConflict {
                    source_scope: "inbound",
                    source_batch_id: Some(item.server_batch_id.clone()),
                    linked_outbox_id: None,
                    linked_inbox_id: Some(item.id),
                    entity_type: item.entity_type.clone(),
                    entity_sync_id: item.entity_sync_id.clone(),
                    operation: item.operation.clone(),
                    conflict_type: code.clone(),
                    local_payload_json: local.as_ref().map(|l| l.payload_json.clone()),
                    inbound_payload_json: Some(item.payload_json.clone()),
                    checkpoint_token: Some(item.checkpoint_token.clone()),
                    local_row_version: local.as_ref().map(|l| l.row_version),
                    inbound_row_version: Some(item.row_version),
                },
            )
            .await?;
        }
        ApplyOutcome::Applied | ApplyOutcome::Unchanged => {}
    }
    tx.commit().await?;
    Ok(outcome)
}

struct LocalChange {
    row_version: i64,
    payload_json: String,
    payload_hash: String,
    base_row_version: Option<i64>,
}

/// Local changes of an entity that the server has not accepted yet, oldest first.
async fn unsent_local_changes(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Vec<LocalChange>> {
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT row_version, payload_json, payload_hash, base_row_version FROM sync_outbox
             WHERE entity_type = ? AND entity_sync_id = ? AND status IN ('pending', 'in_flight', 'conflict')
             ORDER BY id ASC",
            [entity_type.into(), entity_sync_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(LocalChange {
                row_version: r.try_get("", "row_version").map_err(|e| decode("row_version", e))?,
                payload_json: r.try_get("", "payload_json").map_err(|e| decode("payload_json", e))?,
                payload_hash: r.try_get("", "payload_hash").map_err(|e| decode("payload_hash", e))?,
                base_row_version: r
                    .try_get("", "base_row_version")
                    .map_err(|e| decode("base_row_version", e))?,
            })
        })
        .collect()
}

async fn latest_local_change(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<Option<LocalChange>> {
    Ok(unsent_local_changes(conn, entity_type, entity_sync_id).await?.pop())
}

/// Decide whether an inbound row may be applied, then apply it.
async fn evaluate_and_apply(conn: &impl ConnectionTrait, item: &InboxRow) -> AppResult<ApplyOutcome> {
    let Some(def) = registry::entity(&item.entity_type) else {
        return Ok(ApplyOutcome::Deferred {
            code: DEFER_UNKNOWN_ENTITY_TYPE.into(),
            message: format!("entity type '{}' is not known by this version", item.entity_type),
        });
    };
    if !def.has_materializer {
        return Ok(ApplyOutcome::Deferred {
            code: DEFER_NO_MATERIALIZER.into(),
            message: format!("'{}' rows are not applied by this version yet", item.entity_type),
        });
    }
    if let Some(outcome) = concurrency_check(conn, def, item).await? {
        return Ok(outcome);
    }
    if let Some(outcome) = crate::sync::code_clash::check_inbound(
        conn,
        &item.entity_type,
        &item.entity_sync_id,
        &item.operation,
        &item.payload_json,
    )
    .await?
    {
        return Ok(outcome);
    }
    apply_inbound_payload(
        conn,
        &item.entity_type,
        &item.entity_sync_id,
        &item.operation,
        item.row_version,
        &item.payload_json,
    )
    .await
}

/// Owner decision: concurrent changes are never merged silently.
///
/// - Our own unsent change sent back by the server → already current.
/// - A local change the server has not accepted yet → `CONCURRENT_EDIT` (both sides changed the
///   row; with protocol v2 the local change was made on `base_row_version`, which the server
///   version has moved past).
/// - An older version than the one already applied → `STALE_INBOUND`.
/// - The same version with different content → `EQUAL_VERSION_DIVERGENCE`.
async fn concurrency_check(
    conn: &impl ConnectionTrait,
    def: &registry::SyncEntityDef,
    item: &InboxRow,
) -> AppResult<Option<ApplyOutcome>> {
    let unsent = unsent_local_changes(conn, &item.entity_type, &item.entity_sync_id).await?;
    if unsent.iter().any(|c| c.payload_hash == item.payload_hash) {
        return Ok(Some(ApplyOutcome::Unchanged));
    }
    if def.conflict_policy == ConflictPolicy::AppendOnly {
        // History rows are only inserted; the materializer skips rows that already exist.
        return Ok(None);
    }
    if let Some(local) = unsent.last() {
        let base = local
            .base_row_version
            .map_or_else(|| "a new row".to_string(), |v| format!("version {v}"));
        return Ok(Some(ApplyOutcome::Conflict {
            code: "CONCURRENT_EDIT".into(),
            message: format!(
                "Local change v{} (made on {base}) is not on the server yet; the server sent v{}.",
                local.row_version, item.row_version
            ),
        }));
    }
    if let Some((synced_version, synced_hash)) =
        synced_row_version(conn, &item.entity_type, &item.entity_sync_id).await?
    {
        if item.row_version < synced_version {
            return Ok(Some(ApplyOutcome::Conflict {
                code: "STALE_INBOUND".into(),
                message: format!(
                    "Server sent v{} but v{synced_version} is already applied here.",
                    item.row_version
                ),
            }));
        }
        if item.row_version == synced_version {
            if synced_hash.eq_ignore_ascii_case(&item.payload_hash) {
                return Ok(Some(ApplyOutcome::Unchanged));
            }
            return Ok(Some(ApplyOutcome::Conflict {
                code: "EQUAL_VERSION_DIVERGENCE".into(),
                message: format!(
                    "Server sent v{} with content that differs from the v{synced_version} applied here.",
                    item.row_version
                ),
            }));
        }
    }
    Ok(None)
}

async fn mark_inbox(
    conn: &impl ConnectionTrait,
    id: i64,
    status: &str,
    rejection_code: Option<&str>,
    rejection_message: Option<&str>,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_inbox
         SET apply_status = ?,
             rejection_code = ?,
             rejection_message = ?,
             last_attempt_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [
            status.into(),
            rejection_code.into(),
            rejection_message.into(),
            id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Write an inbound row to its domain table without the concurrency check. Used for normal apply
/// (after the check) and when a person chooses the server version of a conflict.
pub(crate) async fn apply_inbound_payload(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
    operation: &str,
    row_version: i64,
    payload_json: &str,
) -> AppResult<ApplyOutcome> {
    if crate::sync::row_spec::row_spec(entity_type).is_some() {
        let layout = crate::sync::row_layout::layout(conn, entity_type).await?;
        crate::sync::capture::begin_suppression(conn).await?;
        let applied =
            crate::sync::row_codec::apply_row(conn, &layout, entity_sync_id, operation, payload_json).await;
        crate::sync::capture::end_suppression(conn).await?;
        return applied;
    }
    let _ = row_version;
    Err(AppError::SyncError(format!(
        "No inbound materializer for '{entity_type}' although the registry declares one."
    )))
}

