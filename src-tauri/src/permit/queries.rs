use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::sync::domain::{
    LotoCardPrintJobSyncPayload, PermitHandoverLogSyncPayload, PermitIsolationSyncPayload,
    PermitSuspensionSyncPayload, PermitTypeSyncPayload, StageOutboxItemInput, WorkPermitSyncPayload,
    SYNC_ENTITY_LOTO_CARD_PRINT_JOBS, SYNC_ENTITY_PERMIT_HANDOVER_LOGS, SYNC_ENTITY_PERMIT_ISOLATIONS,
    SYNC_ENTITY_PERMIT_SUSPENSIONS, SYNC_ENTITY_PERMIT_TYPES, SYNC_ENTITY_WORK_PERMITS,
};
use crate::sync::queries::stage_outbox_item;

use super::domain::{
    LotoCardPrintInput, LotoCardPrintJob, LotoCardView, PermitComplianceKpi30d, PermitHandoverLog,
    PermitHandoverLogInput, PermitIsolation, PermitIsolationUpsertInput, PermitSuspendInput, PermitSuspension,
    PermitType, PermitTypeUpsertInput, WorkPermit, WorkPermitCreateInput, WorkPermitListFilter,
    WorkPermitStatusInput, WorkPermitUpdateInput,
};

const WP_STATUSES: &[&str] = &[
    "draft",
    "pending_review",
    "approved",
    "issued",
    "active",
    "suspended",
    "revalidation_required",
    "closed",
    "handed_back",
    "cancelled",
    "expired",
];

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode permit field '{field}': {err}"))
}

fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<i64>))
}

fn opt_f64(v: Option<f64>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<f64>))
}

fn opt_string(v: Option<String>) -> sea_orm::Value {
    v.map(sea_orm::Value::from)
        .unwrap_or_else(|| sea_orm::Value::from(None::<String>))
}

fn i64_to_bool(v: i64) -> bool {
    v != 0
}

