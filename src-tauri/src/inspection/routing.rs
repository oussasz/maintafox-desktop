use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::di::queries::{create_intervention_request, DiCreateInput};
use crate::errors::{AppError, AppResult};
use crate::inspection::domain::InspectionAnomaly;
use crate::inspection::queries::get_inspection_checkpoint_by_id;
use crate::inspection::results::{map_anomaly, stage_inspection_anomaly};
use crate::sync::domain::{
    InterventionRequestSyncPayload, StageOutboxItemInput, SYNC_ENTITY_INTERVENTION_REQUESTS,
};
use crate::sync::queries::stage_outbox_item;
use crate::wo::sync_stage::stage_work_order_sync;
use crate::wo::domain::WoCreateInput;
use crate::wo::queries::create_work_order;

async fn load_anomaly(db: &DatabaseConnection, id: i64) -> AppResult<InspectionAnomaly> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, round_id, result_id, anomaly_type, severity, description, linked_di_id, linked_work_order_id, \
             requires_permit_review, resolution_status, routing_decision, entity_sync_id, row_version \
             FROM inspection_anomalies WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionAnomaly".into(),
            id: id.to_string(),
        })?;
    map_anomaly(&row)
}

fn assert_routable(a: &InspectionAnomaly) -> AppResult<()> {
    if a.linked_di_id.is_some() || a.linked_work_order_id.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Cette anomalie est déjà liée à un DI ou un OT.".into(),
        ]));
    }
    Ok(())
}

async fn stage_intervention_request_sync(db: &DatabaseConnection, di: &crate::di::domain::InterventionRequest) -> AppResult<()> {
    let payload = InterventionRequestSyncPayload {
        id: di.id,
        row_version: di.row_version,
        source_inspection_anomaly_id: di.source_inspection_anomaly_id,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("intervention_requests:{}:v{}", di.code, di.row_version),
            entity_type: SYNC_ENTITY_INTERVENTION_REQUESTS.to_string(),
            entity_sync_id: di.code.clone(),
            operation: "upsert".to_string(),
            row_version: di.row_version,
            payload_json,
            origin_machine_id: None,
        },
    )
    .await?;
    Ok(())
}

async fn stage_work_order_sync_from_row(db: &DatabaseConnection, wo: &crate::wo::domain::WorkOrder) -> AppResult<()> {
    let status_code = wo.status_code.as_deref().unwrap_or("draft");
    let type_code = wo.type_code.as_deref().unwrap_or("corrective");
    stage_work_order_sync(
        db,
        wo,
        status_code,
        type_code,
        wo.closed_at.as_deref(),
        wo.closeout_validation_profile_id,
        wo.closeout_validation_passed,
    )
    .await
}

pub async fn route_inspection_anomaly_to_di(
    db: &DatabaseConnection,
    anomaly_id: i64,
    expected_row_version: i64,
    submitter_user_id: i64,
    title_override: Option<String>,
    description_override: Option<String>,
) -> AppResult<(crate::di::domain::InterventionRequest, InspectionAnomaly)> {
    let a = load_anomaly(db, anomaly_id).await?;
    if a.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on inspection_anomalies.".into()]));
    }
    assert_routable(&a)?;
    let Some(rid) = a.result_id else {
        return Err(AppError::ValidationFailed(vec![
            "L'anomalie doit être liée à un résultat pour créer un DI.".into(),
        ]));
    };
    let res_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT checkpoint_id FROM inspection_results WHERE id = ?",
            [rid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("result missing")))?;
    let checkpoint_id: i64 = res_row.try_get("", "checkpoint_id").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("checkpoint_id decode: {e}"))
    })?;
    let cp = get_inspection_checkpoint_by_id(db, checkpoint_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionCheckpoint".into(),
            id: checkpoint_id.to_string(),
        })?;
    let Some(asset_id) = cp.asset_id else {
        return Err(AppError::ValidationFailed(vec![
            "Le point de contrôle n'a pas d'équipement (asset_id).".into(),
        ]));
    };
    let node_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ?",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec![format!("Équipement introuvable (id={asset_id}).")]))?;
    let org_node_id: Option<i64> = node_row.try_get("", "installed_at_node_id").ok();
    let Some(org_node_id) = org_node_id else {
        return Err(AppError::ValidationFailed(vec![
            "L'équipement n'a pas de nœud d'installation (installed_at_node_id).".into(),
        ]));
    };

    let title = title_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("Inspection — {}", a.anomaly_type));
    let description = description_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| a.description.clone());

    let reported_urgency = match a.severity {
        1 | 2 => "low",
        3 => "normal",
        _ => "high",
    };

    let di_in = DiCreateInput {
        asset_id,
        org_node_id,
        title,
        description,
        origin_type: "inspection".into(),
        symptom_code_id: None,
        impact_level: "medium".into(),
        production_impact: false,
        safety_flag: a.severity >= 4,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: reported_urgency.to_string(),
        observed_at: None,
        submitter_id: submitter_user_id,
        source_inspection_anomaly_id: Some(anomaly_id),
    };

    let di = create_intervention_request(db, di_in).await?;
    stage_intervention_request_sync(db, &di).await?;

    let new_rv = a.row_version + 1;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_anomalies SET linked_di_id = ?, routing_decision = 'di', resolution_status = 'triaged', \
         row_version = ? WHERE id = ? AND row_version = ?",
        [
            di.id.into(),
            new_rv.into(),
            anomaly_id.into(),
            expected_row_version.into(),
        ],
    ))
    .await?;

    let updated = load_anomaly(db, anomaly_id).await?;
    stage_inspection_anomaly(db, &updated).await?;
    Ok((di, updated))
}

