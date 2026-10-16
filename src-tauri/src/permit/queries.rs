use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

use super::domain::{
    LotoCardPrintInput, LotoCardPrintJob, LotoCardView, PermitComplianceKpi30d, PermitHandoverLog,
    PermitHandoverLogInput, PermitIsolation, PermitIsolationActionInput, PermitIsolationUpsertInput,
    PermitSuspendInput, PermitSuspension, PermitType, PermitTypeUpsertInput, WorkPermit, WorkPermitCreateInput,
    WorkPermitListFilter, WorkPermitStatusInput, WorkPermitUpdateInput,
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

/// Statuses after which a permit no longer protects work (no further lifecycle edits).
const WP_TERMINAL_STATUSES: &[&str] = &["closed", "handed_back", "cancelled", "expired"];

/// Statuses in which isolation points may be defined, applied and verified. An `active`
/// permit must be suspended first so new energy sources are never added under live work.
const ISOLATION_SETUP_STATUSES: &[&str] = &[
    "draft",
    "pending_review",
    "approved",
    "issued",
    "suspended",
    "revalidation_required",
];

/// Statuses in which lock removal may be verified: at the end of work (`active`, before
/// close) or when a permit was abandoned with locks still fitted.
const ISOLATION_REMOVAL_STATUSES: &[&str] = &["active", "cancelled", "expired"];

/// Header fields (asset, WO link, expiry) are frozen once the permit is active.
const WP_EDITABLE_STATUSES: &[&str] = &["draft", "pending_review", "approved", "issued"];

const WORK_PERMIT_SELECT: &str = "SELECT wp.id, wp.entity_sync_id, wp.code, wp.linked_work_order_id, \
     wp.permit_type_id, wp.asset_id, wp.entity_id, wp.status, wp.requested_at, wp.issued_at, \
     wp.activated_at, wp.expires_at, wp.closed_at, wp.handed_back_at, wp.row_version, \
     pt.code AS permit_type_code, pt.name AS permit_type_name, \
     e.asset_id_code AS asset_code, e.name AS asset_label, wo.code AS linked_work_order_code \
     FROM work_permits wp \
     LEFT JOIN permit_types pt ON pt.id = wp.permit_type_id \
     LEFT JOIN equipment e ON e.id = wp.asset_id \
     LEFT JOIN work_orders wo ON wo.id = wp.linked_work_order_id";

const PERMIT_ISOLATION_SELECT: &str = "SELECT pi.id, pi.entity_sync_id, pi.permit_id, pi.isolation_point, \
     pi.energy_type, pi.isolation_method, pi.lock_number, pi.applied_by_id, pi.verified_by_id, \
     pi.applied_at, pi.verified_at, pi.removal_verified_at, pi.removal_verified_by_id, pi.row_version, \
     COALESCE(ua.display_name, ua.username) AS applied_by_name, \
     COALESCE(uv.display_name, uv.username) AS verified_by_name, \
     COALESCE(ur.display_name, ur.username) AS removal_verified_by_name \
     FROM permit_isolations pi \
     LEFT JOIN user_accounts ua ON ua.id = pi.applied_by_id \
     LEFT JOIN user_accounts uv ON uv.id = pi.verified_by_id \
     LEFT JOIN user_accounts ur ON ur.id = pi.removal_verified_by_id";

const PERMIT_SUSPENSION_SELECT: &str = "SELECT ps.id, ps.entity_sync_id, ps.permit_id, ps.reason, \
     ps.suspended_by_id, ps.suspended_at, ps.reinstated_by_id, ps.reinstated_at, \
     ps.reactivation_conditions, ps.row_version, \
     COALESCE(us.display_name, us.username) AS suspended_by_name, \
     COALESCE(ur.display_name, ur.username) AS reinstated_by_name \
     FROM permit_suspensions ps \
     LEFT JOIN user_accounts us ON us.id = ps.suspended_by_id \
     LEFT JOIN user_accounts ur ON ur.id = ps.reinstated_by_id";

fn decode_err(field: &str, err: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("Failed to decode permit field '{field}': {err}"))
}

fn opt_i64(v: Option<i64>) -> sea_orm::Value {
    v.map_or_else(|| sea_orm::Value::from(None::<i64>), sea_orm::Value::from)
}

fn opt_f64(v: Option<f64>) -> sea_orm::Value {
    v.map_or_else(|| sea_orm::Value::from(None::<f64>), sea_orm::Value::from)
}

fn opt_string(v: Option<String>) -> sea_orm::Value {
    v.map_or_else(|| sea_orm::Value::from(None::<String>), sea_orm::Value::from)
}

const fn i64_to_bool(v: i64) -> bool {
    v != 0
}