fn now_ts() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn map_permit_type(row: &sea_orm::QueryResult) -> AppResult<PermitType> {
    Ok(PermitType {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        name: row.try_get("", "name").map_err(|e| decode_err("name", e))?,
        description: row.try_get("", "description").map_err(|e| decode_err("description", e))?,
        requires_hse_approval: i64_to_bool(
            row.try_get::<i64>("", "requires_hse_approval")
                .map_err(|e| decode_err("requires_hse_approval", e))?,
        ),
        requires_operations_approval: i64_to_bool(
            row.try_get::<i64>("", "requires_operations_approval")
                .map_err(|e| decode_err("requires_operations_approval", e))?,
        ),
        requires_atmospheric_test: i64_to_bool(
            row.try_get::<i64>("", "requires_atmospheric_test")
                .map_err(|e| decode_err("requires_atmospheric_test", e))?,
        ),
        max_duration_hours: row.try_get("", "max_duration_hours").ok(),
        mandatory_ppe_ids_json: row
            .try_get("", "mandatory_ppe_ids_json")
            .map_err(|e| decode_err("mandatory_ppe_ids_json", e))?,
        mandatory_control_rules_json: row
            .try_get("", "mandatory_control_rules_json")
            .map_err(|e| decode_err("mandatory_control_rules_json", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_work_permit(row: &sea_orm::QueryResult) -> AppResult<WorkPermit> {
    Ok(WorkPermit {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        linked_work_order_id: row.try_get("", "linked_work_order_id").ok(),
        permit_type_id: row.try_get("", "permit_type_id").map_err(|e| decode_err("permit_type_id", e))?,
        asset_id: row.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?,
        entity_id: row.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        requested_at: row.try_get("", "requested_at").ok(),
        issued_at: row.try_get("", "issued_at").ok(),
        activated_at: row.try_get("", "activated_at").ok(),
        expires_at: row.try_get("", "expires_at").ok(),
        closed_at: row.try_get("", "closed_at").ok(),
        handed_back_at: row.try_get("", "handed_back_at").ok(),
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_permit_isolation(row: &sea_orm::QueryResult) -> AppResult<PermitIsolation> {
    Ok(PermitIsolation {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        isolation_point: row.try_get("", "isolation_point").map_err(|e| decode_err("isolation_point", e))?,
        energy_type: row.try_get("", "energy_type").map_err(|e| decode_err("energy_type", e))?,
        isolation_method: row
            .try_get("", "isolation_method")
            .map_err(|e| decode_err("isolation_method", e))?,
        lock_number: row.try_get("", "lock_number").ok(),
        applied_by_id: row.try_get("", "applied_by_id").ok(),
        verified_by_id: row.try_get("", "verified_by_id").ok(),
        applied_at: row.try_get("", "applied_at").ok(),
        verified_at: row.try_get("", "verified_at").ok(),
        removal_verified_at: row.try_get("", "removal_verified_at").ok(),
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

async fn stage_permit_type(db: &DatabaseConnection, row: &PermitType) -> AppResult<()> {
    let payload = PermitTypeSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        code: row.code.clone(),
        max_duration_hours: row.max_duration_hours,
        mandatory_control_rules_json: row.mandatory_control_rules_json.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("permit_types:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_PERMIT_TYPES.to_string(),
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

async fn stage_work_permit_with_idempotency_key(
    db: &DatabaseConnection,
    row: &WorkPermit,
    idempotency_key: &str,
) -> AppResult<()> {
    let payload = WorkPermitSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        linked_work_order_id: row.linked_work_order_id,
        permit_type_id: row.permit_type_id,
        status: row.status.clone(),
        requested_at: row.requested_at.clone(),
        issued_at: row.issued_at.clone(),
        activated_at: row.activated_at.clone(),
        expires_at: row.expires_at.clone(),
        closed_at: row.closed_at.clone(),
        handed_back_at: row.handed_back_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: idempotency_key.to_string(),
            entity_type: SYNC_ENTITY_WORK_PERMITS.to_string(),
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

async fn stage_work_permit(db: &DatabaseConnection, row: &WorkPermit) -> AppResult<()> {
    stage_work_permit_with_idempotency_key(
        db,
        row,
        &format!("work_permits:{}:v{}", row.entity_sync_id, row.row_version),
    )
    .await
}

pub async fn stage_work_permit_batched(
    db: &DatabaseConnection,
    row: &WorkPermit,
    batch_id: &str,
) -> AppResult<()> {
    let key = format!(
        "{batch_id}:work_permits:{}:v{}",
        row.entity_sync_id, row.row_version
    );
    stage_work_permit_with_idempotency_key(db, row, &key).await
}

pub async fn get_work_permit_linked_to_work_order(
    db: &DatabaseConnection,
    work_order_id: i64,
) -> AppResult<Option<WorkPermit>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, linked_work_order_id, permit_type_id, asset_id, entity_id, \
             status, requested_at, issued_at, activated_at, expires_at, closed_at, handed_back_at, row_version \
             FROM work_permits WHERE linked_work_order_id = ? ORDER BY id DESC LIMIT 1",
            [work_order_id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_work_permit(&r)?)),
        None => Ok(None),
    }
}

async fn stage_permit_isolation(db: &DatabaseConnection, row: &PermitIsolation) -> AppResult<()> {
    let payload = PermitIsolationSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
        permit_id: row.permit_id,
        isolation_point: row.isolation_point.clone(),
        energy_type: row.energy_type.clone(),
        lock_number: row.lock_number.clone(),
        verified_at: row.verified_at.clone(),
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("permit_isolations:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_PERMIT_ISOLATIONS.to_string(),
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

async fn stage_permit_suspension(db: &DatabaseConnection, row: &PermitSuspension) -> AppResult<()> {
    let payload = PermitSuspensionSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        permit_id: row.permit_id,
        reason: row.reason.clone(),
        suspended_by_id: row.suspended_by_id,
        suspended_at: row.suspended_at.clone(),
        reinstated_by_id: row.reinstated_by_id,
        reinstated_at: row.reinstated_at.clone(),
        reactivation_conditions: row.reactivation_conditions.clone(),
        row_version: row.row_version,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("permit_suspensions:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_PERMIT_SUSPENSIONS.to_string(),
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

async fn stage_permit_handover_log(db: &DatabaseConnection, row: &PermitHandoverLog) -> AppResult<()> {
    let payload = PermitHandoverLogSyncPayload {
        id: row.id,
        entity_sync_id: row.entity_sync_id.clone(),
        permit_id: row.permit_id,
        handed_from_role: row.handed_from_role.clone(),
        handed_to_role: row.handed_to_role.clone(),
        confirmation_note: row.confirmation_note.clone(),
        signed_at: row.signed_at.clone(),
        row_version: row.row_version,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("permit_handover_logs:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_PERMIT_HANDOVER_LOGS.to_string(),
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

fn map_permit_suspension(row: &sea_orm::QueryResult) -> AppResult<PermitSuspension> {
    Ok(PermitSuspension {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        reason: row.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
        suspended_by_id: row.try_get("", "suspended_by_id").map_err(|e| decode_err("suspended_by_id", e))?,
        suspended_at: row.try_get("", "suspended_at").map_err(|e| decode_err("suspended_at", e))?,
        reinstated_by_id: row.try_get("", "reinstated_by_id").ok(),
        reinstated_at: row.try_get("", "reinstated_at").ok(),
        reactivation_conditions: row
            .try_get("", "reactivation_conditions")
            .map_err(|e| decode_err("reactivation_conditions", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn map_permit_handover_log(row: &sea_orm::QueryResult) -> AppResult<PermitHandoverLog> {
    Ok(PermitHandoverLog {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        handed_from_role: row.try_get("", "handed_from_role").map_err(|e| decode_err("handed_from_role", e))?,
        handed_to_role: row.try_get("", "handed_to_role").map_err(|e| decode_err("handed_to_role", e))?,
        confirmation_note: row
            .try_get("", "confirmation_note")
            .map_err(|e| decode_err("confirmation_note", e))?,
        signed_at: row.try_get("", "signed_at").map_err(|e| decode_err("signed_at", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

fn validate_wp_status(s: &str) -> AppResult<()> {
    if WP_STATUSES.contains(&s) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!("Invalid work permit status '{s}'.")]))
    }
}

async fn ensure_wo_asset_aligned(
    db: &DatabaseConnection,
    linked_work_order_id: Option<i64>,
    asset_id: i64,
) -> AppResult<()> {
    let Some(wo_id) = linked_work_order_id else {
        return Ok(());
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT equipment_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let equipment_id: Option<i64> = row.try_get("", "equipment_id").map_err(|e| decode_err("equipment_id", e))?;
    let Some(eq) = equipment_id else {
        return Err(AppError::ValidationFailed(vec![
            "Work order has no equipment_id; cannot align permit asset.".into(),
        ]));
    };
    if eq != asset_id {
        return Err(AppError::ValidationFailed(vec![format!(
            "linked_work_order_id equipment_id ({eq}) must match permit asset_id ({asset_id})."
        )]));
    }
    Ok(())
}

async fn permit_type_code(db: &DatabaseConnection, permit_type_id: i64) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code FROM permit_types WHERE id = ?",
            [permit_type_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitType".into(),
            id: permit_type_id.to_string(),
        })?;
    row.try_get("", "code").map_err(|e| decode_err("code", e))
}

async fn count_isolations(db: &DatabaseConnection, permit_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM permit_isolations WHERE permit_id = ?",
            [permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to count isolations.".into()))?;
    row.try_get("", "c").map_err(|e| decode_err("c", e))
}

async fn count_verified_isolations(db: &DatabaseConnection, permit_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM permit_isolations WHERE permit_id = ? AND verified_at IS NOT NULL",
            [permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to count verified isolations.".into()))?;
    row.try_get("", "c").map_err(|e| decode_err("c", e))
}

async fn count_handover_logs(db: &DatabaseConnection, permit_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM permit_handover_logs WHERE permit_id = ?",
            [permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to count handover logs.".into()))?;
    row.try_get("", "c").map_err(|e| decode_err("c", e))
}

fn draft_fields_complete(wp: &WorkPermit) -> bool {
    !wp.code.trim().is_empty() && wp.asset_id > 0
}

pub(crate) async fn activation_ready(
    db: &DatabaseConnection,
    permit_id: i64,
    permit_type_id: i64,
) -> AppResult<()> {
    let code = permit_type_code(db, permit_type_id).await?;
    let n = count_isolations(db, permit_id).await?;
    if code == "loto" && n < 1 {
        return Err(AppError::ValidationFailed(vec![
            "LOTO permit types require at least one isolation before activation.".into(),
        ]));
    }
    if n > 0 {
        let v = count_verified_isolations(db, permit_id).await?;
        if v != n {
            return Err(AppError::ValidationFailed(vec![
                "All isolation points must have verified_at set before activation.".into(),
            ]));
        }
    }
    Ok(())
}

fn assert_transition_set_status(from: &str, to: &str) -> AppResult<()> {
    if from == to {
        return Err(AppError::ValidationFailed(vec!["Status is already applied.".into()]));
    }
    if from == "handed_back" && to != "cancelled" {
        return Err(AppError::ValidationFailed(vec![
            "Only transition from handed_back is to cancelled.".into(),
        ]));
    }
    if to == "cancelled" {
        if from == "handed_back" {
            return Ok(());
        }
        if matches!(
            from,
            "closed" | "handed_back" | "cancelled" | "expired"
        ) {
            return Err(AppError::ValidationFailed(vec![
                "Cancellation is not allowed from this status.".into(),
            ]));
        }
        return Ok(());
    }
    let ok = matches!(
        (from, to),
        ("draft", "pending_review")
            | ("pending_review", "approved")
            | ("approved", "issued")
            | ("issued", "active")
            | ("suspended", "revalidation_required")
            | ("revalidation_required", "active")
            | ("active", "closed")
            | ("closed", "handed_back")
    );
    if ok {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Transition {from} -> {to} is not allowed for set_work_permit_status (use suspend_work_permit for active -> suspended)."
        )]))
    }
}

async fn finalize_open_suspension(
    db: &DatabaseConnection,
    permit_id: i64,
    reinstated_by_id: i64,
    require_row: bool,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, row_version FROM permit_suspensions \
             WHERE permit_id = ? AND reinstated_at IS NULL \
             ORDER BY id DESC LIMIT 1",
            [permit_id.into()],
        ))
        .await?;
    let Some(r) = row else {
        if require_row {
            return Err(AppError::ValidationFailed(vec![
                "No open permit_suspensions row to reinstate.".into(),
            ]));
        }
        return Ok(());
    };
    let sid: i64 = r.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let rv: i64 = r.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?;
    let new_rv = rv + 1;
    let ts = now_ts();
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_suspensions SET reinstated_by_id = ?, reinstated_at = ?, row_version = ? \
             WHERE id = ? AND row_version = ?",
            [
                reinstated_by_id.into(),
                ts.into(),
                new_rv.into(),
                sid.into(),
                rv.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec!["Concurrent update on permit_suspensions.".into()]));
    }
    let updated = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, permit_id, reason, suspended_by_id, suspended_at, \
             reinstated_by_id, reinstated_at, reactivation_conditions, row_version \
             FROM permit_suspensions WHERE id = ?",
            [sid.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitSuspension".into(),
            id: sid.to_string(),
        })?;
    let susp = map_permit_suspension(&updated)?;
    stage_permit_suspension(db, &susp).await?;
    Ok(())
}

pub async fn list_permit_types(db: &DatabaseConnection) -> AppResult<Vec<PermitType>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, name, description, requires_hse_approval, \
             requires_operations_approval, requires_atmospheric_test, max_duration_hours, \
             mandatory_ppe_ids_json, mandatory_control_rules_json, row_version \
             FROM permit_types ORDER BY code ASC"
                .to_string(),
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_permit_type(&row)?);
    }
    Ok(out)
}

pub async fn get_permit_type(db: &DatabaseConnection, id: i64) -> AppResult<Option<PermitType>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, name, description, requires_hse_approval, \
             requires_operations_approval, requires_atmospheric_test, max_duration_hours, \
             mandatory_ppe_ids_json, mandatory_control_rules_json, row_version \
             FROM permit_types WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.as_ref().map(map_permit_type).transpose()
}

pub async fn upsert_permit_type(db: &DatabaseConnection, input: PermitTypeUpsertInput) -> AppResult<PermitType> {
    let ppe = input.mandatory_ppe_ids_json.unwrap_or_else(|| "[]".to_string());
    let rules = input.mandatory_control_rules_json.unwrap_or_else(|| "{}".to_string());
    let desc = input.description.unwrap_or_default();

    if input.code.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["code is required.".into()]));
    }

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let current = get_permit_type(db, id)
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "PermitType".into(),
                id: id.to_string(),
            })?;
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE permit_types SET code = ?, name = ?, description = ?, \
                 requires_hse_approval = ?, requires_operations_approval = ?, requires_atmospheric_test = ?, \
                 max_duration_hours = ?, mandatory_ppe_ids_json = ?, mandatory_control_rules_json = ?, \
                 row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.code.into(),
                    input.name.into(),
                    desc.into(),
                    i64::from(input.requires_hse_approval).into(),
                    i64::from(input.requires_operations_approval).into(),
                    i64::from(input.requires_atmospheric_test).into(),
                    opt_f64(input.max_duration_hours),
                    ppe.into(),
                    rules.into(),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec!["Concurrent update on permit_types.".into()]));
        }
        let updated = get_permit_type(db, id).await?.expect("row");
        stage_permit_type(db, &updated).await?;
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_types (entity_sync_id, code, name, description, \
         requires_hse_approval, requires_operations_approval, requires_atmospheric_test, \
         max_duration_hours, mandatory_ppe_ids_json, mandatory_control_rules_json, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.code.into(),
            input.name.into(),
            desc.into(),
            i64::from(input.requires_hse_approval).into(),
            i64::from(input.requires_operations_approval).into(),
            i64::from(input.requires_atmospheric_test).into(),
            opt_f64(input.max_duration_hours),
            ppe.into(),
            rules.into(),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let created = get_permit_type(db, new_id).await?.expect("row");
    stage_permit_type(db, &created).await?;
    Ok(created)
}

pub async fn list_work_permits(
    db: &DatabaseConnection,
    filter: WorkPermitListFilter,
) -> AppResult<Vec<WorkPermit>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 2000);
    let mut sql = String::from(
        "SELECT id, entity_sync_id, code, linked_work_order_id, permit_type_id, asset_id, entity_id, \
         status, requested_at, issued_at, activated_at, expires_at, closed_at, handed_back_at, row_version \
         FROM work_permits WHERE 1=1",
    );
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(ref s) = filter.status {
        sql.push_str(" AND status = ?");
        params.push(s.clone().into());
    }
    if let Some(t) = filter.permit_type_id {
        sql.push_str(" AND permit_type_id = ?");
        params.push(t.into());
    }
    if let Some(a) = filter.asset_id {
        sql.push_str(" AND asset_id = ?");
        params.push(a.into());
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    params.push(limit.into());

    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params);
    let rows = db.query_all(stmt).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_work_permit(&row)?);
    }
    Ok(out)
}

pub async fn get_work_permit(db: &DatabaseConnection, id: i64) -> AppResult<Option<WorkPermit>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, linked_work_order_id, permit_type_id, asset_id, entity_id, \
             status, requested_at, issued_at, activated_at, expires_at, closed_at, handed_back_at, row_version \
             FROM work_permits WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.as_ref().map(map_work_permit).transpose()
}

pub async fn create_work_permit(db: &DatabaseConnection, input: WorkPermitCreateInput) -> AppResult<WorkPermit> {
    let _ = permit_type_code(db, input.permit_type_id).await?;
    ensure_wo_asset_aligned(db, input.linked_work_order_id, input.asset_id).await?;

    let sync_id = Uuid::new_v4().to_string();
    let requested = now_ts();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_permits (entity_sync_id, code, linked_work_order_id, permit_type_id, \
         asset_id, entity_id, status, requested_at, expires_at, row_version) \
         VALUES (?, '', ?, ?, ?, ?, 'draft', ?, ?, 1)",
        [
            sync_id.into(),
            opt_i64(input.linked_work_order_id),
            input.permit_type_id.into(),
            input.asset_id.into(),
            input.entity_id.into(),
            requested.into(),
            opt_string(input.expires_at),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let code = format!("PTW-{}-{:06}", Utc::now().format("%Y"), new_id);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET code = ? WHERE id = ?",
        [code.into(), new_id.into()],
    ))
    .await?;
    let created = get_work_permit(db, new_id).await?.expect("row");
    stage_work_permit(db, &created).await?;
    Ok(created)
}

