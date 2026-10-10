//! Stable sync identity for business rows (Universal Entity Sync Standard).
//!
//! Every business row carries an `entity_sync_id` that is set when the row is created and never
//! changes. It is the merge key across replicas, so it must never be derived from a mutable code or
//! invented at staging time.
//!
//! - Rows created by a user action get a random UUID ([`new_entity_sync_id`]).
//! - Rows that every replica seeds on its own (system roles, built-in catalogs) get a
//!   deterministic id derived from the entity type and the row's natural key
//!   ([`seeded_entity_sync_id`]), so two machines seeding the same catalog agree on the identity.

use sea_orm::{ConnectionTrait, DatabaseBackend, QueryResult, Statement};
use sha2::{Digest, Sha256};

use crate::errors::{AppError, AppResult};

/// Domain separator for seeded ids. Changing it would re-key every seeded catalog row.
const SEEDED_ID_NAMESPACE: &str = "maintafox.sync.seeded-entity.v1";

/// A new random identity for a row created on this replica.
pub fn new_entity_sync_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Deterministic identity for a row that each replica seeds independently.
///
/// `natural_key` must be the row's immutable business key (for example a system role name or a
/// `domain:code` pair). The result is an RFC 9562 version-8 UUID built from SHA-256.
pub fn seeded_entity_sync_id(entity_type: &str, natural_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(SEEDED_ID_NAMESPACE.as_bytes());
    hasher.update([0u8]);
    hasher.update(entity_type.as_bytes());
    hasher.update([0u8]);
    hasher.update(natural_key.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid().to_string()
}

/// Read a row's `entity_sync_id` and fail when it is missing.
///
/// Stagers use this instead of falling back to a code or a fresh UUID: a row without identity is a
/// data defect that must surface, not be papered over with an id that changes on every call.
pub fn require_entity_sync_id(table: &str, local_id: i64, value: Option<String>) -> AppResult<String> {
    match value {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(AppError::SyncError(format!(
            "{table} row {local_id} has no entity_sync_id; it cannot be synchronised."
        ))),
    }
}

/// Sync id of a referenced row. A reference to a row that is missing or has no identity is an
/// error: dropping the link would silently change the replicated row.
pub async fn parent_entity_sync_id(conn: &impl ConnectionTrait, table: &str, local_id: i64) -> AppResult<String> {
    // Table names are internal constants, never input.
    let sql = format!("SELECT entity_sync_id FROM {table} WHERE id = ? LIMIT 1");
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [local_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError(format!("Referenced {table} row {local_id} does not exist.")))?;
    let value: Option<String> = row
        .try_get("", "entity_sync_id")
        .map_err(|e| AppError::SyncError(format!("{table}.entity_sync_id: {e}")))?;
    require_entity_sync_id(table, local_id, value)
}

/// Read a joined parent sync id column (`..._entity_sync_id`) that must be present.
pub fn require_parent_sync_id(row: &QueryResult, column: &str) -> AppResult<String> {
    let value: Option<String> = row
        .try_get("", column)
        .map_err(|e| AppError::SyncError(format!("{column}: {e}")))?;
    value
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| AppError::SyncError(format!("Parent row has no {column}; it cannot be synchronised.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_ids_are_stable_and_distinct() {
        let a = seeded_entity_sync_id("roles", "Administrator");
        let b = seeded_entity_sync_id("roles", "Administrator");
        let c = seeded_entity_sync_id("roles", "Supervisor");
        let d = seeded_entity_sync_id("teams", "Administrator");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert!(uuid::Uuid::parse_str(&a).is_ok());
    }

    #[test]
    fn new_ids_are_uuids() {
        let id = new_entity_sync_id();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_ne!(id, new_entity_sync_id());
    }

    #[test]
    fn missing_identity_is_an_error() {
        assert!(require_entity_sync_id("work_orders", 7, None).is_err());
        assert!(require_entity_sync_id("work_orders", 7, Some("  ".into())).is_err());
        assert_eq!(
            require_entity_sync_id("work_orders", 7, Some("abc".into())).unwrap(),
            "abc"
        );
    }
}