pub async fn route_inspection_anomaly_to_wo(
    db: &DatabaseConnection,
    anomaly_id: i64,
    expected_row_version: i64,
    creator_user_id: i64,
    type_id: i64,
    title_override: Option<String>,
) -> AppResult<(crate::wo::domain::WorkOrder, InspectionAnomaly)> {
    let a = load_anomaly(db, anomaly_id).await?;
    if a.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on inspection_anomalies.".into()]));
    }
    assert_routable(&a)?;

    let open = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_orders WHERE source_inspection_anomaly_id = ? \
             AND closed_at IS NULL AND cancelled_at IS NULL",
            [anomaly_id.into()],
        ))
        .await?;
    if open.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Un ordre de travail ouvert existe déjà pour cette anomalie.".into(),
        ]));
    }

    let Some(rid) = a.result_id else {
        return Err(AppError::ValidationFailed(vec![
            "L'anomalie doit être liée à un résultat pour créer un OT.".into(),
        ]));
    };
    let res_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT checkpoint_id FROM inspection_results WHERE id = ?",
            [rid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("result missing")))?;
    let checkpoint_id: i64 = res_row.try_get("", "checkpoint_id").map_err(|e| {
        AppError::Internal(anyhow::anyhow!("checkpoint_id decode: {e}"))
    })?;
    let cp = get_inspection_checkpoint_by_id(db, checkpoint_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionCheckpoint".into(),
            id: checkpoint_id.to_string(),
        })?;
    let equipment_id = cp.asset_id;

    let title = title_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("Inspection — {}", a.anomaly_type));

    let wo_in = WoCreateInput {
        type_id,
        equipment_id,
        location_id: None,
        source_di_id: None,
        source_inspection_anomaly_id: Some(anomaly_id),
        source_ram_ishikawa_diagram_id: None,
        source_ishikawa_flow_node_id: None,
        source_rca_cause_text: None,
        entity_id: None,
        planner_id: None,
        urgency_id: None,
        title,
        description: Some(a.description.clone()),
        notes: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        creator_id: creator_user_id,
        requires_permit: Some(a.requires_permit_review),
    };

    let wo = create_work_order(db, wo_in).await?;
    stage_work_order_sync_from_row(db, &wo).await?;

    let new_rv = a.row_version + 1;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_anomalies SET linked_work_order_id = ?, routing_decision = 'wo', resolution_status = 'triaged', \
         row_version = ? WHERE id = ? AND row_version = ?",
        [
            wo.id.into(),
            new_rv.into(),
            anomaly_id.into(),
            expected_row_version.into(),
        ],
    ))
    .await?;

    let updated = load_anomaly(db, anomaly_id).await?;
    stage_inspection_anomaly(db, &updated).await?;
    Ok((wo, updated))
}

pub async fn defer_inspection_anomaly(
    db: &DatabaseConnection,
    anomaly_id: i64,
    expected_row_version: i64,
) -> AppResult<InspectionAnomaly> {
    let a = load_anomaly(db, anomaly_id).await?;
    if a.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on inspection_anomalies.".into()]));
    }
    assert_routable(&a)?;
    let new_rv = a.row_version + 1;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_anomalies SET routing_decision = 'defer', resolution_status = 'deferred', row_version = ? \
         WHERE id = ? AND row_version = ?",
        [new_rv.into(), anomaly_id.into(), expected_row_version.into()],
    ))
    .await?;
    let updated = load_anomaly(db, anomaly_id).await?;
    stage_inspection_anomaly(db, &updated).await?;
    Ok(updated)
}
