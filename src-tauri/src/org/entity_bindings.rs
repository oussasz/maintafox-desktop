//! External entity binding service.
//!
//! Maps org nodes to identifiers in external systems (ERP plant codes,
//! SAP functional locations, legacy CMMS codes, cost center references, etc.).
//!
//! Uniqueness rules:
//! - An active `(external_system, external_id)` pair must be unique across the
//!   entire tenant — two nodes cannot claim the same external identity.
//! - Only one primary binding per `(node_id, binding_type, external_system)` is
//!   allowed at a time. Setting a new primary automatically clears the previous.
//!
//! Bindings are never deleted; they are expired by setting `valid_to`.

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgEntityBinding {
    pub id: i64,
    pub node_id: i64,
    pub binding_type: String,
    pub external_system: String,
    pub external_id: String,
    pub is_primary: bool,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct UpsertOrgEntityBindingPayload {
    pub node_id: i64,
    pub binding_type: String,
    pub external_system: String,
    pub external_id: String,
    pub is_primary: bool,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

const fn i64_to_bool(n: i64) -> bool {
    n != 0
}

fn bool_to_i64(b: bool) -> i64 {
    i64::from(b)
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_entity_bindings row decode failed for column '{column}': {e}"
    ))
}

const SELECT_COLS: &str = r"
    id, node_id, binding_type, external_system, external_id,
    is_primary, valid_from, valid_to, created_at
";

fn map_binding(row: &QueryResult) -> AppResult<OrgEntityBinding> {
    Ok(OrgEntityBinding {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        node_id: row
            .try_get::<i64>("", "node_id")
            .map_err(|e| decode_err("node_id", e))?,
        binding_type: row
            .try_get::<String>("", "binding_type")
            .map_err(|e| decode_err("binding_type", e))?,
        external_system: row
            .try_get::<String>("", "external_system")
            .map_err(|e| decode_err("external_system", e))?,
        external_id: row
            .try_get::<String>("", "external_id")
            .map_err(|e| decode_err("external_id", e))?,
        is_primary: i64_to_bool(
            row.try_get::<i64>("", "is_primary")
                .map_err(|e| decode_err("is_primary", e))?,
        ),
        valid_from: row
            .try_get::<Option<String>>("", "valid_from")
            .map_err(|e| decode_err("valid_from", e))?,
        valid_to: row
            .try_get::<Option<String>>("", "valid_to")
            .map_err(|e| decode_err("valid_to", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
    })
}

// ─── Service functions ────────────────────────────────────────────────────────

/// List entity bindings for a node.
/// When `include_inactive` is false, only bindings with `valid_to IS NULL` are returned.
pub async fn list_entity_bindings(
    db: &DatabaseConnection,
    node_id: i64,
    include_inactive: bool,
) -> AppResult<Vec<OrgEntityBinding>> {
    let sql = if include_inactive {
        format!(
            "SELECT {SELECT_COLS} FROM org_entity_bindings \
             WHERE node_id = ? ORDER BY binding_type ASC, external_system ASC, created_at DESC"
        )
    } else {
        format!(
            "SELECT {SELECT_COLS} FROM org_entity_bindings \
             WHERE node_id = ? AND valid_to IS NULL \
             ORDER BY binding_type ASC, external_system ASC, created_at DESC"
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [node_id.into()],
        ))
        .await?;
    rows.iter().map(map_binding).collect()
}

/// Create or update an entity binding. Runs inside a transaction.
///
/// Validation:
/// - Node must exist and not be deleted
/// - `binding_type`, `external_system`, `external_id` must be non-empty
/// - Active `(external_system, external_id)` must be unique across all nodes
/// - If `is_primary = true`, clears previous primary for same `(node_id, binding_type, external_system)`
pub async fn upsert_entity_binding(
    db: &DatabaseConnection,
    payload: UpsertOrgEntityBindingPayload,
    _actor_id: i32,
) -> AppResult<OrgEntityBinding> {
    // Input validation
    let binding_type = payload.binding_type.trim().to_string();
    let external_system = payload.external_system.trim().to_string();
    let external_id = payload.external_id.trim().to_string();

    if binding_type.is_empty() || external_system.is_empty() || external_id.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "binding_type, external_system, and external_id must all be non-empty".to_string(),
        ]));
    }

    let txn = db.begin().await?;

    // Verify node exists and is not deleted
    let node_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT status FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [payload.node_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node".to_string(),
            id: payload.node_id.to_string(),
        })?;
    let node_status: String = node_row
        .try_get("", "status")
        .map_err(|e| decode_err("status", e))?;
    if node_status == "inactive" {
        return Err(AppError::ValidationFailed(vec![format!(
            "node {} is inactive — bindings cannot be added to deactivated nodes",
            payload.node_id
        )]));
    }

    // Check tenant-wide uniqueness of active (external_system, external_id)
    let dup_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_entity_bindings \
             WHERE external_system = ? AND external_id = ? AND valid_to IS NULL",
            [external_system.clone().into(), external_id.clone().into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let dup_count: i64 = dup_row.try_get("", "cnt").unwrap_or(0);
    if dup_count > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "an active binding for ({external_system}, {external_id}) already exists"
        )]));
    }

    // If primary, clear previous primary for same (node_id, binding_type, external_system)
    if payload.is_primary {
        txn.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE org_entity_bindings SET is_primary = 0 \
             WHERE node_id = ? AND binding_type = ? AND external_system = ? \
               AND is_primary = 1 AND valid_to IS NULL",
            [
                payload.node_id.into(),
                binding_type.clone().into(),
                external_system.clone().into(),
            ],
        ))
        .await?;
    }

    let now = Utc::now().to_rfc3339();

    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_entity_bindings
          (node_id, binding_type, external_system, external_id,
           is_primary, valid_from, valid_to, created_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            payload.node_id.into(),
            binding_type.into(),
            external_system.into(),
            external_id.into(),
            bool_to_i64(payload.is_primary).into(),
            payload.valid_from.into(),
            payload.valid_to.into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve the inserted row
    let row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT {SELECT_COLS} FROM org_entity_bindings \
                 WHERE id = last_insert_rowid()"
            ),
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "entity binding created but not found after insert"
            ))
        })?;

    let binding = map_binding(&row)?;
    txn.commit().await?;

    tracing::info!(
        binding_id = binding.id,
        node_id = binding.node_id,
        "entity binding created"
    );
    Ok(binding)
}