fn now_ts() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Parses an RFC 3339 timestamp and returns it in the canonical stored form
/// (`%Y-%m-%dT%H:%M:%SZ`, UTC) so lexical comparisons against `now_ts()` are valid.
fn normalize_ts(field: &str, value: &str) -> AppResult<String> {
    chrono::DateTime::parse_from_rfc3339(value.trim())
        .map(|dt| dt.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .map_err(|_| AppError::ValidationFailed(vec![format!("{field} must be an RFC 3339 timestamp.")]))
}

/// Normalizes an optional expiry and rejects expiries that are already past.
fn normalize_future_expiry(value: Option<String>) -> AppResult<Option<String>> {
    let Some(raw) = value.filter(|v| !v.trim().is_empty()) else {
        return Ok(None);
    };
    let ts = normalize_ts("expires_at", &raw)?;
    if ts <= now_ts() {
        return Err(AppError::ValidationFailed(vec![
            "expires_at must be in the future.".into()
        ]));
    }
    Ok(Some(ts))
}

fn is_expired(wp: &WorkPermit) -> bool {
    wp.expires_at.as_deref().is_some_and(|e| e <= now_ts().as_str())
}

fn map_permit_type(row: &sea_orm::QueryResult) -> AppResult<PermitType> {
    Ok(PermitType {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        code: row.try_get("", "code").map_err(|e| decode_err("code", e))?,
        name: row.try_get("", "name").map_err(|e| decode_err("name", e))?,
        description: row
            .try_get("", "description")
            .map_err(|e| decode_err("description", e))?,
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
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
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
        permit_type_id: row
            .try_get("", "permit_type_id")
            .map_err(|e| decode_err("permit_type_id", e))?,
        asset_id: row.try_get("", "asset_id").map_err(|e| decode_err("asset_id", e))?,
        entity_id: row.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?,
        status: row.try_get("", "status").map_err(|e| decode_err("status", e))?,
        requested_at: row.try_get("", "requested_at").ok(),
        issued_at: row.try_get("", "issued_at").ok(),
        activated_at: row.try_get("", "activated_at").ok(),
        expires_at: row.try_get("", "expires_at").ok(),
        closed_at: row.try_get("", "closed_at").ok(),
        handed_back_at: row.try_get("", "handed_back_at").ok(),
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        permit_type_code: row.try_get("", "permit_type_code").ok(),
        permit_type_name: row.try_get("", "permit_type_name").ok(),
        asset_code: row.try_get("", "asset_code").ok(),
        asset_label: row.try_get("", "asset_label").ok(),
        linked_work_order_code: row.try_get("", "linked_work_order_code").ok(),
    })
}

fn map_permit_isolation(row: &sea_orm::QueryResult) -> AppResult<PermitIsolation> {
    Ok(PermitIsolation {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        isolation_point: row
            .try_get("", "isolation_point")
            .map_err(|e| decode_err("isolation_point", e))?,
        energy_type: row
            .try_get("", "energy_type")
            .map_err(|e| decode_err("energy_type", e))?,
        isolation_method: row
            .try_get("", "isolation_method")
            .map_err(|e| decode_err("isolation_method", e))?,
        lock_number: row.try_get("", "lock_number").ok(),
        applied_by_id: row.try_get("", "applied_by_id").ok(),
        verified_by_id: row.try_get("", "verified_by_id").ok(),
        applied_at: row.try_get("", "applied_at").ok(),
        verified_at: row.try_get("", "verified_at").ok(),
        removal_verified_at: row.try_get("", "removal_verified_at").ok(),
        removal_verified_by_id: row.try_get("", "removal_verified_by_id").ok(),
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        applied_by_name: row.try_get("", "applied_by_name").ok(),
        verified_by_name: row.try_get("", "verified_by_name").ok(),
        removal_verified_by_name: row.try_get("", "removal_verified_by_name").ok(),
    })
}

pub async fn get_work_permit_linked_to_work_order(
    db: &impl ConnectionTrait,
    work_order_id: i64,
) -> AppResult<Option<WorkPermit>> {
    // A WO may accumulate cancelled/closed permits; the gate must see the live one.
    let sql = format!(
        "{WORK_PERMIT_SELECT} WHERE wp.linked_work_order_id = ? \
         ORDER BY CASE WHEN wp.status = 'active' THEN 0 \
                       WHEN wp.status IN ('closed', 'handed_back', 'cancelled', 'expired') THEN 2 \
                       ELSE 1 END, wp.id DESC LIMIT 1"
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            &sql,
            [work_order_id.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(map_work_permit(&r)?)),
        None => Ok(None),
    }
}

/// Every permit linked to a work order that still protects (or will protect) work: all but
/// closed, handed back, cancelled and expired ones. The start gate checks each of them.
pub async fn list_live_permits_linked_to_work_order(
    db: &impl ConnectionTrait,
    work_order_id: i64,
) -> AppResult<Vec<WorkPermit>> {
    let sql = format!(
        "{WORK_PERMIT_SELECT} WHERE wp.linked_work_order_id = ? AND wp.status NOT IN ('closed', 'handed_back', 'cancelled', 'expired') ORDER BY wp.id"
    );
    db.query_all(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        &sql,
        [work_order_id.into()],
    ))
    .await?
    .iter()
    .map(map_work_permit)
    .collect()
}