pub async fn update_work_permit(db: &DatabaseConnection, input: WorkPermitUpdateInput) -> AppResult<WorkPermit> {
    let current = get_work_permit(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: input.id.to_string(),
        })?;
    if input.expected_row_version != current.row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on work_permits.".into()]));
    }
    let asset_id = input.asset_id.unwrap_or(current.asset_id);
    let entity_id = input.entity_id.unwrap_or(current.entity_id);
    let linked = input.linked_work_order_id.or(current.linked_work_order_id);
    ensure_wo_asset_aligned(db, linked, asset_id).await?;

    let new_rv = current.row_version + 1;
    let expires = input.expires_at.or(current.expires_at);
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_permits SET linked_work_order_id = ?, asset_id = ?, entity_id = ?, \
             expires_at = ?, row_version = ? \
             WHERE id = ? AND row_version = ?",
            [
                opt_i64(linked),
                asset_id.into(),
                entity_id.into(),
                opt_string(expires),
                new_rv.into(),
                input.id.into(),
                current.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec!["Concurrent update on work_permits.".into()]));
    }
    let updated = get_work_permit(db, input.id).await?.expect("row");
    stage_work_permit(db, &updated).await?;
    Ok(updated)
}

pub async fn set_work_permit_status(db: &DatabaseConnection, input: WorkPermitStatusInput) -> AppResult<WorkPermit> {
    validate_wp_status(&input.status)?;
    let current = get_work_permit(db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: input.id.to_string(),
        })?;
    if input.expected_row_version != current.row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on work_permits.".into()]));
    }

    let from = current.status.as_str();
    let to = input.status.as_str();
    assert_transition_set_status(from, to)?;

    match (from, to) {
        ("draft", "pending_review") => {
            if !draft_fields_complete(&current) {
                return Err(AppError::ValidationFailed(vec![
                    "Draft permit must have a code and asset before review.".into(),
                ]));
            }
        }
        ("issued", "active") => {
            activation_ready(db, input.id, current.permit_type_id).await?;
        }
        ("revalidation_required", "active") => {
            activation_ready(db, input.id, current.permit_type_id).await?;
            let rid = input.reinstated_by_id.ok_or_else(|| {
                AppError::ValidationFailed(vec!["reinstated_by_id is required when returning to active.".into()])
            })?;
            finalize_open_suspension(db, input.id, rid, true).await?;
        }
        ("closed", "handed_back") => {
            if count_handover_logs(db, input.id).await? < 1 {
                return Err(AppError::ValidationFailed(vec![
                    "At least one permit_handover_logs row is required before handed_back.".into(),
                ]));
            }
        }
        _ => {}
    }

    let new_rv = current.row_version + 1;
    let now = now_ts();
    let mut issued_at = input.issued_at.or(current.issued_at.clone());
    let mut activated_at = input.activated_at.or(current.activated_at.clone());
    let mut closed_at = input.closed_at.or(current.closed_at.clone());
    let mut handed_back_at = input.handed_back_at.or(current.handed_back_at.clone());
    if to == "issued" && issued_at.is_none() {
        issued_at = Some(now.clone());
    }
    if to == "active" && activated_at.is_none() {
        activated_at = Some(now.clone());
    }
    if to == "closed" && closed_at.is_none() {
        closed_at = Some(now.clone());
    }
    if to == "handed_back" && handed_back_at.is_none() {
        handed_back_at = Some(now.clone());
    }

    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_permits SET status = ?, issued_at = ?, activated_at = ?, closed_at = ?, \
             handed_back_at = ?, row_version = ? \
             WHERE id = ? AND row_version = ?",
            [
                input.status.into(),
                opt_string(issued_at),
                opt_string(activated_at),
                opt_string(closed_at),
                opt_string(handed_back_at),
                new_rv.into(),
                input.id.into(),
                current.row_version.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec!["Concurrent update on work_permits.".into()]));
    }
    let updated = get_work_permit(db, input.id).await?.expect("row");
    stage_work_permit(db, &updated).await?;
    Ok(updated)
}

