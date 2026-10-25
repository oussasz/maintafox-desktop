//! Who may sign a break-in, and the signature rule (owner decision D6, CMMS Advisor answers Q1, Q1b).
//!
//! An urgent break-in (a reason tagged urgent in `PLANNING.BREAK_IN_REASONS`), or one that forces a
//! blocking check (crew overload, locked window, shutdown window, blocking readiness conflict), is
//! signed by a second person in the same dialog:
//! - not the requester (no self-approval, no fallback: emergency work runs as an unscheduled work
//!   order);
//! - holding plan.confirm at the crew's planning site (in force, plant-wide or on the site or one of
//!   its parents, or an elevated administrator): [`break_in_approvers`], the one list used by the
//!   approver picker, the local check and the check on arrival;
//! - linked to a personnel record;
//! - with their password typed on this PC (`auth::second_signer`: counted toward their lockout,
//!   a locked-out approver not checked).
//!
//! A named approver is always signed: a break-in never carries an approver who did not sign.

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::Value;

use crate::auth::second_signer::SignerPassword;
use crate::errors::{AppError, AppResult};

use super::domain::BreakInApproverOption;

/// The blocking checks a break-in can force (with a recorded dangerous-override reason and the
/// signature), as stored in `schedule_break_ins.forced_checks_json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForcedCheck {
    CrewCapacity,
    LockedWindow,
    Readiness,
    ShutdownWindow,
}

impl ForcedCheck {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::CrewCapacity => "crew_capacity",
            Self::LockedWindow => "locked_window",
            Self::Readiness => "readiness",
            Self::ShutdownWindow => "shutdown_window",
        }
    }
}

/// The forced checks as stored on the row: a JSON array of codes, `None` when nothing was forced.
pub(crate) fn forced_checks_json(forced: &[ForcedCheck]) -> Option<String> {
    (!forced.is_empty()).then(|| Value::from(forced.iter().map(|c| c.code()).collect::<Vec<_>>()).to_string())
}

/// The planning site a break-in of the crew `team_id` is judged at: the crew's planning site
/// (`None`: the crew has no site, or is not here: only plant-wide holders may sign).
pub(crate) async fn team_planning_site(conn: &impl ConnectionTrait, team_id: i64) -> AppResult<Option<i64>> {
    let node: Option<i64> = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT primary_node_id FROM teams WHERE id = ?",
            [team_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "primary_node_id").ok().flatten());
    match node {
        Some(node) => super::weekly_schedule::planning_site_of_node(conn, node).await,
        None => Ok(None),
    }
}

/// Active users who may sign a break-in at `site`: plan.confirm in force, plant-wide or on the site
/// or one of its parents, and the elevated administrators (`None`: plant-wide holders only).
pub(crate) async fn break_in_approvers(conn: &impl ConnectionTrait, site: Option<i64>) -> AppResult<Vec<i64>> {
    crate::notifications::router::permission_holder_user_ids(conn, crate::rbac::permissions::PLAN_CONFIRM, site).await
}

/// The people the signed-in user may ask to sign a break-in of the crew `team_id`: the
/// [`break_in_approvers`] of the crew's site, linked to a personnel record, other than the caller,
/// sorted by name.
pub async fn list_break_in_approvers(
    conn: &impl ConnectionTrait,
    caller_id: i64,
    team_id: i64,
) -> AppResult<Vec<BreakInApproverOption>> {
    let site = team_planning_site(conn, team_id).await?;
    let holders: Vec<i64> = break_in_approvers(conn, site)
        .await?
        .into_iter()
        .filter(|id| *id != caller_id)
        .collect();
    if holders.is_empty() {
        return Ok(Vec::new());
    }
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, COALESCE(NULLIF(TRIM(display_name), ''), username) AS display_name, personnel_id \
               FROM user_accounts \
              WHERE id IN (SELECT value FROM json_each(?)) AND personnel_id IS NOT NULL \
                AND is_active = 1 AND deleted_at IS NULL \
              ORDER BY display_name COLLATE NOCASE, id",
            [serde_json::to_string(&holders)?.into()],
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(BreakInApproverOption {
                user_id: r.try_get("", "id").map_err(|e| decode_err("id", e))?,
                display_name: r.try_get("", "display_name").map_err(|e| decode_err("display_name", e))?,
                personnel_id: r.try_get("", "personnel_id").map_err(|e| decode_err("personnel_id", e))?,
            })
        })
        .collect()
}

