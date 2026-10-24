//! Conflict resolution that acts on data.
//!
//! Owner decision: a concurrent change is always resolved by a person, never by silent
//! last-write-wins. The two choices change the data in the same transaction that closes the
//! conflict:
//! - keep mine (`accept_local`): the server's version becomes the base, every unsent local change
//!   of the entity is superseded, and the current local row is staged again on top of the
//!   server's version, so the server accepts it;
//! - take theirs (`accept_remote`): every unsent local change of the entity is superseded and the
//!   server's row is written to the local table through the materializer.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::SyncConflictRecord;
use crate::sync::materialize::{apply_inbound_payload, ApplyOutcome};
use crate::sync::queries::{hash_payload, overwrite_synced_state};
use crate::sync::registry;

/// Marks unsent, refused or dead-lettered local changes of an entity as replaced by the person's
/// decision. They stay in the outbox for the record but are never sent.
async fn supersede_local_changes(
    conn: &impl ConnectionTrait,
    entity_type: &str,
    entity_sync_id: &str,
) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE sync_outbox
         SET status = 'superseded', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE entity_type = ? AND entity_sync_id = ?
           AND status IN ('pending', 'in_flight', 'conflict', 'rejected', 'dead_letter')",
        [entity_type.into(), entity_sync_id.into()],
    ))
    .await?;
    Ok(())
}

pub(crate) async fn set_linked_inbox_status(
    conn: &impl ConnectionTrait,
    conflict: &SyncConflictRecord,
    status: &str,
) -> AppResult<()> {
    if let Some(inbox_id) = conflict.linked_inbox_id {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sync_inbox
             SET apply_status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [status.into(), inbox_id.into()],
        ))
        .await?;
    }
    Ok(())
}

/// Hash recorded for the server's version. A server tombstone carries no payload; its hash is the
/// hash of the empty document so it never equals a real payload.
fn server_payload_hash(conflict: &SyncConflictRecord) -> String {
    hash_payload(conflict.inbound_payload_json.as_deref().unwrap_or_default())
}

/// "Keep mine": stage the current local row again on top of the server's version.
pub(crate) async fn keep_local_version(conn: &impl ConnectionTrait, conflict: &SyncConflictRecord) -> AppResult<()> {
    let def = registry::require_entity(&conflict.entity_type)?;
    // Captured local changes join the outbox first, so they are superseded with the others.
    crate::sync::capture::drain_capture_journal(conn).await?;
    if !def.has_stager {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_SYNC_KEEP_LOCAL_UNAVAILABLE:entity={0}: Keeping the local version is not available for '{0}': this entity is not synchronised yet.",
            conflict.entity_type
        )]));
    }
    if let Some(server_version) = conflict.inbound_row_version {
        overwrite_synced_state(
            conn,
            &conflict.entity_type,
            &conflict.entity_sync_id,
            server_version,
            &server_payload_hash(conflict),
            "conflict_keep_local",
        )
        .await?;
    }
    supersede_local_changes(conn, &conflict.entity_type, &conflict.entity_sync_id).await?;
    set_linked_inbox_status(conn, conflict, "superseded").await?;
    crate::sync::capture::restage_current_state(
        conn,
        &conflict.entity_type,
        &conflict.entity_sync_id,
        &[
            conflict.local_payload_json.as_deref(),
            conflict.inbound_payload_json.as_deref(),
        ],
    )
    .await
}

/// "Take theirs": write the server's version to the local table.
pub(crate) async fn take_remote_version(conn: &impl ConnectionTrait, conflict: &SyncConflictRecord) -> AppResult<()> {
    let def = registry::require_entity(&conflict.entity_type)?;
    crate::sync::capture::drain_capture_journal(conn).await?;
    if !def.has_materializer {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_SYNC_TAKE_SERVER_UNAVAILABLE:entity={0}: Taking the server version is not available for '{0}' yet: this version cannot apply it.",
            conflict.entity_type
        )]));
    }
    let (Some(payload_json), Some(server_version)) = (&conflict.inbound_payload_json, conflict.inbound_row_version)
    else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_SYNC_NO_SERVER_VERSION: This conflict carries no server version, so it cannot be taken. Keep the local version or dismiss it."
                .to_string(),
        ]));
    };
    supersede_local_changes(conn, &conflict.entity_type, &conflict.entity_sync_id).await?;
    match apply_inbound_payload(
        conn,
        &conflict.entity_type,
        &conflict.entity_sync_id,
        &conflict.operation,
        server_version,
        payload_json,
    )
    .await?
    {
        ApplyOutcome::Applied | ApplyOutcome::Unchanged => {}
        ApplyOutcome::Deferred { message, .. } | ApplyOutcome::Conflict { message, .. } => {
            return Err(AppError::ValidationFailed(vec![format!(
                "GATE_SYNC_SERVER_VERSION_NOT_APPLIED: The server version cannot be applied yet: {message}"
            )]));
        }
    }
    overwrite_synced_state(
        conn,
        &conflict.entity_type,
        &conflict.entity_sync_id,
        server_version,
        &hash_payload(payload_json),
        "conflict_take_remote",
    )
    .await?;
    // A reservation row or movement taken from the server changes reserved stock: recomputed in
    // this transaction (`inventory::reservations`).
    crate::inventory::reservations::recompute_dirty_reserved_in(conn).await?;
    set_linked_inbox_status(conn, conflict, "applied").await
}