pub async fn suspend_work_permit(db: &DatabaseConnection, input: PermitSuspendInput) -> AppResult<(WorkPermit, PermitSuspension)> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["Suspension reason is required.".into()]));
    }
    let current = get_work_permit(db, input.permit_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: input.permit_id.to_string(),
        })?;
    if current.status != "active" {
        return Err(AppError::ValidationFailed(vec![
            "Only active permits can be suspended.".into(),
        ]));
    }
    if input.expected_row_version != current.row_version {
        return Err(AppError::ValidationFailed(vec!["row_version mismatch on work_permits.".into()]));
    }

    let sync_id = Uuid::new_v4().to_string();
    let suspended_at = now_ts();
    let react = input.reactivation_conditions.unwrap_or_default();

    let txn = db.begin().await?;
    txn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_suspensions (entity_sync_id, permit_id, reason, suspended_by_id, suspended_at, \
         reactivation_conditions, row_version) VALUES (?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            input.reason.into(),
            input.suspended_by_id.into(),
            suspended_at.clone().into(),
            react.into(),
        ],
    ))
    .await?;
    let id_row = txn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let sid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let new_wp_rv = current.row_version + 1;
    let aff = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_permits SET status = 'suspended', row_version = ? WHERE id = ? AND row_version = ?",
            [new_wp_rv.into(), input.permit_id.into(), current.row_version.into()],
        ))
        .await?;
    if aff.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec!["Concurrent update on work_permits.".into()]));
    }
    txn.commit().await?;

    let susp_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, permit_id, reason, suspended_by_id, suspended_at, \
             reinstated_by_id, reinstated_at, reactivation_conditions, row_version \
             FROM permit_suspensions WHERE id = ?",
            [sid.into()],
        ))
        .await?
        .expect("row");
    let suspension = map_permit_suspension(&susp_row)?;
    let wp = get_work_permit(db, input.permit_id).await?.expect("row");
    stage_permit_suspension(db, &suspension).await?;
    stage_work_permit(db, &wp).await?;
    Ok((wp, suspension))
}

