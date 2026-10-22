//! Permit / LOTO requirement on a work order — no status change.
//!
//! `requires_permit` drives the start gate (`permit::wo_gate`): a WO that needs
//! a permit cannot go in progress without an active, activation-ready permit.
//! The flag can be set or cleared only before execution starts (draft, planning,
//! ready); once work is under way it is frozen so the gate cannot be bypassed.
//!
//! A flag a plant permit rule set when the work order was converted from a request
//! (`permit::requirement_rules`, `work_orders.permit_rule_code`), or the requirement carried from a
//! request flagged "sécurité" (`work_orders.permit_from_safety_di`, migration 255), is not cleared
//! by the general edit: [`clear_permit_rule_flag`] needs the permit-issue right (`ptw.issue`:
//! plant-wide for a rule, at the work order's site for a safety request) and a reason of 10
//! characters or more, and records who, when and why on the work order (audited).

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::Deserialize;

use crate::errors::{AppError, AppResult};
use crate::wo::domain::WorkOrder;
use crate::wo::queries;
use crate::wo::time::now_utc_z;
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::WoStatus;
use crate::wo::workflow::transition::{check_concurrency, load_wo_status};

#[derive(Debug, Clone, Deserialize)]
pub struct WoPermitRequirementInput {
    pub wo_id: i64,
    pub expected_row_version: i64,
    pub requires_permit: bool,
}

/// Statuses in which the permit requirement may still change.
pub fn permit_requirement_editable(status: &WoStatus) -> bool {
    matches!(status, WoStatus::Draft | WoStatus::Planning | WoStatus::Ready)
}

pub async fn set_permit_requirement(
    db: &DatabaseConnection,
    input: WoPermitRequirementInput,
    actor_id: i64,
) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    if !permit_requirement_editable(&status) {
        return Err(AppError::ValidationFailed(vec![
            "L'exigence de permis / consignation ne peut plus être modifiée une fois l'exécution commencée."
                .to_string(),
        ]));
    }
    if !input.requires_permit {
        assert_not_rule_flagged(&txn, input.wo_id).await?;
    }

    let now = now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                requires_permit = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                i64::from(input.requires_permit).into(),
                now.into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let payload = serde_json::json!({ "requires_permit": input.requires_permit }).to_string();
    emit_action_event(
        &txn,
        input.wo_id,
        "permit_requirement_changed",
        Some(actor_id),
        Some(&from_code),
        Some(&from_code),
        Some(&payload),
    )
    .await?;

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    txn.commit().await?;
    Ok(wo)
}

/// Stable codes of the rule flag.
pub const CODE_RULE_FLAG_NEEDS_CLEAR: &str = "WO_PERMIT_RULE_FLAG_NEEDS_CLEAR";
pub const CODE_RULE_FLAG_REASON: &str = "WO_PERMIT_RULE_CLEAR_REASON_REQUIRED";
pub const CODE_RULE_FLAG_ABSENT: &str = "WO_PERMIT_RULE_FLAG_ABSENT";
pub const CODE_RULE_FLAG_RIGHT: &str = "WO_PERMIT_RULE_CLEAR_RIGHT_REQUIRED";
/// The permit requirement came from a request flagged "sécurité" (migration 255): the general edit
/// may not clear it either (audit finding AU-01, CMMS Advisor decision D1).
pub const CODE_SAFETY_FLAG_NEEDS_CLEAR: &str = "WO_PERMIT_SAFETY_FLAG_NEEDS_CLEAR";

/// Shortest reason accepted to clear a rule flag (the audit trail's "why").
pub const MIN_CLEAR_REASON_CHARS: usize = 10;

/// A permit requirement in force that the general edit may not clear (on `work_orders`, no alias):
/// set by a plant rule or carried from a request flagged "sécurité", and not cleared.
pub const PROTECTED_REQUIREMENT_SQL: &str = "requires_permit = 1 AND permit_rule_cleared_at IS NULL \
     AND (permit_rule_code IS NOT NULL OR permit_from_safety_di = 1)";

/// Where a protected permit requirement came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectedOrigin {
    /// A plant rule (`PERMIT.REQUIREMENT_RULES`) matched at conversion. When the request was also
    /// flagged "sécurité", the rule's stricter clearing applies.
    Rule(String),
    /// The request it was converted from was flagged "sécurité" (no rule matched).
    SafetyRequest,
}

