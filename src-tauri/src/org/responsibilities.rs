//! Responsibility assignment service.
//!
//! Tracks named ownership bindings on org nodes with effective dating.
//! Each `(node_id, responsibility_type)` pair can have at most one active
//! (i.e. `valid_to IS NULL`) assignment at any time. Historical assignments
//! are preserved by ending them with `valid_to` before creating a new one.
//!
//! Exactly one of `person_id` or `team_id` must be set per assignment (XOR).
//!
//! Default responsibility codes seeded by the system:
//!   `maintenance_owner`, `production_owner`, `hse_owner`, `planner`, `approver`

use crate::errors::{AppError, AppResult};
use chrono::Utc;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNodeResponsibility {
    pub id: i64,
    pub node_id: i64,
    pub responsibility_type: String,
    pub person_id: Option<i64>,
    pub team_id: Option<i64>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct AssignResponsibilityPayload {
    pub node_id: i64,
    pub responsibility_type: String,
    pub person_id: Option<i64>,
    pub team_id: Option<i64>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_node_responsibilities row decode failed for column '{column}': {e}"
    ))
}

const SELECT_COLS: &str = r"
    id, node_id, responsibility_type, person_id, team_id,
    valid_from, valid_to, created_at, updated_at
";

fn map_responsibility(row: &QueryResult) -> AppResult<OrgNodeResponsibility> {
    Ok(OrgNodeResponsibility {
        id: row.try_get::<i64>("", "id").map_err(|e| decode_err("id", e))?,
        node_id: row
            .try_get::<i64>("", "node_id")
            .map_err(|e| decode_err("node_id", e))?,
        responsibility_type: row
            .try_get::<String>("", "responsibility_type")
            .map_err(|e| decode_err("responsibility_type", e))?,
        person_id: row
            .try_get::<Option<i64>>("", "person_id")
            .map_err(|e| decode_err("person_id", e))?,
        team_id: row
            .try_get::<Option<i64>>("", "team_id")
            .map_err(|e| decode_err("team_id", e))?,
        valid_from: row
            .try_get::<Option<String>>("", "valid_from")
            .map_err(|e| decode_err("valid_from", e))?,
        valid_to: row
            .try_get::<Option<String>>("", "valid_to")
            .map_err(|e| decode_err("valid_to", e))?,
        created_at: row
            .try_get::<String>("", "created_at")
            .map_err(|e| decode_err("created_at", e))?,
        updated_at: row
            .try_get::<String>("", "updated_at")
            .map_err(|e| decode_err("updated_at", e))?,
    })
}

// ─── Service functions ────────────────────────────────────────────────────────

/// List responsibility assignments for a node.
/// When `include_inactive` is false, only assignments with `valid_to IS NULL` are returned.
pub async fn list_node_responsibilities(
    db: &DatabaseConnection,
    node_id: i64,
    include_inactive: bool,
) -> AppResult<Vec<OrgNodeResponsibility>> {
    let sql = if include_inactive {
        format!(
            "SELECT {SELECT_COLS} FROM org_node_responsibilities \
             WHERE node_id = ? ORDER BY responsibility_type ASC, created_at DESC"
        )
    } else {
        format!(
            "SELECT {SELECT_COLS} FROM org_node_responsibilities \
             WHERE node_id = ? AND valid_to IS NULL \
             ORDER BY responsibility_type ASC, created_at DESC"
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [node_id.into()],
        ))
        .await?;
    rows.iter().map(map_responsibility).collect()
}