fn decode_err(column: &str, e: sea_orm::DbErr) -> AppError {
    AppError::Internal(anyhow::anyhow!("break-in approver decode '{column}': {e}"))
}

/// What a break-in asks to be signed.
pub(crate) struct ApprovalRequest<'a> {
    pub actor_id: i64,
    /// The crew whose site the approver must hold plan.confirm at.
    pub team_id: i64,
    pub reason_code: &'a str,
    pub urgent: bool,
    pub forced: &'a [ForcedCheck],
    pub approver: Option<i64>,
    pub password: SignerPassword<'a>,
}

/// The signature as it is stored on the break-in.
#[derive(Debug, Clone, Default)]
pub(crate) struct SignedApproval {
    pub approver: Option<i64>,
    pub approver_personnel: Option<i64>,
    pub approver_name: Option<String>,
    /// When the approver's password was verified (`None`: nobody signed).
    pub signed_at: Option<String>,
}

fn gate(message: &str) -> AppError {
    AppError::ValidationFailed(vec![message.to_string()])
}

/// The signature rule (see the module documentation). `password_checked` receives the outcome of
/// the password check when it ran (`Some(ok)`), for the caller to count it toward the approver's
/// lockout after its transaction (`auth::second_signer::record_outcome`), refused or not.
pub(crate) async fn validate_break_in_approval(
    conn: &impl ConnectionTrait,
    req: &ApprovalRequest<'_>,
    password_checked: &mut Option<bool>,
) -> AppResult<SignedApproval> {
    let needs_signature = req.urgent || !req.forced.is_empty();
    let Some(approver) = req.approver.filter(|id| *id > 0) else {
        if needs_signature {
            let forced: Vec<&str> = req.forced.iter().map(|c| c.code()).collect();
            return Err(crate::errors::org_validation_failed(
                "PLANNING_BREAK_IN_APPROVAL_REQUIRED",
                "An urgent break-in, or one that forces a blocking check, needs the signature of a second planner \
                 holding plan.confirm at the crew's site.",
                std::collections::HashMap::from([
                    ("reason".to_string(), req.reason_code.to_string()),
                    ("forced".to_string(), forced.join(",")),
                ]),
            ));
        }
        return Ok(SignedApproval::default());
    };
    if approver == req.actor_id {
        return Err(gate(
            "GATE_PLAN_APPROVER_SELF: The break-in must be signed by another planner, not by the person recording it.",
        ));
    }
    let site = team_planning_site(conn, req.team_id).await?;
    if !break_in_approvers(conn, site).await?.contains(&approver) {
        return Err(gate(
            "GATE_PLAN_APPROVER_NOT_ALLOWED: The approver must hold plan.confirm at the crew's site.",
        ));
    }
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id, COALESCE(NULLIF(TRIM(display_name), ''), username) AS display_name \
               FROM user_accounts WHERE id = ?",
            [approver.into()],
        ))
        .await?;
    let personnel: Option<i64> = row
        .as_ref()
        .and_then(|r| r.try_get::<Option<i64>>("", "personnel_id").ok().flatten());
    let name: Option<String> = row
        .as_ref()
        .and_then(|r| r.try_get::<Option<String>>("", "display_name").ok().flatten());
    let Some(personnel) = personnel else {
        return Err(gate(
            "GATE_PLAN_APPROVER_NO_PERSONNEL: approved_by_user_id must be linked to a personnel record.",
        ));
    };
    match req.password {
        SignerPassword::Locked => {
            return Err(gate(
                "GATE_PLAN_APPROVER_LOCKED: The approver's account is locked: their password cannot be checked now.",
            ));
        }
        SignerPassword::Given(raw) => {
            let Some(raw) = raw.filter(|p| !p.trim().is_empty()) else {
                return Err(gate(
                    "GATE_PLAN_APPROVER_PASSWORD_REQUIRED: The approver must type their password to sign the break-in.",
                ));
            };
            let ok = crate::auth::second_signer::verify_user_password(conn, approver, raw).await?;
            *password_checked = Some(ok);
            if !ok {
                return Err(gate(
                    "GATE_PLAN_APPROVER_AUTH_FAILED: The approver's password is not correct.",
                ));
            }
        }
        // Only a received break-in, judged by `planning::sync_check` (never this function).
        SignerPassword::NotVerifiableHere => {}
    }
    Ok(SignedApproval {
        approver: Some(approver),
        approver_personnel: Some(personnel),
        approver_name: name,
        signed_at: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
    })
}

