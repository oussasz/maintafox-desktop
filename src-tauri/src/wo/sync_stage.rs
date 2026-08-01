use crate::errors::AppResult;
use crate::sync::domain::{
    StageOutboxItemInput, WorkOrderSyncPayload, SYNC_ENTITY_WORK_ORDERS,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::WorkOrder;

pub async fn stage_work_order_sync(
    db: &sea_orm::DatabaseConnection,
    wo: &WorkOrder,
    status_code: &str,
    maintenance_type_code: &str,
    closed_at: Option<&str>,
    closeout_profile_id: Option<i64>,
    closeout_passed: bool,
) -> AppResult<()> {
    let esid = wo.entity_sync_id.clone().unwrap_or_else(|| wo.code.clone());
    let payload = WorkOrderSyncPayload {
        id: wo.id,
        entity_sync_id: esid.clone(),
        row_version: wo.row_version,
        status: status_code.to_string(),
        maintenance_type_code: maintenance_type_code.to_string(),
        asset_id: wo.equipment_id,
        closed_at: closed_at.map(|s| s.to_string()),
        closeout_validation_profile_id: closeout_profile_id,
        closeout_validation_passed: closeout_passed,
        code: wo.code.clone(),
        status_id: wo.status_id,
        requires_permit: wo.requires_permit,
        source_inspection_anomaly_id: wo.source_inspection_anomaly_id,
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
