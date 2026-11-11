//! After-the-fact validation of a dépannage (CMMS Advisor decision, 2026-11-04; migration 277).
//!
//! A dépannage is started without the planning approval (`wo::depannage`): the four-eyes check
//! moves after the fact, and it never blocks the work.
//! - The work order shows "dépannage — à valider" until it is validated.
//! - It is validated ([`validate_depannage`]) by a holder of `ot.approve` at the work order's site
//!   (checked by the command, `wo::access::require_on_wo`) who is not the person who started it
//!   (`requester_id`, the user who launched the dépannage). Who and when are stored
//!   (`validated_by_id`, `validated_at`) and audited; a validation is never removed or rewritten.
//! - The close of a dépannage not validated is refused ([`CODE_NOT_VALIDATED`],
//!   `wo::closeout::close_preflight`), unless its site opted out of the validation
//!   ([`AUTO_VALIDATION_SETTING`], a site rule, default off): the close then marks it approved
//!   automatically (`validation_auto_approved`), with no validator — a flag, never a fake approval.
//!
//! Received rows (`sync::apply_hooks`, [`inbound_validation_check`]): a validation names a person
//! here who holds `ot.approve` at the work order's node and is not its starter, at a readable time
//! not in the future; a validation or automatic approval recorded here is never removed or
//! rewritten; a received close is judged by the close checks (`wo::closeout_sync`), so an automatic
//! approval holds only where the site opted out here.
//!
//! The times of a dépannage (EN 13306, decision of 2026-11-04) are [`depannage_times`].

use chrono::Duration;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::errors::{AppError, AppResult};
use crate::settings::site::{InboundVerdict, MAX_CLOCK_SKEW_MINUTES};
use crate::wo::depannage::CODE_NOT_A_DEPANNAGE;
use crate::wo::domain::WorkOrder;
use crate::wo::queries::get_work_order;
use crate::wo::workflow::transition::{check_concurrency, load_wo_status};

/// Site rule: dépannages of the site are approved automatically at their close (no validator).
pub const AUTO_VALIDATION_SETTING: &str = "wo.depannage.auto_validation";

/// Refusal codes.
pub const CODE_NOT_VALIDATED: &str = "GATE_WO_DEPANNAGE_NOT_VALIDATED";
pub const CODE_ALREADY_VALIDATED: &str = "GATE_WO_DEPANNAGE_ALREADY_VALIDATED";
pub const CODE_VALIDATOR_IS_STARTER: &str = "GATE_WO_DEPANNAGE_VALIDATOR_IS_STARTER";
pub const CODE_VALIDATION_TERMINAL: &str = "GATE_WO_DEPANNAGE_VALIDATION_TERMINAL";

/// Sync conflict code of a received validation that does not hold here.
pub const CODE_SYNC_VALIDATION_INVALID: &str = "WO_DEPANNAGE_VALIDATION_INVALID";

#[derive(Debug, Clone, Deserialize)]
pub struct WoDepannageValidateInput {
    pub wo_id: i64,
    pub expected_row_version: i64,
    /// The signed-in user (set by the command).
    #[serde(default)]
    pub actor_id: i64,
}

