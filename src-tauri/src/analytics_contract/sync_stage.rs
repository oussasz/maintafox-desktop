use crate::errors::AppResult;
use crate::sync::domain::{
    AnalyticsContractVersionSyncPayload, StageOutboxItemInput, SYNC_ENTITY_ANALYTICS_CONTRACT_VERSIONS,
};
use crate::sync::queries::stage_outbox_item;
use sea_orm::ConnectionTrait;

pub async fn stage_analytics_contract_version_sync(
    db: &impl ConnectionTrait,
    row: &super::queries::AnalyticsContractVersionRow,
) -> AppResult<()> {
    let payload = AnalyticsContractVersionSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        contract_id: row.contract_id.clone(),
        version_semver: row.version_semver.clone(),
        content_sha256: row.content_sha256.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "analytics_contract_versions:{}:v{}",
                row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_ANALYTICS_CONTRACT_VERSIONS.to_string(),
            entity_sync_id: row.entity_sync_id.clone(),
            operation: "upsert".to_string(),
            row_version: row.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}