/// The approver whose password a break-in checks (an approver named with a non-empty password).
pub(crate) fn signer_account(approver: Option<i64>, password: Option<&str>) -> Option<i32> {
    approver
        .filter(|id| *id > 0)
        .filter(|_| password.is_some_and(|p| !p.trim().is_empty()))
        .and_then(|id| i32::try_from(id).ok())
}

/// The approver's lockout state read before the break-in's transaction.
pub(crate) async fn signer_password_state<'a>(
    db: &sea_orm::DatabaseConnection,
    signer: Option<i32>,
    password: Option<&'a str>,
) -> AppResult<SignerPassword<'a>> {
    Ok(match signer {
        Some(id) if crate::auth::second_signer::lockout_state(db, id).await? => SignerPassword::Locked,
        _ => SignerPassword::Given(password),
    })
}

/// Counts the approver's password outcome (outside the break-in's transaction, refused or not).
pub(crate) async fn record_signer_outcome(
    db: &sea_orm::DatabaseConnection,
    actor_id: i64,
    signer: Option<i32>,
    checked: Option<bool>,
    commitment_id: Option<i64>,
) -> AppResult<()> {
    crate::auth::second_signer::record_outcome(
        db,
        signer,
        checked,
        &crate::auth::second_signer::SignatureContext {
            actor_id,
            context: "planning_break_in",
            summary: "Break-in signature refused: wrong approver password",
            detail: serde_json::json!({ "schedule_commitment_id": commitment_id }),
        },
    )
    .await
}

/// Audit event of a break-in signed by a second person: actor the requester, both names in the
/// detail.
pub(crate) const BREAK_IN_SIGNED_EVENT: &str = "planning.break_in.signed";

/// Writes [`BREAK_IN_SIGNED_EVENT`] once the break-in is committed (never refuses it).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn audit_signed_break_in(
    db: &sea_orm::DatabaseConnection,
    actor_id: i64,
    commitment_id: i64,
    break_in_id: i64,
    kind: &str,
    reason_code: &str,
    approval: &SignedApproval,
    forced: &[ForcedCheck],
) {
    let Some(approver) = approval.approver else {
        return;
    };
    let commitment_ref = commitment_id.to_string();
    crate::audit::emit(
        db,
        crate::audit::AuditEvent {
            event_type: BREAK_IN_SIGNED_EVENT,
            actor_id: i32::try_from(actor_id).ok(),
            entity_type: Some("schedule_commitment"),
            entity_id: Some(&commitment_ref),
            summary: "Break-in signed by a second planner",
            detail_json: Some(
                serde_json::json!({
                    "break_in_id": break_in_id,
                    "kind": kind,
                    "requested_by_user_id": actor_id,
                    "approved_by_user_id": approver,
                    "approved_by_display_name": approval.approver_name,
                    "approver_signed_at": approval.signed_at,
                    "reason": reason_code,
                    "forced_checks": forced.iter().map(|c| c.code()).collect::<Vec<_>>(),
                })
                .to_string(),
            ),
            ..Default::default()
        },
    )
    .await;
}