/// Validates a dépannage after the fact. The command checks `ot.approve` at the work order's site.
pub async fn validate_depannage(db: &DatabaseConnection, input: WoDepannageValidateInput) -> AppResult<WorkOrder> {
    let txn = db.begin().await?;
    let (_, status, _) = load_wo_status(&txn, input.wo_id).await?;
    let current = get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    if !current.is_depannage {
        return Err(AppError::ValidationFailed(vec![format!(
            "{CODE_NOT_A_DEPANNAGE}: Cet OT n'est pas un dépannage."
        )]));
    }
    if current.validated_by_id.is_some() || current.validation_auto_approved == Some(true) {
        return Err(AppError::ValidationFailed(vec![format!(
            "{CODE_ALREADY_VALIDATED}: Ce dépannage est déjà validé."
        )]));
    }
    if status.is_terminal() {
        return Err(AppError::ValidationFailed(vec![format!(
            "{CODE_VALIDATION_TERMINAL}:status={}: Un dépannage clôturé ou annulé ne se valide plus.",
            crate::wo::workflow::state_machine::status_i18n_key(status.as_str())
        )]));
    }
    if current.requester_id == Some(input.actor_id) {
        return Err(AppError::ValidationFailed(vec![format!(
            "{CODE_VALIDATOR_IS_STARTER}: La personne qui a lancé le dépannage ne peut pas le valider."
        )]));
    }
    let now = crate::wo::time::now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET validated_by_id = ?, validated_at = ?, row_version = row_version + 1, \
                    updated_at = ? \
              WHERE id = ? AND row_version = ? AND validated_by_id IS NULL",
            [
                input.actor_id.into(),
                now.clone().into(),
                now.into(),
                input.wo_id.into(),
                input.expected_row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(result.rows_affected())?;
    let wo = get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    txn.commit().await?;
    Ok(wo)
}

/// What the close of a work order needs from the dépannage validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CloseValidation {
    /// Not a dépannage: nothing to check.
    NotDepannage,
    /// Validated by a person.
    Validated,
    /// Not validated, and its site opted out: the close approves it automatically.
    AutoApproved,
    /// Not validated, and validation is required: the close is refused with this message.
    Refused(String),
}

/// The validation state of a work order at its close (local close and received close alike).
pub(crate) async fn close_validation(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<CloseValidation> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_depannage, validated_by_id, entity_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let read = |e: sea_orm::DbErr| AppError::Internal(anyhow::anyhow!("work_orders validation: {e}"));
    let is_depannage = row
        .try_get::<Option<i64>>("", "is_depannage")
        .map_err(read)?
        .unwrap_or(0)
        != 0;
    if !is_depannage {
        return Ok(CloseValidation::NotDepannage);
    }
    let validator: Option<i64> = row.try_get("", "validated_by_id").map_err(read)?;
    if validator.is_some() {
        return Ok(CloseValidation::Validated);
    }
    let node: Option<i64> = row.try_get("", "entity_id").map_err(read)?;
    if auto_validation_at_node(conn, node).await? {
        return Ok(CloseValidation::AutoApproved);
    }
    Ok(CloseValidation::Refused(format!(
        "{CODE_NOT_VALIDATED}: Clôture bloquée : ce dépannage doit d'abord être validé par un \
         approbateur du site (autre que la personne qui l'a lancé). (Dépannage not validated.)"
    )))
}

/// Whether the site of a work order's node opted out of the dépannage validation.
async fn auto_validation_at_node(conn: &impl ConnectionTrait, node: Option<i64>) -> AppResult<bool> {
    let Some(node) = node.filter(|n| *n > 0) else {
        return Ok(false);
    };
    let Some(site) = crate::planning::weekly_schedule::planning_site_of_node(conn, node).await? else {
        return Ok(false);
    };
    Ok(crate::settings::site::site_setting(conn, site, AUTO_VALIDATION_SETTING).await? == Some(true))
}

/// Marks a dépannage approved automatically, in the close's transaction (no validator recorded).
pub(crate) async fn mark_auto_approved(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE work_orders SET validation_auto_approved = 1 \
          WHERE id = ? AND is_depannage = 1 AND validated_by_id IS NULL",
        [wo_id.into()],
    ))
    .await?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Times (EN 13306)
// ═══════════════════════════════════════════════════════════════════════════════