fn map_permit_suspension(row: &sea_orm::QueryResult) -> AppResult<PermitSuspension> {
    Ok(PermitSuspension {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        reason: row.try_get("", "reason").map_err(|e| decode_err("reason", e))?,
        suspended_by_id: row
            .try_get("", "suspended_by_id")
            .map_err(|e| decode_err("suspended_by_id", e))?,
        suspended_at: row
            .try_get("", "suspended_at")
            .map_err(|e| decode_err("suspended_at", e))?,
        reinstated_by_id: row.try_get("", "reinstated_by_id").ok(),
        reinstated_at: row.try_get("", "reinstated_at").ok(),
        reactivation_conditions: row
            .try_get("", "reactivation_conditions")
            .map_err(|e| decode_err("reactivation_conditions", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
        suspended_by_name: row.try_get("", "suspended_by_name").ok(),
        reinstated_by_name: row.try_get("", "reinstated_by_name").ok(),
    })
}

fn map_permit_handover_log(row: &sea_orm::QueryResult) -> AppResult<PermitHandoverLog> {
    Ok(PermitHandoverLog {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        handed_from_role: row
            .try_get("", "handed_from_role")
            .map_err(|e| decode_err("handed_from_role", e))?,
        handed_to_role: row
            .try_get("", "handed_to_role")
            .map_err(|e| decode_err("handed_to_role", e))?,
        confirmation_note: row
            .try_get("", "confirmation_note")
            .map_err(|e| decode_err("confirmation_note", e))?,
        signed_at: row.try_get("", "signed_at").map_err(|e| decode_err("signed_at", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

fn validate_wp_status(s: &str) -> AppResult<()> {
    if WP_STATUSES.contains(&s) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Invalid work permit status '{s}'."
        )]))
    }
}

async fn ensure_wo_asset_aligned(
    db: &impl ConnectionTrait,
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
    let equipment_id: Option<i64> = row
        .try_get("", "equipment_id")
        .map_err(|e| decode_err("equipment_id", e))?;
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

async fn permit_type_code(db: &impl ConnectionTrait, permit_type_id: i64) -> AppResult<String> {
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

async fn count_isolations(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<i64> {
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

async fn count_verified_isolations(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<i64> {
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

async fn count_handover_logs(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<i64> {
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

async fn count_isolations_where(db: &impl ConnectionTrait, permit_id: i64, condition: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT COUNT(*) AS c FROM permit_isolations WHERE permit_id = ? AND {condition}"),
            [permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to count isolations.".into()))?;
    row.try_get("", "c").map_err(|e| decode_err("c", e))
}

fn draft_fields_complete(wp: &WorkPermit) -> bool {
    !wp.code.trim().is_empty() && wp.asset_id > 0
}

/// Gate for any transition into `active` and for starting/resuming a linked WO:
/// the permit must not be expired, LOTO permits need at least one isolation, every
/// isolation must be verified (two-person) and no lock may already be removed.
pub(crate) async fn activation_ready(db: &impl ConnectionTrait, wp: &WorkPermit) -> AppResult<()> {
    let code_param = || std::collections::HashMap::from([("permit".to_string(), wp.code.clone())]);
    if is_expired(wp) {
        return Err(crate::errors::org_validation_failed(
            "PERMIT_EXPIRED",
            format!(
                "Permit {} has expired; it cannot be activated or used to start work.",
                wp.code
            ),
            code_param(),
        ));
    }
    let code = permit_type_code(db, wp.permit_type_id).await?;
    let n = count_isolations(db, wp.id).await?;
    if code == "loto" && n < 1 {
        return Err(crate::errors::org_validation_failed(
            "LOTO_NO_ISOLATION",
            format!(
                "LOTO permit {} has no isolation point: define and verify them first.",
                wp.code
            ),
            code_param(),
        ));
    }
    if n > 0 {
        let v = count_verified_isolations(db, wp.id).await?;
        if v != n {
            let mut params = code_param();
            params.insert("verified".to_string(), v.to_string());
            params.insert("isolations".to_string(), n.to_string());
            return Err(crate::errors::org_validation_failed(
                "LOTO_NOT_VERIFIED",
                format!(
                    "Permit {}: {v} of {n} isolation points are verified; all must be verified.",
                    wp.code
                ),
                params,
            ));
        }
        let removed = count_isolations_where(db, wp.id, "removal_verified_at IS NOT NULL").await?;
        if removed > 0 {
            return Err(crate::errors::org_validation_failed(
                "LOTO_ISOLATION_REMOVED",
                format!(
                    "Permit {}: isolations have already been removed; it no longer protects work.",
                    wp.code
                ),
                code_param(),
            ));
        }
    }
    Ok(())
}

/// Close gate (P0-8): every isolation must have its lock removal verified.
async fn close_ready(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<()> {
    let pending = count_isolations_where(db, permit_id, "removal_verified_at IS NULL").await?;
    if pending > 0 {
        return Err(AppError::ValidationFailed(vec![format!(
            "{pending} isolation(s) still need verified lock removal before the permit can be closed."
        )]));
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
        if matches!(from, "closed" | "handed_back" | "cancelled" | "expired") {
            return Err(AppError::ValidationFailed(vec![
                "Cancellation is not allowed from this status.".into(),
            ]));
        }
        return Ok(());
    }
    #[allow(clippy::unnested_or_patterns)] // one (from, to) pair per line reads as a transition table
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

async fn get_permit_suspension(db: &impl ConnectionTrait, id: i64) -> AppResult<PermitSuspension> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{PERMIT_SUSPENSION_SELECT} WHERE ps.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitSuspension".into(),
            id: id.to_string(),
        })?;
    map_permit_suspension(&row)
}

async fn finalize_open_suspension(
    db: &impl ConnectionTrait,
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
            [reinstated_by_id.into(), ts.into(), new_rv.into(), sid.into(), rv.into()],
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on permit_suspensions.".into(),
        ]));
    }
    Ok(())
}

pub async fn list_permit_types(db: &impl ConnectionTrait) -> AppResult<Vec<PermitType>> {
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

pub async fn get_permit_type(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<PermitType>> {
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
    let txn = db.begin().await?;
    let out = upsert_permit_type_in(&txn, input).await?;
    txn.commit().await?;
    Ok(out)
}

async fn upsert_permit_type_in(db: &impl ConnectionTrait, input: PermitTypeUpsertInput) -> AppResult<PermitType> {
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
        let current = get_permit_type(db, id).await?.ok_or_else(|| AppError::NotFound {
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
            return Err(AppError::ValidationFailed(vec![
                "Concurrent update on permit_types.".into()
            ]));
        }
        let updated = get_permit_type(db, id).await?.expect("row");
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
    Ok(created)
}

pub async fn list_work_permits(db: &impl ConnectionTrait, filter: WorkPermitListFilter) -> AppResult<Vec<WorkPermit>> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 2000);
    let mut sql = format!("{WORK_PERMIT_SELECT} WHERE 1=1");
    let mut params: Vec<sea_orm::Value> = Vec::new();
    if let Some(ref s) = filter.status {
        sql.push_str(" AND wp.status = ?");
        params.push(s.clone().into());
    }
    if let Some(t) = filter.permit_type_id {
        sql.push_str(" AND wp.permit_type_id = ?");
        params.push(t.into());
    }
    if let Some(a) = filter.asset_id {
        sql.push_str(" AND wp.asset_id = ?");
        params.push(a.into());
    }
    if let Some(w) = filter.linked_work_order_id {
        sql.push_str(" AND wp.linked_work_order_id = ?");
        params.push(w.into());
    }
    sql.push_str(" ORDER BY wp.id DESC LIMIT ?");
    params.push(limit.into());

    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params);
    let rows = db.query_all(stmt).await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_work_permit(&row)?);
    }
    Ok(out)
}

pub async fn get_work_permit(db: &impl ConnectionTrait, id: i64) -> AppResult<Option<WorkPermit>> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{WORK_PERMIT_SELECT} WHERE wp.id = ?"),
            [id.into()],
        ))
        .await?;
    row.as_ref().map(map_work_permit).transpose()
}

async fn require_work_permit(db: &impl ConnectionTrait, id: i64) -> AppResult<WorkPermit> {
    get_work_permit(db, id).await?.ok_or_else(|| AppError::NotFound {
        entity: "WorkPermit".into(),
        id: id.to_string(),
    })
}

/// Resolves `(asset_id, entity_id)` for a new permit: explicit values win, then the
/// linked WO (equipment + entity), then the asset's installation node for the entity.
async fn resolve_permit_scope(db: &impl ConnectionTrait, input: &WorkPermitCreateInput) -> AppResult<(i64, i64)> {
    let mut asset_id = input.asset_id;
    let mut entity_id = input.entity_id;
    if let Some(wo_id) = input.linked_work_order_id {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT equipment_id, entity_id FROM work_orders WHERE id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: wo_id.to_string(),
            })?;
        let wo_equipment: Option<i64> = row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?;
        let wo_entity: Option<i64> = row.try_get("", "entity_id").map_err(|e| decode_err("entity_id", e))?;
        asset_id = asset_id.or(wo_equipment);
        entity_id = entity_id.or(wo_entity);
    }
    let asset_id = asset_id.ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "An asset is required (the linked work order has no equipment).".into()
        ])
    })?;
    if entity_id.is_none() {
        let row = db
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
        entity_id = row
            .try_get("", "installed_at_node_id")
            .map_err(|e| decode_err("installed_at_node_id", e))?;
    }
    let entity_id = entity_id.ok_or_else(|| {
        AppError::ValidationFailed(vec![
            "The permit's organisational entity could not be determined from the work order or asset.".into(),
        ])
    })?;
    Ok((asset_id, entity_id))
}

