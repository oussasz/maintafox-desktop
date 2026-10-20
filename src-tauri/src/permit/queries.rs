use chrono::{Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};

use super::domain::{
    LotoCardPrintInput, LotoCardPrintJob, LotoCardView, PermitComplianceKpi30d, PermitHandoverLog,
    PermitHandoverLogInput, PermitIsolation, PermitIsolationActionInput, PermitIsolationCancelInput,
    PermitIsolationUpsertInput, PermitSuspendInput, PermitSuspension, PermitType, PermitTypeUpsertInput, WorkPermit,
    WorkPermitCreateInput, WorkPermitListFilter, WorkPermitListPage, WorkPermitStatusInput, WorkPermitUpdateInput,
};
use crate::auth::scope::AccessScope;

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

/// SQL list of the statuses of a permit still open: issued and not yet closed, handed back,
/// cancelled or expired (a suspended permit, or one waiting for revalidation, is still open). The
/// open-permits report and the "permit expiring / expired" notification checks share it.
pub const OPEN_PERMIT_STATUSES_SQL: &str = "('issued', 'active', 'suspended', 'revalidation_required')";

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

/// Header fields (expiry, org node) are frozen once the permit is active.
const WP_EDITABLE_STATUSES: &[&str] = &["draft", "pending_review", "approved", "issued"];

/// Statuses in which a permit's equipment or linked work order may still change (CMMS Advisor
/// decision A): from `approved` on, the permit was approved for that equipment. A change in
/// `pending_review` sends the permit back to `draft` (a fresh review is needed).
pub(crate) const WP_EQUIPMENT_CHANGE_STATUSES: &[&str] = &["draft", "pending_review"];

/// Statuses in which a planned isolation line (never applied) may be cancelled (CMMS Advisor
/// decision): those in which the permit's equipment may still change. A cancellation in
/// `pending_review` sends the permit back to `draft`, as an equipment change does.
pub(crate) const ISOLATION_CANCEL_STATUSES: &[&str] = WP_EQUIPMENT_CHANGE_STATUSES;

/// SQL condition (columns of `permit_isolations`, unqualified) of a line that counts as an
/// isolation of its permit: every line except a cancelled one never applied (migration 243). A
/// cancelled line is never applied here nor accepted applied by sync; a line that would hold both
/// still counts (fail-safe: a fitted lock is never hidden).
pub(crate) const COUNTED_ISOLATION_SQL: &str = "(cancelled_at IS NULL OR applied_at IS NOT NULL)";

const WORK_PERMIT_SELECT: &str = "SELECT wp.id, wp.entity_sync_id, wp.code, wp.linked_work_order_id, \
     wp.permit_type_id, wp.asset_id, wp.entity_id, wp.status, wp.requested_at, wp.issued_at, \
     wp.activated_at, wp.expires_at, wp.closed_at, wp.handed_back_at, wp.row_version, \
     pt.code AS permit_type_code, pt.name AS permit_type_name, \
     COALESCE(pt.requires_energy_isolation, 0) AS permit_type_requires_energy_isolation, \
     COALESCE(pt.requires_three_person_removal, 0) AS permit_type_requires_three_person_removal, \
     e.asset_id_code AS asset_code, e.name AS asset_label, wo.code AS linked_work_order_code, \
     wp.created_by_id, wp.submitted_by_id, wp.submitted_at, wp.approved_by_id, wp.approved_at, \
     COALESCE(wp.self_approved, 0) AS self_approved, \
     COALESCE(ucr.display_name, ucr.username) AS created_by_name, \
     COALESCE(usu.display_name, usu.username) AS submitted_by_name, \
     COALESCE(uap.display_name, uap.username) AS approved_by_name \
     FROM work_permits wp \
     LEFT JOIN permit_types pt ON pt.id = wp.permit_type_id \
     LEFT JOIN equipment e ON e.id = wp.asset_id \
     LEFT JOIN work_orders wo ON wo.id = wp.linked_work_order_id \
     LEFT JOIN user_accounts ucr ON ucr.id = wp.created_by_id \
     LEFT JOIN user_accounts usu ON usu.id = wp.submitted_by_id \
     LEFT JOIN user_accounts uap ON uap.id = wp.approved_by_id";

const PERMIT_ISOLATION_SELECT: &str = "SELECT pi.id, pi.entity_sync_id, pi.permit_id, pi.isolation_point, \
     pi.energy_type, pi.isolation_method, pi.energy_type_id, et.code AS energy_type_code, \
     et.label AS energy_type_label, pi.isolation_method_id, im.label AS isolation_method_label, \
     pi.mapping_review_required, pi.lock_number, pi.applied_by_id, pi.verified_by_id, \
     pi.applied_at, pi.verified_at, pi.removal_verified_at, pi.removal_verified_by_id, pi.row_version, \
     pi.cancelled_at, pi.cancelled_by_id, \
     COALESCE(ua.display_name, ua.username) AS applied_by_name, \
     COALESCE(uv.display_name, uv.username) AS verified_by_name, \
     COALESCE(ur.display_name, ur.username) AS removal_verified_by_name, \
     COALESCE(uc.display_name, uc.username) AS cancelled_by_name, \
     pi.removed_by_id, pi.removed_at, COALESCE(pi.forced_removal, 0) AS forced_removal, \
     pi.forced_authorized_by_id, pi.forced_reason, COALESCE(pi.forced_worker_absent, 0) AS forced_worker_absent, \
     pi.forced_contact_attempts_json, pi.informed_by_id, pi.informed_at, pi.informed_method, \
     COALESCE(pi.informed_in_app, 0) AS informed_in_app, \
     COALESCE(urm.display_name, urm.username) AS removed_by_name, \
     COALESCE(ufa.display_name, ufa.username) AS forced_authorized_by_name, \
     COALESCE(uin.display_name, uin.username) AS informed_by_name \
     FROM permit_isolations pi \
     LEFT JOIN reference_values et ON et.id = pi.energy_type_id \
     LEFT JOIN reference_values im ON im.id = pi.isolation_method_id \
     LEFT JOIN user_accounts ua ON ua.id = pi.applied_by_id \
     LEFT JOIN user_accounts uv ON uv.id = pi.verified_by_id \
     LEFT JOIN user_accounts ur ON ur.id = pi.removal_verified_by_id \
     LEFT JOIN user_accounts uc ON uc.id = pi.cancelled_by_id \
     LEFT JOIN user_accounts urm ON urm.id = pi.removed_by_id \
     LEFT JOIN user_accounts ufa ON ufa.id = pi.forced_authorized_by_id \
     LEFT JOIN user_accounts uin ON uin.id = pi.informed_by_id";

const PERMIT_SUSPENSION_SELECT: &str = "SELECT ps.id, ps.entity_sync_id, ps.permit_id, ps.reason, \
     ps.suspended_by_id, ps.suspended_at, ps.reinstated_by_id, ps.reinstated_at, \
     ps.reactivation_conditions, ps.row_version, \
     COALESCE(us.display_name, us.username) AS suspended_by_name, \
     COALESCE(ur.display_name, ur.username) AS reinstated_by_name \
     FROM permit_suspensions ps \
     LEFT JOIN user_accounts us ON us.id = ps.suspended_by_id \
     LEFT JOIN user_accounts ur ON ur.id = ps.reinstated_by_id";

/// A coded refusal of a permit rule: the code is translated on screen (`training:errors`), the
/// message stays for logs and tests.
fn refused(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(
        code,
        message,
        params.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
    )
}

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

/// Stable code: a permit type's code cannot change once the type exists (here and on a row
/// received from another PC).
pub const CODE_PERMIT_TYPE_CODE_LOCKED: &str = "PERMIT_TYPE_CODE_LOCKED";

/// Sync identity of the shipped LOTO permit type (migration 67): its energy-isolation rule is locked.
const SHIPPED_LOTO_TYPE_SYNC_ID: &str = "11111111-1111-4111-8111-111111111111";

/// A permit must expire: a non-empty expiry in the future and, when the type sets a maximum
/// duration, no later than now + that duration. Coded refusals (translated on screen).
async fn checked_expiry(db: &impl ConnectionTrait, permit_type_id: i64, value: Option<&str>) -> AppResult<String> {
    let Some(raw) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Err(crate::errors::org_validation_failed(
            "PERMIT_EXPIRY_REQUIRED",
            "A permit needs an expiry date and time.",
            std::collections::HashMap::new(),
        ));
    };
    let ts = normalize_ts("expires_at", raw)?;
    if ts <= now_ts() {
        return Err(crate::errors::org_validation_failed(
            "PERMIT_EXPIRY_PAST",
            "The permit expiry must be in the future.",
            std::collections::HashMap::new(),
        ));
    }
    let max_hours: Option<f64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT max_duration_hours FROM permit_types WHERE id = ?",
            [permit_type_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitType".into(),
            id: permit_type_id.to_string(),
        })?
        .try_get("", "max_duration_hours")
        .map_err(|e| decode_err("max_duration_hours", e))?;
    if let Some(hours) = max_hours.filter(|h| *h > 0.0) {
        // Whole seconds: the stored form has no fractions.
        #[allow(clippy::cast_possible_truncation)]
        let limit = (Utc::now() + chrono::Duration::seconds((hours * 3600.0).round() as i64))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        if ts > limit {
            return Err(crate::errors::org_validation_failed(
                "PERMIT_EXPIRY_TOO_LONG",
                format!("This permit type lasts at most {hours} h."),
                std::collections::HashMap::from([("hours".to_string(), hours.to_string())]),
            ));
        }
    }
    Ok(ts)
}