/// The times of a dépannage, in hours (CMMS Advisor decision 2026-11-04), each `None` while one of
/// its two ends is not known:
/// - `downtime_start`: the breakdown when the machine was stopped, else the later machine stop
///   recorded (the first downtime segment), else none;
/// - `response_hours`: breakdown → actual start (includes any wait for a permit);
/// - `active_repair_hours`: actual start → actual end;
/// - `restore_hours`: breakdown → return to service (the verification confirming it, else the
///   close);
/// - `stopped_hours`: when the machine was stopped at the breakdown, breakdown → return to service
///   (else the downtime segments recorded tell it).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DepannageTimes {
    pub breakdown_at: Option<String>,
    pub downtime_start: Option<String>,
    pub restored_at: Option<String>,
    pub response_hours: Option<f64>,
    pub active_repair_hours: Option<f64>,
    pub restore_hours: Option<f64>,
    pub stopped_hours: Option<f64>,
}

pub(crate) fn hours_between(from: Option<&str>, to: Option<&str>) -> Option<f64> {
    let a = from.and_then(crate::settings::site::instant)?;
    let b = to.and_then(crate::settings::site::instant)?;
    let h = (b - a).num_seconds() as f64 / 3600.0;
    Some((h.max(0.0) * 100.0).round() / 100.0)
}

/// [`DepannageTimes`] of a work order (`None`: not a dépannage).
pub async fn depannage_times(conn: &impl ConnectionTrait, wo_id: i64) -> AppResult<Option<DepannageTimes>> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT wo.is_depannage, wo.breakdown_at, wo.machine_stopped, wo.actual_start, wo.actual_end, \
                    wo.closed_at, \
                    (SELECT MIN(started_at) FROM work_order_downtime_segments WHERE work_order_id = wo.id) \
                        AS first_stop, \
                    (SELECT verified_at FROM work_order_verifications \
                      WHERE work_order_id = wo.id AND result = 'pass' AND return_to_service_confirmed = 1 \
                      ORDER BY verified_at DESC LIMIT 1) AS returned_at \
               FROM work_orders wo WHERE wo.id = ?",
            [wo_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
    let read = |e: sea_orm::DbErr| AppError::Internal(anyhow::anyhow!("depannage times: {e}"));
    let text = |col: &str| -> AppResult<Option<String>> { row.try_get::<Option<String>>("", col).map_err(read) };
    if row
        .try_get::<Option<i64>>("", "is_depannage")
        .map_err(read)?
        .unwrap_or(0)
        == 0
    {
        return Ok(None);
    }
    let breakdown = text("breakdown_at")?;
    let stopped = row.try_get::<Option<i64>>("", "machine_stopped").map_err(read)? == Some(1);
    let downtime_start = if stopped {
        breakdown.clone()
    } else {
        text("first_stop")?
    };
    let start = text("actual_start")?;
    let end = text("actual_end")?;
    let restored_at = match text("returned_at")? {
        Some(t) => Some(t),
        None => text("closed_at")?,
    };
    Ok(Some(DepannageTimes {
        response_hours: hours_between(breakdown.as_deref(), start.as_deref()),
        active_repair_hours: hours_between(start.as_deref(), end.as_deref()),
        restore_hours: hours_between(breakdown.as_deref(), restored_at.as_deref()),
        stopped_hours: if stopped {
            hours_between(breakdown.as_deref(), restored_at.as_deref())
        } else {
            None
        },
        breakdown_at: breakdown,
        downtime_start,
        restored_at,
    }))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Synchronisation
// ═══════════════════════════════════════════════════════════════════════════════

type Payload = crate::permit::sync_check::Payload;

fn payload_flag(p: &Payload, key: &str) -> bool {
    match p.get(key) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_i64().is_some_and(|v| v != 0),
        _ => false,
    }
}

fn payload_text(p: &Payload, key: &str) -> Option<String> {
    crate::permit::sync_check::field(p, key).map(str::to_string)
}