pub async fn append_permit_handover_log(
    db: &DatabaseConnection,
    input: PermitHandoverLogInput,
) -> AppResult<PermitHandoverLog> {
    let current = get_work_permit(db, input.permit_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: input.permit_id.to_string(),
        })?;
    if current.status != "closed" {
        return Err(AppError::ValidationFailed(vec![
            "Handover logs can only be recorded when permit status is closed.".into(),
        ]));
    }
    if input.handed_from_role.trim().is_empty() || input.handed_to_role.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["Roles are required.".into()]));
    }
    let signed = input.signed_at.unwrap_or_else(now_ts);
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
         confirmation_note, signed_at, row_version) VALUES (?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            input.handed_from_role.into(),
            input.handed_to_role.into(),
            input.confirmation_note.into(),
            signed.into(),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, permit_id, handed_from_role, handed_to_role, confirmation_note, \
             signed_at, row_version FROM permit_handover_logs WHERE id = ?",
            [new_id.into()],
        ))
        .await?
        .expect("row");
    let log = map_permit_handover_log(&row)?;
    stage_permit_handover_log(db, &log).await?;
    Ok(log)
}

pub async fn list_permit_suspensions(db: &DatabaseConnection, permit_id: i64) -> AppResult<Vec<PermitSuspension>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, permit_id, reason, suspended_by_id, suspended_at, \
             reinstated_by_id, reinstated_at, reactivation_conditions, row_version \
             FROM permit_suspensions WHERE permit_id = ? ORDER BY id ASC",
            [permit_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_permit_suspension(&row)?);
    }
    Ok(out)
}