/// The stored expiry of a permit, refused when missing (permits created before expiry was required).
fn checked_expiry_present(wp: &WorkPermit) -> AppResult<String> {
    wp.expires_at.clone().filter(|e| !e.trim().is_empty()).ok_or_else(|| {
        crate::errors::org_validation_failed(
            "PERMIT_EXPIRY_MISSING",
            format!("Permit {} has no expiry: set one first.", wp.code),
            std::collections::HashMap::from([("permit".to_string(), wp.code.clone())]),
        )
    })
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
        requires_energy_isolation: i64_to_bool(
            row.try_get::<i64>("", "requires_energy_isolation")
                .map_err(|e| decode_err("requires_energy_isolation", e))?,
        ),
        requires_three_person_removal: i64_to_bool(
            row.try_get::<i64>("", "requires_three_person_removal")
                .map_err(|e| decode_err("requires_three_person_removal", e))?,
        ),
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
        permit_type_requires_energy_isolation: i64_to_bool(
            row.try_get::<i64>("", "permit_type_requires_energy_isolation")
                .map_err(|e| decode_err("permit_type_requires_energy_isolation", e))?,
        ),
        permit_type_requires_three_person_removal: i64_to_bool(
            row.try_get::<i64>("", "permit_type_requires_three_person_removal")
                .map_err(|e| decode_err("permit_type_requires_three_person_removal", e))?,
        ),
        created_by_id: row
            .try_get("", "created_by_id")
            .map_err(|e| decode_err("created_by_id", e))?,
        created_by_name: row
            .try_get("", "created_by_name")
            .map_err(|e| decode_err("created_by_name", e))?,
        submitted_by_id: row
            .try_get("", "submitted_by_id")
            .map_err(|e| decode_err("submitted_by_id", e))?,
        submitted_by_name: row
            .try_get("", "submitted_by_name")
            .map_err(|e| decode_err("submitted_by_name", e))?,
        submitted_at: row
            .try_get("", "submitted_at")
            .map_err(|e| decode_err("submitted_at", e))?,
        approved_by_id: row
            .try_get("", "approved_by_id")
            .map_err(|e| decode_err("approved_by_id", e))?,
        approved_by_name: row
            .try_get("", "approved_by_name")
            .map_err(|e| decode_err("approved_by_name", e))?,
        approved_at: row
            .try_get("", "approved_at")
            .map_err(|e| decode_err("approved_at", e))?,
        self_approved: i64_to_bool(
            row.try_get::<i64>("", "self_approved")
                .map_err(|e| decode_err("self_approved", e))?,
        ),
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
        energy_type_id: row.try_get("", "energy_type_id").ok().flatten(),
        energy_type_code: row.try_get("", "energy_type_code").ok().flatten(),
        energy_type_label: row.try_get("", "energy_type_label").ok().flatten(),
        isolation_method_id: row.try_get("", "isolation_method_id").ok().flatten(),
        isolation_method_label: row.try_get("", "isolation_method_label").ok().flatten(),
        mapping_review_required: i64_to_bool(
            row.try_get::<i64>("", "mapping_review_required")
                .map_err(|e| decode_err("mapping_review_required", e))?,
        ),
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
        cancelled_at: row.try_get("", "cancelled_at").ok().flatten(),
        cancelled_by_id: row.try_get("", "cancelled_by_id").ok().flatten(),
        cancelled_by_name: row.try_get("", "cancelled_by_name").ok().flatten(),
        removed_by_id: row.try_get("", "removed_by_id").ok().flatten(),
        removed_by_name: row.try_get("", "removed_by_name").ok().flatten(),
        removed_at: row.try_get("", "removed_at").ok().flatten(),
        forced_removal: i64_to_bool(
            row.try_get::<i64>("", "forced_removal")
                .map_err(|e| decode_err("forced_removal", e))?,
        ),
        forced_authorized_by_id: row.try_get("", "forced_authorized_by_id").ok().flatten(),
        forced_authorized_by_name: row.try_get("", "forced_authorized_by_name").ok().flatten(),
        forced_reason: row.try_get("", "forced_reason").ok().flatten(),
        forced_worker_absent: i64_to_bool(
            row.try_get::<i64>("", "forced_worker_absent")
                .map_err(|e| decode_err("forced_worker_absent", e))?,
        ),
        forced_contact_attempts: row
            .try_get::<Option<String>>("", "forced_contact_attempts_json")
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str::<Vec<super::domain::ContactAttempt>>(&j).ok())
            .unwrap_or_default(),
        informed_by_id: row.try_get("", "informed_by_id").ok().flatten(),
        informed_by_name: row.try_get("", "informed_by_name").ok().flatten(),
        informed_at: row.try_get("", "informed_at").ok().flatten(),
        informed_method: row.try_get("", "informed_method").ok().flatten(),
        informed_in_app: i64_to_bool(
            row.try_get::<i64>("", "informed_in_app")
                .map_err(|e| decode_err("informed_in_app", e))?,
        ),
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

/// The name shown for the user account of alias `alias` in a hand-over: its display name, or its
/// login name when the display name is missing, empty or blank. One expression for the stored
/// hand-overs and the receiver list, so a person picked under a name is shown under that name.
fn handover_person_name_sql(alias: &str) -> String {
    format!("COALESCE(NULLIF(TRIM({alias}.display_name), ''), {alias}.username)")
}

/// The label of the hand-over role stored in `column`, from the plant's list: looked up over every
/// published version of the list (the current one and those it replaced), roles since deactivated
/// included, so a stored hand-over keeps its role's name. The exact code first, else the code
/// ignoring case (adopted free texts); each from the most recent version that holds it. NULL when
/// no published version holds it.
fn handover_role_label_sql(column: &str) -> String {
    let lookup = |matches: String| {
        format!(
            "(SELECT rv.label FROM reference_values rv                 JOIN reference_sets rs ON rs.id = rv.set_id                 JOIN reference_domains d ON d.id = rs.domain_id                WHERE UPPER(TRIM(d.code)) = '{domain}' AND rs.status IN ('published', 'superseded')                  AND {matches}                ORDER BY rs.version_no DESC, rv.is_active DESC, rv.id DESC LIMIT 1)",
            domain = super::handover_roles::DOMAIN,
        )
    };
    format!(
        "COALESCE({}, {})",
        lookup(format!("rv.code = {column}")),
        lookup(format!("UPPER(TRIM(rv.code)) = UPPER(TRIM({column}))")),
    )
}

/// Columns of a hand-over log with the role labels and the signer's and receiver's names (alias `h`).
fn handover_select() -> String {
    format!(
        "SELECT h.id, h.entity_sync_id, h.permit_id, h.handed_from_role, h.handed_to_role, \
         {} AS handed_from_label, {} AS handed_to_label, \
         h.confirmation_note, h.signed_at, h.signed_by_id, h.received_by_id, h.row_version, \
         {} AS signed_by_name, {} AS received_by_name \
         FROM permit_handover_logs h \
         LEFT JOIN user_accounts us ON us.id = h.signed_by_id \
         LEFT JOIN user_accounts ur ON ur.id = h.received_by_id",
        handover_role_label_sql("h.handed_from_role"),
        handover_role_label_sql("h.handed_to_role"),
        handover_person_name_sql("us"),
        handover_person_name_sql("ur"),
    )
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
        handed_from_label: row
            .try_get("", "handed_from_label")
            .map_err(|e| decode_err("handed_from_label", e))?,
        handed_to_label: row
            .try_get("", "handed_to_label")
            .map_err(|e| decode_err("handed_to_label", e))?,
        confirmation_note: row
            .try_get("", "confirmation_note")
            .map_err(|e| decode_err("confirmation_note", e))?,
        signed_at: row.try_get("", "signed_at").map_err(|e| decode_err("signed_at", e))?,
        signed_by_id: row
            .try_get("", "signed_by_id")
            .map_err(|e| decode_err("signed_by_id", e))?,
        signed_by_name: row
            .try_get("", "signed_by_name")
            .map_err(|e| decode_err("signed_by_name", e))?,
        received_by_id: row
            .try_get("", "received_by_id")
            .map_err(|e| decode_err("received_by_id", e))?,
        received_by_name: row
            .try_get("", "received_by_name")
            .map_err(|e| decode_err("received_by_name", e))?,
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
        return Err(refused(
            "PERMIT_WO_NO_EQUIPMENT",
            "Work order has no equipment_id; cannot align permit asset.",
            &[],
        ));
    };
    if eq != asset_id {
        return Err(refused(
            "PERMIT_WO_EQUIPMENT_MISMATCH",
            format!("The linked work order's equipment ({eq}) must be the permit's equipment ({asset_id})."),
            &[],
        ));
    }
    Ok(())
}

/// Whether permits of the type need at least one energy isolation before they start
/// (`permit_types.requires_energy_isolation`, migration 225 — set for the LOTO type).
pub(crate) async fn permit_type_requires_energy_isolation(
    db: &impl ConnectionTrait,
    permit_type_id: i64,
) -> AppResult<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT requires_energy_isolation FROM permit_types WHERE id = ?",
            [permit_type_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitType".into(),
            id: permit_type_id.to_string(),
        })?;
    Ok(i64_to_bool(
        row.try_get::<i64>("", "requires_energy_isolation")
            .map_err(|e| decode_err("requires_energy_isolation", e))?,
    ))
}

async fn count_isolations(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT COUNT(*) AS c FROM permit_isolations WHERE permit_id = ? AND {COUNTED_ISOLATION_SQL}"),
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
            format!(
                "SELECT COUNT(*) AS c FROM permit_isolations WHERE permit_id = ? AND verified_at IS NOT NULL \
                 AND {COUNTED_ISOLATION_SQL}"
            ),
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
    // Read from the type itself (not the joined copy on `wp`, which a caller may hold stale).
    let needs_isolation = permit_type_requires_energy_isolation(db, wp.permit_type_id).await?;
    let n = count_isolations(db, wp.id).await?;
    if needs_isolation && n < 1 {
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

/// Close gate (P0-8): every isolation must have its lock removal verified (a cancelled planned line
/// was never fitted: nothing to remove).
async fn close_ready(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<()> {
    let pending = count_isolations_where(
        db,
        permit_id,
        &format!("removal_verified_at IS NULL AND {COUNTED_ISOLATION_SQL}"),
    )
    .await?;
    if pending > 0 {
        return Err(refused(
            "PERMIT_REMOVAL_PENDING",
            format!("{pending} isolation(s) still need verified lock removal before the permit can be closed."),
            &[("pending", pending.to_string())],
        ));
    }
    Ok(())
}

fn assert_transition_set_status(from: &str, to: &str) -> AppResult<()> {
    if from == to {
        return Err(refused("PERMIT_STATUS_UNCHANGED", "Status is already applied.", &[]));
    }
    if from == "handed_back" && to != "cancelled" {
        return Err(refused(
            "PERMIT_TRANSITION_NOT_ALLOWED",
            "Only transition from handed_back is to cancelled.",
            &[("from", from.to_string()), ("to", to.to_string())],
        ));
    }
    if to == "cancelled" {
        if from == "handed_back" {
            return Ok(());
        }
        if matches!(from, "closed" | "handed_back" | "cancelled" | "expired") {
            return Err(refused(
                "PERMIT_CANCEL_NOT_ALLOWED",
                "Cancellation is not allowed from this status.",
                &[("status", from.to_string())],
            ));
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
        Err(refused(
            "PERMIT_TRANSITION_NOT_ALLOWED",
            format!(
                "Transition {from} -> {to} is not allowed for set_work_permit_status (use suspend_work_permit for active -> suspended)."
            ),
            &[("from", from.to_string()), ("to", to.to_string())],
        ))
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
             mandatory_ppe_ids_json, mandatory_control_rules_json, requires_energy_isolation, \
             requires_three_person_removal, row_version \
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
             mandatory_ppe_ids_json, mandatory_control_rules_json, requires_energy_isolation, \
             requires_three_person_removal, row_version \
             FROM permit_types WHERE id = ?",
            [id.into()],
        ))
        .await?;
    row.as_ref().map(map_permit_type).transpose()
}

/// Who saves a permit type and what the command checked (`commands::permit::upsert_permit_type`).
#[derive(Debug, Clone, Copy)]
pub struct PermitTypeWriter {
    pub actor_id: i64,
    /// The caller holds `adm.settings` and a valid step-up: only then may the energy-isolation
    /// rule of a type be switched off.
    pub may_relax_isolation: bool,
    /// The caller holds `adm.settings` and a valid step-up: only then may the three-person
    /// lock-removal setting of a type be changed (either way, including on a new type).
    pub may_change_three_person: bool,
}

/// True when the save would switch the energy-isolation rule of an existing type off.
pub fn relaxes_energy_isolation(current: &PermitType, input: &PermitTypeUpsertInput) -> bool {
    current.requires_energy_isolation && input.requires_energy_isolation == Some(false)
}

/// True when the save would change the three-person lock-removal setting: on a new type
/// (`current` is `None`) when it asks for it, on an existing type when it sends another value.
pub fn changes_three_person_removal(current: Option<&PermitType>, input: &PermitTypeUpsertInput) -> bool {
    match (current, input.requires_three_person_removal) {
        (_, None) => false,
        (None, Some(wanted)) => wanted,
        (Some(current), Some(wanted)) => wanted != current.requires_three_person_removal,
    }
}

pub async fn upsert_permit_type(
    db: &DatabaseConnection,
    input: PermitTypeUpsertInput,
    writer: PermitTypeWriter,
) -> AppResult<PermitType> {
    let txn = db.begin().await?;
    let out = upsert_permit_type_in(&txn, input, writer).await?;
    txn.commit().await?;
    Ok(out)
}

/// The shipped LOTO permit type (migration 67), by its sync identity or its code: its
/// energy-isolation rule is never switched off, here or by a row received from another PC.
pub(crate) fn is_shipped_loto_type(entity_sync_id: &str, code: &str) -> bool {
    entity_sync_id == SHIPPED_LOTO_TYPE_SYNC_ID || code.trim().eq_ignore_ascii_case("loto")
}

/// The reason recorded when the update switches the shipped LOTO type's energy-isolation rule back
/// on (migration 241). Fixed text, the same on every PC.
pub(crate) const LOTO_ISOLATION_RESTORE_REASON: &str = "Mise à jour Maintafox : le type LOTO livré exige toujours une isolation énergétique (rétablie). / Maintafox update: the shipped LOTO type always requires an energy isolation (restored).";

/// The record of that restore: written by nobody (the update itself) with the fixed reason.
pub(crate) fn is_loto_isolation_restore_record(by: Option<&str>, reason: Option<&str>) -> bool {
    by.is_none() && reason.map(str::trim) == Some(LOTO_ISOLATION_RESTORE_REASON)
}

/// Migration 241: the shipped LOTO type (identified as [`is_shipped_loto_type`] does) requires an
/// energy isolation again where a switch-off received from another PC before migration 240 left it
/// off. Touches that row only, and only while it is off (idempotent): the rule on, the record says
/// the update did it (nobody, now, [`LOTO_ISOLATION_RESTORE_REASON`]), audited as a system change
/// in the same transaction. Returns how many rows it switched back on.
pub(crate) async fn restore_shipped_loto_isolation(db: &impl ConnectionTrait) -> AppResult<u64> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code FROM permit_types \
              WHERE (entity_sync_id = ? OR LOWER(TRIM(code)) = 'loto') \
                AND COALESCE(requires_energy_isolation, 0) = 0",
            [SHIPPED_LOTO_TYPE_SYNC_ID.into()],
        ))
        .await?;
    let mut restored = 0;
    for row in &rows {
        let id: i64 = row.try_get("", "id").map_err(|e| decode_err("id", e))?;
        let code: String = row.try_get("", "code").map_err(|e| decode_err("code", e))?;
        let at = now_ts();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_types SET requires_energy_isolation = 1, isolation_changed_by_id = NULL, \
             isolation_changed_at = ?, isolation_change_reason = ?, row_version = row_version + 1 \
             WHERE id = ? AND COALESCE(requires_energy_isolation, 0) = 0",
            [at.clone().into(), LOTO_ISOLATION_RESTORE_REASON.into(), id.into()],
        ))
        .await?;
        crate::audit::writer::write_audit_event_in(
            db,
            crate::audit::writer::AuditEventInput {
                action_code: "permit_type.energy_isolation.restored".to_string(),
                target_type: Some("permit_type".to_string()),
                target_id: Some(id.to_string()),
                actor_id: None,
                auth_context: "system".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                details_json: Some(serde_json::json!({
                    "summary": "Shipped LOTO permit type requires an energy isolation again",
                    "code": code,
                    "reason": LOTO_ISOLATION_RESTORE_REASON,
                    "origin": "migration_241",
                    "at": at,
                })),
            },
        )
        .await?;
        restored += 1;
    }
    Ok(restored)
}