/// The work order's org node here (`entity_id`, sent as `entity_sync_id`): `Ok(None)` when it has
/// none (plant-wide holders only), `Err(verdict)` while the node is not applied here.
async fn payload_node(
    conn: &impl ConnectionTrait,
    code: &str,
    wo: &Payload,
) -> AppResult<Result<Option<i64>, InboundVerdict>> {
    let Some(node_sync) = payload_text(wo, "entity_sync_id") else {
        return Ok(Ok(None));
    };
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE entity_sync_id = ?",
            [node_sync.clone().into()],
        ))
        .await?;
    match row {
        None => Ok(Err(InboundVerdict::Defer(format!(
            "dépannage {code}: its site {node_sync} is not applied here yet"
        )))),
        Some(r) => {
            let id = r
                .try_get::<i64>("", "id")
                .map_err(|e| AppError::SyncError(format!("dépannage validation re-check: {e}")))?;
            Ok(Ok(Some(id)))
        }
    }
}

/// The status code of the received work order: `Err(verdict)` while its status is not here.
async fn payload_status(
    conn: &impl ConnectionTrait,
    code: &str,
    wo: &Payload,
) -> AppResult<Result<Option<String>, InboundVerdict>> {
    let Some(status_sync) = payload_text(wo, "status_sync_id") else {
        return Ok(Ok(None));
    };
    match crate::permit::sync_check::status_code(conn, &status_sync).await? {
        None => Ok(Err(InboundVerdict::Defer(format!(
            "dépannage {code}: its status {status_sync} is not applied here yet"
        )))),
        Some(s) => Ok(Ok(Some(s))),
    }
}