/// Assign a responsibility to a node.
///
/// Validation:
/// - `responsibility_type` must be non-empty
/// - Exactly one of `person_id` or `team_id` must be set
/// - `node_id` must reference an active (non-deleted) node
/// - No overlapping active assignment for the same `(node_id, responsibility_type)`
pub async fn assign_responsibility(
    db: &DatabaseConnection,
    payload: AssignResponsibilityPayload,
    _actor_id: i32,
) -> AppResult<OrgNodeResponsibility> {
    let resp_type = payload.responsibility_type.trim().to_string();
    if resp_type.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "responsibility_type must not be empty".to_string(),
        ]));
    }

    // XOR: exactly one of person_id or team_id must be set
    match (payload.person_id, payload.team_id) {
        (Some(_), Some(_)) => {
            return Err(AppError::ValidationFailed(vec![
                "exactly one of person_id or team_id must be set, not both".to_string(),
            ]));
        }
        (None, None) => {
            return Err(AppError::ValidationFailed(vec![
                "exactly one of person_id or team_id must be set".to_string(),
            ]));
        }
        _ => {} // valid
    }

    // Verify node exists and is active
    let node_row = db
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
    if node_status != "active" {
        return Err(AppError::ValidationFailed(vec![format!(
            "node {} is '{}', not 'active' — responsibilities can only be assigned to active nodes",
            payload.node_id, node_status
        )]));
    }

    // Check for overlapping active assignment on the same (node_id, responsibility_type)
    let overlap_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM org_node_responsibilities \
             WHERE node_id = ? AND responsibility_type = ? AND valid_to IS NULL",
            [payload.node_id.into(), resp_type.clone().into()],
        ))
        .await?
        .expect("COUNT always returns a row");
    let overlap_count: i64 = overlap_row.try_get("", "cnt").unwrap_or(0);
    if overlap_count > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "an active '{resp_type}' assignment already exists on node {} — end it before assigning a new one",
            payload.node_id
        )]));
    }

    let now = Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        r"INSERT INTO org_node_responsibilities
          (node_id, responsibility_type, person_id, team_id,
           valid_from, valid_to, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            payload.node_id.into(),
            resp_type.into(),
            payload.person_id.into(),
            payload.team_id.into(),
            payload.valid_from.into(),
            payload.valid_to.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve the inserted row
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT {SELECT_COLS} FROM org_node_responsibilities \
                 WHERE id = last_insert_rowid()"
            ),
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "responsibility created but not found after insert"
            ))
        })?;

    let assignment = map_responsibility(&row)?;
    tracing::info!(
        assignment_id = assignment.id,
        node_id = assignment.node_id,
        "responsibility assigned"
    );
    Ok(assignment)
}

/// End a responsibility assignment by setting `valid_to`.
/// Rejects if `valid_to` is earlier than the assignment's `valid_from`.
pub async fn end_responsibility_assignment(
    db: &DatabaseConnection,
    assignment_id: i64,
    valid_to: Option<String>,
    _actor_id: i32,
) -> AppResult<OrgNodeResponsibility> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM org_node_responsibilities WHERE id = ?"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [assignment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node_responsibility".to_string(),
            id: assignment_id.to_string(),
        })?;
    let assignment = map_responsibility(&row)?;

    // Validate valid_to >= valid_from when both are present
    if let (Some(ref from), Some(ref to)) = (&assignment.valid_from, &valid_to) {
        if to < from {
            return Err(AppError::ValidationFailed(vec![format!(
                "valid_to ({to}) cannot be earlier than valid_from ({from})"
            )]));
        }
    }

    let end_ts = valid_to.unwrap_or_else(|| Utc::now().to_rfc3339());
    let now = Utc::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_node_responsibilities SET valid_to = ?, updated_at = ? WHERE id = ?",
        [end_ts.into(), now.into(), assignment_id.into()],
    ))
    .await?;

    // Re-fetch the updated row
    let updated_sql = format!(
        "SELECT {SELECT_COLS} FROM org_node_responsibilities WHERE id = ?"
    );
    let updated_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            updated_sql,
            [assignment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "org_node_responsibility".to_string(),
            id: assignment_id.to_string(),
        })?;

    let result = map_responsibility(&updated_row)?;
    tracing::info!(assignment_id, "responsibility assignment ended");
    Ok(result)
}

/// Resolve the active responsibility assignment for a `(node_id, responsibility_type)`
/// at a given point in time. Returns `None` if no assignment is active at that instant.
pub async fn resolve_current_responsibility(
    db: &DatabaseConnection,
    node_id: i64,
    responsibility_type: &str,
    at_ts: &str,
) -> AppResult<Option<OrgNodeResponsibility>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM org_node_responsibilities \
         WHERE node_id = ? AND responsibility_type = ? \
           AND (valid_from IS NULL OR valid_from <= ?) \
           AND (valid_to IS NULL OR valid_to > ?) \
         ORDER BY created_at DESC LIMIT 1"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [
                node_id.into(),
                responsibility_type.into(),
                at_ts.into(),
                at_ts.into(),
            ],
        ))
        .await?;
    row.as_ref().map(map_responsibility).transpose()
}