pub async fn list_permit_handover_logs(db: &DatabaseConnection, permit_id: i64) -> AppResult<Vec<PermitHandoverLog>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, permit_id, handed_from_role, handed_to_role, confirmation_note, \
             signed_at, row_version FROM permit_handover_logs WHERE permit_id = ? ORDER BY id ASC",
            [permit_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_permit_handover_log(&row)?);
    }
    Ok(out)
}

pub async fn list_permit_isolations(db: &DatabaseConnection, permit_id: i64) -> AppResult<Vec<PermitIsolation>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, permit_id, isolation_point, energy_type, isolation_method, lock_number, \
             applied_by_id, verified_by_id, applied_at, verified_at, removal_verified_at, row_version \
             FROM permit_isolations WHERE permit_id = ? ORDER BY id ASC",
            [permit_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_permit_isolation(&row)?);
    }
    Ok(out)
}

pub async fn upsert_permit_isolation(
    db: &DatabaseConnection,
    input: PermitIsolationUpsertInput,
) -> AppResult<PermitIsolation> {
    let _ = get_work_permit(db, input.permit_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: input.permit_id.to_string(),
        })?;

    if input.isolation_point.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["isolation_point is required.".into()]));
    }

    if let Some(id) = input.id {
        if id <= 0 {
            return Err(AppError::ValidationFailed(vec!["id must be positive.".into()]));
        }
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, entity_sync_id, permit_id, isolation_point, energy_type, isolation_method, lock_number, \
                 applied_by_id, verified_by_id, applied_at, verified_at, removal_verified_at, row_version \
                 FROM permit_isolations WHERE id = ? AND permit_id = ?",
                [id.into(), input.permit_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "PermitIsolation".into(),
                id: id.to_string(),
            })?;
        let current = map_permit_isolation(&row)?;
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE permit_isolations SET isolation_point = ?, energy_type = ?, isolation_method = ?, lock_number = ?, \
                 applied_by_id = ?, verified_by_id = ?, applied_at = ?, verified_at = ?, removal_verified_at = ?, \
                 row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    input.isolation_point.into(),
                    input.energy_type.into(),
                    input.isolation_method.into(),
                    opt_string(input.lock_number),
                    opt_i64(input.applied_by_id),
                    opt_i64(input.verified_by_id),
                    opt_string(input.applied_at),
                    opt_string(input.verified_at),
                    opt_string(input.removal_verified_at),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec!["Concurrent update on permit_isolations.".into()]));
        }
        let updated = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, entity_sync_id, permit_id, isolation_point, energy_type, isolation_method, lock_number, \
                 applied_by_id, verified_by_id, applied_at, verified_at, removal_verified_at, row_version \
                 FROM permit_isolations WHERE id = ?",
                [id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "PermitIsolation".into(),
                id: id.to_string(),
            })?;
        let out = map_permit_isolation(&updated)?;
        stage_permit_isolation(db, &out).await?;
        return Ok(out);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_isolations (entity_sync_id, permit_id, isolation_point, energy_type, \
         isolation_method, lock_number, applied_by_id, verified_by_id, applied_at, verified_at, removal_verified_at, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            input.isolation_point.into(),
            input.energy_type.into(),
            input.isolation_method.into(),
            opt_string(input.lock_number),
            opt_i64(input.applied_by_id),
            opt_i64(input.verified_by_id),
            opt_string(input.applied_at),
            opt_string(input.verified_at),
            opt_string(input.removal_verified_at),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, permit_id, isolation_point, energy_type, isolation_method, lock_number, \
             applied_by_id, verified_by_id, applied_at, verified_at, removal_verified_at, row_version \
             FROM permit_isolations WHERE id = ?",
            [new_id.into()],
        ))
        .await?
        .expect("row");
    let created = map_permit_isolation(&row)?;
    stage_permit_isolation(db, &created).await?;
    Ok(created)
}

