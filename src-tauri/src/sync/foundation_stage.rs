//! Outbox staging for foundation replica entities (org, RBAC, personnel, reference).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    OrgNodeSyncPayload, PersonnelSyncPayload, ReferenceValueSyncPayload, RoleSyncPayload, StageOutboxItemInput,
    TeamSyncPayload, UserAccountSyncPayload, SYNC_ENTITY_ORG_NODES, SYNC_ENTITY_PERSONNEL,
    SYNC_ENTITY_REFERENCE_VALUES, SYNC_ENTITY_ROLES, SYNC_ENTITY_TEAMS, SYNC_ENTITY_USER_ACCOUNTS,
};
use crate::sync::identity::require_entity_sync_id;
use crate::sync::queries::stage_outbox_item;

pub async fn stage_org_node_sync(db: &impl ConnectionTrait, node_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT n.entity_sync_id, n.code, n.name, n.status, n.depth, n.ancestor_path,
                    n.row_version, n.updated_at, p.entity_sync_id AS parent_entity_sync_id
             FROM org_nodes n
             LEFT JOIN org_nodes p ON p.id = n.parent_id
             WHERE n.id = ?",
            [node_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid = require_entity_sync_id(
        "org_nodes",
        node_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("org_nodes.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = OrgNodeSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code: row.try_get("", "code").unwrap_or_default(),
        name: row.try_get("", "name").unwrap_or_default(),
        status: row.try_get("", "status").unwrap_or_else(|_| "active".into()),
        parent_entity_sync_id: row.try_get("", "parent_entity_sync_id").ok(),
        depth: row.try_get("", "depth").unwrap_or(0),
        ancestor_path: row.try_get("", "ancestor_path").unwrap_or_default(),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_ORG_NODES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_user_account_sync(db: &impl ConnectionTrait, user_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, username, display_name, password_hash, identity_mode,
                    is_active, is_admin, row_version, updated_at
             FROM user_accounts WHERE id = ?",
            [user_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid = require_entity_sync_id(
        "user_accounts",
        user_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("user_accounts.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let is_admin: i64 = row.try_get("", "is_admin").unwrap_or(0);
    let payload = UserAccountSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        username: row.try_get("", "username").unwrap_or_default(),
        display_name: row.try_get("", "display_name").ok(),
        password_hash: row.try_get("", "password_hash").ok(),
        identity_mode: row.try_get("", "identity_mode").unwrap_or_else(|_| "local".into()),
        is_active: is_active != 0,
        is_admin: is_admin != 0,
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_USER_ACCOUNTS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_role_sync(db: &impl ConnectionTrait, role_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, name, description, role_type, status, is_system,
                    row_version, updated_at
             FROM roles WHERE id = ?",
            [role_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid = require_entity_sync_id(
        "roles",
        role_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("roles.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_system: i64 = row.try_get("", "is_system").unwrap_or(0);
    let payload = RoleSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        name: row.try_get("", "name").unwrap_or_default(),
        description: row.try_get("", "description").ok(),
        role_type: row.try_get("", "role_type").unwrap_or_else(|_| "custom".into()),
        status: row.try_get("", "status").unwrap_or_else(|_| "active".into()),
        is_system: is_system != 0,
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_ROLES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_personnel_sync(db: &impl ConnectionTrait, personnel_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, employee_code, full_name, availability_status,
                    row_version, updated_at
             FROM personnel WHERE id = ?",
            [personnel_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid = require_entity_sync_id(
        "personnel",
        personnel_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("personnel.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = PersonnelSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        employee_code: row.try_get("", "employee_code").unwrap_or_default(),
        full_name: row.try_get("", "full_name").unwrap_or_default(),
        availability_status: row
            .try_get("", "availability_status")
            .unwrap_or_else(|_| "available".into()),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_PERSONNEL.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_team_sync(db: &impl ConnectionTrait, team_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, code, name, status, row_version, updated_at
             FROM teams WHERE id = ?",
            [team_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid = require_entity_sync_id(
        "teams",
        team_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| AppError::SyncError(format!("teams.entity_sync_id: {e}")))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let status: String = row.try_get("", "status").unwrap_or_else(|_| "active".into());
    let payload = TeamSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        code: row.try_get("", "code").unwrap_or_default(),
        name: row.try_get("", "name").unwrap_or_default(),
        is_active: status == "active",
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_TEAMS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

/// Stage a reference catalog value identified by its domain code and value code.
pub async fn stage_reference_value_sync(db: &impl ConnectionTrait, set_code: &str, value_code: &str) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rv.id
             FROM reference_values rv
             JOIN reference_sets rs ON rs.id = rv.set_id
             JOIN reference_domains rd ON rd.id = rs.domain_id
             WHERE rd.code = ? AND rv.code = ?
             LIMIT 1",
            [set_code.into(), value_code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reference_value".into(),
            id: format!("{set_code}:{value_code}"),
        })?;
    let id: i64 = row
        .try_get("", "id")
        .map_err(|e| AppError::SyncError(format!("reference_values.id: {e}")))?;
    stage_reference_value_by_id(db, id).await
}

/// Stage a reference catalog value by local id.
pub async fn stage_reference_value_by_id(db: &impl ConnectionTrait, value_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rv.entity_sync_id, rv.code, rv.label, rv.is_active, rv.row_version, rv.updated_at,
                    rd.code AS set_code, parent.code AS parent_code
             FROM reference_values rv
             JOIN reference_sets rs ON rs.id = rv.set_id
             JOIN reference_domains rd ON rd.id = rs.domain_id
             LEFT JOIN reference_values parent ON parent.id = rv.parent_id
             WHERE rv.id = ?",
            [value_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "reference_value".into(),
            id: value_id.to_string(),
        })?;
    let decode = |field: &str, e: sea_orm::DbErr| AppError::SyncError(format!("reference_values.{field}: {e}"));
    let esid = require_entity_sync_id(
        "reference_values",
        value_id,
        row.try_get::<Option<String>>("", "entity_sync_id")
            .map_err(|e| decode("entity_sync_id", e))?,
    )?;
    let row_version: i64 = row.try_get("", "row_version").map_err(|e| decode("row_version", e))?;
    let is_active: i64 = row.try_get("", "is_active").map_err(|e| decode("is_active", e))?;
    let payload = ReferenceValueSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        set_code: row.try_get("", "set_code").map_err(|e| decode("set_code", e))?,
        code: row.try_get("", "code").map_err(|e| decode("code", e))?,
        label: row.try_get("", "label").map_err(|e| decode("label", e))?,
        is_active: is_active != 0,
        parent_code: row.try_get("", "parent_code").map_err(|e| decode("parent_code", e))?,
        updated_at: row.try_get("", "updated_at").map_err(|e| decode("updated_at", e))?,
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            entity_type: SYNC_ENTITY_REFERENCE_VALUES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            payload_json: serde_json::to_string(&payload)?,
        },
    )
    .await?;
    Ok(())
}

/// Best-effort staging after a durable mutation (never fails the write path).
pub fn warn_stage(result: AppResult<()>, context: &str) {
    if let Err(e) = result {
        tracing::warn!(target: "maintafox", "sync stage {context}: {e}");
    }
}
