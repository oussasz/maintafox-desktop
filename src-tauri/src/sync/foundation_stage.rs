//! Outbox staging for foundation replica entities (org, RBAC, personnel, reference).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::errors::AppResult;
use crate::sync::domain::{
    OrgNodeSyncPayload, PersonnelSyncPayload, ReferenceValueSyncPayload, RoleSyncPayload,
    StageOutboxItemInput, TeamSyncPayload, UserAccountSyncPayload, SYNC_ENTITY_ORG_NODES,
    SYNC_ENTITY_PERSONNEL, SYNC_ENTITY_REFERENCE_VALUES, SYNC_ENTITY_ROLES, SYNC_ENTITY_TEAMS,
    SYNC_ENTITY_USER_ACCOUNTS,
};
use crate::sync::queries::stage_outbox_item;

async fn lookup_entity_sync_id(
    db: &impl ConnectionTrait,
    table: &str,
    id: i64,
) -> AppResult<Option<String>> {
    let sql = format!("SELECT entity_sync_id FROM {table} WHERE id = ? LIMIT 1");
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            &sql,
            [id.into()],
        ))
        .await?;
    Ok(row
        .map(|r| r.try_get::<String>("", "entity_sync_id"))
        .transpose()
        .ok()
        .flatten()
        .filter(|s| !s.is_empty()))
}

pub async fn stage_org_node_sync(db: &DatabaseConnection, node_id: i64) -> AppResult<()> {
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
    let esid: String = row
        .try_get("", "entity_sync_id")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
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
            idempotency_key: format!("org_nodes:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_ORG_NODES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_user_account_sync(db: &DatabaseConnection, user_id: i64) -> AppResult<()> {
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
    let esid: String = row
        .try_get("", "entity_sync_id")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let is_admin: i64 = row.try_get("", "is_admin").unwrap_or(0);
    let payload = UserAccountSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        username: row.try_get("", "username").unwrap_or_default(),
        display_name: row.try_get("", "display_name").ok(),
        password_hash: row.try_get("", "password_hash").ok(),
        identity_mode: row
            .try_get("", "identity_mode")
            .unwrap_or_else(|_| "local".into()),
        is_active: is_active != 0,
        is_admin: is_admin != 0,
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("user_accounts:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_USER_ACCOUNTS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_role_sync(db: &DatabaseConnection, role_id: i64) -> AppResult<()> {
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
    let esid: String = row
        .try_get("", "entity_sync_id")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
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
            idempotency_key: format!("roles:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_ROLES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_personnel_sync(db: &DatabaseConnection, personnel_id: i64) -> AppResult<()> {
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
    let esid: String = row
        .try_get("", "entity_sync_id")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
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
            idempotency_key: format!("personnel:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_PERSONNEL.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_team_sync(db: &DatabaseConnection, team_id: i64) -> AppResult<()> {
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
    let esid: String = row
        .try_get("", "entity_sync_id")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
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
            idempotency_key: format!("teams:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_TEAMS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

/// Stage a reference catalog value. Stable identity is `set_code:value_code`.
pub async fn stage_reference_value_sync(
    db: &DatabaseConnection,
    set_code: &str,
    value_code: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT rv.entity_sync_id, rv.code, rv.label, rv.is_active, rv.row_version, rv.updated_at,
                    rd.code AS set_code, parent.code AS parent_code
             FROM reference_values rv
             JOIN reference_sets rs ON rs.id = rv.set_id
             JOIN reference_domains rd ON rd.id = rs.domain_id
             LEFT JOIN reference_values parent ON parent.id = rv.parent_id
             WHERE rd.code = ? AND rv.code = ?
             LIMIT 1",
            [set_code.into(), value_code.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let set = row
        .try_get::<String>("", "set_code")
        .unwrap_or_else(|_| set_code.to_string());
    let code: String = row.try_get("", "code").unwrap_or_else(|_| value_code.to_string());
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| format!("{set}:{code}"));
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_active: i64 = row.try_get("", "is_active").unwrap_or(1);
    let payload = ReferenceValueSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        set_code: set,
        code,
        label: row.try_get("", "label").unwrap_or_default(),
        is_active: is_active != 0,
        parent_code: row.try_get("", "parent_code").ok(),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("reference_values:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_REFERENCE_VALUES.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json: serde_json::to_string(&payload)?,
            origin_machine_id: None,
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

#[allow(dead_code)]
pub async fn resolve_parent_sync_id(
    db: &impl ConnectionTrait,
    table: &str,
    id: Option<i64>,
) -> AppResult<Option<String>> {
    match id {
        Some(i) => lookup_entity_sync_id(db, table, i).await,
        None => Ok(None),
    }
}