impl ProtectedOrigin {
    /// The origin as recorded in the action history and the audit trail.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rule(_) => "rule",
            Self::SafetyRequest => "safety_request",
        }
    }
}

/// The protected requirement of a work order, with its site (`entity_id`); `None` when it has none.
async fn protected_requirement(
    conn: &impl ConnectionTrait,
    wo_id: i64,
) -> AppResult<Option<(ProtectedOrigin, Option<i64>)>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT permit_rule_code, entity_id FROM work_orders WHERE id = ? AND {PROTECTED_REQUIREMENT_SQL}"),
            [wo_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let rule: Option<String> = row.try_get("", "permit_rule_code")?;
    let site: Option<i64> = row.try_get("", "entity_id")?;
    let origin = rule.map_or(ProtectedOrigin::SafetyRequest, ProtectedOrigin::Rule);
    Ok(Some((origin, site)))
}

/// The work order's permit requirement was set by a plant rule, or carried from a request flagged
/// "sécurité", and has not been cleared: the general edit may not clear it.
pub async fn assert_not_rule_flagged(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    match protected_requirement(conn, wo_id).await? {
        None => Ok(()),
        Some((ProtectedOrigin::Rule(rule), _)) => Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_NEEDS_CLEAR,
            format!(
                "The permit requirement was set by the plant rule '{rule}': clearing it needs the \
                 permit-issue right and a reason."
            ),
            crate::errors::issue_params(&[("rule", rule)]),
        )),
        Some((ProtectedOrigin::SafetyRequest, _)) => Err(crate::errors::org_validation_failed(
            CODE_SAFETY_FLAG_NEEDS_CLEAR,
            "The permit requirement comes from a request flagged as a safety concern: clearing it needs the \
             permit-issue right at the work order's site and a reason."
                .to_string(),
            crate::errors::issue_params(&[("origin", ProtectedOrigin::SafetyRequest.as_str().to_string())]),
        )),
    }
}

/// Whether `user_id` may clear a protected permit requirement of `origin` on a work order at `site`
/// (its `entity_id`; `None`, `0` or negative: no site).
///
/// - A plant rule: the permit-issue right plant-wide ([`holds_tenant_right`]), as before (a site
///   role does not lift a plant rule).
/// - A request flagged "sécurité" (CMMS Advisor decision D1): the permit-issue right at the work
///   order's site, that is plant-wide, or through a permanent role on the site's node or one of its
///   parents (live node, strict node reference: the subtree rule of `auth::scope`). A work order
///   without a site: plant-wide only.
///
/// Emergency grants never count (as in [`holds_tenant_right`]). Used by the clear action and, on
/// arrival, by `permit::sync_check::inbound_permit_rule_check` (the same rule on every PC).
///
/// [`holds_tenant_right`]: crate::settings::site::holds_tenant_right
pub(crate) async fn may_clear_protected_requirement(
    conn: &impl ConnectionTrait,
    user_id: i64,
    origin: &ProtectedOrigin,
    site: Option<i64>,
) -> AppResult<bool> {
    use crate::rbac::permissions::PTW_ISSUE;
    if crate::settings::site::holds_tenant_right(conn, user_id, PTW_ISSUE).await? {
        return Ok(true);
    }
    let (ProtectedOrigin::SafetyRequest, Some(node)) = (origin, site.filter(|n| *n > 0)) else {
        return Ok(false);
    };
    let chain = crate::auth::scope::ancestor_chain(conn, node).await?;
    if chain.is_empty() {
        return Ok(false);
    }
    let now = crate::rbac::resolver::assignment_clock();
    let marks = vec!["?"; chain.len()].join(",");
    let mut values: Vec<sea_orm::Value> = vec![user_id.into(), PTW_ISSUE.into()];
    values.extend(crate::rbac::resolver::permanent_binds(&now));
    values.extend(chain.into_iter().map(sea_orm::Value::from));
    Ok(conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "SELECT 1 AS ok FROM user_accounts ua \
                  WHERE ua.id = ? AND ua.is_active = 1 AND ua.deleted_at IS NULL AND EXISTS ( \
                    SELECT 1 FROM user_scope_assignments usa \
                      JOIN role_permissions rp ON rp.role_id = usa.role_id \
                      JOIN permissions p ON p.id = rp.permission_id \
                     WHERE usa.user_id = ua.id AND p.name = ? AND {permanent} \
                       AND {live} AND {node_id} IN ({marks}))",
                permanent = crate::rbac::resolver::PERMANENT_ASSIGNMENT_IN_FORCE_SQL,
                live = crate::auth::scope::LIVE_NODE_ASSIGNMENT_SQL,
                node_id = crate::auth::scope::NODE_REFERENCE_ID_SQL,
            ),
            values,
        ))
        .await?
        .is_some())
}

