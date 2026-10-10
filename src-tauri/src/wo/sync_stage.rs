use crate::errors::AppResult;
use crate::sync::domain::{
    EquipmentSyncPayload, StageOutboxItemInput, WorkOrderIntervenerSyncPayload, WorkOrderSyncPayload,
    WorkOrderTaskSyncPayload, SYNC_ENTITY_EQUIPMENT, SYNC_ENTITY_WORK_ORDERS, SYNC_ENTITY_WORK_ORDER_INTERVENERS,
    SYNC_ENTITY_WORK_ORDER_TASKS,
};
use crate::sync::foundation_stage::warn_stage;
use crate::sync::queries::stage_outbox_item;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::domain::WorkOrder;

async fn lookup_entity_sync_id(db: &impl ConnectionTrait, table: &str, id: i64) -> AppResult<Option<String>> {
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

pub async fn stage_work_order_sync(
    db: &DatabaseConnection,
    wo: &WorkOrder,
    status_code: &str,
    maintenance_type_code: &str,
    closed_at: Option<&str>,
    closeout_profile_id: Option<i64>,
    closeout_passed: bool,
) -> AppResult<()> {
    let esid = wo.entity_sync_id.clone().unwrap_or_else(|| wo.code.clone());
    let equipment_entity_sync_id = match wo.equipment_id {
        Some(id) => lookup_entity_sync_id(db, "equipment", id).await?,
        None => None,
    };
    let primary_responsible_entity_sync_id = match wo.primary_responsible_id {
        Some(id) => lookup_entity_sync_id(db, "user_accounts", id).await?,
        None => None,
    };
    let assigned_team_entity_sync_id = match wo.assigned_team_id {
        Some(id) => lookup_entity_sync_id(db, "teams", id).await?,
        None => None,
    };
    let payload = WorkOrderSyncPayload {
        entity_sync_id: esid.clone(),
        row_version: wo.row_version,
        code: wo.code.clone(),
        title: wo.title.clone(),
        description: wo.description.clone(),
        status: status_code.to_string(),
        maintenance_type_code: Some(maintenance_type_code.to_string()),
        equipment_entity_sync_id,
        primary_responsible_entity_sync_id,
        assigned_team_entity_sync_id,
        planned_start: wo.planned_start.clone(),
        planned_end: wo.planned_end.clone(),
        scheduled_at: wo.scheduled_at.clone(),
        closed_at: closed_at.map(|s| s.to_string()),
        closeout_validation_profile_id: closeout_profile_id,
        closeout_validation_passed: closeout_passed,
        requires_permit: wo.requires_permit,
        source_inspection_anomaly_entity_sync_id: None,
        updated_at: Some(wo.updated_at.clone()),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("work_orders:{}:v{}", esid, wo.row_version),
            entity_type: SYNC_ENTITY_WORK_ORDERS.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version: wo.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

/// Load WO by id and stage (preferred post-commit hook).
pub async fn stage_work_order_by_id(db: &DatabaseConnection, wo_id: i64) -> AppResult<()> {
    let wo = crate::wo::queries::get_work_order(db, wo_id).await?;
    let Some(wo) = wo else {
        return Ok(());
    };
    let status = wo.status_code.as_deref().unwrap_or("draft");
    let type_code = wo.type_code.as_deref().unwrap_or("corrective");
    stage_work_order_sync(
        db,
        &wo,
        status,
        type_code,
        wo.closed_at.as_deref(),
        wo.closeout_validation_profile_id,
        wo.closeout_validation_passed,
    )
    .await
}

pub async fn warn_stage_wo(db: &DatabaseConnection, wo_id: i64) {
    warn_stage(stage_work_order_by_id(db, wo_id).await, "work_order");
}

pub async fn stage_work_order_task_sync(db: &DatabaseConnection, task_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT t.entity_sync_id, t.task_description, t.sequence_order, t.is_mandatory,
                    t.is_completed, t.result_code, t.notes, t.row_version,
                    w.entity_sync_id AS work_order_entity_sync_id
             FROM work_order_tasks t
             JOIN work_orders w ON w.id = t.work_order_id
             WHERE t.id = ?",
            [task_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let wo_esid: String = row.try_get("", "work_order_entity_sync_id").unwrap_or_default();
    if wo_esid.is_empty() {
        return Ok(());
    }
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let is_mandatory: i64 = row.try_get("", "is_mandatory").unwrap_or(0);
    let is_completed: i64 = row.try_get("", "is_completed").unwrap_or(0);
    let payload = WorkOrderTaskSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        work_order_entity_sync_id: wo_esid,
        task_description: row.try_get("", "task_description").unwrap_or_default(),
        sequence_order: row.try_get("", "sequence_order").unwrap_or(0),
        is_mandatory: is_mandatory != 0,
        is_completed: is_completed != 0,
        result_code: row.try_get("", "result_code").ok(),
        notes: row.try_get("", "notes").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("work_order_tasks:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_WORK_ORDER_TASKS.to_string(),
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

pub async fn stage_work_order_intervener_sync(db: &DatabaseConnection, intervener_row_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT i.entity_sync_id, i.hours_worked, i.started_at, i.ended_at, i.notes,
                    i.row_version, i.intervener_id,
                    w.entity_sync_id AS work_order_entity_sync_id,
                    u.entity_sync_id AS intervener_entity_sync_id
             FROM work_order_interveners i
             JOIN work_orders w ON w.id = i.work_order_id
             LEFT JOIN user_accounts u ON u.id = i.intervener_id
             WHERE i.id = ?",
            [intervener_row_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let wo_esid: String = row.try_get("", "work_order_entity_sync_id").unwrap_or_default();
    if wo_esid.is_empty() {
        return Ok(());
    }
    let esid: String = row
        .try_get("", "entity_sync_id")
        .ok()
        .filter(|s: &String| !s.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = WorkOrderIntervenerSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        work_order_entity_sync_id: wo_esid,
        intervener_entity_sync_id: row.try_get("", "intervener_entity_sync_id").ok(),
        hours_worked: row.try_get("", "hours_worked").ok(),
        started_at: row.try_get("", "started_at").ok(),
        ended_at: row.try_get("", "ended_at").ok(),
        notes: row.try_get("", "notes").ok(),
    };
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("work_order_interveners:{esid}:v{row_version}"),
            entity_type: SYNC_ENTITY_WORK_ORDER_INTERVENERS.to_string(),
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

pub async fn stage_equipment_sync(db: &DatabaseConnection, equipment_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT e.entity_sync_id, e.asset_id_code, e.name, e.lifecycle_status, e.row_version,
                    e.manufacturer, e.model, e.serial_number, e.updated_at,
                    o.entity_sync_id AS org_entity_sync_id
             FROM equipment e
             LEFT JOIN org_nodes o ON o.id = e.installed_at_node_id
             WHERE e.id = ?",
            [equipment_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let esid: String = row
        .try_get("", "entity_sync_id")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    let row_version: i64 = row.try_get("", "row_version").unwrap_or(1);
    let payload = EquipmentSyncPayload {
        entity_sync_id: esid.clone(),
        row_version,
        asset_code: row.try_get("", "asset_id_code").unwrap_or_default(),
        asset_name: row.try_get("", "name").unwrap_or_default(),
        status_code: row
            .try_get("", "lifecycle_status")
            .unwrap_or_else(|_| "active_in_service".into()),
        org_node_entity_sync_id: row.try_get("", "org_entity_sync_id").ok(),
        manufacturer: row.try_get("", "manufacturer").ok(),
        model: row.try_get("", "model").ok(),
        serial_number: row.try_get("", "serial_number").ok(),
        updated_at: row.try_get("", "updated_at").ok(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("equipment:{}:v{row_version}", esid),
            entity_type: SYNC_ENTITY_EQUIPMENT.to_string(),
            entity_sync_id: esid,
            operation: "upsert".to_string(),
            row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}
