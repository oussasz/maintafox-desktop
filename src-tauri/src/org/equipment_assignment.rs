//! Equipment ↔ Org-node assignment queries.
//!
//! Thin query layer for the Equipment Assignment Widget (GAP ORG-02).
//! Equipment rows already carry `installed_at_node_id` — these functions
//! read and update that column.

use crate::errors::{AppError, AppResult};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement};
use serde::{Deserialize, Serialize};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgNodeEquipmentRow {
    pub id: i32,
    pub asset_id_code: String,
    pub name: String,
    pub lifecycle_status: String,
    pub installed_at_node_id: Option<i32>,
    /// Populated when the asset is currently assigned to a *different* node.
    pub current_node_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssignEquipmentPayload {
    pub equipment_id: i32,
    pub node_id: i64,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn map_equipment_row(row: QueryResult) -> AppResult<OrgNodeEquipmentRow> {
    Ok(OrgNodeEquipmentRow {
        id: row.try_get::<i32>("", "id").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("equipment row decode: {e}"))
        })?,
        asset_id_code: row.try_get::<String>("", "asset_id_code").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("equipment row decode: {e}"))
        })?,
        name: row.try_get::<String>("", "name").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("equipment row decode: {e}"))
        })?,
        lifecycle_status: row.try_get::<String>("", "lifecycle_status").map_err(|e| {
            AppError::Internal(anyhow::anyhow!("equipment row decode: {e}"))
        })?,
        installed_at_node_id: row
            .try_get::<Option<i32>>("", "installed_at_node_id")
            .unwrap_or(None),
        current_node_name: row
            .try_get::<Option<String>>("", "current_node_name")
            .unwrap_or(None),
    })
}

// ─── Service functions ────────────────────────────────────────────────────────

/// List equipment currently installed at the given org node.
pub async fn list_equipment_by_node(
    db: &DatabaseConnection,
    node_id: i64,
) -> AppResult<Vec<OrgNodeEquipmentRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT e.id, e.asset_id_code, e.name, e.lifecycle_status,
                     e.installed_at_node_id, NULL AS current_node_name
              FROM equipment e
              WHERE e.installed_at_node_id = ?
              ORDER BY e.asset_id_code ASC",
            [node_id.into()],
        ))
        .await?;
    rows.into_iter().map(map_equipment_row).collect()
}

/// Search equipment not installed at any node, or installed elsewhere.
/// Used by the "Assign Equipment" combobox.
pub async fn search_unassigned_equipment(
    db: &DatabaseConnection,
    query: &str,
    limit: i32,
) -> AppResult<Vec<OrgNodeEquipmentRow>> {
    let pattern = format!("%{query}%");
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT e.id, e.asset_id_code, e.name, e.lifecycle_status,
                     e.installed_at_node_id,
                     n.name AS current_node_name
              FROM equipment e
              LEFT JOIN org_nodes n ON n.id = e.installed_at_node_id AND n.deleted_at IS NULL
              WHERE (e.asset_id_code LIKE ? OR e.name LIKE ?)
                AND e.lifecycle_status NOT IN ('decommissioned', 'scrapped')
              ORDER BY e.asset_id_code ASC
              LIMIT ?",
            [pattern.clone().into(), pattern.into(), limit.into()],
        ))
        .await?;
    rows.into_iter().map(map_equipment_row).collect()
}

/// Set `installed_at_node_id` on the equipment row.
pub async fn assign_equipment_to_node(
    db: &DatabaseConnection,
    payload: AssignEquipmentPayload,
) -> AppResult<()> {
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET installed_at_node_id = ? WHERE id = ?",
            [payload.node_id.into(), payload.equipment_id.into()],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "equipment".to_string(),
            id: payload.equipment_id.to_string(),
        });
    }
    tracing::info!(
        equipment_id = payload.equipment_id,
        node_id = payload.node_id,
        "equipment assigned to org node"
    );
    Ok(())
}

/// Clear `installed_at_node_id` on the equipment row.
pub async fn unassign_equipment_from_node(
    db: &DatabaseConnection,
    equipment_id: i32,
) -> AppResult<()> {
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET installed_at_node_id = NULL WHERE id = ?",
            [equipment_id.into()],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "equipment".to_string(),
            id: equipment_id.to_string(),
        });
    }
    tracing::info!(equipment_id, "equipment unassigned from org node");
    Ok(())
}
