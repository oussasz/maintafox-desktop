use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::di::queries::{create_intervention_request, DiCreateInput};
use crate::errors::{AppError, AppResult};
use crate::inspection::domain::InspectionAnomaly;
use crate::inspection::queries::get_inspection_checkpoint_by_id;
use crate::inspection::results::{map_anomaly, stage_inspection_anomaly};
use crate::wo::domain::WoCreateInput;
use crate::wo::queries::create_work_order;

async fn load_anomaly(db: &impl ConnectionTrait, id: i64) -> AppResult<InspectionAnomaly> {
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
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on inspection_anomalies.".into(),
        ]));
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
    let checkpoint_id: i64 = res_row
        .try_get("", "checkpoint_id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("checkpoint_id decode: {e}")))?;
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

    let symptom_code_id = crate::di::reference_catalog::resolve_system_di_symptom_id(db, &a.anomaly_type).await?;

    let di_in = DiCreateInput {
        equipment_id: asset_id,
        org_node_id,
        title,
        description,
        origin_type: "inspection".into(),
        request_type: "repair".to_string(),
        symptom_code_id: Some(symptom_code_id),
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

    // create_intervention_request commits the DI together with its own outbox entry.
    let di = create_intervention_request(db, di_in).await?;

    let txn = db.begin().await?;
    let new_rv = a.row_version + 1;
    txn.execute(Statement::from_sql_and_values(
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

    let updated = load_anomaly(&txn, anomaly_id).await?;
    stage_inspection_anomaly(&txn, &updated).await?;
    txn.commit().await?;
    Ok((di, updated))
}

pub async fn route_inspection_anomaly_to_wo(
    db: &DatabaseConnection,
    anomaly_id: i64,
    expected_row_version: i64,
    creator_user_id: i64,
    type_code: String,
    title_override: Option<String>,
) -> AppResult<(crate::wo::domain::WorkOrder, InspectionAnomaly)> {
    let a = load_anomaly(db, anomaly_id).await?;
    if a.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on inspection_anomalies.".into(),
        ]));
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
    let checkpoint_id: i64 = res_row
        .try_get("", "checkpoint_id")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("checkpoint_id decode: {e}")))?;
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
        type_code,
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

    // create_work_order commits the WO together with its own outbox entry.
    let wo = create_work_order(db, wo_in).await?;

    let txn = db.begin().await?;
    let new_rv = a.row_version + 1;
    txn.execute(Statement::from_sql_and_values(
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

    let updated = load_anomaly(&txn, anomaly_id).await?;
    stage_inspection_anomaly(&txn, &updated).await?;
    txn.commit().await?;
    Ok((wo, updated))
}

pub async fn defer_inspection_anomaly(
    db: &DatabaseConnection,
    anomaly_id: i64,
    expected_row_version: i64,
) -> AppResult<InspectionAnomaly> {
    let txn = db.begin().await?;
    let a = load_anomaly(&txn, anomaly_id).await?;
    if a.row_version != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on inspection_anomalies.".into(),
        ]));
    }
    assert_routable(&a)?;
    let new_rv = a.row_version + 1;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE inspection_anomalies SET routing_decision = 'defer', resolution_status = 'deferred', row_version = ? \
         WHERE id = ? AND row_version = ?",
        [new_rv.into(), anomaly_id.into(), expected_row_version.into()],
    ))
    .await?;
    let updated = load_anomaly(&txn, anomaly_id).await?;
    stage_inspection_anomaly(&txn, &updated).await?;
    txn.commit().await?;
    Ok(updated)
}