pub async fn create_work_permit(db: &DatabaseConnection, input: WorkPermitCreateInput) -> AppResult<WorkPermit> {
    let txn = db.begin().await?;
    let out = create_work_permit_in(&txn, input).await?;
    txn.commit().await?;
    Ok(out)
}

async fn create_work_permit_in(db: &impl ConnectionTrait, input: WorkPermitCreateInput) -> AppResult<WorkPermit> {
    let _ = permit_type_code(db, input.permit_type_id).await?;
    let (asset_id, entity_id) = resolve_permit_scope(db, &input).await?;
    ensure_wo_asset_aligned(db, input.linked_work_order_id, asset_id).await?;
    let expires_at = normalize_future_expiry(input.expires_at.clone())?;

    if let Some(wo_id) = input.linked_work_order_id {
        if let Some(existing) = get_work_permit_linked_to_work_order(db, wo_id).await? {
            if !WP_TERMINAL_STATUSES.contains(&existing.status.as_str()) {
                return Err(AppError::ValidationFailed(vec![format!(
                    "Work order already has an open permit ({}).",
                    existing.code
                )]));
            }
        }
    }

    let entity_sync_id = Uuid::new_v4().to_string();
    let requested = now_ts();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_permits (entity_sync_id, code, linked_work_order_id, permit_type_id,          asset_id, entity_id, status, requested_at, expires_at, row_version)          VALUES (?, '', ?, ?, ?, ?, 'draft', ?, ?, 1)",
        [
            entity_sync_id.into(),
            opt_i64(input.linked_work_order_id),
            input.permit_type_id.into(),
            asset_id.into(),
            entity_id.into(),
            requested.into(),
            opt_string(expires_at),
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
    // `PTW-<year>-<device>-NNNNNN`: unique across devices (`sync::device_code`).
    let code = crate::sync::code_clash::WORK_PERMITS.next_number(db).await?;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET code = ? WHERE id = ?",
        [code.into(), new_id.into()],
    ))
    .await?;
    let created = require_work_permit(db, new_id).await?;
    Ok(created)
}

