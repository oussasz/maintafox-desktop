//! Equipment ↔ Org-node assignment queries.
//!
//! Thin query layer for the Equipment Assignment Widget (GAP ORG-02).
//! Equipment rows already carry `installed_at_node_id` — these functions
//! read and update that column.

use crate::errors::{AppError, AppResult};
use crate::org::fail::{fail, fail_params};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, TransactionTrait};
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
        id: row
            .try_get::<i32>("", "id")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment row decode: {e}")))?,
        asset_id_code: row
            .try_get::<String>("", "asset_id_code")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment row decode: {e}")))?,
        name: row
            .try_get::<String>("", "name")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment row decode: {e}")))?,
        lifecycle_status: row
            .try_get::<String>("", "lifecycle_status")
            .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment row decode: {e}")))?,
        installed_at_node_id: row.try_get::<Option<i32>>("", "installed_at_node_id").unwrap_or(None),
        current_node_name: row.try_get::<Option<String>>("", "current_node_name").unwrap_or(None),
    })
}

// ─── Service functions ────────────────────────────────────────────────────────

/// List equipment currently installed at the given org node.
pub async fn list_equipment_by_node(db: &DatabaseConnection, node_id: i64) -> AppResult<Vec<OrgNodeEquipmentRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r"SELECT e.id, e.asset_id_code, e.name, e.lifecycle_status,
                     e.installed_at_node_id, NULL AS current_node_name
              FROM equipment e
              WHERE e.installed_at_node_id = ? AND e.deleted_at IS NULL
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
                AND e.deleted_at IS NULL
                AND UPPER(e.lifecycle_status) NOT IN ('DECOMMISSIONED', 'SCRAPPED')
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
    actor_id: i32,
) -> AppResult<()> {
    crate::org::model_scope::assert_org_node_active(db, payload.node_id).await?;

    // Only units whose level hosts assets can receive equipment.
    let hosts_assets = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT t.can_host_assets AS can_host_assets FROM org_nodes n \
             JOIN org_node_types t ON t.id = n.node_type_id \
             WHERE n.id = ? AND n.deleted_at IS NULL",
            [payload.node_id.into()],
        ))
        .await?
        .map(|row| {
            row.try_get::<i32>("", "can_host_assets").map(|v| v != 0).map_err(|e| {
                AppError::Internal(anyhow::anyhow!(
                    "org node type decode failed for column 'can_host_assets': {e}"
                ))
            })
        })
        .transpose()?
        .unwrap_or(false);
    if !hosts_assets {
        return Err(fail(
            "ORG_NODE_CANNOT_HOST_ASSETS",
            "This organization level cannot host equipment.",
        ));
    }

    // Retired equipment cannot be placed in the structure.
    let status = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lifecycle_status FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [payload.equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".to_string(),
            id: payload.equipment_id.to_string(),
        })?
        .try_get::<String>("", "lifecycle_status")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment row decode: {e}")))?;
    let status_upper = status.trim().to_ascii_uppercase();
    if status_upper == "DECOMMISSIONED" || status_upper == "SCRAPPED" {
        return Err(fail_params(
            "ORG_EQUIPMENT_RETIRED",
            format!("Equipment with status '{status}' cannot be assigned to an organization unit."),
            &[("status", status)],
        ));
    }

    // A move like every other one (D19): MOVED lifecycle event + audit with the move, in one
    // transaction; the version is the one read here (this screen shows none).
    let equipment_id = i64::from(payload.equipment_id);
    let txn = db.begin().await?;
    let row_version: i64 = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT row_version FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [equipment_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".to_string(),
            id: payload.equipment_id.to_string(),
        })?
        .try_get("", "row_version")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment row decode: {e}")))?;
    crate::assets::lifecycle::move_asset_recorded_in(&txn, equipment_id, payload.node_id, row_version, actor_id)
        .await?;
    txn.commit().await?;
    tracing::info!(
        equipment_id = payload.equipment_id,
        node_id = payload.node_id,
        "equipment assigned to org node"
    );
    Ok(())
}

/// Clear `installed_at_node_id` on the equipment row.
pub async fn unassign_equipment_from_node(db: &DatabaseConnection, equipment_id: i32) -> AppResult<()> {
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE equipment SET installed_at_node_id = NULL, updated_at = ?, \
             row_version = row_version + 1 WHERE id = ? AND deleted_at IS NULL",
            [crate::clock::now().to_rfc3339().into(), equipment_id.into()],
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
