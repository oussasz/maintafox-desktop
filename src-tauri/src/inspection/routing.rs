use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::di::queries::{create_intervention_request_in, DiCreateInput};
use crate::errors::{AppError, AppResult};
use crate::inspection::domain::InspectionAnomaly;
use crate::inspection::queries::get_inspection_checkpoint_by_id;
use crate::inspection::results::map_anomaly;
use crate::wo::domain::WoCreateInput;
use crate::wo::queries::create_work_order_in;

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

/// The follow-up title is written (or confirmed) by a person: it is what planners read in the
/// request list, so it is never generated from an internal anomaly code.
fn required_title(title: Option<String>) -> AppResult<String> {
    title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::ValidationFailed(vec![
                "GATE_INSP_FOLLOW_UP_TITLE_REQUIRED: give the request or work order a title.".into(),
            ])
        })
}

/// The anomaly changed since it was read (or another follow-up was written from it meanwhile).
fn changed_elsewhere() -> AppError {
    AppError::ValidationFailed(vec![
        "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on inspection_anomalies.".into(),
    ])
}

/// Links the follow-up to the anomaly, in the transaction that wrote the follow-up: the anomaly is
/// still at the version read (else nothing is kept, audit finding AU-71).
async fn link_follow_up(
    txn: &impl ConnectionTrait,
    anomaly_id: i64,
    expected_row_version: i64,
    set_sql: &str,
    follow_up_id: i64,
) -> AppResult<InspectionAnomaly> {
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "UPDATE inspection_anomalies SET {set_sql}, resolution_status = 'triaged', \
                 row_version = row_version + 1 WHERE id = ? AND row_version = ?"
            ),
            [follow_up_id.into(), anomaly_id.into(), expected_row_version.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(changed_elsewhere());
    }
    load_anomaly(txn, anomaly_id).await
}

fn assert_routable(a: &InspectionAnomaly) -> AppResult<()> {
    if a.linked_di_id.is_some() || a.linked_work_order_id.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_ANOMALY_ALREADY_ROUTED: this anomaly is already linked to a request or a work order.".into(),
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
    // The request and its link to the anomaly commit together (audit finding AU-71).
    let txn = db.begin().await?;
    let a = load_anomaly(&txn, anomaly_id).await?;
    if a.row_version != expected_row_version {
        return Err(changed_elsewhere());
    }
    assert_routable(&a)?;
    let Some(rid) = a.result_id else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_ANOMALY_NO_RESULT: the anomaly is not attached to a recorded result.".into(),
        ]));
    };
    let res_row = txn
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
    let cp = get_inspection_checkpoint_by_id(&txn, checkpoint_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionCheckpoint".into(),
            id: checkpoint_id.to_string(),
        })?;
    let Some(asset_id) = cp.asset_id else {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_ANOMALY_NO_EQUIPMENT: checkpoint {} has no equipment; a request needs one.",
            cp.checkpoint_code
        )]));
    };
    let node_row = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ?",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Equipment".into(),
            id: asset_id.to_string(),
        })?;
    let org_node_id: Option<i64> = node_row.try_get("", "installed_at_node_id").ok();
    let Some(org_node_id) = org_node_id else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_EQUIPMENT_NO_LOCATION: the equipment has no installation location; a request needs one.".into(),
        ]));
    };

    let title = required_title(title_override)?;
    let description = description_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| a.description.clone());

    let reported_urgency = severity_urgency_code(a.severity);

    let symptom_code_id = crate::di::reference_catalog::resolve_system_di_symptom_id(&txn, &a.anomaly_type).await?;

    let di_in = DiCreateInput {
        equipment_id: asset_id,
        org_node_id,
        title,
        description,
        origin_type: "inspection".into(),
        request_type: "repair".to_string(),
        symptom_code_id: Some(symptom_code_id),
        // A reading tells nothing about production impact: the reviewer assesses it.
        impact_level: "unknown".into(),
        production_impact: false,
        safety_flag: a.severity >= 4,
        environmental_flag: false,
        quality_flag: false,
        reported_urgency: reported_urgency.to_string(),
        observed_at: None,
        submitter_id: submitter_user_id,
        source_inspection_anomaly_id: Some(anomaly_id),
    };

    let di = create_intervention_request_in(&txn, di_in).await?;
    let updated = link_follow_up(
        &txn,
        anomaly_id,
        expected_row_version,
        "linked_di_id = ?, routing_decision = 'di'",
        di.id,
    )
    .await?;
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
    description_override: Option<String>,
) -> AppResult<(crate::wo::domain::WorkOrder, InspectionAnomaly)> {
    // The work order and its link to the anomaly commit together (audit finding AU-71).
    let txn = db.begin().await?;
    let a = load_anomaly(&txn, anomaly_id).await?;
    if a.row_version != expected_row_version {
        return Err(changed_elsewhere());
    }
    assert_routable(&a)?;

    let open = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT wo.id FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
                 WHERE wo.source_inspection_anomaly_id = ? AND {}",
                crate::wo::backlog::NOT_CLOSED_SQL
            ),
            [anomaly_id.into()],
        ))
        .await?;
    if open.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_ANOMALY_OPEN_WO_EXISTS: an open work order already exists for this anomaly.".into(),
        ]));
    }

    let Some(rid) = a.result_id else {
        return Err(AppError::ValidationFailed(vec![
            "GATE_INSP_ANOMALY_NO_RESULT: the anomaly is not attached to a recorded result.".into(),
        ]));
    };
    let res_row = txn
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
    let cp = get_inspection_checkpoint_by_id(&txn, checkpoint_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InspectionCheckpoint".into(),
            id: checkpoint_id.to_string(),
        })?;
    // A work order is always on an equipment (`wo::queries` refuses orphan work orders).
    if cp.asset_id.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "GATE_INSP_ANOMALY_NO_EQUIPMENT: checkpoint {} has no equipment; a work order needs one.",
            cp.checkpoint_code
        )]));
    }
    let equipment_id = cp.asset_id;

    let title = required_title(title_override)?;
    let description = description_override
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| a.description.clone());

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
        urgency_code: Some(severity_urgency_code(a.severity).to_string()),
        subtype_code: None,
        title,
        description: Some(description),
        notes: None,
        planned_start: None,
        planned_end: None,
        shift: None,
        expected_duration_hours: None,
        creator_id: creator_user_id,
        // The anomaly's "revue permis requise"; the plant's permit rules are applied on top of it
        // when the work order is created (`wo::queries::create_work_order_in`, AU-04).
        requires_permit: Some(a.requires_permit_review),
    };

    let wo = create_work_order_in(&txn, wo_in).await?;
    let updated = link_follow_up(
        &txn,
        anomaly_id,
        expected_row_version,
        "linked_work_order_id = ?, routing_decision = 'wo'",
        wo.id,
    )
    .await?;
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
            "GATE_RECORD_CHANGED_ELSEWHERE: row_version mismatch on inspection_anomalies.".into(),
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
    txn.commit().await?;
    Ok(updated)
}

/// Urgency of a request or work order raised from a finding: a level of the shared DI / OT
/// scale (`reference::urgency_scale::severity_urgency`, one severity step per level).
pub(crate) fn severity_urgency_code(severity: i64) -> &'static str {
    crate::reference::urgency_scale::severity_urgency(severity).as_str()
}
