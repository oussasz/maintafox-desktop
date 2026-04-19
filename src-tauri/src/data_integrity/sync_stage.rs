use crate::errors::AppResult;
use crate::sync::domain::{
    DataIntegrityFindingSyncPayload, DataIntegrityRepairActionSyncPayload, StageOutboxItemInput,
    SYNC_ENTITY_DATA_INTEGRITY_FINDINGS, SYNC_ENTITY_DATA_INTEGRITY_REPAIR_ACTIONS,
};
use crate::sync::queries::stage_outbox_item;
use sea_orm::ConnectionTrait;

pub async fn stage_finding_sync(
    db: &impl ConnectionTrait,
    id: i64,
    entity_sync_id: &str,
    row_version: i64,
    severity: &str,
    domain: &str,
    record_class: &str,
    record_id: i64,
    finding_code: &str,
    details_json: &str,
    status: &str,
) -> AppResult<()> {
    let payload = DataIntegrityFindingSyncPayload {
        id,
        entity_sync_id: entity_sync_id.to_string(),
        row_version,
        severity: severity.to_string(),
        domain: domain.to_string(),
        record_class: record_class.to_string(),
        record_id,
        finding_code: finding_code.to_string(),
        details_json: details_json.to_string(),
        status: status.to_string(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("data_integrity_findings:{entity_sync_id}:v{row_version}"),
            entity_type: SYNC_ENTITY_DATA_INTEGRITY_FINDINGS.to_string(),
            entity_sync_id: entity_sync_id.to_string(),
            operation: "upsert".to_string(),
            row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn stage_repair_action_sync(
    db: &impl ConnectionTrait,
    id: i64,
    entity_sync_id: &str,
    row_version: i64,
    finding_id: i64,
    action: &str,
    actor_id: i64,
    before_json: &str,
    after_json: &str,
) -> AppResult<()> {
    let payload = DataIntegrityRepairActionSyncPayload {
        id,
        entity_sync_id: entity_sync_id.to_string(),
        row_version,
        finding_id,
        action: action.to_string(),
        actor_id,
        before_json: before_json.to_string(),
        after_json: after_json.to_string(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("data_integrity_repair_actions:{entity_sync_id}:v{row_version}"),
            entity_type: SYNC_ENTITY_DATA_INTEGRITY_REPAIR_ACTIONS.to_string(),
            entity_sync_id: entity_sync_id.to_string(),
            operation: "upsert".to_string(),
            row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}