/// Switching the energy-isolation rule off weakens the start gate: never for the shipped LOTO
/// type, otherwise only for an administrator with step-up and a reason of at least
/// [`crate::settings::catalog::MIN_REASON_CHARS`] characters, audited in the same transaction.
/// Returns the reason when the save switches it off (recorded on the row as who, when and why:
/// migration 240, re-checked on arrival). Switching it on is a plain edit.
async fn guard_isolation_relax(
    db: &impl ConnectionTrait,
    current: &PermitType,
    input: &PermitTypeUpsertInput,
    writer: PermitTypeWriter,
) -> AppResult<Option<String>> {
    if !relaxes_energy_isolation(current, input) {
        return Ok(None);
    }
    let min = crate::settings::catalog::MIN_REASON_CHARS;
    let params = std::collections::HashMap::from([
        ("type".to_string(), current.code.clone()),
        ("min".to_string(), min.to_string()),
    ]);
    if is_shipped_loto_type(&current.entity_sync_id, &current.code) {
        return Err(crate::errors::org_validation_failed(
            "PERMIT_TYPE_LOTO_ISOLATION_LOCKED",
            "The LOTO permit type always requires an energy isolation.",
            params,
        ));
    }
    if !writer.may_relax_isolation {
        return Err(crate::errors::org_validation_failed(
            "PERMIT_TYPE_ISOLATION_RELAX_FORBIDDEN",
            "Only an administrator (with step-up) can stop requiring an energy isolation.",
            params,
        ));
    }
    let reason = input
        .disable_isolation_reason
        .as_deref()
        .map(str::trim)
        .filter(|r| r.chars().count() >= min)
        .ok_or_else(|| {
            crate::errors::org_validation_failed(
                "PERMIT_TYPE_ISOLATION_RELAX_REASON",
                format!("Give the reason (at least {min} characters) for no longer requiring an energy isolation."),
                params.clone(),
            )
        })?;
    guard_change_clock(db, current, ISOLATION_CHANGE).await?;
    crate::audit::writer::write_audit_event_in(
        db,
        crate::audit::writer::AuditEventInput {
            action_code: "permit_type.energy_isolation.disabled".to_string(),
            target_type: Some("permit_type".to_string()),
            target_id: Some(current.id.to_string()),
            actor_id: Some(writer.actor_id),
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({ "code": current.code, "reason": reason })),
        },
    )
    .await?;
    Ok(Some(reason.to_string()))
}

/// The three-person lock-removal chain changes who may confirm that locks came off: changing it
/// (either way, or creating a type with it on) needs an administrator with step-up and a reason
/// of at least [`crate::settings::catalog::MIN_REASON_CHARS`] characters. Returns the reason
/// when the save changes the setting (audited once the row is written, `audit_three_person_change`).
fn guard_three_person_change(
    current: Option<&PermitType>,
    input: &PermitTypeUpsertInput,
    writer: PermitTypeWriter,
) -> AppResult<Option<String>> {
    if !changes_three_person_removal(current, input) {
        return Ok(None);
    }
    let type_label = current.map_or_else(|| input.code.trim().to_string(), |c| c.code.clone());
    let min = crate::settings::catalog::MIN_REASON_CHARS;
    let params =
        std::collections::HashMap::from([("type".to_string(), type_label), ("min".to_string(), min.to_string())]);
    if !writer.may_change_three_person {
        return Err(crate::errors::org_validation_failed(
            "PERMIT_TYPE_THREE_PERSON_FORBIDDEN",
            "Only an administrator (with step-up) can change the three-person lock-removal rule.",
            params,
        ));
    }
    let reason = input
        .three_person_change_reason
        .as_deref()
        .map(str::trim)
        .filter(|r| r.chars().count() >= min)
        .ok_or_else(|| {
            crate::errors::org_validation_failed(
                "PERMIT_TYPE_THREE_PERSON_REASON",
                format!("Give the reason (at least {min} characters) for changing the three-person lock-removal rule."),
                params,
            )
        })?;
    Ok(Some(reason.to_string()))
}

/// Audits a change of the three-person setting in the save's transaction.
async fn audit_three_person_change(
    db: &impl ConnectionTrait,
    saved: &PermitType,
    previous: Option<bool>,
    reason: String,
    writer: PermitTypeWriter,
) -> AppResult<()> {
    let action_code = if saved.requires_three_person_removal {
        "permit_type.three_person_removal.enabled"
    } else {
        "permit_type.three_person_removal.disabled"
    };
    crate::audit::writer::write_audit_event_in(
        db,
        crate::audit::writer::AuditEventInput {
            action_code: action_code.to_string(),
            target_type: Some("permit_type".to_string()),
            target_id: Some(saved.id.to_string()),
            actor_id: Some(writer.actor_id),
            auth_context: "step_up".to_string(),
            result: "success".to_string(),
            before_hash: None,
            after_hash: None,
            retention_class: "standard".to_string(),
            details_json: Some(serde_json::json!({
                "code": saved.code,
                "reason": reason,
                "before": previous,
                "after": saved.requires_three_person_removal,
            })),
        },
    )
    .await?;
    Ok(())
}

/// The recorded time of a change of one safety setting of a permit type: its column, refusal code
/// and the setting's name in the message.
struct ChangeClock {
    column: &'static str,
    code: &'static str,
    setting: &'static str,
}

const THREE_PERSON_CHANGE: ChangeClock = ChangeClock {
    column: "three_person_changed_at",
    code: "PERMIT_TYPE_THREE_PERSON_CLOCK_BEHIND",
    setting: "three-person lock-removal rule",
};

const ISOLATION_CHANGE: ChangeClock = ChangeClock {
    column: "isolation_changed_at",
    code: "PERMIT_TYPE_ISOLATION_CLOCK_BEHIND",
    setting: "energy-isolation rule",
};

/// A change of the three-person or energy-isolation setting is recorded at this PC's time, and
/// every other PC applies it only when that time is later than the change it holds
/// (`permit::sync_check::inbound_permit_type_check`). When the change already recorded here is not
/// earlier than now, this PC's clock is behind (or the type was already changed within this second):
/// the change would be refused everywhere else while shown as made here, so it is refused here,
/// naming the time to wait for (no time is made up). A recorded time that cannot be read is not
/// compared: the new record (readable) replaces it.
async fn guard_change_clock(db: &impl ConnectionTrait, current: &PermitType, clock: ChangeClock) -> AppResult<()> {
    let held: Option<String> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT {} AS at FROM permit_types WHERE id = ?", clock.column),
            [current.id.into()],
        ))
        .await?
        .map(|r| r.try_get::<Option<String>>("", "at"))
        .transpose()
        .map_err(|e| decode_err(clock.column, e))?
        .flatten();
    let Some(held) = held.as_deref().map(str::trim).filter(|h| !h.is_empty()) else {
        return Ok(());
    };
    let Some(last) = crate::settings::site::instant(held) else {
        return Ok(());
    };
    // `now_ts()` has whole seconds: the record written would be `now` truncated.
    let now = crate::settings::site::instant(&now_ts()).unwrap_or_else(Utc::now);
    if last >= now {
        return Err(crate::errors::org_validation_failed(
            clock.code,
            format!(
                "The {} of permit type {} was last changed at {held}, not earlier than this PC's time: check this PC's clock, then change it again.",
                clock.setting, current.code
            ),
            std::collections::HashMap::from([
                ("type".to_string(), current.code.clone()),
                ("last".to_string(), held.to_string()),
            ]),
        ));
    }
    Ok(())
}

/// The author, time and reason of a change of a safety setting (three-person: migration 238;
/// energy-isolation switch-off: migration 240); all `None` when the save does not change it.
struct ChangeTrace {
    by: Option<i64>,
    at: Option<String>,
    reason: Option<String>,
}