async fn stage_loto_card_print_job(db: &DatabaseConnection, row: &LotoCardPrintJob) -> AppResult<()> {
    let payload = LotoCardPrintJobSyncPayload {
        id: row.id,
        permit_id: row.permit_id,
        isolation_id: row.isolation_id,
        printed_at: row.printed_at.clone(),
        printed_by_id: row.printed_by_id,
        entity_sync_id: row.entity_sync_id.clone(),
        row_version: row.row_version,
    };
    let payload_json = serde_json::to_string(&payload)?;
    stage_outbox_item(
        db,
        StageOutboxItemInput {
            idempotency_key: format!("loto_card_print_jobs:{}:v{}", row.entity_sync_id, row.row_version),
            entity_type: SYNC_ENTITY_LOTO_CARD_PRINT_JOBS.to_string(),
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

fn map_loto_card_print_job(row: &sea_orm::QueryResult) -> AppResult<LotoCardPrintJob> {
    Ok(LotoCardPrintJob {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        isolation_id: row.try_get("", "isolation_id").map_err(|e| decode_err("isolation_id", e))?,
        printed_at: row.try_get("", "printed_at").map_err(|e| decode_err("printed_at", e))?,
        printed_by_id: row.try_get("", "printed_by_id").map_err(|e| decode_err("printed_by_id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        row_version: row.try_get("", "row_version").map_err(|e| decode_err("row_version", e))?,
    })
}

pub async fn get_loto_card_view(
    db: &DatabaseConnection,
    permit_id: i64,
    isolation_id: i64,
) -> AppResult<LotoCardView> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wp.code AS permit_code, wp.expires_at AS permit_expires, pi.id AS isolation_id, \
             pi.isolation_point, pi.energy_type, pi.lock_number, pi.verified_by_id, pi.verified_at, \
             e.asset_id_code, e.name AS equipment_name, ua.username AS verifier_username \
             FROM permit_isolations pi \
             JOIN work_permits wp ON wp.id = pi.permit_id \
             LEFT JOIN equipment e ON e.id = wp.asset_id \
             LEFT JOIN user_accounts ua ON ua.id = pi.verified_by_id \
             WHERE pi.id = ? AND pi.permit_id = ?",
            [isolation_id.into(), permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitIsolation".into(),
            id: isolation_id.to_string(),
        })?;
    let permit_code: String = row.try_get("", "permit_code").map_err(|e| decode_err("permit_code", e))?;
    let asset_code: Option<String> = row.try_get("", "asset_id_code").ok();
    let eq_name: Option<String> = row.try_get("", "equipment_name").ok();
    let equipment_label = match (asset_code, eq_name) {
        (Some(c), Some(n)) => format!("{c} — {n}"),
        (Some(c), None) => c,
        (None, Some(n)) => n,
        (None, None) => "—".to_string(),
    };
    let isolation_point: String = row.try_get("", "isolation_point").map_err(|e| decode_err("isolation_point", e))?;
    let energy_type: String = row.try_get("", "energy_type").map_err(|e| decode_err("energy_type", e))?;
    let lock_number: Option<String> = row.try_get("", "lock_number").ok();
    let expires_at: Option<String> = row.try_get("", "permit_expires").ok();
    let verified_at: Option<String> = row.try_get("", "verified_at").ok().flatten();
    let verifier_username: Option<String> = row.try_get("", "verifier_username").ok().flatten();
    let verifier_signature = if verified_at.is_some() {
        verifier_username
    } else {
        None
    };
    let iso_id: i64 = row.try_get("", "isolation_id").map_err(|e| decode_err("isolation_id", e))?;
    Ok(LotoCardView {
        permit_code,
        equipment_label,
        energy_type,
        isolation_id: iso_id,
        isolation_point,
        lock_number,
        verifier_signature,
        expires_at,
    })
}

pub async fn record_loto_card_print(
    db: &DatabaseConnection,
    input: LotoCardPrintInput,
) -> AppResult<LotoCardPrintJob> {
    let iso = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT verified_at FROM permit_isolations WHERE id = ? AND permit_id = ?",
            [input.isolation_id.into(), input.permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitIsolation".into(),
            id: input.isolation_id.to_string(),
        })?;
    let verified_at: Option<String> = iso.try_get("", "verified_at").ok().flatten();
    if verified_at.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Isolation must be verified before LOTO card print.".into(),
        ]));
    }
    let sync_id = Uuid::new_v4().to_string();
    let printed_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO loto_card_print_jobs (permit_id, isolation_id, printed_at, printed_by_id, entity_sync_id, \
         row_version) VALUES (?, ?, ?, ?, ?, 1)",
        [
            input.permit_id.into(),
            input.isolation_id.into(),
            printed_at.clone().into(),
            input.printed_by_id.into(),
            sync_id.clone().into(),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("last_insert_rowid")))?;
    let new_id: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, permit_id, isolation_id, printed_at, printed_by_id, entity_sync_id, row_version \
             FROM loto_card_print_jobs WHERE id = ?",
            [new_id.into()],
        ))
        .await?
        .expect("row");
    let job = map_loto_card_print_job(&row)?;
    stage_loto_card_print_job(db, &job).await?;
    Ok(job)
}