#[derive(Debug, Clone, Deserialize)]
pub struct WoPermitRuleClearInput {
    pub wo_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
}

/// Clear a protected permit requirement: the flag a plant rule set at conversion, or the requirement
/// carried from a request flagged "sécurité" (migration 255). The person clearing it holds the
/// permit-issue right (`ptw.issue`): plant-wide for a rule, at the work order's site for a safety
/// request ([`may_clear_protected_requirement`]) — checked here as well as by the command, so
/// converting or editing work orders is never enough. Allowed before execution starts, like any
/// permit requirement change; records who, when and why on the work order and in its action history.
pub async fn clear_permit_rule_flag(
    db: &DatabaseConnection,
    input: WoPermitRuleClearInput,
    actor_id: i64,
) -> AppResult<WorkOrder> {
    let reason = input.reason.trim().to_string();
    if reason.chars().count() < MIN_CLEAR_REASON_CHARS {
        return Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_REASON,
            format!("Give a reason of at least {MIN_CLEAR_REASON_CHARS} characters."),
            crate::errors::issue_params(&[("min", MIN_CLEAR_REASON_CHARS.to_string())]),
        ));
    }
    let txn = db.begin().await?;
    let (from_code, status, _) = load_wo_status(&txn, input.wo_id).await?;
    let Some((origin, site)) = protected_requirement(&txn, input.wo_id).await? else {
        return Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_ABSENT,
            "This work order carries no permit requirement set by a plant rule or a safety request.".to_string(),
            std::collections::HashMap::new(),
        ));
    };
    if !may_clear_protected_requirement(&txn, actor_id, &origin, site).await? {
        let message = match origin {
            ProtectedOrigin::Rule(_) => {
                "Clearing a permit requirement set by a plant rule needs the permit-issue right."
            }
            ProtectedOrigin::SafetyRequest => {
                "Clearing a permit requirement from a safety request needs the permit-issue right at the work \
                 order's site."
            }
        };
        return Err(crate::errors::org_validation_failed(
            CODE_RULE_FLAG_RIGHT,
            message.to_string(),
            crate::errors::issue_params(&[("origin", origin.as_str().to_string())]),
        ));
    }
    if !permit_requirement_editable(&status) {
        return Err(AppError::ValidationFailed(vec![
            "L'exigence de permis / consignation ne peut plus être modifiée une fois l'exécution commencée."
                .to_string(),
        ]));
    }
    let rule = match &origin {
        ProtectedOrigin::Rule(rule) => Some(rule.clone()),
        ProtectedOrigin::SafetyRequest => None,
    };

    let now = now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET \
                requires_permit = 0, \
                permit_rule_cleared_by_id = ?, \
                permit_rule_cleared_at = ?, \
                permit_rule_clear_reason = ?, \
                row_version = row_version + 1, \
                updated_at = ? \
             WHERE id = ? AND row_version = ?",
            [
                actor_id.into(),
                now.clone().into(),
                reason.clone().into(),
                now.into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;

    let payload = serde_json::json!({
        "requires_permit": false,
        "rule": rule,
        "origin": origin.as_str(),
        "reason": reason,
    })
    .to_string();
    emit_action_event(
        &txn,
        input.wo_id,
        "permit_rule_flag_cleared",
        Some(actor_id),
        Some(&from_code),
        Some(&from_code),
        Some(&payload),
    )
    .await?;

    let wo = queries::get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    txn.commit().await?;
    Ok(wo)
}
