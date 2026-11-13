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
use crate::org::fail::{fail, fail_params};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNodeResponsibility {
    pub id: i64,
    pub node_id: i64,
    pub responsibility_type: String,
    pub person_id: Option<i64>,
    pub team_id: Option<i64>,
    /// Display enrichment (personnel.full_name). Prefer over person_id in UI.
    pub person_display_name: Option<String>,
    /// Display enrichment (teams.code + name). Prefer over team_id in UI.
    pub team_label: Option<String>,
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
    /// Hand the responsibility over: end the current active assignment of the same type on
    /// the node and create this one in the same transaction. Defaults to false (reject overlap).
    #[serde(default)]
    pub replace_active: bool,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!(
        "org_node_responsibilities row decode failed for column '{column}': {e}"
    ))
}

const SELECT_COLS: &str = r"
    r.id, r.node_id, r.responsibility_type, r.person_id, r.team_id,
    p.full_name AS person_display_name,
    CASE
      WHEN t.code IS NOT NULL AND t.name IS NOT NULL THEN t.code || ' — ' || t.name
      ELSE COALESCE(t.name, t.code)
    END AS team_label,
    r.valid_from, r.valid_to, r.created_at, r.updated_at
";

const SELECT_FROM: &str = r"
    FROM org_node_responsibilities r
    LEFT JOIN personnel p ON p.id = r.person_id
    LEFT JOIN teams t ON t.id = r.team_id AND t.deleted_at IS NULL
";

fn select_by_id_sql() -> String {
    format!("SELECT {SELECT_COLS} {SELECT_FROM} WHERE r.id = ?")
}

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
        person_display_name: row.try_get::<Option<String>>("", "person_display_name").unwrap_or(None),
        team_label: row.try_get::<Option<String>>("", "team_label").unwrap_or(None),
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
            "SELECT {SELECT_COLS} FROM org_node_responsibilities r \
             LEFT JOIN personnel p ON p.id = r.person_id \
             LEFT JOIN teams t ON t.id = r.team_id AND t.deleted_at IS NULL \
             WHERE r.node_id = ? ORDER BY r.responsibility_type ASC, r.created_at DESC"
        )
    } else {
        format!(
            "SELECT {SELECT_COLS} FROM org_node_responsibilities r \
             LEFT JOIN personnel p ON p.id = r.person_id \
             LEFT JOIN teams t ON t.id = r.team_id AND t.deleted_at IS NULL \
             WHERE r.node_id = ? AND r.valid_to IS NULL \
             ORDER BY r.responsibility_type ASC, r.created_at DESC"
        )
    };
    let rows = db
        .query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, [node_id.into()]))
        .await?;
    rows.iter().map(map_responsibility).collect()
}

/// Result of an assignment that may have replaced the previous active assignee.
#[derive(Debug, Clone)]
pub struct ResponsibilityAssignOutcome {
    /// The new, active assignment.
    pub assignment: OrgNodeResponsibility,
    /// The assignment that was ended to make room for it (`replace_active`), if any.
    pub ended: Option<OrgNodeResponsibility>,
}

/// Assign a responsibility to a node.
///
/// Validation:
/// - `responsibility_type` must be non-empty
/// - Exactly one of `person_id` or `team_id` must be set
/// - `node_id` must reference an active (non-deleted) node
/// - No overlapping active assignment for the same `(node_id, responsibility_type)`
///   unless `replace_active` is set (see [`assign_or_replace_responsibility`]).
pub async fn assign_responsibility(
    db: &DatabaseConnection,
    payload: AssignResponsibilityPayload,
    actor_id: i32,
) -> AppResult<OrgNodeResponsibility> {
    Ok(assign_or_replace_responsibility(db, payload, actor_id)
        .await?
        .assignment)
}