pub async fn list_open_permits_report(db: &DatabaseConnection) -> AppResult<Vec<WorkPermit>> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_sync_id, code, linked_work_order_id, permit_type_id, asset_id, entity_id, \
             status, requested_at, issued_at, activated_at, expires_at, closed_at, handed_back_at, row_version \
             FROM work_permits \
             WHERE status IN ('issued', 'active', 'suspended') \
             AND (expires_at IS NULL OR expires_at > ?) \
             ORDER BY id DESC",
            [now.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_work_permit(&row)?);
    }
    Ok(out)
}

pub async fn permit_compliance_kpi_30d(db: &DatabaseConnection) -> AppResult<PermitComplianceKpi30d> {
    let cutoff = (Utc::now() - Duration::days(30))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let activated_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_permits WHERE activated_at IS NOT NULL AND activated_at >= ?",
            [cutoff.clone().into()],
        ))
        .await?
        .expect("c");
    let activated_count: i64 = activated_row.try_get("", "c").map_err(|e| decode_err("c", e))?;
    let on_time_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS c FROM work_permits WHERE activated_at IS NOT NULL AND activated_at >= ? \
             AND handed_back_at IS NOT NULL \
             AND (expires_at IS NULL OR handed_back_at <= expires_at)",
            [cutoff.into()],
        ))
        .await?
        .expect("c");
    let handed_back_on_time_count: i64 = on_time_row.try_get("", "c").map_err(|e| decode_err("c", e))?;
    let rate = if activated_count > 0 {
        Some(handed_back_on_time_count as f64 / activated_count as f64)
    } else {
        None
    };
    Ok(PermitComplianceKpi30d {
        activated_count,
        handed_back_on_time_count,
        rate,
    })
}