pub async fn update_work_permit(db: &DatabaseConnection, input: WorkPermitUpdateInput) -> AppResult<WorkPermit> {
    let txn = db.begin().await?;
    let out = update_work_permit_in(&txn, input).await?;
    txn.commit().await?;
    Ok(out)
}

async fn update_work_permit_in(db: &impl ConnectionTrait, input: WorkPermitUpdateInput) -> AppResult<WorkPermit> {
    let current = require_work_permit(db, input.id).await?;
    if input.expected_row_version != current.row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on work_permits.".into(),
        ]));
    }
    if !WP_EDITABLE_STATUSES.contains(&current.status.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Permit details cannot be edited in status '{}'.",
            current.status
        )]));
    }
    let asset_id = input.asset_id.unwrap_or(current.asset_id);
    let entity_id = input.entity_id.unwrap_or(current.entity_id);
    let linked = input.linked_work_order_id.or(current.linked_work_order_id);
    ensure_wo_asset_aligned(db, linked, asset_id).await?;
    let expires = match input.expires_at {
        Some(e) => normalize_future_expiry(Some(e))?,
        None => current.expires_at.clone(),
    };

    let new_rv = current.row_version + 1;
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
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on work_permits.".into()
        ]));
    }
    let updated = require_work_permit(db, input.id).await?;
    Ok(updated)
}

/// `actor_id` is the session user; it is recorded as the reinstating person when a
/// suspended permit returns to `active`.
pub async fn set_work_permit_status(
    db: &DatabaseConnection,
    input: WorkPermitStatusInput,
    actor_id: i64,
) -> AppResult<WorkPermit> {
    let txn = db.begin().await?;
    let out = set_work_permit_status_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn set_work_permit_status_in(
    db: &impl ConnectionTrait,
    input: WorkPermitStatusInput,
    actor_id: i64,
) -> AppResult<WorkPermit> {
    validate_wp_status(&input.status)?;
    let current = require_work_permit(db, input.id).await?;
    if input.expected_row_version != current.row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on work_permits.".into(),
        ]));
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
            activation_ready(db, &current).await?;
        }
        ("revalidation_required", "active") => {
            activation_ready(db, &current).await?;
            finalize_open_suspension(db, input.id, actor_id, true).await?;
        }
        ("active", "closed") => {
            close_ready(db, input.id).await?;
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
    let mut issued_at = current.issued_at.clone();
    let mut activated_at = current.activated_at.clone();
    let mut closed_at = current.closed_at.clone();
    let mut handed_back_at = current.handed_back_at.clone();
    match to {
        "issued" => issued_at = issued_at.or(Some(now)),
        "active" => activated_at = activated_at.or(Some(now)),
        "closed" => closed_at = Some(now),
        "handed_back" => handed_back_at = Some(now),
        _ => {}
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
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on work_permits.".into()
        ]));
    }
    let updated = require_work_permit(db, input.id).await?;
    Ok(updated)
}

pub async fn suspend_work_permit(
    db: &DatabaseConnection,
    input: PermitSuspendInput,
    actor_id: i64,
) -> AppResult<(WorkPermit, PermitSuspension)> {
    let txn = db.begin().await?;
    let out = suspend_work_permit_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn suspend_work_permit_in(
    db: &impl ConnectionTrait,
    input: PermitSuspendInput,
    actor_id: i64,
) -> AppResult<(WorkPermit, PermitSuspension)> {
    if input.reason.trim().is_empty() {
        return Err(AppError::ValidationFailed(
            vec!["Suspension reason is required.".into()],
        ));
    }
    let current = require_work_permit(db, input.permit_id).await?;
    if current.status != "active" {
        return Err(AppError::ValidationFailed(vec![
            "Only active permits can be suspended.".into(),
        ]));
    }
    if input.expected_row_version != current.row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on work_permits.".into(),
        ]));
    }

    let sync_id = Uuid::new_v4().to_string();
    let suspended_at = now_ts();
    let react = input.reactivation_conditions.unwrap_or_default();

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_suspensions (entity_sync_id, permit_id, reason, suspended_by_id, suspended_at, \
         reactivation_conditions, row_version) VALUES (?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            input.reason.into(),
            actor_id.into(),
            suspended_at.clone().into(),
            react.into(),
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
    let sid: i64 = id_row.try_get("", "id").map_err(|e| decode_err("id", e))?;
    let new_wp_rv = current.row_version + 1;
    let aff = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_permits SET status = 'suspended', row_version = ? WHERE id = ? AND row_version = ?",
            [new_wp_rv.into(), input.permit_id.into(), current.row_version.into()],
        ))
        .await?;
    if aff.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on work_permits.".into()
        ]));
    }

    let suspension = get_permit_suspension(db, sid).await?;
    let wp = require_work_permit(db, input.permit_id).await?;
    Ok((wp, suspension))
}