/// Re-check of the validation of a received dépannage (see the module documentation). Called by
/// `wo::depannage::inbound_depannage_check` once the row is known to be a dépannage; `wo` is the
/// received row completed by the local one.
/// - Its starter (`requester_id`) never changes once here: the four-eyes rule is judged against
///   the starter recorded here, never one named by the received row.
/// - A validation or automatic approval recorded here is never removed or rewritten.
/// - A new validation names a person here, not its starter, holding `ot.approve` at the work
///   order's node, at a readable time not in the future and not after its close. The right is
///   judged as held here now (as the permit re-checks do): while the validator's plant-wide rights
///   are still arriving it waits; a site-scoped right still in the inbox is a conflict for a person
///   to decide.
/// - A new automatic approval holds only on a closed work order, with no validator, at a site that
///   opted out here.
/// - A closed dépannage is validated or approved automatically.
pub(crate) async fn inbound_validation_check(
    conn: &impl ConnectionTrait,
    code: &str,
    local: Option<&Payload>,
    wo: &Payload,
) -> AppResult<Option<InboundVerdict>> {
    use crate::permit::sync_check::{conflict, field, wait};

    // The starter recorded here stays (`requester_id` is written once, at creation).
    if let Some(l) = local {
        if payload_text(l, "requester_sync_id") != payload_text(wo, "requester_sync_id") {
            return conflict(
                CODE_SYNC_VALIDATION_INVALID,
                format!("dépannage {code}: the person who started it cannot be changed"),
            );
        }
    }
    let starter = match local {
        Some(l) => payload_text(l, "requester_sync_id"),
        None => payload_text(wo, "requester_sync_id"),
    };

    let local_validator = local.and_then(|l| payload_text(l, "validated_by_sync_id"));
    let local_auto = local.is_some_and(|l| payload_flag(l, "validation_auto_approved"));
    let validator = payload_text(wo, "validated_by_sync_id");
    let auto = payload_flag(wo, "validation_auto_approved");

    // What is recorded here stays.
    if let Some(l) = local {
        if local_validator.is_some()
            && (local_validator != validator || payload_text(l, "validated_at") != payload_text(wo, "validated_at"))
        {
            return conflict(
                CODE_SYNC_VALIDATION_INVALID,
                format!("dépannage {code}: the validation recorded here cannot be removed or rewritten"),
            );
        }
        if local_auto && !auto {
            return conflict(
                CODE_SYNC_VALIDATION_INVALID,
                format!("dépannage {code}: the automatic approval recorded here cannot be removed"),
            );
        }
    }
    if validator.is_some() && auto {
        return conflict(
            CODE_SYNC_VALIDATION_INVALID,
            format!("dépannage {code}: received both validated by a person and approved automatically"),
        );
    }
    if validator.is_none() && field(wo, "validated_at").is_some() {
        return conflict(
            CODE_SYNC_VALIDATION_INVALID,
            format!("dépannage {code}: a validation time was received without the person who validated it"),
        );
    }

    let status = match payload_status(conn, code, wo).await? {
        Ok(s) => s,
        Err(v) => return Ok(Some(v)),
    };
    let closed = status.as_deref() == Some("closed");
    if closed && validator.is_none() && !auto {
        return conflict(
            CODE_SYNC_VALIDATION_INVALID,
            format!("dépannage {code}: received closed without its validation or its automatic approval"),
        );
    }

    // A new automatic approval: closed, at a site that opted out here.
    if auto && !local_auto {
        if !closed {
            return conflict(
                CODE_SYNC_VALIDATION_INVALID,
                format!("dépannage {code}: approved automatically while it is not closed"),
            );
        }
        let node = match payload_node(conn, code, wo).await? {
            Ok(n) => n,
            Err(v) => return Ok(Some(v)),
        };
        if !auto_validation_at_node(conn, node).await? {
            return conflict(
                CODE_SYNC_VALIDATION_INVALID,
                format!("dépannage {code}: approved automatically at a site that requires its validation here"),
            );
        }
        return Ok(None);
    }

    // A new validation by a person.
    let Some(validator_sync) = validator.filter(|_| local_validator.is_none()) else {
        return Ok(None);
    };
    if starter.is_none() {
        return conflict(
            CODE_SYNC_VALIDATION_INVALID,
            format!("dépannage {code}: validated while the person who started it is unknown"),
        );
    }
    if starter.as_deref() == Some(validator_sync.as_str()) {
        return conflict(
            CODE_SYNC_VALIDATION_INVALID,
            format!("dépannage {code}: validated by the person who started it (four-eyes)"),
        );
    }
    let Some(at) = field(wo, "validated_at").and_then(crate::settings::site::instant) else {
        return conflict(
            CODE_SYNC_VALIDATION_INVALID,
            format!("dépannage {code}: its validation has no readable time"),
        );
    };
    let skew = Duration::minutes(MAX_CLOCK_SKEW_MINUTES);
    if at > crate::clock::now() + skew {
        return conflict(
            CODE_SYNC_VALIDATION_INVALID,
            format!("dépannage {code}: its validation is dated in the future ({at})"),
        );
    }
    if let Some(closed_at) = field(wo, "closed_at").and_then(crate::settings::site::instant) {
        if at > closed_at + skew {
            return conflict(
                CODE_SYNC_VALIDATION_INVALID,
                format!("dépannage {code}: validated ({at}) after its close ({closed_at})"),
            );
        }
    }
    let read = |e: sea_orm::DbErr| AppError::SyncError(format!("dépannage validation re-check: {e}"));
    let Some(row) = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM user_accounts WHERE entity_sync_id = ?",
            [validator_sync.clone().into()],
        ))
        .await?
    else {
        return wait(format!(
            "dépannage {code}: the person who validated it ({validator_sync}) is not applied here yet"
        ));
    };
    let validator_id: i64 = row.try_get("", "id").map_err(read)?;
    let node = match payload_node(conn, code, wo).await? {
        Ok(n) => n,
        Err(v) => return Ok(Some(v)),
    };
    let holders =
        crate::notifications::router::permission_holder_user_ids(conn, crate::rbac::permissions::OT_APPROVE, node)
            .await?;
    if holders.contains(&validator_id) {
        return Ok(None);
    }
    if crate::permit::sync_check::rights_may_still_arrive(conn, &validator_sync, crate::rbac::permissions::OT_APPROVE)
        .await?
    {
        return wait(format!(
            "dépannage {code}: the rights of the person who validated it ({validator_sync}) are still arriving"
        ));
    }
    conflict(
        CODE_SYNC_VALIDATION_INVALID,
        format!("dépannage {code}: validated by a person who does not hold the approval right at its site here"),
    )
}