/// Expire an entity binding by setting `valid_to`.
pub async fn expire_entity_binding(
    db: &DatabaseConnection,
    binding_id: i64,
    valid_to: Option<String>,
    _actor_id: i32,
) -> AppResult<OrgEntityBinding> {
    // Verify binding exists
    let check_sql = format!(
        "SELECT {SELECT_COLS} FROM org_entity_bindings WHERE id = ?"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            check_sql,
            [binding_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_entity_binding".to_string(),
            id: binding_id.to_string(),
        })?;
    let existing = map_binding(&row)?;

    // Validate valid_to >= valid_from when both are present
    if let (Some(ref from), Some(ref to)) = (&existing.valid_from, &valid_to) {
        if to < from {
            return Err(AppError::ValidationFailed(vec![format!(
                "valid_to ({to}) cannot be earlier than valid_from ({from})"
            )]));
        }
    }

    let end_ts = valid_to.unwrap_or_else(|| Utc::now().to_rfc3339());

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_entity_bindings SET valid_to = ? WHERE id = ?",
        [end_ts.into(), binding_id.into()],
    ))
    .await?;

    // Re-fetch updated row
    let updated_sql = format!(
        "SELECT {SELECT_COLS} FROM org_entity_bindings WHERE id = ?"
    );
    let updated_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            updated_sql,
            [binding_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_entity_binding".to_string(),
            id: binding_id.to_string(),
        })?;

    let result = map_binding(&updated_row)?;
    tracing::info!(binding_id, "entity binding expired");
    Ok(result)
}