/// Assign a responsibility, optionally handing it over from the current active assignee.
///
/// When `payload.replace_active` is true and an active assignment of the same type exists on
/// the node, that assignment is ended (`valid_to = now`) and the new one is created in the same
/// transaction, so the node is never left without a responsible and never has two.
/// Handing a responsibility to the person or team who already holds it is rejected.
pub async fn assign_or_replace_responsibility(
    db: &DatabaseConnection,
    payload: AssignResponsibilityPayload,
    _actor_id: i32,
) -> AppResult<ResponsibilityAssignOutcome> {
    let resp_type = payload.responsibility_type.trim().to_string();
    if resp_type.is_empty() {
        return Err(fail(
            "ORG_RESPONSIBILITY_TYPE_EMPTY",
            "Responsibility type must not be empty.",
        ));
    }

    // XOR: exactly one of person_id or team_id must be set
    match (payload.person_id, payload.team_id) {
        (Some(_), Some(_)) => {
            return Err(fail(
                "ORG_ASSIGNEE_BOTH_SET",
                "Exactly one of person or team must be set, not both.",
            ));
        }
        (None, None) => {
            return Err(fail(
                "ORG_ASSIGNEE_REQUIRED",
                "Exactly one of person or team must be set.",
            ));
        }
        _ => {} // valid
    }

    let txn = db.begin().await?;

    // Node must exist in the active (production) tree and be status=active
    crate::org::model_scope::assert_org_node_active(&txn, payload.node_id).await?;

    if let Some(team_id) = payload.team_id {
        crate::repository::team_repository::assert_active_crew_exists(&txn, team_id).await?;
    }

    // Current active assignment on the same (node_id, responsibility_type), if any.
    let active_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, person_id, team_id FROM org_node_responsibilities \
             WHERE node_id = ? AND responsibility_type = ? AND valid_to IS NULL \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            [payload.node_id.into(), resp_type.clone().into()],
        ))
        .await?;

    let now = crate::clock::now().to_rfc3339();

    let ended = match active_row {
        None => None,
        Some(_) if !payload.replace_active => {
            return Err(fail_params(
                "ORG_ASSIGNMENT_EXISTS",
                format!(
                    "An active '{resp_type}' assignment already exists on this node — end it before assigning a new one."
                ),
                &[("responsibilityType", resp_type)],
            ));
        }
        Some(row) => {
            let active_id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
            let active_person: Option<i64> = row.try_get("", "person_id").map_err(|e| decode_err("person_id", e))?;
            let active_team: Option<i64> = row.try_get("", "team_id").map_err(|e| decode_err("team_id", e))?;
            let unchanged = (payload.person_id.is_some() && payload.person_id == active_person)
                || (payload.team_id.is_some() && payload.team_id == active_team);
            if unchanged {
                return Err(fail_params(
                    "ORG_ASSIGNEE_UNCHANGED",
                    format!("This person or team already holds the '{resp_type}' responsibility on this node."),
                    &[("responsibilityType", resp_type)],
                ));
            }
            txn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE org_node_responsibilities SET valid_to = ?, updated_at = ? WHERE id = ?",
                [now.clone().into(), now.clone().into(), active_id.into()],
            ))
            .await?;
            let ended_row = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    select_by_id_sql(),
                    [active_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "org_node_responsibility".to_string(),
                    id: active_id.to_string(),
                })?;
            Some(map_responsibility(&ended_row)?)
        }
    };

    // A handover starts when the previous assignment ends unless the caller dated it.
    let valid_from = match (&payload.valid_from, &ended) {
        (Some(from), _) => Some(from.clone()),
        (None, Some(_)) => Some(now.clone()),
        (None, None) => None,
    };

    txn.execute(Statement::from_sql_and_values(
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
            valid_from.into(),
            payload.valid_to.into(),
            now.clone().into(),
            now.into(),
        ],
    ))
    .await?;

    // Retrieve the inserted row
    let row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!("SELECT {SELECT_COLS} {SELECT_FROM} WHERE r.id = last_insert_rowid()"),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("responsibility created but not found after insert")))?;

    let assignment = map_responsibility(&row)?;
    txn.commit().await?;

    tracing::info!(
        assignment_id = assignment.id,
        node_id = assignment.node_id,
        replaced = ended.as_ref().map(|e| e.id),
        "responsibility assigned"
    );
    Ok(ResponsibilityAssignOutcome { assignment, ended })
}

/// End a responsibility assignment by setting `valid_to`.
/// Rejects if `valid_to` is earlier than the assignment's `valid_from`.
pub async fn end_responsibility_assignment(
    db: &DatabaseConnection,
    assignment_id: i64,
    valid_to: Option<String>,
    _actor_id: i32,
) -> AppResult<OrgNodeResponsibility> {
    let sql = select_by_id_sql();
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
            return Err(fail_params(
                "ORG_INVALID_DATE_RANGE",
                format!("End date ({to}) cannot be earlier than start date ({from})."),
                &[("validTo", to.clone()), ("validFrom", from.clone())],
            ));
        }
    }

    let end_ts = valid_to.unwrap_or_else(|| crate::clock::now().to_rfc3339());
    let now = crate::clock::now().to_rfc3339();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE org_node_responsibilities SET valid_to = ?, updated_at = ? WHERE id = ?",
        [end_ts.into(), now.into(), assignment_id.into()],
    ))
    .await?;

    // Re-fetch the updated row
    let updated_sql = select_by_id_sql();
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
        "SELECT {SELECT_COLS} {SELECT_FROM} \
         WHERE r.node_id = ? AND r.responsibility_type = ? \
           AND (r.valid_from IS NULL OR r.valid_from <= ?) \
           AND (r.valid_to IS NULL OR r.valid_to > ?) \
         ORDER BY r.created_at DESC LIMIT 1"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            sql,
            [node_id.into(), responsibility_type.into(), at_ts.into(), at_ts.into()],
        ))
        .await?;
    row.as_ref().map(map_responsibility).transpose()
}