async fn upsert_permit_type_in(
    db: &impl ConnectionTrait,
    input: PermitTypeUpsertInput,
    writer: PermitTypeWriter,
) -> AppResult<PermitType> {
    // Before any field is moved out of `input`: the energy-isolation off-switch guard and the
    // three-person chain guard (a new type, or an existing one; an unknown id fails below).
    let existing = match input.id.filter(|id| *id > 0) {
        Some(id) => get_permit_type(db, id).await?,
        None => None,
    };
    let isolation_reason = match &existing {
        Some(current) => guard_isolation_relax(db, current, &input, writer).await?,
        None => None,
    };
    let three_person_reason = if input.id.is_none() || existing.is_some() {
        guard_three_person_change(existing.as_ref(), &input, writer)?
    } else {
        None
    };
    if let (Some(_), Some(current)) = (&three_person_reason, &existing) {
        guard_change_clock(db, current, THREE_PERSON_CHANGE).await?;
    }
    // Who changed the setting, when and why, written with the row (sent with it by sync and
    // re-checked on arrival: `permit::sync_check::inbound_permit_type_check`); kept otherwise.
    let traced = |reason: &Option<String>| ChangeTrace {
        by: reason.as_ref().map(|_| writer.actor_id),
        at: reason.as_ref().map(|_| now_ts()),
        reason: reason.clone(),
    };
    let trace = traced(&three_person_reason);
    let isolation_trace = traced(&isolation_reason);
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
        // A type's code is fixed once created: plant rules, permits and work orders name the type by
        // it (the stored code is kept as it is, spaces included).
        if input.code.trim() != current.code.trim() {
            return Err(crate::errors::org_validation_failed(
                CODE_PERMIT_TYPE_CODE_LOCKED,
                format!(
                    "The code of permit type {} cannot be changed (asked: {}).",
                    current.code,
                    input.code.trim()
                ),
                std::collections::HashMap::from([
                    ("type".to_string(), current.code.clone()),
                    ("code".to_string(), input.code.trim().to_string()),
                ]),
            ));
        }
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE permit_types SET code = ?, name = ?, description = ?, \
                 requires_hse_approval = ?, requires_operations_approval = ?, requires_atmospheric_test = ?, \
                 max_duration_hours = ?, mandatory_ppe_ids_json = ?, mandatory_control_rules_json = ?, \
                 requires_energy_isolation = ?, requires_three_person_removal = ?, \
                 three_person_changed_by_id = COALESCE(?, three_person_changed_by_id), \
                 three_person_changed_at = COALESCE(?, three_person_changed_at), \
                 three_person_change_reason = COALESCE(?, three_person_change_reason), \
                 isolation_changed_by_id = COALESCE(?, isolation_changed_by_id), \
                 isolation_changed_at = COALESCE(?, isolation_changed_at), \
                 isolation_change_reason = COALESCE(?, isolation_change_reason), row_version = ? \
                 WHERE id = ? AND row_version = ?",
                [
                    current.code.clone().into(),
                    input.name.into(),
                    desc.into(),
                    i64::from(input.requires_hse_approval).into(),
                    i64::from(input.requires_operations_approval).into(),
                    i64::from(input.requires_atmospheric_test).into(),
                    opt_f64(input.max_duration_hours),
                    ppe.into(),
                    rules.into(),
                    i64::from(
                        input
                            .requires_energy_isolation
                            .unwrap_or(current.requires_energy_isolation),
                    )
                    .into(),
                    i64::from(
                        input
                            .requires_three_person_removal
                            .unwrap_or(current.requires_three_person_removal),
                    )
                    .into(),
                    trace.by.into(),
                    trace.at.clone().into(),
                    trace.reason.clone().into(),
                    isolation_trace.by.into(),
                    isolation_trace.at.into(),
                    isolation_trace.reason.into(),
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
        if let Some(reason) = three_person_reason {
            audit_three_person_change(
                db,
                &updated,
                Some(current.requires_three_person_removal),
                reason,
                writer,
            )
            .await?;
        }
        return Ok(updated);
    }

    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_types (entity_sync_id, code, name, description, \
         requires_hse_approval, requires_operations_approval, requires_atmospheric_test, \
         max_duration_hours, mandatory_ppe_ids_json, mandatory_control_rules_json, \
         requires_energy_isolation, requires_three_person_removal, three_person_changed_by_id, \
         three_person_changed_at, three_person_change_reason, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
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
            i64::from(input.requires_energy_isolation.unwrap_or(false)).into(),
            i64::from(input.requires_three_person_removal.unwrap_or(false)).into(),
            trace.by.into(),
            trace.at.clone().into(),
            trace.reason.clone().into(),
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
    if let Some(reason) = three_person_reason {
        audit_three_person_change(db, &created, None, reason, writer).await?;
    }
    Ok(created)
}

/// Permits the reader's scope leaves out, as `(outside their sites, without a site)`, whatever the
/// user's filters (only the scope's binds): a count that followed the search or the filters would
/// let a site user probe other sites' permits. `(0, 0)` plant-wide. `shown` is the screen's own
/// fixed condition on `wp` (never a user filter), with its binds: only the permits the screen
/// would show are counted (`1=1` for the list, the open, non-expired ones for the open report).
async fn hidden_permit_counts(
    db: &impl ConnectionTrait,
    scope: &AccessScope,
    shown: &str,
    shown_binds: Vec<sea_orm::Value>,
) -> AppResult<(i64, i64)> {
    if scope.is_everywhere() {
        return Ok((0, 0));
    }
    let scope_sql = super::access::scope_sql(scope);
    let mut values = shown_binds;
    values.extend(scope_sql.binds);
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COALESCE(SUM(CASE WHEN {without} THEN 1 ELSE 0 END), 0) AS without_site, \
                        COALESCE(SUM(CASE WHEN {without} THEN 0 ELSE 1 END), 0) AS outside_scope \
                   FROM work_permits wp \
                  WHERE ({shown}) AND NOT COALESCE(({pred}), 0)",
                without = super::access::PERMIT_WITHOUT_SITE_SQL,
                pred = scope_sql.predicate,
            ),
            values,
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("permit hidden count query returned no rows")))?;
    Ok((
        row.try_get("", "outside_scope")
            .map_err(|e| decode_err("outside_scope", e))?,
        row.try_get("", "without_site")
            .map_err(|e| decode_err("without_site", e))?,
    ))
}

/// Every permit matching the filter (plant-wide: the internal callers and the tests).
pub async fn list_work_permits(
    db: &impl ConnectionTrait,
    filter: WorkPermitListFilter,
) -> AppResult<WorkPermitListPage> {
    list_work_permits_in_scope(db, filter, &AccessScope::Everywhere).await
}

/// [`list_work_permits`] limited to the reader's scope (where they hold `ptw.view`), in SQL before
/// the limit, with the count of the permits left out and why.
pub async fn list_work_permits_in_scope(
    db: &impl ConnectionTrait,
    filter: WorkPermitListFilter,
    scope: &AccessScope,
) -> AppResult<WorkPermitListPage> {
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
    if let Some(term) = filter.search.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let escaped = term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        sql.push_str(
            " AND (wp.code LIKE ? ESCAPE '\\' OR pt.name LIKE ? ESCAPE '\\' \
               OR e.asset_id_code LIKE ? ESCAPE '\\' OR e.name LIKE ? ESCAPE '\\' \
               OR wo.code LIKE ? ESCAPE '\\')",
        );
        for _ in 0..5 {
            params.push(pattern.clone().into());
        }
    }
    // Site scope (rule §2.5): last predicate, so its binds follow the filters' and precede the limit.
    let scope_sql = super::access::scope_sql(scope);
    sql.push_str(" AND (");
    sql.push_str(&scope_sql.predicate);
    sql.push(')');
    params.extend(scope_sql.binds);
    sql.push_str(" ORDER BY wp.id DESC LIMIT ?");
    params.push(limit.into());

    let stmt = Statement::from_sql_and_values(DbBackend::Sqlite, &sql, params);
    let rows = db.query_all(stmt).await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(map_work_permit(&row)?);
    }
    let (hidden_outside_scope, hidden_without_site) = hidden_permit_counts(db, scope, "1=1", Vec::new()).await?;
    Ok(WorkPermitListPage {
        items,
        hidden_outside_scope,
        hidden_without_site,
    })
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

/// A new permit with no equipment given and none on its linked work order (or no work order).
pub(crate) fn asset_required_error() -> AppError {
    AppError::ValidationFailed(vec![
        "An asset is required (the linked work order has no equipment).".into()
    ])
}

/// A permit whose equipment and work order give it no usable site (see [`derived_permit_node`]).
fn undetermined_site_error() -> AppError {
    AppError::ValidationFailed(vec![
        "The permit's organisational entity could not be determined from the work order or asset.".into(),
    ])
}

/// The org node a permit's site follows (CMMS Advisor decision A), one rule for the creation, an
/// edit and a permit received by sync (`permit::sync_check`): the linked work order's node when the
/// work order records one, else the equipment's installed node. `None` when that node is not usable
/// (a work order recording `0` or a negative node, equipment installed nowhere or on `0`): such a
/// permit is refused, it never falls back to another node. Equipment or a linked work order that
/// does not exist is not found.
pub(crate) async fn derived_permit_node(
    db: &impl ConnectionTrait,
    asset_id: i64,
    linked_work_order_id: Option<i64>,
) -> AppResult<Option<i64>> {
    let installed: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT installed_at_node_id FROM equipment WHERE id = ?",
            [asset_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Equipment".into(),
            id: asset_id.to_string(),
        })?
        .try_get("", "installed_at_node_id")
        .map_err(|e| decode_err("installed_at_node_id", e))?;
    if let Some(wo_id) = linked_work_order_id {
        let wo_node: Option<i64> = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT entity_id FROM work_orders WHERE id = ?",
                [wo_id.into()],
            ))
            .await?
            .ok_or_else(|| AppError::NotFound {
                entity: "WorkOrder".into(),
                id: wo_id.to_string(),
            })?
            .try_get("", "entity_id")
            .map_err(|e| decode_err("entity_id", e))?;
        if let Some(node) = wo_node {
            return Ok(Some(node).filter(|n| *n > 0));
        }
    }
    Ok(installed.filter(|n| *n > 0))
}

/// Whether `node` is an org node that exists here and is not deleted.
pub(crate) async fn live_org_node(db: &impl ConnectionTrait, node: i64) -> AppResult<bool> {
    Ok(db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM org_nodes WHERE id = ? AND deleted_at IS NULL",
            [node.into()],
        ))
        .await?
        .is_some())
}

/// Whether a permit whose site follows `derived` may be recorded on `node`: that node or one of its
/// parents (never a sibling, a node below it or another site). Both must exist here and not be
/// deleted: a deleted parent is no site for a permit, and a derived node missing here gives none.
/// (`auth::scope::ancestor_chain` follows parents whatever their status, as the rights need.)
pub(crate) async fn node_on_permit_chain(db: &impl ConnectionTrait, derived: i64, node: i64) -> AppResult<bool> {
    if !live_org_node(db, derived).await? || !live_org_node(db, node).await? {
        return Ok(false);
    }
    Ok(crate::auth::scope::ancestor_chain(db, derived).await?.contains(&node))
}

/// The node a permit on `asset_id` (and `linked_work_order_id`) is recorded on: `given` when it is
/// the derived node or one of its parents, the derived node when none is given. Refused when the
/// derived node is not usable (none, missing here or deleted, whether it is given or not), or when
/// `given` is off its chain.
async fn checked_permit_node(
    db: &impl ConnectionTrait,
    asset_id: i64,
    linked_work_order_id: Option<i64>,
    given: Option<i64>,
) -> AppResult<i64> {
    let derived = derived_permit_node(db, asset_id, linked_work_order_id)
        .await?
        .ok_or_else(undetermined_site_error)?;
    if !live_org_node(db, derived).await? {
        return Err(undetermined_site_error());
    }
    match given {
        None => Ok(derived),
        Some(node) if node_on_permit_chain(db, derived, node).await? => Ok(node),
        Some(node) => Err(refused(
            "PERMIT_SITE_NOT_EQUIPMENT_SITE",
            format!(
                "A permit is recorded on the site of its equipment or work order ({derived}) or a level above it, \
                 not on {node}."
            ),
            &[("org_node_id", node.to_string())],
        )),
    }
}

