use sea_orm::DatabaseConnection;

use crate::errors::AppResult;
use crate::finance::domain::{
    BudgetAlertConfig, BudgetAlertEvent, BudgetLine, BudgetVersion, IntegrationException, PostedExportBatch,
};
use crate::sync::domain::{
    BudgetAlertConfigSyncPayload, BudgetAlertEventSyncPayload, BudgetLineSyncPayload, BudgetVersionSyncPayload,
    IntegrationExceptionSyncPayload, PostedExportBatchSyncPayload, StageOutboxItemInput,
    SYNC_ENTITY_BUDGET_ALERT_CONFIGS, SYNC_ENTITY_BUDGET_ALERT_EVENTS, SYNC_ENTITY_BUDGET_LINES,
    SYNC_ENTITY_BUDGET_VERSIONS, SYNC_ENTITY_INTEGRATION_EXCEPTIONS, SYNC_ENTITY_POSTED_EXPORT_BATCHES,
};
use crate::sync::queries::stage_outbox_item;

pub async fn stage_budget_version(db: &DatabaseConnection, row: &BudgetVersion) -> AppResult<()> {
    let payload = BudgetVersionSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        fiscal_year: row.fiscal_year,
        scenario_type: row.scenario_type.clone(),
        version_no: row.version_no,
        status: row.status.clone(),
        currency_code: row.currency_code.clone(),
        title: row.title.clone(),
        planning_basis: row.planning_basis.clone(),
        source_basis_mix_json: row.source_basis_mix_json.clone(),
        labor_assumptions_json: row.labor_assumptions_json.clone(),
        baseline_reference: row.baseline_reference.clone(),
        erp_external_ref: row.erp_external_ref.clone(),
        successor_of_version_id: row.successor_of_version_id,
        created_by_id: row.created_by_id,
        approved_at: row.approved_at.clone(),
        approved_by_id: row.approved_by_id,
        frozen_at: row.frozen_at.clone(),
        frozen_by_id: row.frozen_by_id,
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "{}:{}:v{}",
                SYNC_ENTITY_BUDGET_VERSIONS, row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_BUDGET_VERSIONS.to_string(),
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

pub async fn stage_budget_line(db: &DatabaseConnection, row: &BudgetLine) -> AppResult<()> {
    let payload = BudgetLineSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        budget_version_id: row.budget_version_id,
        cost_center_id: row.cost_center_id,
        period_month: row.period_month,
        budget_bucket: row.budget_bucket.clone(),
        planned_amount: row.planned_amount,
        source_basis: row.source_basis.clone(),
        justification_note: row.justification_note.clone(),
        asset_family: row.asset_family.clone(),
        work_category: row.work_category.clone(),
        shutdown_package_ref: row.shutdown_package_ref.clone(),
        team_id: row.team_id,
        skill_pool_id: row.skill_pool_id,
        labor_lane: row.labor_lane.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "{}:{}:v{}",
                SYNC_ENTITY_BUDGET_LINES, row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_BUDGET_LINES.to_string(),
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

pub async fn stage_budget_alert_config(db: &DatabaseConnection, row: &BudgetAlertConfig) -> AppResult<()> {
    let payload = BudgetAlertConfigSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        budget_version_id: row.budget_version_id,
        cost_center_id: row.cost_center_id,
        budget_bucket: row.budget_bucket.clone(),
        alert_type: row.alert_type.clone(),
        threshold_pct: row.threshold_pct,
        threshold_amount: row.threshold_amount,
        recipient_user_id: row.recipient_user_id,
        recipient_role_id: row.recipient_role_id,
        labor_template: row.labor_template.clone(),
        dedupe_window_minutes: row.dedupe_window_minutes,
        requires_ack: row.requires_ack,
        is_active: row.is_active,
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "{}:{}:v{}",
                SYNC_ENTITY_BUDGET_ALERT_CONFIGS, row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_BUDGET_ALERT_CONFIGS.to_string(),
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

pub async fn stage_budget_alert_event(db: &DatabaseConnection, row: &BudgetAlertEvent) -> AppResult<()> {
    let payload = BudgetAlertEventSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        alert_config_id: row.alert_config_id,
        budget_version_id: row.budget_version_id,
        cost_center_id: row.cost_center_id,
        period_month: row.period_month,
        budget_bucket: row.budget_bucket.clone(),
        alert_type: row.alert_type.clone(),
        severity: row.severity.clone(),
        title: row.title.clone(),
        message: row.message.clone(),
        dedupe_key: row.dedupe_key.clone(),
        current_value: row.current_value,
        threshold_value: row.threshold_value,
        variance_amount: row.variance_amount,
        currency_code: row.currency_code.clone(),
        payload_json: row.payload_json.clone(),
        notification_event_id: row.notification_event_id,
        notification_id: row.notification_id,
        acknowledged_at: row.acknowledged_at.clone(),
        acknowledged_by_id: row.acknowledged_by_id,
        acknowledgement_note: row.acknowledgement_note.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "{}:{}:v{}",
                SYNC_ENTITY_BUDGET_ALERT_EVENTS, row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_BUDGET_ALERT_EVENTS.to_string(),
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

pub async fn stage_posted_export_batch(db: &DatabaseConnection, row: &PostedExportBatch) -> AppResult<()> {
    let payload = PostedExportBatchSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        batch_uuid: row.batch_uuid.clone(),
        export_kind: row.export_kind.clone(),
        tenant_id: row.tenant_id.clone(),
        relay_payload_json: row.relay_payload_json.clone(),
        total_posted: row.total_posted,
        line_count: row.line_count,
        status: row.status.clone(),
        erp_ack_at: row.erp_ack_at.clone(),
        erp_http_code: row.erp_http_code,
        rejection_code: row.rejection_code.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "{}:{}:v{}",
                SYNC_ENTITY_POSTED_EXPORT_BATCHES, row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_POSTED_EXPORT_BATCHES.to_string(),
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

pub async fn stage_integration_exception(db: &DatabaseConnection, row: &IntegrationException) -> AppResult<()> {
    let payload = IntegrationExceptionSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        posted_export_batch_id: row.posted_export_batch_id,
        source_record_kind: row.source_record_kind.clone(),
        source_record_id: row.source_record_id,
        maintafox_value_snapshot: row.maintafox_value_snapshot.clone(),
        external_value_snapshot: row.external_value_snapshot.clone(),
        resolution_status: row.resolution_status.clone(),
        rejection_code: row.rejection_code.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!(
                "{}:{}:v{}",
                SYNC_ENTITY_INTEGRATION_EXCEPTIONS, row.entity_sync_id, row.row_version
            ),
            entity_type: SYNC_ENTITY_INTEGRATION_EXCEPTIONS.to_string(),
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