pub async fn append_permit_handover_log(
    db: &DatabaseConnection,
    input: PermitHandoverLogInput,
) -> AppResult<PermitHandoverLog> {
    let txn = db.begin().await?;
    let out = append_permit_handover_log_in(&txn, input).await?;
    txn.commit().await?;
    Ok(out)
}

async fn append_permit_handover_log_in(
    db: &impl ConnectionTrait,
    input: PermitHandoverLogInput,
) -> AppResult<PermitHandoverLog> {
    let current = require_work_permit(db, input.permit_id).await?;
    if current.status != "closed" {
        return Err(AppError::ValidationFailed(vec![
            "Handover logs can only be recorded when permit status is closed.".into(),
        ]));
    }
    if input.handed_from_role.trim().is_empty() || input.handed_to_role.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["Roles are required.".into()]));
    }
    let signed = now_ts();
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
         confirmation_note, signed_at, row_version) VALUES (?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            input.handed_from_role.trim().to_string().into(),
            input.handed_to_role.trim().to_string().into(),
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
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitHandoverLog".into(),
            id: new_id.to_string(),
        })?;
    let log = map_permit_handover_log(&row)?;
    Ok(log)
}

pub async fn list_permit_suspensions(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<PermitSuspension>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{PERMIT_SUSPENSION_SELECT} WHERE ps.permit_id = ? ORDER BY ps.id ASC"),
            [permit_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_permit_suspension(&row)?);
    }
    Ok(out)
}

pub async fn list_permit_handover_logs(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<PermitHandoverLog>> {
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

pub async fn list_permit_isolations(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<PermitIsolation>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{PERMIT_ISOLATION_SELECT} WHERE pi.permit_id = ? ORDER BY pi.id ASC"),
            [permit_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_permit_isolation(&row)?);
    }
    Ok(out)
}

async fn get_permit_isolation(db: &impl ConnectionTrait, id: i64) -> AppResult<PermitIsolation> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{PERMIT_ISOLATION_SELECT} WHERE pi.id = ?"),
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitIsolation".into(),
            id: id.to_string(),
        })?;
    map_permit_isolation(&row)
}

fn ensure_permit_status_in(wp: &WorkPermit, allowed: &[&str], action: &str) -> AppResult<()> {
    if allowed.contains(&wp.status.as_str()) {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "Cannot {action} while the permit is '{}'.",
            wp.status
        )]))
    }
}

/// Defines a new isolation point or edits one that has not been applied yet.
/// Application / verification state is never written here.
pub async fn upsert_permit_isolation(
    db: &DatabaseConnection,
    input: PermitIsolationUpsertInput,
) -> AppResult<PermitIsolation> {
    let txn = db.begin().await?;
    let out = upsert_permit_isolation_in(&txn, input).await?;
    txn.commit().await?;
    Ok(out)
}

async fn upsert_permit_isolation_in(
    db: &impl ConnectionTrait,
    input: PermitIsolationUpsertInput,
) -> AppResult<PermitIsolation> {
    let permit = require_work_permit(db, input.permit_id).await?;
    ensure_permit_status_in(&permit, ISOLATION_SETUP_STATUSES, "define isolations")?;

    let point = input.isolation_point.trim().to_string();
    let energy = input.energy_type.trim().to_string();
    let method = input.isolation_method.trim().to_string();
    if point.is_empty() {
        return Err(AppError::ValidationFailed(vec!["isolation_point is required.".into()]));
    }
    if energy.is_empty() {
        return Err(AppError::ValidationFailed(vec!["energy_type is required.".into()]));
    }
    if method.is_empty() {
        return Err(AppError::ValidationFailed(vec!["isolation_method is required.".into()]));
    }
    let lock_number = input
        .lock_number
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty());

    if let Some(id) = input.id {
        let current = get_permit_isolation(db, id).await?;
        if current.permit_id != input.permit_id {
            return Err(AppError::NotFound {
                entity: "PermitIsolation".into(),
                id: id.to_string(),
            });
        }
        if current.applied_at.is_some() {
            return Err(AppError::ValidationFailed(vec![
                "An applied isolation cannot be edited.".into(),
            ]));
        }
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE permit_isolations SET isolation_point = ?, energy_type = ?, isolation_method = ?, \
                 lock_number = ?, row_version = ? \
                 WHERE id = ? AND row_version = ? AND applied_at IS NULL",
                [
                    point.into(),
                    energy.into(),
                    method.into(),
                    opt_string(lock_number),
                    new_rv.into(),
                    id.into(),
                    current.row_version.into(),
                ],
            ))
            .await?;
        if affected.rows_affected() == 0 {
            return Err(AppError::ValidationFailed(vec![
                "Concurrent update on permit_isolations.".into(),
            ]));
        }
        let out = get_permit_isolation(db, id).await?;
        return Ok(out);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_isolations (entity_sync_id, permit_id, isolation_point, energy_type, \
         isolation_method, lock_number, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            point.into(),
            energy.into(),
            method.into(),
            opt_string(lock_number),
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
    let created = get_permit_isolation(db, new_id).await?;
    Ok(created)
}

/// Loads an isolation and its permit for a lifecycle action, checking optimistic locking.
async fn load_isolation_for_action(
    db: &impl ConnectionTrait,
    input: &PermitIsolationActionInput,
) -> AppResult<(PermitIsolation, WorkPermit)> {
    let iso = get_permit_isolation(db, input.isolation_id).await?;
    if iso.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on permit_isolations.".into(),
        ]));
    }
    let permit = require_work_permit(db, iso.permit_id).await?;
    Ok((iso, permit))
}