/// Resolves `(asset_id, entity_id)` for a new permit: the equipment given, else the linked work
/// order's; the node given when it is on the derived node's chain ([`checked_permit_node`]), else
/// the derived node. The scope check of the creation (`access::check_create_at`) applies the same
/// rule.
pub(crate) async fn resolve_permit_scope(
    db: &impl ConnectionTrait,
    input: &WorkPermitCreateInput,
) -> AppResult<(i64, i64)> {
    let mut asset_id = input.asset_id;
    if let Some(wo_id) = input.linked_work_order_id {
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
        let wo_equipment: Option<i64> = row
            .try_get("", "equipment_id")
            .map_err(|e| decode_err("equipment_id", e))?;
        asset_id = asset_id.or(wo_equipment);
    }
    let asset_id = asset_id.ok_or_else(asset_required_error)?;
    let entity_id = checked_permit_node(db, asset_id, input.linked_work_order_id, input.entity_id).await?;
    Ok((asset_id, entity_id))
}

/// `actor_id` is the session user: it is recorded as the person who wrote the permit (four-eyes:
/// they never approve it on a site that keeps four-eyes).
pub async fn create_work_permit(
    db: &DatabaseConnection,
    input: WorkPermitCreateInput,
    actor_id: i64,
) -> AppResult<WorkPermit> {
    let txn = db.begin().await?;
    let out = create_work_permit_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn create_work_permit_in(
    db: &impl ConnectionTrait,
    input: WorkPermitCreateInput,
    actor_id: i64,
) -> AppResult<WorkPermit> {
    // The type must exist (NotFound otherwise).
    let _ = permit_type_requires_energy_isolation(db, input.permit_type_id).await?;
    let (asset_id, entity_id) = resolve_permit_scope(db, &input).await?;
    ensure_wo_asset_aligned(db, input.linked_work_order_id, asset_id).await?;
    let expires_at = Some(checked_expiry(db, input.permit_type_id, input.expires_at.as_deref()).await?);

    if let Some(wo_id) = input.linked_work_order_id {
        if let Some(existing) = get_work_permit_linked_to_work_order(db, wo_id).await? {
            if !WP_TERMINAL_STATUSES.contains(&existing.status.as_str()) {
                return Err(refused(
                    "PERMIT_WO_ALREADY_HAS_OPEN",
                    format!("Work order already has an open permit ({}).", existing.code),
                    &[("permit", existing.code.clone())],
                ));
            }
        }
    }

    let entity_sync_id = Uuid::new_v4().to_string();
    let requested = now_ts();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO work_permits (entity_sync_id, code, linked_work_order_id, permit_type_id,          asset_id, entity_id, status, requested_at, expires_at, row_version, created_by_id)          VALUES (?, '', ?, ?, ?, ?, 'draft', ?, ?, 1, ?)",
        [
            entity_sync_id.into(),
            opt_i64(input.linked_work_order_id),
            input.permit_type_id.into(),
            asset_id.into(),
            entity_id.into(),
            requested.into(),
            opt_string(expires_at),
            actor_id.into(),
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

/// What an edit of a permit stores: its equipment, linked work order and org node, and whether
/// the equipment or linked work order, or the node, changes. The node is the one the edit gives;
/// without one, when the equipment or the linked work order changes, the node derived from them
/// ([`derived_permit_node`]; `None` when not usable), else the current one. The scope check
/// (`access::check_update`) and the save use this one rule, so the right is checked at the node
/// actually stored; the save refuses a target other than the one checked (the permit or its work
/// order changed in between).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermitEditTarget {
    pub asset_id: i64,
    pub linked_work_order_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub equipment_changes: bool,
    pub site_changes: bool,
}

pub(crate) async fn permit_edit_target(
    db: &impl ConnectionTrait,
    current: &WorkPermit,
    input: &WorkPermitUpdateInput,
) -> AppResult<PermitEditTarget> {
    let asset_id = input.asset_id.unwrap_or(current.asset_id);
    let linked = input.linked_work_order_id.or(current.linked_work_order_id);
    let equipment_changes = asset_id != current.asset_id || linked != current.linked_work_order_id;
    // Derived whenever the equipment or work order changes: equipment or a work order that does
    // not exist is not found, even when a node is given.
    let derived = if equipment_changes {
        derived_permit_node(db, asset_id, linked).await?
    } else {
        None
    };
    let entity_id = match input.entity_id {
        Some(given) => Some(given),
        None if equipment_changes => derived,
        None => Some(current.entity_id),
    };
    Ok(PermitEditTarget {
        asset_id,
        linked_work_order_id: linked,
        entity_id,
        equipment_changes,
        site_changes: entity_id != Some(current.entity_id),
    })
}

/// The rules of a change of a permit's equipment, linked work order or site (CMMS Advisor decision
/// A): only while the permit is a draft or waiting for review (from approved on, the permit was
/// approved for that equipment at that site); for an equipment or work-order change, never while a
/// lock fitted under it has not had its removal verified, nor while isolation points are planned
/// on it (they name the current equipment's points); its node is the derived one or a level above
/// it ([`checked_permit_node`]: never a sibling, a level below or another site). Returns that node.
async fn ensure_move_allowed(
    db: &impl ConnectionTrait,
    current: &WorkPermit,
    target: &PermitEditTarget,
) -> AppResult<i64> {
    let code = || ("permit", current.code.clone());
    if !WP_EQUIPMENT_CHANGE_STATUSES.contains(&current.status.as_str()) {
        return Err(if target.equipment_changes {
            refused(
                "PERMIT_EQUIPMENT_LOCKED",
                "This permit was approved for this equipment. Cancel it and raise a new permit.",
                &[code(), ("status", current.status.clone())],
            )
        } else {
            refused(
                "PERMIT_SITE_LOCKED",
                "This permit was approved at this site: its site can no longer change. Cancel it and raise a new permit.",
                &[code(), ("status", current.status.clone())],
            )
        });
    }
    if target.equipment_changes {
        let fitted =
            count_isolations_where(db, current.id, "applied_at IS NOT NULL AND removal_verified_at IS NULL").await?;
        if fitted > 0 {
            return Err(refused(
                "PERMIT_EQUIPMENT_LOCK_FITTED",
                format!(
                    "{fitted} lock(s) are fitted under this permit and their removal is not verified: its equipment cannot be changed."
                ),
                &[code(), ("locks", fitted.to_string())],
            ));
        }
        // A cancelled line no longer names a point to isolate (CMMS Advisor decision).
        let planned = count_isolations_where(db, current.id, "applied_at IS NULL AND cancelled_at IS NULL").await?;
        if planned > 0 {
            return Err(refused(
                "PERMIT_EQUIPMENT_ISOLATIONS_PLANNED",
                format!(
                    "{planned} isolation point(s) are planned on this permit for its current equipment: its equipment cannot be changed. Cancel the planned isolation lines, or cancel the permit."
                ),
                &[code(), ("isolations", planned.to_string())],
            ));
        }
    }
    let node = target.entity_id.ok_or_else(undetermined_site_error)?;
    checked_permit_node(db, target.asset_id, target.linked_work_order_id, Some(node)).await
}

/// Records a change of a permit's equipment, linked work order or site (who, when, old and new
/// equipment, work order and site), and, when it sent a permit waiting for review back to draft,
/// that reset, in the save's transaction.
async fn audit_equipment_change(
    db: &impl ConnectionTrait,
    before: &WorkPermit,
    after: &WorkPermit,
    actor_id: i64,
) -> AppResult<()> {
    let site_moved = before.entity_id != after.entity_id;
    let equipment_changed =
        before.asset_id != after.asset_id || before.linked_work_order_id != after.linked_work_order_id;
    let review_reset = before.status == "pending_review" && after.status == "draft";
    let event =
        |action_code: &str, details: serde_json::Value| permit_audit_event(action_code, after.id, actor_id, details);
    crate::audit::writer::write_audit_event_in(
        db,
        event(
            if site_moved {
                "permit.site_moved"
            } else {
                "permit.equipment_changed"
            },
            serde_json::json!({
                "code": after.code,
                "old_equipment_id": before.asset_id,
                "new_equipment_id": after.asset_id,
                "old_equipment_code": before.asset_code,
                "new_equipment_code": after.asset_code,
                "old_work_order_id": before.linked_work_order_id,
                "new_work_order_id": after.linked_work_order_id,
                "old_entity_id": before.entity_id,
                "new_entity_id": after.entity_id,
                "status_before": before.status,
                "status_after": after.status,
            }),
        ),
    )
    .await?;
    if review_reset {
        audit_review_reset(
            db,
            before,
            after,
            if equipment_changed {
                "equipment_changed"
            } else {
                "site_changed"
            },
            actor_id,
        )
        .await?;
    }
    Ok(())
}

/// Records that a change sent a permit waiting for review back to draft (`permit.review_reset`):
/// the review it waited for judged what changed, and the submission it cleared (who submitted it
/// and when: it must be submitted again). `reason`: `equipment_changed`, `site_changed` or
/// `isolation_cancelled`.
async fn audit_review_reset(
    db: &impl ConnectionTrait,
    before: &WorkPermit,
    after: &WorkPermit,
    reason: &str,
    actor_id: i64,
) -> AppResult<()> {
    crate::audit::writer::write_audit_event_in(
        db,
        permit_audit_event(
            "permit.review_reset",
            after.id,
            actor_id,
            serde_json::json!({
                "code": after.code,
                "reason": reason,
                "cleared_submitted_by_id": before.submitted_by_id,
                "cleared_submitted_at": before.submitted_at,
                "status_before": before.status,
                "status_after": after.status,
            }),
        ),
    )
    .await?;
    Ok(())
}

/// An audit event on a permit by the session user.
fn permit_audit_event(
    action_code: &str,
    permit_id: i64,
    actor_id: i64,
    details: serde_json::Value,
) -> crate::audit::writer::AuditEventInput {
    crate::audit::writer::AuditEventInput {
        action_code: action_code.to_string(),
        target_type: Some("work_permit".to_string()),
        target_id: Some(permit_id.to_string()),
        actor_id: Some(actor_id),
        auth_context: "session".to_string(),
        result: "success".to_string(),
        before_hash: None,
        after_hash: None,
        retention_class: "standard".to_string(),
        details_json: Some(details),
    }
}

/// `actor_id` is the session user: it is recorded as the person who changed the permit's
/// equipment, linked work order or site. Without a scope check before it: tests only, the command
/// uses [`update_work_permit_checked`].
#[cfg(test)]
pub(crate) async fn update_work_permit(
    db: &DatabaseConnection,
    input: WorkPermitUpdateInput,
    actor_id: i64,
) -> AppResult<WorkPermit> {
    let txn = db.begin().await?;
    let out = update_work_permit_in(&txn, input, actor_id, None).await?;
    txn.commit().await?;
    Ok(out)
}

/// The edit of a permit after the scope check (`access::require_update`): `checked` is the
/// target that check judged. The save computes the target again inside its transaction and
/// refuses the edit as a concurrent update when it differs (the permit, its work order or the
/// equipment changed between the check and the save), so the node stored is always the node
/// checked.
pub(crate) async fn update_work_permit_checked(
    db: &DatabaseConnection,
    input: WorkPermitUpdateInput,
    actor_id: i64,
    checked: &PermitEditTarget,
) -> AppResult<WorkPermit> {
    let txn = db.begin().await?;
    let out = update_work_permit_in(&txn, input, actor_id, Some(checked)).await?;
    txn.commit().await?;
    Ok(out)
}

async fn update_work_permit_in(
    db: &impl ConnectionTrait,
    input: WorkPermitUpdateInput,
    actor_id: i64,
    checked: Option<&PermitEditTarget>,
) -> AppResult<WorkPermit> {
    let current = require_work_permit(db, input.id).await?;
    if input.expected_row_version != current.row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on work_permits.".into(),
        ]));
    }
    if !WP_EDITABLE_STATUSES.contains(&current.status.as_str()) {
        return Err(refused(
            "PERMIT_NOT_EDITABLE",
            format!("Permit details cannot be edited in status '{}'.", current.status),
            &[("status", current.status.clone())],
        ));
    }
    let target = permit_edit_target(db, &current, &input).await?;
    if checked.is_some_and(|c| *c != target) {
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on work_permits.".into()
        ]));
    }
    let entity_id = if target.equipment_changes || target.site_changes {
        ensure_move_allowed(db, &current, &target).await?
    } else {
        current.entity_id
    };
    let asset_id = target.asset_id;
    let linked = target.linked_work_order_id;
    // Same rule as creation: a work order has at most one open permit.
    if let Some(wo_id) = linked.filter(|w| Some(*w) != current.linked_work_order_id) {
        let others = list_live_permits_linked_to_work_order(db, wo_id).await?;
        if let Some(existing) = others.iter().find(|p| p.id != current.id) {
            return Err(refused(
                "PERMIT_WO_ALREADY_HAS_OPEN",
                format!("Work order already has an open permit ({}).", existing.code),
                &[("permit", existing.code.clone())],
            ));
        }
    }
    // Every path (equipment only, work order only, both): the permit is on its work order's equipment.
    ensure_wo_asset_aligned(db, linked, asset_id).await?;
    // The permit keeps an expiry: a new one is checked like on creation; an update that leaves it
    // unchanged needs one already (permits created before the rule must be given one).
    let expires = match input.expires_at.as_deref() {
        Some(e) => Some(checked_expiry(db, current.permit_type_id, Some(e)).await?),
        None => Some(checked_expiry_present(&current)?),
    };
    // A change of equipment, work order or site while waiting for review sends the permit back to
    // draft: the review it waited for judged the former equipment at the former site, and who
    // issues it may change. Its submission is cleared with it (four-eyes): it is submitted again.
    let status = if (target.equipment_changes || target.site_changes) && current.status == "pending_review" {
        "draft".to_string()
    } else {
        current.status.clone()
    };
    let back_to_draft = current.status == "pending_review" && status == "draft";

    let new_rv = current.row_version + 1;
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_permits SET linked_work_order_id = ?, asset_id = ?, entity_id = ?, \
             expires_at = ?, status = ?, row_version = ? \
             WHERE id = ? AND row_version = ?",
            [
                opt_i64(linked),
                asset_id.into(),
                entity_id.into(),
                opt_string(expires),
                status.into(),
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
    if back_to_draft {
        clear_review_record(db, input.id).await?;
    }
    let updated = require_work_permit(db, input.id).await?;
    if target.equipment_changes || target.site_changes {
        audit_equipment_change(db, &current, &updated, actor_id).await?;
    }
    Ok(updated)
}

