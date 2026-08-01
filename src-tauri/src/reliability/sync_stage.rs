use sea_orm::DatabaseConnection;

use crate::errors::AppResult;
use crate::sync::domain::{
    FailureCodeSyncPayload, FailureEventSyncPayload, FailureHierarchySyncPayload, ReliabilityKpiSnapshotSyncPayload,
    RuntimeExposureLogSyncPayload, StageOutboxItemInput, UserDismissalSyncPayload, SYNC_ENTITY_FAILURE_CODES,
    SYNC_ENTITY_FAILURE_EVENTS, SYNC_ENTITY_FAILURE_HIERARCHIES, SYNC_ENTITY_RELIABILITY_KPI_SNAPSHOTS,
    SYNC_ENTITY_RUNTIME_EXPOSURE_LOGS, SYNC_ENTITY_USER_DISMISSALS,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::{
    FailureCode, FailureEvent, FailureHierarchy, ReliabilityKpiSnapshot, RuntimeExposureLog, UserDismissal,
};

pub async fn stage_failure_hierarchy(db: &DatabaseConnection, row: &FailureHierarchy) -> AppResult<()> {
    let payload = FailureHierarchySyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        name: row.name.clone(),
        version_no: row.version_no,
        is_active: row.is_active,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("failure_hierarchies:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_FAILURE_HIERARCHIES.to_string(),
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

pub async fn stage_failure_code(db: &DatabaseConnection, row: &FailureCode) -> AppResult<()> {
    let payload = FailureCodeSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        hierarchy_id: row.hierarchy_id,
        parent_id: row.parent_id,
        code: row.code.clone(),
        code_type: row.code_type.clone(),
        iso_14224_annex_ref: row.iso_14224_annex_ref.clone(),
        is_active: row.is_active,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("failure_codes:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_FAILURE_CODES.to_string(),
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

pub async fn stage_failure_event(db: &DatabaseConnection, row: &FailureEvent) -> AppResult<()> {
    let payload = FailureEventSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        source_type: row.source_type.clone(),
        source_id: row.source_id,
        equipment_id: row.equipment_id,
        component_id: row.component_id,
        detected_at: row.detected_at.clone(),
        failed_at: row.failed_at.clone(),
        restored_at: row.restored_at.clone(),
        downtime_duration_hours: row.downtime_duration_hours,
        active_repair_hours: row.active_repair_hours,
        waiting_hours: row.waiting_hours,
        is_planned: row.is_planned,
        failure_class_id: row.failure_class_id,
        failure_mode_id: row.failure_mode_id,
        failure_cause_id: row.failure_cause_id,
        failure_effect_id: row.failure_effect_id,
        failure_mechanism_id: row.failure_mechanism_id,
        cause_not_determined: row.cause_not_determined,
        production_impact_level: row.production_impact_level,
        safety_impact_level: row.safety_impact_level,
        recorded_by_id: row.recorded_by_id,
        verification_status: row.verification_status.clone(),
        eligible_flags_json: row.eligible_flags_json.clone(),
        row_version: row.row_version,
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("failure_events:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_FAILURE_EVENTS.to_string(),
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

pub async fn stage_runtime_exposure_log(db: &DatabaseConnection, row: &RuntimeExposureLog) -> AppResult<()> {
    let payload = RuntimeExposureLogSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        equipment_id: row.equipment_id,
        exposure_type: row.exposure_type.clone(),
        value: row.value,
        recorded_at: row.recorded_at.clone(),
        source_type: row.source_type.clone(),
        row_version: row.row_version,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("runtime_exposure_logs:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_RUNTIME_EXPOSURE_LOGS.to_string(),
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

pub async fn stage_reliability_kpi_snapshot(db: &DatabaseConnection, row: &ReliabilityKpiSnapshot) -> AppResult<()> {
    let payload = ReliabilityKpiSnapshotSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        equipment_id: row.equipment_id,
        asset_group_id: row.asset_group_id,
        period_start: row.period_start.clone(),
        period_end: row.period_end.clone(),
        mtbf: row.mtbf,
        mttr: row.mttr,
        availability: row.availability,
        failure_rate: row.failure_rate,
        repeat_failure_rate: row.repeat_failure_rate,
        event_count: row.event_count,
        data_quality_score: row.data_quality_score,
        inspection_signal_json: row.inspection_signal_json.clone(),
        analysis_dataset_hash_sha256: row.analysis_dataset_hash_sha256.clone(),
        analysis_input_spec_json: row.analysis_input_spec_json.clone(),
        plot_payload_json: row.plot_payload_json.clone(),
        row_version: row.row_version,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("reliability_kpi_snapshots:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_RELIABILITY_KPI_SNAPSHOTS.to_string(),
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

pub async fn stage_user_dismissal(db: &DatabaseConnection, row: &UserDismissal) -> AppResult<()> {
    let payload = UserDismissalSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        user_id: row.user_id,
        equipment_id: row.equipment_id,
        issue_code: row.issue_code.clone(),
        scope_key: row.scope_key.clone(),
        dismissed_at: row.dismissed_at.clone(),
        row_version: row.row_version,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("user_dismissals:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_USER_DISMISSALS.to_string(),
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