async fn write_isolation_action(
    db: &impl ConnectionTrait,
    iso: &PermitIsolation,
    set_clause: &str,
    mut values: Vec<sea_orm::Value>,
) -> AppResult<PermitIsolation> {
    let new_rv = iso.row_version + 1;
    values.push(new_rv.into());
    values.push(iso.id.into());
    values.push(iso.row_version.into());
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("UPDATE permit_isolations SET {set_clause}, row_version = ? WHERE id = ? AND row_version = ?"),
            values,
        ))
        .await?;
    if affected.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on permit_isolations.".into(),
        ]));
    }
    let out = get_permit_isolation(db, iso.id).await?;
    Ok(out)
}

/// Locks must never come off under running work: removal is refused while the linked
/// work order is still being executed.
async fn ensure_linked_work_not_running(db: &impl ConnectionTrait, permit: &WorkPermit) -> AppResult<()> {
    let Some(wo_id) = permit.linked_work_order_id else {
        return Ok(());
    };
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wos.code AS status_code FROM work_orders wo              JOIN work_order_statuses wos ON wos.id = wo.status_id WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let status: String = row
        .try_get("", "status_code")
        .map_err(|e| decode_err("status_code", e))?;
    if crate::wo::domain::WoStatus::try_from_str(&status).is_ok_and(|s| s == crate::wo::domain::WoStatus::InProgress) {
        return Err(AppError::ValidationFailed(vec![
            "Work on the linked work order is still in progress; pause or complete it before removing locks.".into(),
        ]));
    }
    Ok(())
}

/// Records that `actor_id` (the session user) fitted the lock / isolation.
pub async fn apply_permit_isolation(
    db: &DatabaseConnection,
    input: PermitIsolationActionInput,
    actor_id: i64,
) -> AppResult<PermitIsolation> {
    let txn = db.begin().await?;
    let out = apply_permit_isolation_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn apply_permit_isolation_in(
    db: &impl ConnectionTrait,
    input: PermitIsolationActionInput,
    actor_id: i64,
) -> AppResult<PermitIsolation> {
    let (iso, permit) = load_isolation_for_action(db, &input).await?;
    ensure_permit_status_in(&permit, ISOLATION_SETUP_STATUSES, "apply isolations")?;
    enforce_loto_role(db, actor_id, permit.id, "isolation_apply").await?;
    if iso.applied_at.is_some() {
        return Err(AppError::ValidationFailed(vec!["Isolation is already applied.".into()]));
    }
    let lock_number = input
        .lock_number
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .or_else(|| iso.lock_number.clone());
    write_isolation_action(
        db,
        &iso,
        "applied_by_id = ?, applied_at = ?, lock_number = ?",
        vec![actor_id.into(), now_ts().into(), opt_string(lock_number)],
    )
    .await
}

/// The person acting in a LOTO role must hold what the role requires
/// (`qualification_requirements`, target `loto_role`), or have a supervisor override recorded
/// for this permit and role. When the role has requirements, a login not linked to a personnel
/// record cannot be checked and is refused (fail closed).
async fn enforce_loto_role(db: &impl ConnectionTrait, actor_id: i64, permit_id: i64, role: &str) -> AppResult<()> {
    match crate::qualification::gate::personnel_for_user(db, actor_id).await? {
        Some(personnel_id) => {
            crate::qualification::gate::enforce_permit_role(db, personnel_id, permit_id, role, None).await?;
        }
        None => {
            if !crate::qualification::gate::loto_role_requirements(db, role)
                .await?
                .is_empty()
            {
                return Err(crate::qualification::issue(
                    "LOTO_ROLE_PERSON_UNKNOWN",
                    "This LOTO role requires qualifications: link your account to your personnel record first.",
                    &[("role", role.to_string())],
                ));
            }
        }
    }
    Ok(())
}

/// Two-person rule (P0-8): the verifier (session user) must differ from the applier.
pub async fn verify_permit_isolation(
    db: &DatabaseConnection,
    input: PermitIsolationActionInput,
    actor_id: i64,
) -> AppResult<PermitIsolation> {
    let txn = db.begin().await?;
    let out = verify_permit_isolation_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn verify_permit_isolation_in(
    db: &impl ConnectionTrait,
    input: PermitIsolationActionInput,
    actor_id: i64,
) -> AppResult<PermitIsolation> {
    let (iso, permit) = load_isolation_for_action(db, &input).await?;
    ensure_permit_status_in(&permit, ISOLATION_SETUP_STATUSES, "verify isolations")?;
    enforce_loto_role(db, actor_id, permit.id, "isolation_verify").await?;
    let Some(applied_by) = iso.applied_by_id.filter(|_| iso.applied_at.is_some()) else {
        return Err(AppError::ValidationFailed(vec![
            "Isolation must be applied before it can be verified.".into(),
        ]));
    };
    if iso.verified_at.is_some() {
        return Err(AppError::ValidationFailed(
            vec!["Isolation is already verified.".into()],
        ));
    }
    if applied_by == actor_id {
        return Err(AppError::ValidationFailed(vec![
            "Two-person rule: the person who applied an isolation cannot verify it.".into(),
        ]));
    }
    write_isolation_action(
        db,
        &iso,
        "verified_by_id = ?, verified_at = ?",
        vec![actor_id.into(), now_ts().into()],
    )
    .await
}

/// Records verified lock removal. Same two-person rule as verification: the person
/// confirming removal must not be the person who applied the isolation.
pub async fn verify_permit_isolation_removal(
    db: &DatabaseConnection,
    input: PermitIsolationActionInput,
    actor_id: i64,
) -> AppResult<PermitIsolation> {
    let txn = db.begin().await?;
    let out = verify_permit_isolation_removal_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn verify_permit_isolation_removal_in(
    db: &impl ConnectionTrait,
    input: PermitIsolationActionInput,
    actor_id: i64,
) -> AppResult<PermitIsolation> {
    let (iso, permit) = load_isolation_for_action(db, &input).await?;
    ensure_permit_status_in(&permit, ISOLATION_REMOVAL_STATUSES, "verify isolation removal")?;
    enforce_loto_role(db, actor_id, permit.id, "removal_verify").await?;
    if iso.verified_at.is_none() {
        return Err(AppError::ValidationFailed(vec![
            "Only a verified isolation can have its removal verified.".into(),
        ]));
    }
    if iso.removal_verified_at.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Isolation removal is already verified.".into(),
        ]));
    }
    if iso.applied_by_id == Some(actor_id) {
        return Err(AppError::ValidationFailed(vec![
            "Two-person rule: the person who applied an isolation cannot verify its removal.".into(),
        ]));
    }
    ensure_linked_work_not_running(db, &permit).await?;
    write_isolation_action(
        db,
        &iso,
        "removal_verified_by_id = ?, removal_verified_at = ?",
        vec![actor_id.into(), now_ts().into()],
    )
    .await
}