/// A permit back to draft is submitted again (four-eyes, migration 244): who submitted it and any
/// approval are cleared, in the transaction that sends it back.
async fn clear_review_record(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_permits SET submitted_by_id = NULL, submitted_at = NULL, approved_by_id = NULL, \
         approved_at = NULL, self_approved = NULL WHERE id = ? AND status = 'draft'",
        [permit_id.into()],
    ))
    .await?;
    Ok(())
}

/// Four-eyes or the site's opt-out for a permit at org node `entity_id`: the per-site rule shared
/// with reliability decisions (owner decision 2026-10-17: one opt-out per site,
/// `reliability::study::decisions::SELF_APPROVAL_SETTING`, `settings::site`), read at the
/// permit's planning site (`planning_site_of_node`). A node with no planning site: four-eyes.
pub(crate) async fn permit_four_eyes_rule(
    db: &impl ConnectionTrait,
    entity_id: i64,
) -> AppResult<crate::reliability::study::decisions::SelfApprovalRule> {
    use crate::reliability::study::decisions::{SelfApprovalRule, SELF_APPROVAL_SETTING};
    let Some(site) = crate::planning::weekly_schedule::planning_site_of_node(db, entity_id).await? else {
        return Ok(SelfApprovalRule::SiteUnresolved);
    };
    Ok(
        if crate::settings::site::site_setting(db, site, SELF_APPROVAL_SETTING).await? == Some(true) {
            SelfApprovalRule::Allowed
        } else {
            SelfApprovalRule::FourEyes
        },
    )
}

/// Whether `approver` is shown to be a second person: the permit records who wrote it and who
/// submitted it, and the approver is neither (`permit::sync_check` judges a received approval on
/// the same rule, by sync identity).
pub(crate) fn approver_is_second_person<T: PartialEq>(
    creator: Option<&T>,
    submitter: Option<&T>,
    approver: &T,
) -> bool {
    creator.is_some_and(|c| c != approver) && submitter.is_some_and(|s| s != approver)
}

