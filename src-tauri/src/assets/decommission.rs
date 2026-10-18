//! Asset decommission orchestration.
//!
//! Production entry point for the Déclasser / Decommission UI. Reuses the
//! governed lifecycle event path — does not write status outside lifecycle.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};

use super::identity::{self, Asset};
use super::lifecycle::{self, RecordLifecycleEventPayload};

const TERMINAL_STATUSES: &[&str] = &["DECOMMISSIONED", "SCRAPPED"];

/// IPC payload for `decommission_asset` (aligned with `shared/ipc-types.ts`).
#[derive(Debug, Clone, Deserialize)]
pub struct DecommissionAssetPayload {
    pub asset_id: i64,
    pub target_status: String,
    pub reason: String,
    pub notes: Option<String>,
}

fn normalize_target_status(raw: &str) -> AppResult<String> {
    let code = identity::normalize_status_code_for_reference(raw)?;
    if !TERMINAL_STATUSES.contains(&code.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Statut cible '{raw}' invalide. Valeurs autorisées: DECOMMISSIONED, SCRAPPED."
        )]));
    }
    Ok(code)
}

fn compose_notes(reason: &str, notes: Option<&str>) -> String {
    match notes.map(str::trim).filter(|n| !n.is_empty()) {
        Some(extra) => format!("{reason}\n\n{extra}"),
        None => reason.to_string(),
    }
}

/// Decommission an asset through the lifecycle SSOT and return the updated asset.
pub async fn decommission_asset(
    db: &DatabaseConnection,
    payload: DecommissionAssetPayload,
    actor_id: i32,
) -> AppResult<Asset> {
    let reason = payload.reason.trim().to_string();
    if reason.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le motif de déclassement est obligatoire.".into(),
        ]));
    }

    let target_status = normalize_target_status(&payload.target_status)?;

    // The checks and the event are one transaction: no request or work order can open, and no
    // other retirement can land, between the check and the change.
    let txn = db.begin().await?;
    let status: String = txn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lifecycle_status FROM equipment WHERE id = ? AND deleted_at IS NULL",
            [payload.asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "equipment".into(),
            id: payload.asset_id.to_string(),
        })?
        .try_get("", "lifecycle_status")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("equipment.lifecycle_status: {e}")))?;
    if TERMINAL_STATUSES.contains(&status.as_str()) || identity::is_retired_status_code(&txn, &status).await? {
        return Err(AppError::ValidationFailed(vec![format!(
            "L'équipement est déjà en statut terminal '{status}'."
        )]));
    }

    let count = |sql: String| {
        let txn = &txn;
        async move {
            let row = txn
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    sql,
                    [payload.asset_id.into()],
                ))
                .await?;
            Ok::<i64, AppError>(row.and_then(|r| r.try_get::<i64>("", "cnt").ok()).unwrap_or(0))
        }
    };
    // Same open rules as the binding summary (`assets::bindings`).
    let open_di = count(format!(
        "SELECT COUNT(*) AS cnt FROM intervention_requests ir WHERE ir.equipment_id = ? AND {}",
        crate::di::stats::OPEN_SQL
    ))
    .await?;
    let open_wo = count(format!(
        "SELECT COUNT(*) AS cnt FROM work_orders wo JOIN work_order_statuses s ON s.id = wo.status_id \
         WHERE wo.equipment_id = ? AND {}",
        crate::wo::backlog::NOT_CLOSED_SQL
    ))
    .await?;
    if open_di > 0 || open_wo > 0 {
        let mut msgs = Vec::new();
        if open_di > 0 {
            msgs.push(format!(
                "Impossible de déclasser: {open_di} demande(s) d'intervention ouverte(s)."
            ));
        }
        if open_wo > 0 {
            msgs.push(format!(
                "Impossible de déclasser: {open_wo} ordre(s) de travail ouvert(s)."
            ));
        }
        return Err(AppError::ValidationFailed(msgs));
    }

    let notes = compose_notes(&reason, payload.notes.as_deref());

    lifecycle::record_lifecycle_event_in(
        &txn,
        RecordLifecycleEventPayload {
            asset_id: payload.asset_id,
            event_type: "DECOMMISSIONED".to_string(),
            event_at: None,
            from_org_node_id: None,
            to_org_node_id: None,
            from_status_code: None,
            to_status_code: Some(target_status),
            from_class_code: None,
            to_class_code: None,
            related_asset_id: None,
            reason_code: None,
            notes: Some(notes),
            approved_by_id: None,
            expected_row_version: None,
        },
        actor_id,
    )
    .await?;
    txn.commit().await?;

    identity::get_asset_by_id(db, payload.asset_id).await
}