fn map_loto_card_print_job(row: &sea_orm::QueryResult) -> AppResult<LotoCardPrintJob> {
    Ok(LotoCardPrintJob {
        id: row.try_get("", "id").map_err(|e| decode_err("id", e))?,
        permit_id: row.try_get("", "permit_id").map_err(|e| decode_err("permit_id", e))?,
        isolation_id: row
            .try_get("", "isolation_id")
            .map_err(|e| decode_err("isolation_id", e))?,
        printed_at: row.try_get("", "printed_at").map_err(|e| decode_err("printed_at", e))?,
        printed_by_id: row
            .try_get("", "printed_by_id")
            .map_err(|e| decode_err("printed_by_id", e))?,
        entity_sync_id: row
            .try_get("", "entity_sync_id")
            .map_err(|e| decode_err("entity_sync_id", e))?,
        row_version: row
            .try_get("", "row_version")
            .map_err(|e| decode_err("row_version", e))?,
    })
}

pub async fn get_loto_card_view(
    db: &impl ConnectionTrait,
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
    let permit_code: String = row
        .try_get("", "permit_code")
        .map_err(|e| decode_err("permit_code", e))?;
    let asset_code: Option<String> = row.try_get("", "asset_id_code").ok();
    let eq_name: Option<String> = row.try_get("", "equipment_name").ok();
    let equipment_label = match (asset_code, eq_name) {
        (Some(c), Some(n)) => format!("{c} — {n}"),
        (Some(c), None) => c,
        (None, Some(n)) => n,
        (None, None) => "—".to_string(),
    };
    let isolation_point: String = row
        .try_get("", "isolation_point")
        .map_err(|e| decode_err("isolation_point", e))?;
    let energy_type: String = row
        .try_get("", "energy_type")
        .map_err(|e| decode_err("energy_type", e))?;
    let lock_number: Option<String> = row.try_get("", "lock_number").ok();
    let expires_at: Option<String> = row.try_get("", "permit_expires").ok();
    let verified_at: Option<String> = row.try_get("", "verified_at").ok().flatten();
    let verifier_username: Option<String> = row.try_get("", "verifier_username").ok().flatten();
    let verifier_signature = if verified_at.is_some() { verifier_username } else { None };
    let iso_id: i64 = row
        .try_get("", "isolation_id")
        .map_err(|e| decode_err("isolation_id", e))?;
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
    actor_id: i64,
) -> AppResult<LotoCardPrintJob> {
    let txn = db.begin().await?;
    let out = record_loto_card_print_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn record_loto_card_print_in(
    db: &impl ConnectionTrait,
    input: LotoCardPrintInput,
    actor_id: i64,
) -> AppResult<LotoCardPrintJob> {
    // The lock card is issued by someone authorised to apply the isolation.
    enforce_loto_role(db, actor_id, input.permit_id, "isolation_apply").await?;
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
            actor_id.into(),
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
    Ok(job)
}

pub async fn list_open_permits_report(db: &impl ConnectionTrait) -> AppResult<Vec<WorkPermit>> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{WORK_PERMIT_SELECT} \
                 WHERE wp.status IN ('issued', 'active', 'suspended') \
                 AND (wp.expires_at IS NULL OR wp.expires_at > ?) \
                 ORDER BY wp.id DESC"
            ),
            [now.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(map_work_permit(&row)?);
    }
    Ok(out)
}

pub async fn permit_compliance_kpi_30d(db: &impl ConnectionTrait) -> AppResult<PermitComplianceKpi30d> {
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
    #[allow(clippy::cast_precision_loss)] // permit counts are far below 2^52
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