/// The four-eyes check of an approval by `actor_id` (pending_review → approved). Returns whether
/// it is a self-approval (no verified second person), allowed only where the permit's site opted
/// out of four-eyes; elsewhere it is refused, saying why: the approver wrote the permit, submitted
/// it, or who did was not recorded (written or submitted before this version: nobody is invented,
/// so the separation cannot be shown).
async fn four_eyes_approval(db: &impl ConnectionTrait, permit: &WorkPermit, actor_id: i64) -> AppResult<bool> {
    use crate::reliability::study::decisions::SelfApprovalRule;
    let creator = permit.created_by_id;
    let submitter = permit.submitted_by_id;
    if approver_is_second_person(creator.as_ref(), submitter.as_ref(), &actor_id) {
        return Ok(false);
    }
    if permit_four_eyes_rule(db, permit.entity_id).await? == SelfApprovalRule::Allowed {
        return Ok(true);
    }
    let code = permit.code.clone();
    Err(if submitter == Some(actor_id) {
        refused(
            "PERMIT_APPROVER_IS_SUBMITTER",
            format!("Permit {code} was submitted for review by you: another person approves it (four-eyes)."),
            &[("permit", code.clone())],
        )
    } else if creator == Some(actor_id) {
        refused(
            "PERMIT_APPROVER_IS_CREATOR",
            format!("Permit {code} was written by you: another person approves it (four-eyes)."),
            &[("permit", code.clone())],
        )
    } else {
        refused(
            "PERMIT_APPROVAL_AUTHOR_UNKNOWN",
            format!(
                "Permit {code}: author not recorded (permit older than the update): four-eyes cannot be shown. \
                 Cancel it and raise a new permit."
            ),
            &[("permit", code.clone())],
        )
    })
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

    // Four-eyes (migration 244): who submitted and who approved, recorded with the status.
    let mut self_approved = None;
    match (from, to) {
        ("draft", "pending_review") => {
            if !draft_fields_complete(&current) {
                return Err(refused(
                    "PERMIT_DRAFT_INCOMPLETE",
                    "Draft permit must have a code and asset before review.",
                    &[],
                ));
            }
        }
        ("pending_review", "approved") => {
            self_approved = Some(four_eyes_approval(db, &current, actor_id).await?);
        }
        ("issued", "active") => {
            // No open-ended permit is activated (permits created before expiry was required).
            checked_expiry_present(&current)?;
            activation_ready(db, &current).await?;
        }
        ("revalidation_required", "active") => {
            // No open-ended permit is activated (permits created before expiry was required).
            checked_expiry_present(&current)?;
            activation_ready(db, &current).await?;
            finalize_open_suspension(db, input.id, actor_id, true).await?;
        }
        ("active", "closed") => {
            close_ready(db, input.id).await?;
        }
        ("closed", "handed_back") => {
            if count_handover_logs(db, input.id).await? < 1 {
                return Err(refused(
                    "PERMIT_HANDOVER_REQUIRED",
                    "At least one permit_handover_logs row is required before handed_back.",
                    &[],
                ));
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
    let mut submitted = (current.submitted_by_id, current.submitted_at.clone());
    let mut approved = (current.approved_by_id, current.approved_at.clone());
    match (from, to) {
        ("draft", "pending_review") => submitted = (Some(actor_id), Some(now.clone())),
        ("pending_review", "approved") => approved = (Some(actor_id), Some(now.clone())),
        _ => {}
    }
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
             handed_back_at = ?, submitted_by_id = ?, submitted_at = ?, approved_by_id = ?, approved_at = ?, \
             self_approved = COALESCE(?, self_approved), row_version = ? \
             WHERE id = ? AND row_version = ?",
            [
                input.status.into(),
                opt_string(issued_at),
                opt_string(activated_at),
                opt_string(closed_at),
                opt_string(handed_back_at),
                opt_i64(submitted.0),
                opt_string(submitted.1),
                opt_i64(approved.0),
                opt_string(approved.1),
                opt_i64(self_approved.map(i64::from)),
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
    audit_review_step(db, &current, &updated, actor_id).await?;
    Ok(updated)
}

/// Audits a submission for review (`permit.submitted`) and an approval (`permit.approved`, with
/// `self_approved` and the site's four-eyes rule: a self-approval on a site that opted out is
/// recorded as such), in the transaction of the status change.
async fn audit_review_step(
    db: &impl ConnectionTrait,
    before: &WorkPermit,
    after: &WorkPermit,
    actor_id: i64,
) -> AppResult<()> {
    let (action, details) = match (before.status.as_str(), after.status.as_str()) {
        ("draft", "pending_review") => (
            "permit.submitted",
            serde_json::json!({
                "code": after.code,
                "created_by_id": after.created_by_id,
                "submitted_at": after.submitted_at,
                "status_before": before.status,
                "status_after": after.status,
            }),
        ),
        ("pending_review", "approved") => (
            "permit.approved",
            serde_json::json!({
                "code": after.code,
                "created_by_id": after.created_by_id,
                "submitted_by_id": after.submitted_by_id,
                "submitted_at": after.submitted_at,
                "approved_at": after.approved_at,
                "self_approved": after.self_approved,
                "site_rule": if after.self_approved { "four_eyes_opted_out" } else { "four_eyes" },
                "status_before": before.status,
                "status_after": after.status,
            }),
        ),
        _ => return Ok(()),
    };
    crate::audit::writer::write_audit_event_in(db, permit_audit_event(action, after.id, actor_id, details)).await?;
    Ok(())
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
        return Err(refused(
            "PERMIT_SUSPEND_NOT_ACTIVE",
            "Only active permits can be suspended.",
            &[("status", current.status.clone())],
        ));
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

/// Record a hand-over of a closed permit, signed by `signer_id` (the session user — never taken
/// from the screen). Roles are values of `PERMIT.HANDOVER_ROLES`; the confirmation note has at
/// least `MIN_HANDOVER_NOTE_CHARS` characters; the receiving person, when named, is an active
/// user account other than the signer.
pub async fn append_permit_handover_log(
    db: &DatabaseConnection,
    input: PermitHandoverLogInput,
    signer_id: i64,
) -> AppResult<PermitHandoverLog> {
    let txn = db.begin().await?;
    let out = append_permit_handover_log_in(&txn, input, signer_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn append_permit_handover_log_in(
    db: &impl ConnectionTrait,
    input: PermitHandoverLogInput,
    signer_id: i64,
) -> AppResult<PermitHandoverLog> {
    let current = require_work_permit(db, input.permit_id).await?;
    if current.status != "closed" {
        return Err(refused(
            "PERMIT_HANDOVER_NOT_CLOSED",
            "Handover logs can only be recorded when permit status is closed.",
            &[],
        ));
    }
    if input.handed_from_role.trim().is_empty() || input.handed_to_role.trim().is_empty() {
        return Err(refused("PERMIT_HANDOVER_ROLES_REQUIRED", "Roles are required.", &[]));
    }
    let from_role = super::handover_roles::checked_role(db, "handed_from_role", &input.handed_from_role).await?;
    let to_role = super::handover_roles::checked_role(db, "handed_to_role", &input.handed_to_role).await?;
    let note = input.confirmation_note.trim().to_string();
    if note.chars().count() < super::domain::MIN_HANDOVER_NOTE_CHARS {
        return Err(refused(
            "PERMIT_HANDOVER_NOTE_TOO_SHORT",
            format!(
                "The confirmation note needs at least {} characters.",
                super::domain::MIN_HANDOVER_NOTE_CHARS
            ),
            &[("min", super::domain::MIN_HANDOVER_NOTE_CHARS.to_string())],
        ));
    }
    if let Some(receiver) = input.received_by_id {
        if receiver == signer_id {
            return Err(refused(
                "PERMIT_HANDOVER_RECEIVER_IS_SIGNER",
                "The receiving person cannot be the person signing the hand-over.",
                &[],
            ));
        }
        let active = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT 1 AS ok FROM user_accounts WHERE id = ? AND is_active = 1 AND deleted_at IS NULL",
                [receiver.into()],
            ))
            .await?
            .is_some();
        if !active {
            return Err(refused(
                "PERMIT_HANDOVER_RECEIVER_UNKNOWN",
                "The receiving person is not an active user account.",
                &[("userId", receiver.to_string())],
            ));
        }
        // CMMS Advisor decision (option A): the receiver holds the permit close right at the
        // permit's site — checked here, never left to the picker.
        if !super::access::may_receive_handover(db, input.permit_id, receiver).await? {
            return Err(refused(
                "PERMIT_HANDOVER_RECEIVER_NO_RIGHT",
                "The receiving person does not hold the permit close right at this permit's site.",
                &[("userId", receiver.to_string())],
            ));
        }
    }
    let signed = now_ts();
    let sync_id = Uuid::new_v4().to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO permit_handover_logs (entity_sync_id, permit_id, handed_from_role, handed_to_role, \
         confirmation_note, signed_at, signed_by_id, received_by_id, row_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            from_role.into(),
            to_role.into(),
            note.into(),
            signed.into(),
            signer_id.into(),
            opt_i64(input.received_by_id),
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
            format!("{} WHERE h.id = ?", handover_select()),
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

/// A person who may receive a permit hand-over: an active user account, by name only.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HandoverReceiver {
    pub id: i64,
    /// The account's display name (its login name only when no display name was ever given).
    pub display_name: String,
}

/// One page of hand-over receivers with the total that match, so a screen never shows a cut list
/// as complete.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct HandoverReceiverPage {
    pub items: Vec<HandoverReceiver>,
    pub total: i64,
    /// [`NO_RECEIVER_AT_SITE`] when nobody but the person asking holds the permit close right at
    /// the permit's site (whatever the search): the screen says so instead of an empty list.
    #[serde(default)]
    pub empty_reason: Option<String>,
}

/// Largest page of hand-over receivers.
pub const HANDOVER_RECEIVERS_PAGE_MAX: u64 = 200;

/// Nobody (other than the signer) holds `ptw.close` at the permit's site: no hand-over can be
/// received there (translated on screen, `errors:scope`).
pub const NO_RECEIVER_AT_SITE: &str = "PERMIT_HANDOVER_NO_RECEIVER_AT_SITE";

/// Active, non-deleted accounts that may receive a hand-over of the permit `permit_id` (CMMS
/// Advisor decision, option A: holding `ptw.close` at the permit's site, plant-wide holders
/// included — `permit::access::eligible_receiver_sql`), excluding the person asking (who signs
/// it), optionally filtered by name (case and accents ignored, no wildcards), ordered by name,
/// paged, with the total that match. The person asking holds `ptw.close` at the permit's site
/// (checked by the command, `permit::access`). Not found when the permit does not exist.
pub async fn list_handover_receivers(
    db: &impl ConnectionTrait,
    actor_id: i64,
    permit_id: i64,
    search: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
) -> AppResult<HandoverReceiverPage> {
    let anchor = super::access::permit_anchor(db, permit_id).await?;
    let (eligible, eligible_binds) = super::access::eligible_receiver_sql(db, anchor).await?;
    let name = handover_person_name_sql("ua");
    let mut values: Vec<sea_orm::Value> = vec![actor_id.into()];
    values.extend(eligible_binds.iter().cloned());
    // Every person eligible at the permit's site; the name filter is applied here rather than by
    // SQL LIKE: "%" and "_" are plain characters, and case and accents are ignored the way the
    // asset search ignores them (`assets::search::fold_search_text`).
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT ua.id, {name} AS display_name FROM user_accounts ua \
                 WHERE ua.is_active = 1 AND ua.deleted_at IS NULL AND ua.id <> ? AND {eligible}"
            ),
            values,
        ))
        .await?;
    // Nobody but the person asking is eligible at the permit's site, whatever the search.
    let empty_reason = rows.is_empty().then(|| NO_RECEIVER_AT_SITE.to_string());
    let wanted = search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(crate::assets::search::fold_search_text);
    let mut matching: Vec<(String, HandoverReceiver)> = Vec::with_capacity(rows.len());
    for r in &rows {
        let receiver = HandoverReceiver {
            id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
            display_name: r
                .try_get("", "display_name")
                .map_err(|e| decode_err("display_name", e))?,
        };
        let folded = crate::assets::search::fold_search_text(&receiver.display_name);
        if wanted.as_deref().is_none_or(|w| folded.contains(w)) {
            matching.push((folded, receiver));
        }
    }
    // By name (case and accents ignored), then by account.
    matching.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    let total = i64::try_from(matching.len()).unwrap_or(i64::MAX);
    let limit = limit
        .unwrap_or(HANDOVER_RECEIVERS_PAGE_MAX)
        .clamp(1, HANDOVER_RECEIVERS_PAGE_MAX);
    let items = matching
        .into_iter()
        .skip(usize::try_from(offset.unwrap_or(0)).unwrap_or(usize::MAX))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(|(_, receiver)| receiver)
        .collect();
    Ok(HandoverReceiverPage {
        items,
        total,
        empty_reason,
    })
}

pub async fn list_permit_handover_logs(db: &impl ConnectionTrait, permit_id: i64) -> AppResult<Vec<PermitHandoverLog>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("{} WHERE h.permit_id = ? ORDER BY h.id ASC", handover_select()),
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
        Err(refused(
            "PERMIT_ISOLATION_WRONG_STATUS",
            format!("Cannot {action} while the permit is '{}'.", wp.status),
            &[("status", wp.status.clone())],
        ))
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
    if point.is_empty() {
        return Err(refused(
            "PERMIT_ISOLATION_POINT_REQUIRED",
            "isolation_point is required.",
            &[],
        ));
    }
    // Links only (migration 225): the energy type from the SYSTEM list, the method from the
    // plant's list, both active. The texts typed before stay on migrated rows as history.
    let energy_id = crate::permit::energy_reference::require_active_value(
        db,
        crate::permit::energy_reference::ENERGY_TYPES_DOMAIN,
        "energy_type_id",
        input.energy_type_id,
    )
    .await?;
    let method_id = crate::permit::energy_reference::require_active_value(
        db,
        crate::permit::energy_reference::ISOLATION_METHODS_DOMAIN,
        "isolation_method_id",
        input.isolation_method_id,
    )
    .await?;
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
            return Err(refused(
                "PERMIT_ISOLATION_APPLIED_LOCKED",
                "An applied isolation cannot be edited.",
                &[],
            ));
        }
        refuse_if_cancelled(&current)?;
        let new_rv = current.row_version + 1;
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                // A person chose both values: the row no longer needs review. The original texts
                // are not touched (history).
                "UPDATE permit_isolations SET isolation_point = ?, energy_type_id = ?, \
                 isolation_method_id = ?, mapping_review_required = 0, lock_number = ?, row_version = ? \
                 WHERE id = ? AND row_version = ? AND applied_at IS NULL AND cancelled_at IS NULL",
                [
                    point.into(),
                    energy_id.into(),
                    method_id.into(),
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
        // `energy_type` / `isolation_method` are the pre-225 texts (NOT NULL): empty = recorded on
        // the lists, no original entry.
        "INSERT INTO permit_isolations (entity_sync_id, permit_id, isolation_point, energy_type, \
         isolation_method, energy_type_id, isolation_method_id, lock_number, row_version) \
         VALUES (?, ?, ?, '', '', ?, ?, ?, 1)",
        [
            sync_id.into(),
            input.permit_id.into(),
            point.into(),
            energy_id.into(),
            method_id.into(),
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

fn isolation_cancelled_error() -> AppError {
    refused(
        "PERMIT_ISOLATION_CANCELLED",
        "This isolation line is cancelled: it is kept as history only.",
        &[],
    )
}

/// A cancelled line is kept as history: it is never edited, applied, verified or cancelled again.
fn refuse_if_cancelled(iso: &PermitIsolation) -> AppResult<()> {
    if iso.cancelled_at.is_some() {
        return Err(isolation_cancelled_error());
    }
    Ok(())
}

/// Cancels a planned isolation line (CMMS Advisor decision): the line is marked cancelled by
/// `actor_id` (the session user) and kept, never deleted. Only a line never applied (no fitter, no
/// verification, no removal) on a permit that is a draft or waiting for review; an applied line
/// keeps the lock-removal path. In `pending_review` the permit goes back to `draft` (the review
/// judged the former lines). Audited in the same transaction (who, when, the line's content, and
/// the review reset). Returns the permit and the cancelled line. Without a scope check before it
/// (the command checks `ptw.issue` at the permit's site, `access::require_isolation_cancel`).
pub async fn cancel_permit_isolation(
    db: &DatabaseConnection,
    input: PermitIsolationCancelInput,
    actor_id: i64,
) -> AppResult<(WorkPermit, PermitIsolation)> {
    let txn = db.begin().await?;
    let out = cancel_permit_isolation_in(&txn, input, actor_id).await?;
    txn.commit().await?;
    Ok(out)
}

async fn cancel_permit_isolation_in(
    db: &impl ConnectionTrait,
    input: PermitIsolationCancelInput,
    actor_id: i64,
) -> AppResult<(WorkPermit, PermitIsolation)> {
    let iso = get_permit_isolation(db, input.isolation_id).await?;
    if iso.row_version != input.expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "row_version mismatch on permit_isolations.".into(),
        ]));
    }
    let permit = require_work_permit(db, iso.permit_id).await?;
    refuse_if_cancelled(&iso)?;
    let acted = iso.applied_at.is_some()
        || iso.applied_by_id.is_some()
        || iso.verified_at.is_some()
        || iso.verified_by_id.is_some()
        || iso.removal_verified_at.is_some()
        || iso.removal_verified_by_id.is_some();
    if acted {
        return Err(refused(
            "PERMIT_ISOLATION_CANCEL_APPLIED",
            "Only a planned isolation line never applied can be cancelled; a fitted lock is removed with a verified removal.",
            &[],
        ));
    }
    if !ISOLATION_CANCEL_STATUSES.contains(&permit.status.as_str()) {
        return Err(refused(
            "PERMIT_ISOLATION_CANCEL_WRONG_STATUS",
            format!(
                "Planned isolation lines can be cancelled while the permit is a draft or waiting for review; it is '{}'.",
                permit.status
            ),
            &[("status", permit.status.clone())],
        ));
    }
    let cancelled_at = now_ts();
    let affected = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE permit_isolations SET cancelled_at = ?, cancelled_by_id = ?, row_version = row_version + 1 \
             WHERE id = ? AND row_version = ? AND cancelled_at IS NULL AND applied_at IS NULL \
               AND verified_at IS NULL AND removal_verified_at IS NULL \
               AND EXISTS (SELECT 1 FROM work_permits WHERE id = ? AND status IN ('draft', 'pending_review'))",
            [
                cancelled_at.clone().into(),
                actor_id.into(),
                iso.id.into(),
                iso.row_version.into(),
                permit.id.into(),
            ],
        ))
        .await?;
    if affected.rows_affected() != 1 {
        return Err(AppError::ValidationFailed(vec![
            "Concurrent update on permit_isolations.".into(),
        ]));
    }
    // Waiting for review: back to draft, it must be submitted again.
    let after = if permit.status == "pending_review" {
        let affected = db
            .execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "UPDATE work_permits SET status = 'draft', row_version = row_version + 1 \
                 WHERE id = ? AND row_version = ? AND status = 'pending_review'",
                [permit.id.into(), permit.row_version.into()],
            ))
            .await?;
        if affected.rows_affected() != 1 {
            return Err(AppError::ValidationFailed(vec![
                "Concurrent update on work_permits.".into()
            ]));
        }
        clear_review_record(db, permit.id).await?;
        require_work_permit(db, permit.id).await?
    } else {
        permit.clone()
    };
    crate::audit::writer::write_audit_event_in(
        db,
        permit_audit_event(
            "permit.isolation_cancelled",
            permit.id,
            actor_id,
            serde_json::json!({
                "code": permit.code,
                "isolation_id": iso.id,
                "isolation_sync_id": iso.entity_sync_id,
                "isolation_point": iso.isolation_point,
                "energy_type_id": iso.energy_type_id,
                "energy_type_code": iso.energy_type_code,
                "energy_type_label": iso.energy_type_label,
                "isolation_method_id": iso.isolation_method_id,
                "isolation_method_label": iso.isolation_method_label,
                "original_energy_type": iso.energy_type,
                "original_isolation_method": iso.isolation_method,
                "lock_number": iso.lock_number,
                "mapping_review_required": iso.mapping_review_required,
                "cancelled_at": cancelled_at,
                "status_before": permit.status,
                "status_after": after.status,
            }),
        ),
    )
    .await?;
    if after.status != permit.status {
        audit_review_reset(db, &permit, &after, "isolation_cancelled", actor_id).await?;
    }
    let cancelled = get_permit_isolation(db, iso.id).await?;
    Ok((after, cancelled))
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
        return Err(refused(
            "PERMIT_REMOVAL_WORK_IN_PROGRESS",
            "Work on the linked work order is still in progress; pause or complete it before removing locks.",
            &[],
        ));
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
    refuse_if_cancelled(&iso)?;
    enforce_loto_role(db, actor_id, permit.id, "isolation_apply").await?;
    if iso.applied_at.is_some() {
        return Err(refused(
            "PERMIT_ISOLATION_ALREADY_APPLIED",
            "Isolation is already applied.",
            &[],
        ));
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
    refuse_if_cancelled(&iso)?;
    enforce_loto_role(db, actor_id, permit.id, "isolation_verify").await?;
    let Some(applied_by) = iso.applied_by_id.filter(|_| iso.applied_at.is_some()) else {
        return Err(refused(
            "PERMIT_ISOLATION_NOT_APPLIED",
            "Isolation must be applied before it can be verified.",
            &[],
        ));
    };
    if iso.verified_at.is_some() {
        return Err(AppError::ValidationFailed(
            vec!["Isolation is already verified.".into()],
        ));
    }
    if applied_by == actor_id {
        return Err(refused(
            "PERMIT_TWO_PERSON_VERIFY",
            "Two-person rule: the person who applied an isolation cannot verify it.",
            &[],
        ));
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
/// confirming removal must not be the person who applied the isolation. When the permit's type
/// requires the three-person chain (migration 236), not the isolation's verifier either (same
/// rule as received rows, `permit::sync_check::isolation_rule_breach`).
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
    refuse_if_cancelled(&iso)?;
    enforce_loto_role(db, actor_id, permit.id, "removal_verify").await?;
    if iso.verified_at.is_none() {
        return Err(refused(
            "PERMIT_REMOVAL_NOT_VERIFIABLE",
            "Only a verified isolation can have its removal verified.",
            &[],
        ));
    }
    if iso.removal_verified_at.is_some() {
        return Err(refused(
            "PERMIT_REMOVAL_ALREADY_VERIFIED",
            "Isolation removal is already verified.",
            &[],
        ));
    }
    if iso.applied_by_id == Some(actor_id) {
        return Err(refused(
            "PERMIT_TWO_PERSON_REMOVAL",
            "Two-person rule: the person who applied an isolation cannot verify its removal.",
            &[],
        ));
    }
    if permit.permit_type_requires_three_person_removal && iso.verified_by_id == Some(actor_id) {
        return Err(refused(
            "PERMIT_THREE_PERSON_REMOVAL",
            "Three-person rule: the person who verified an isolation cannot verify its removal.",
            &[],
        ));
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
             pi.isolation_point, COALESCE(et.label, pi.energy_type) AS energy_type, pi.lock_number, \
             pi.verified_by_id, pi.verified_at, pi.cancelled_at, \
             COALESCE(pi.forced_removal, 0) AS forced_removal, pi.removed_at, pi.forced_reason, \
             COALESCE(uf.display_name, uf.username) AS forced_authorized_by, \
             e.asset_id_code, e.name AS equipment_name, ua.username AS verifier_username \
             FROM permit_isolations pi \
             JOIN work_permits wp ON wp.id = pi.permit_id \
             LEFT JOIN reference_values et ON et.id = pi.energy_type_id \
             LEFT JOIN equipment e ON e.id = wp.asset_id \
             LEFT JOIN user_accounts ua ON ua.id = pi.verified_by_id \
             LEFT JOIN user_accounts uf ON uf.id = pi.forced_authorized_by_id \
             WHERE pi.id = ? AND pi.permit_id = ?",
            [isolation_id.into(), permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitIsolation".into(),
            id: isolation_id.to_string(),
        })?;
    // A cancelled line is no lock: it has no LOTO card.
    if row
        .try_get::<Option<String>>("", "cancelled_at")
        .ok()
        .flatten()
        .is_some()
    {
        return Err(isolation_cancelled_error());
    }
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
    // A lock taken off by a forced removal (applier absent): the card says so, with who authorised
    // it, when and why (migration 249).
    let forced_removal = i64_to_bool(
        row.try_get::<i64>("", "forced_removal")
            .map_err(|e| decode_err("forced_removal", e))?,
    );
    let forced_text = |column: &str| -> Option<String> {
        if forced_removal {
            row.try_get("", column).ok().flatten()
        } else {
            None
        }
    };
    Ok(LotoCardView {
        permit_code,
        equipment_label,
        energy_type,
        isolation_id: iso_id,
        isolation_point,
        lock_number,
        verifier_signature,
        expires_at,
        forced_removal,
        forced_authorized_by: forced_text("forced_authorized_by"),
        forced_removed_at: forced_text("removed_at"),
        forced_reason: forced_text("forced_reason"),
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
    // The isolation must belong to the permit named (not found otherwise) before anything is judged
    // on that permit: a mismatched permit id never yields that permit's qualification refusals.
    let iso = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT verified_at, cancelled_at FROM permit_isolations WHERE id = ? AND permit_id = ?",
            [input.isolation_id.into(), input.permit_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "PermitIsolation".into(),
            id: input.isolation_id.to_string(),
        })?;
    if iso
        .try_get::<Option<String>>("", "cancelled_at")
        .ok()
        .flatten()
        .is_some()
    {
        return Err(isolation_cancelled_error());
    }
    // The lock card is issued by someone authorised to apply the isolation.
    enforce_loto_role(db, actor_id, input.permit_id, "isolation_apply").await?;
    let verified_at: Option<String> = iso.try_get("", "verified_at").ok().flatten();
    if verified_at.is_none() {
        return Err(refused(
            "PERMIT_CARD_NOT_VERIFIED",
            "Isolation must be verified before LOTO card print.",
            &[],
        ));
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

/// Lockout points still fitted: isolations applied whose lock removal is not verified yet (the
/// close gate's condition), whatever the permit's status — a lock stays on the plant until its
/// removal is verified. Returns `(permits, points)`.
pub async fn isolation_locks_in_place(db: &impl ConnectionTrait) -> AppResult<(i64, i64)> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(DISTINCT pi.permit_id) AS permits, COUNT(*) AS points FROM permit_isolations pi \
             WHERE pi.applied_at IS NOT NULL AND pi.removal_verified_at IS NULL"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::SyncError("Failed to count isolation locks in place.".into()))?;
    Ok((
        row.try_get("", "permits").map_err(|e| decode_err("permits", e))?,
        row.try_get("", "points").map_err(|e| decode_err("points", e))?,
    ))
}

/// Open permits not expired, plant-wide (the home supervision count, scoped in a later stage).
pub async fn list_open_permits_report(db: &impl ConnectionTrait) -> AppResult<WorkPermitListPage> {
    list_open_permits_report_in_scope(db, &AccessScope::Everywhere).await
}

/// [`list_open_permits_report`] limited to the reader's scope (where they hold `ptw.view`; rule
/// §2.8: no plant-wide figures for a site-only user), with the permits left out and why.
pub async fn list_open_permits_report_in_scope(
    db: &impl ConnectionTrait,
    scope: &AccessScope,
) -> AppResult<WorkPermitListPage> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    // What the report shows (open, not expired): one condition for the rows and the hidden counts.
    let open_sql = format!("wp.status IN {OPEN_PERMIT_STATUSES_SQL} AND (wp.expires_at IS NULL OR wp.expires_at > ?)");
    let scope_sql = super::access::scope_sql(scope);
    let mut values: Vec<sea_orm::Value> = vec![now.clone().into()];
    values.extend(scope_sql.binds);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{WORK_PERMIT_SELECT} \
                 WHERE {open_sql} \
                 AND ({pred}) \
                 ORDER BY wp.id DESC",
                pred = scope_sql.predicate,
            ),
            values,
        ))
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        items.push(map_work_permit(&row)?);
    }
    let (hidden_outside_scope, hidden_without_site) =
        hidden_permit_counts(db, scope, &open_sql, vec![now.into()]).await?;
    Ok(WorkPermitListPage {
        items,
        hidden_outside_scope,
        hidden_without_site,
    })
}

/// The 30-day hand-back compliance, plant-wide (internal callers and tests).
pub async fn permit_compliance_kpi_30d(db: &impl ConnectionTrait) -> AppResult<PermitComplianceKpi30d> {
    permit_compliance_kpi_30d_in_scope(db, &AccessScope::Everywhere).await
}

/// [`permit_compliance_kpi_30d`] on the permits of the reader's scope only (rule §2.8).
pub async fn permit_compliance_kpi_30d_in_scope(
    db: &impl ConnectionTrait,
    scope: &AccessScope,
) -> AppResult<PermitComplianceKpi30d> {
    let cutoff = (Utc::now() - Duration::days(30))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let scope_sql = super::access::scope_sql(scope);
    let binds = || -> Vec<sea_orm::Value> {
        let mut v: Vec<sea_orm::Value> = vec![cutoff.clone().into()];
        v.extend(scope_sql.binds.iter().cloned());
        v
    };
    let activated_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS c FROM work_permits wp WHERE wp.activated_at IS NOT NULL \
                 AND wp.activated_at >= ? AND ({})",
                scope_sql.predicate
            ),
            binds(),
        ))
        .await?
        .expect("c");
    let activated_count: i64 = activated_row.try_get("", "c").map_err(|e| decode_err("c", e))?;
    let on_time_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT COUNT(*) AS c FROM work_permits wp WHERE wp.activated_at IS NOT NULL \
                 AND wp.activated_at >= ? \
                 AND wp.handed_back_at IS NOT NULL \
                 AND (wp.expires_at IS NULL OR wp.handed_back_at <= wp.expires_at) AND ({})",
                scope_sql.predicate
            ),
            binds(),
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
