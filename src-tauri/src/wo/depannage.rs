//! Dépannage (breakdown) path: create a corrective work order and start it at once (CMMS Advisor
//! decision, 2026-11-04; migration 277).
//!
//! One transaction: the work order is created as every work order is ([`create_work_order_in`]:
//! creator, equipment known and not retired, org node, the plant's permit rules stamped — owner
//! decision D2), marked as a dépannage with its "Panne constatée à" and "Machine arrêtée", assigned
//! (to the user, or to someone else with `ot.emergency_assign`, checked by the command,
//! `wo::access::check_depannage_at`), then moved draft → planning → ready → in progress through the
//! state machine. The planning approval, the planned dates and duration and the parts readiness
//! are not asked for: the two intermediate moves and the start are recorded on the transition log
//! as automatic ([`AUTO_REASON_CODE`], [`AUTO_NOTE`]), by the user who started the dépannage, with
//! no approver — the planning is validated after the fact (`validated_by_id`, a later step).
//!
//! Gates that stay: the rights (checked by the command), the equipment (known, not retired), the
//! qualification of the assignee (`qualification::gate`), and the permit / LOTO gate of the start
//! (`wo::execution::start_wo_in`, `permit::wo_gate`). Any refusal rolls the whole transaction back: no
//! work order is left half created.
//!
//! A dépannage is never refused because a permit is needed (CMMS Advisor decision, 2026-11-05: a
//! breakdown report is never lost). Whether the start would wait for a permit is read before the
//! start, from the requirement the start gate itself reads (`permit::wo_gate::permit_requirement`):
//! every permit type of the work order's required set not cleared (`permit::required_set`, stamped
//! at its creation by `create_work_order_in` from every matching plant rule — CMMS Advisor
//! 2026-11-04). When it would, the work order stays "prête" (ready), assigned, not started
//! ([`DepannageOutcome::WaitingForPermit`], with every permit type it needs), and:
//! - when the user may create a permit at the work order's site (`ptw.issue`, as a permit created by
//!   hand: `permit::access::check_create_at`), one permit draft per required type is written with
//!   the dépannage (a work order holds one open permit of each type, never two of the same), linked
//!   to the work order and its equipment (the work order carries the description); hazards,
//!   isolations and the approval are the issuer's. A type whose maximum duration is not set gets no
//!   draft (the expiry is the issuer's to enter, as on the create form);
//! - in every case the permit issuers of the site are told at once ([`PERMIT_NEEDED_CATEGORY`]),
//!   naming every type, with a link to the first draft, or to "Créer le permis" for the work order
//!   when there is none, and
//!   the planners and the node's responsibles are told the dépannage waits for its permit
//!   ([`NOTIFICATION_CATEGORY`], message [`WAITING_MESSAGE_KEY`]).
//!
//! It is started later by the normal start (`wo::execution::start_wo`), once every required permit
//! is live: the normal gate decides and the start is logged as any start. Its response time and
//! time to restore include the wait for the permit.
//!
//! A dépannage is unscheduled: it gets no schedule commitment, so it never enters a weekly
//! schedule, frozen or not; the work order's `is_depannage` flag says it broke the schedule.
//!
//! The planners of the work order's site and the responsibles of its node are told
//! ([`NOTIFICATION_CATEGORY`], migration 277).
//!
//! By synchronisation the rows travel as rows: the received work order entering in progress is
//! judged by the permit gate of a start (`permit::sync_check::inbound_work_order_check`), its plant
//! rule by `inbound_permit_rule_check` and `requirement_rules`, retired equipment by
//! `wo::retired_equipment_review`, and its dépannage facts by [`inbound_depannage_check`].

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::auth::scope::AccessMap;
use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit_event_with, NotificationEventInput, NotificationMessage};
use crate::permit::domain::{WorkPermit, WorkPermitCreateInput};
use crate::settings::site::{InboundVerdict, MAX_CLOCK_SKEW_MINUTES};
use crate::wo::domain::{WoCreateInput, WorkOrder};
use crate::wo::execution::{start_wo_in, StartMark, WoStartInput};
use crate::wo::queries::{create_work_order_in, get_work_order};
use crate::wo::time::{now_utc_second, planned_time, stamp, PlannedBound};
use crate::wo::types::{default_type_for_nature, list_work_order_types, MaintenanceNature};
use crate::wo::workflow::events::emit_action_event;
use crate::wo::workflow::state_machine::{assert_action_allowed, WoAction, WoStatus};
use crate::wo::workflow::transition::{apply_status_transition, check_concurrency, load_wo_status};

/// Notification category of a dépannage started, or created and waiting for its permit (migration
/// 277): the planners of the site and the responsibles of the node.
pub const NOTIFICATION_CATEGORY: &str = "wo_depannage_started";
/// Message of that category when the dépannage waits for its permit.
pub const WAITING_MESSAGE_KEY: &str = "wo_depannage_waiting_permit";
/// Notification category "permis à établir — dépannage" (migration 277): the permit issuers of the
/// work order's site (`notifications::catalog::ALWAYS_DELIVERED`).
pub const PERMIT_NEEDED_CATEGORY: &str = "wo_depannage_permit_needed";

/// Reason code of the transitions the dépannage makes by itself (transition log).
pub const AUTO_REASON_CODE: &str = "depannage_auto";
/// Note of those transitions.
pub const AUTO_NOTE: &str = "passage automatique (dépannage)";

/// A breakdown found longer ago than this is accepted with a warning (a late entry to check).
pub const OLD_BREAKDOWN_WARNING_DAYS: i64 = 7;

/// Refusal codes.
pub const CODE_BREAKDOWN_AT_REQUIRED: &str = "GATE_WO_DEPANNAGE_BREAKDOWN_AT_REQUIRED";
pub const CODE_BREAKDOWN_AT_UNREADABLE: &str = "GATE_WO_DEPANNAGE_BREAKDOWN_AT_UNREADABLE";
pub const CODE_BREAKDOWN_AT_FUTURE: &str = "GATE_WO_DEPANNAGE_BREAKDOWN_AT_FUTURE";
pub const CODE_MACHINE_STOPPED_REQUIRED: &str = "GATE_WO_DEPANNAGE_MACHINE_STOPPED_REQUIRED";
pub const CODE_TITLE_REQUIRED: &str = "GATE_WO_DEPANNAGE_TITLE_REQUIRED";
pub const CODE_TYPE_NOT_CORRECTIVE: &str = "GATE_WO_DEPANNAGE_TYPE_NOT_CORRECTIVE";
pub const CODE_ASSIGNEE_UNKNOWN: &str = "GATE_WO_DEPANNAGE_ASSIGNEE_UNKNOWN";
pub const CODE_NOT_A_DEPANNAGE: &str = "GATE_WO_DEPANNAGE_NOT_A_DEPANNAGE";
pub const CODE_CLOSED: &str = "GATE_WO_DEPANNAGE_CLOSED";
/// Sync conflict: a received dépannage whose facts are missing or inconsistent.
pub const CODE_SYNC_FACTS_INVALID: &str = "WO_DEPANNAGE_FACTS_INVALID";

#[derive(Debug, Clone, Deserialize)]
pub struct WoDepannageInput {
    pub equipment_id: i64,
    /// An OT type of a corrective nature; absent: the plant's type for immediate corrective work.
    #[serde(default)]
    pub type_code: Option<String>,
    /// Level of the shared urgency scale.
    #[serde(default)]
    pub urgency_code: Option<String>,
    #[serde(default)]
    pub subtype_code: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// "Panne constatée à": RFC 3339, or a date and time typed without a zone (this PC's zone).
    #[serde(default)]
    pub breakdown_at: Option<String>,
    /// "Machine arrêtée": required, no default.
    #[serde(default)]
    pub machine_stopped: Option<bool>,
    /// Who carries the work out; absent: the user.
    #[serde(default)]
    pub assignee_id: Option<i64>,
    /// An explicit "requires permit" (a plant rule may require one anyway).
    #[serde(default)]
    pub requires_permit: Option<bool>,
    /// The signed-in user (set by the command, never by the screen).
    #[serde(default)]
    pub actor_id: i64,
    /// The user's rights for the permit drafts (`permit::access::permit_drafting_rights`; set by
    /// the command, never by the screen). `None`: no permit draft is written.
    #[serde(skip)]
    pub permit_drafting: Option<AccessMap>,
}

/// Where a dépannage stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DepannageOutcome {
    /// In progress ("en cours").
    Started,
    /// Ready ("prête"), assigned, not started: a permit must be live first.
    WaitingForPermit,
}

/// The work order a dépannage created (the create / get DTO), whether its breakdown was found more
/// than [`OLD_BREAKDOWN_WARNING_DAYS`] days ago (a warning, not a refusal), whether it started or
/// waits for a permit, the permit types it waits for (codes; empty when it started, or when it
/// requires a permit of no particular type), and the permit drafts written for it.
#[derive(Debug, Clone, Serialize)]
pub struct WoDepannageStartResult {
    pub wo: WorkOrder,
    pub breakdown_old_warning: bool,
    pub outcome: DepannageOutcome,
    pub required_permit_types: Vec<String>,
    pub permit_draft_ids: Vec<i64>,
}

/// A dépannage after its "Panne constatée à" changed, and whether that breakdown is old.
#[derive(Debug, Clone, Serialize)]
pub struct WoDepannageResult {
    pub wo: WorkOrder,
    pub breakdown_old_warning: bool,
}

/// Changing the "Panne constatée à" of a dépannage, until it is closed.
#[derive(Debug, Clone, Deserialize)]
pub struct WoBreakdownAtInput {
    pub wo_id: i64,
    pub expected_row_version: i64,
    pub breakdown_at: Option<String>,
    /// The signed-in user (set by the command).
    #[serde(default)]
    pub actor_id: i64,
}

fn refused(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

/// The "Panne constatée à" in its stored form (UTC, whole seconds), judged at `now`: required,
/// readable, not later than `now` plus the tolerated clock difference. Returns it and whether it is
/// older than [`OLD_BREAKDOWN_WARNING_DAYS`].
pub fn checked_breakdown_at(raw: Option<&str>, now: DateTime<Utc>) -> Result<(String, bool), String> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Err(refused(
            CODE_BREAKDOWN_AT_REQUIRED,
            "La date et l'heure de constat de la panne sont obligatoires.",
        ));
    };
    let Some(at) = planned_time(raw, PlannedBound::Start) else {
        return Err(format!(
            "{CODE_BREAKDOWN_AT_UNREADABLE}: La date de constat de la panne n'est pas une date et heure lisible."
        ));
    };
    if at > now + Duration::minutes(MAX_CLOCK_SKEW_MINUTES) {
        return Err(format!(
            "{CODE_BREAKDOWN_AT_FUTURE}:value={}: La panne ne peut pas être constatée dans le futur.",
            stamp(at)
        ));
    }
    Ok((stamp(at), at < now - Duration::days(OLD_BREAKDOWN_WARNING_DAYS)))
}

/// The plain checks of a dépannage, all reported at once: title, breakdown time, machine stopped.
fn checked_facts(input: &WoDepannageInput, now: DateTime<Utc>) -> AppResult<(String, bool, bool)> {
    let mut errors = Vec::new();
    if input.title.trim().is_empty() {
        errors.push(refused(CODE_TITLE_REQUIRED, "Le titre est obligatoire."));
    }
    let breakdown = match checked_breakdown_at(input.breakdown_at.as_deref(), now) {
        Ok(found) => Some(found),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    if input.machine_stopped.is_none() {
        errors.push(refused(
            CODE_MACHINE_STOPPED_REQUIRED,
            "Indiquez si la machine est arrêtée (oui ou non).",
        ));
    }
    match (breakdown, input.machine_stopped) {
        (Some((at, old)), Some(stopped)) if errors.is_empty() => Ok((at, old, stopped)),
        _ => Err(AppError::ValidationFailed(errors)),
    }
}

/// The OT type of the dépannage: the one given, of a corrective nature; else the plant's type for
/// immediate corrective work (`wo::types::default_type_for_nature`).
async fn depannage_type_code(conn: &impl ConnectionTrait, given: Option<&str>) -> AppResult<String> {
    let Some(code) = given.map(str::trim).filter(|c| !c.is_empty()) else {
        return Ok(default_type_for_nature(conn, MaintenanceNature::CorrectiveImmediate)
            .await?
            .code);
    };
    // Unknown or inactive: the create refuses it with its own code.
    let types = list_work_order_types(conn).await?;
    let Some(chosen) = types.iter().find(|t| t.code == code) else {
        return Ok(code.to_string());
    };
    let corrective = chosen
        .nature
        .as_deref()
        .and_then(MaintenanceNature::parse)
        .is_some_and(MaintenanceNature::is_corrective);
    if !corrective {
        return Err(AppError::ValidationFailed(vec![format!(
            "{CODE_TYPE_NOT_CORRECTIVE}:type={code}: Un dépannage est un OT correctif : choisissez un type de nature corrective."
        )]));
    }
    Ok(code.to_string())
}

/// The assignee must be an active account here.
async fn refuse_unknown_assignee(conn: &impl ConnectionTrait, user_id: i64) -> AppResult<()> {
    let active = conn
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS ok FROM user_accounts WHERE id = ? AND is_active = 1 AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?
        .is_some();
    if active {
        Ok(())
    } else {
        Err(AppError::ValidationFailed(vec![format!(
            "{CODE_ASSIGNEE_UNKNOWN}:user_id={user_id}: La personne désignée n'est pas un compte actif."
        )]))
    }
}

/// Origin of the dépannage's assignment in the work order's history (`origin` of the event, as
/// `wo::workflow::actions::assign` records `assign` / `emergency_reassign`).
pub const ASSIGNMENT_ORIGIN: &str = "depannage";

/// The assignment in the work order's history, as an assignment records it: the action event of an
/// assignment (`emergency_reassigned`, the event `wo::workflow::actions::assign` writes, with its
/// `origin`) and the history entry "Affecté" (responsible: none → the assignee), both written with
/// the dépannage or not at all.
async fn record_assignment(txn: &impl ConnectionTrait, wo_id: i64, actor: i64, assignee: i64) -> AppResult<()> {
    let wo = get_work_order(txn, wo_id).await?.ok_or_else(|| AppError::NotFound {
        entity: "WorkOrder".into(),
        id: wo_id.to_string(),
    })?;
    let status = wo
        .status_code
        .clone()
        .unwrap_or_else(|| WoStatus::Draft.as_str().to_string());
    let payload = serde_json::json!({
        "origin": ASSIGNMENT_ORIGIN,
        "override_reason": serde_json::Value::Null,
        "assigned_team_id": serde_json::Value::Null,
        "primary_responsible_id": assignee,
        "skips_planning_gates": true,
        "shift_cleared": serde_json::Value::Null,
    })
    .to_string();
    emit_action_event(
        txn,
        wo_id,
        "emergency_reassigned",
        Some(actor),
        Some(&status),
        Some(&status),
        Some(&payload),
    )
    .await?;
    let shown = wo
        .responsible_display_name
        .as_deref()
        .or(wo.responsible_username.as_deref())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or(serde_json::Value::Null, |n| serde_json::Value::String(n.to_string()));
    crate::wo::audit::insert_wo_change_event(
        txn,
        &crate::wo::audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "assigned".into(),
            actor_id: Some(actor),
            summary: Some("Assigned by the dépannage".into()),
            details_json: Some(
                serde_json::json!({
                    "origin": ASSIGNMENT_ORIGIN,
                    "primary_responsible_id": assignee,
                    "changes": [{ "field": "primary_responsible", "old": serde_json::Value::Null, "new": shown }],
                })
                .to_string(),
            ),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await?;
    Ok(())
}

/// The assignee, when it is not the user who started the dépannage, is told at once through the
/// work-order assignment notice (`wo_assigned`, `notifications::watchers::raise_wo_assigned_now`:
/// the watcher's own occurrence, so it is never told twice). A user who assigns themself is not
/// told at once; the watcher's next `wo_assigned` round may still raise it. Fire-and-log, as every
/// notification: the notice runs in its own savepoint, so a failure leaves nothing of it.
async fn notify_assignee(txn: &DatabaseTransaction, wo_id: i64, actor: i64, assignee: i64, fr: bool) {
    if assignee == actor {
        return;
    }
    if let Err(err) = crate::notifications::watchers::raise_wo_assigned_now(txn, wo_id, crate::clock::now(), fr).await {
        tracing::warn!(error = %err, wo_id, "dépannage assignment notification failed");
    }
}

/// Creates the dépannage and starts it — or leaves it ready, waiting for its permit — in one
/// transaction (rolled back on any refusal).
pub async fn start_depannage(db: &DatabaseConnection, input: WoDepannageInput) -> AppResult<WoDepannageStartResult> {
    // The language of the notices' fallback texts, as the notification watchers choose it.
    let fr = crate::pm::wo_content::tenant_text_locale(db)
        .await
        .map(|l| l.to_ascii_lowercase().starts_with("fr"))
        .unwrap_or(true);
    let txn = db.begin().await?;
    let result = start_depannage_in(&txn, input, fr).await?;
    txn.commit().await?;
    Ok(result)
}

/// [`start_depannage`] in the caller's transaction; `fr`: the notices' fallback texts in French.
pub async fn start_depannage_in(
    txn: &DatabaseTransaction,
    input: WoDepannageInput,
    fr: bool,
) -> AppResult<WoDepannageStartResult> {
    let (breakdown_at, breakdown_old_warning, machine_stopped) = checked_facts(&input, now_utc_second())?;
    let actor = input.actor_id;
    let assignee = input.assignee_id.unwrap_or(actor);
    refuse_unknown_assignee(txn, assignee).await?;
    let type_code = depannage_type_code(txn, input.type_code.as_deref()).await?;

    // Created as every work order is: creator, equipment (known, not retired), node, plant permit
    // rules (D2). No planner, no planned dates or duration: a dépannage is not planned.
    let created = create_work_order_in(
        txn,
        WoCreateInput {
            type_code,
            equipment_id: Some(input.equipment_id),
            location_id: None,
            source_di_id: None,
            source_inspection_anomaly_id: None,
            source_ram_ishikawa_diagram_id: None,
            source_ishikawa_flow_node_id: None,
            source_rca_cause_text: None,
            entity_id: None,
            planner_id: None,
            urgency_code: input.urgency_code,
            subtype_code: input.subtype_code,
            title: input.title.trim().to_string(),
            description: input.description,
            notes: None,
            planned_start: None,
            planned_end: None,
            shift: None,
            expected_duration_hours: None,
            creator_id: actor,
            requires_permit: input.requires_permit,
        },
    )
    .await?;
    let wo_id = created.id;

    // Part of the creation (the draft is not shown to anyone before the commit).
    let marked = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET is_depannage = 1, breakdown_at = ?, machine_stopped = ?, \
                    primary_responsible_id = ? \
              WHERE id = ? AND row_version = ?",
            [
                breakdown_at.clone().into(),
                i64::from(machine_stopped).into(),
                assignee.into(),
                wo_id.into(),
                created.row_version.into(),
            ],
        ))
        .await?;
    check_concurrency(marked.rows_affected())?;

    // The assignee holds what the work order requires (or an override is recorded for them).
    crate::qualification::gate::enforce_user_on_work_order(txn, assignee, wo_id).await?;
    record_assignment(txn, wo_id, actor, assignee).await?;

    // draft → planning → ready: through the state machine, recorded as automatic, no approver.
    let auto_payload = serde_json::json!({ "reason_code": AUTO_REASON_CODE }).to_string();
    for (action, to, event) in [
        (WoAction::Submit, WoStatus::Planning, "submitted"),
        (WoAction::MarkReady, WoStatus::Ready, "marked_ready"),
    ] {
        let (from_code, status, row_version) = load_wo_status(txn, wo_id).await?;
        assert_action_allowed(&status, action).map_err(|e| AppError::ValidationFailed(vec![e]))?;
        apply_status_transition(
            txn,
            wo_id,
            row_version,
            actor,
            to,
            action.as_str(),
            Some(AUTO_REASON_CODE),
            Some(AUTO_NOTE),
            "",
        )
        .await?;
        emit_action_event(
            txn,
            wo_id,
            event,
            Some(actor),
            Some(&from_code),
            Some(to.as_str()),
            Some(&auto_payload),
        )
        .await?;
    }

    // A permit needed: decided before the start, from the requirement the start gate reads. The work
    // order stays ready, assigned; it is started later by the normal start, once its permits are live.
    let requirement = crate::permit::wo_gate::permit_requirement(txn, wo_id).await?;
    if requirement.needs_permit() {
        let drafts = draft_permits(
            txn,
            wo_id,
            input.equipment_id,
            actor,
            &requirement.permit_types,
            input.permit_drafting.as_ref(),
        )
        .await?;
        let wo = get_work_order(txn, wo_id).await?.ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: wo_id.to_string(),
        })?;
        let type_names = permit_type_names(txn, &requirement.permit_types).await?;
        notify_waiting(txn, &wo, &type_names).await;
        notify_permit_needed(txn, &wo, &type_names, &drafts).await;
        notify_assignee(txn, wo_id, actor, assignee, fr).await;
        return Ok(WoDepannageStartResult {
            wo,
            breakdown_old_warning,
            outcome: DepannageOutcome::WaitingForPermit,
            required_permit_types: requirement.permit_types,
            permit_draft_ids: drafts.iter().map(|d| d.id).collect(),
        });
    }

    // ready → in progress: the start itself, with its permit / LOTO gate.
    let (_, _, row_version) = load_wo_status(txn, wo_id).await?;
    let wo = start_wo_in(
        txn,
        &WoStartInput {
            wo_id,
            actor_id: actor,
            expected_row_version: row_version,
        },
        Some(StartMark {
            reason_code: AUTO_REASON_CODE,
            notes: AUTO_NOTE,
        }),
    )
    .await?;

    notify_started(txn, &wo).await;
    notify_assignee(txn, wo_id, actor, assignee, fr).await;
    Ok(WoDepannageStartResult {
        wo,
        breakdown_old_warning,
        outcome: DepannageOutcome::Started,
        required_permit_types: Vec::new(),
        permit_draft_ids: Vec::new(),
    })
}

/// A refusal of a permit draft by its checks, before anything is written (no right at the site, a
/// site or an expiry the permit rules do not accept, an open permit of that type already linked):
/// no draft of that type, the dépannage goes on and the issuers create the permit. Anything else (the
/// database), and any error of the write itself, still fails the dépannage.
fn draft_refused(err: &AppError) -> bool {
    matches!(
        err,
        AppError::PermissionDenied(_)
            | AppError::Permission { .. }
            | AppError::TenantScopeViolation(_)
            | AppError::LicenseDenied { .. }
            | AppError::ValidationFailed(_)
            | AppError::OrgValidationFailed(_)
            | AppError::NotFound { .. }
    )
}

/// One permit draft per required type, linked to the work order and its equipment, written by the
/// permit creation itself (`permit::queries::check_work_permit_create`, then
/// `permit::queries::insert_work_permit`) after the check of a permit
/// created by hand at the work order's site (`permit::access::check_create_at`). Its expiry is the
/// type's maximum duration from now, as the create form proposes it; a type without one gets no
/// draft. None at all without `rights`.
async fn draft_permits(
    txn: &impl ConnectionTrait,
    wo_id: i64,
    equipment_id: i64,
    actor: i64,
    permit_types: &[String],
    rights: Option<&AccessMap>,
) -> AppResult<Vec<WorkPermit>> {
    let Some(map) = rights else {
        return Ok(Vec::new());
    };
    let mut drafts = Vec::new();
    for code in permit_types {
        let Some(row) = txn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id, max_duration_hours FROM permit_types WHERE code = ?",
                [code.clone().into()],
            ))
            .await?
        else {
            tracing::warn!(wo_id, permit_type = %code, "dépannage: unknown permit type, no draft");
            continue;
        };
        let type_id: i64 = row.try_get("", "id")?;
        let max_hours: Option<f64> = row.try_get("", "max_duration_hours")?;
        let Some(hours) = max_hours.filter(|h| h.is_finite() && *h > 0.0) else {
            tracing::info!(wo_id, permit_type = %code, "dépannage: no maximum duration, no draft");
            continue;
        };
        // Whole seconds, never past the type's maximum.
        #[allow(clippy::cast_possible_truncation)]
        let expires_at = stamp(now_utc_second() + Duration::seconds((hours * 3600.0).floor() as i64));
        let permit = WorkPermitCreateInput {
            permit_type_id: type_id,
            asset_id: Some(equipment_id),
            entity_id: None,
            linked_work_order_id: Some(wo_id),
            expires_at: Some(expires_at),
        };
        // Only the refusals of the checks, before anything is written, mean "no draft": once the
        // insert has begun, any error fails the dépannage (no draft left half written).
        let checked = match crate::permit::access::check_create_at(txn, map, &permit).await {
            Ok(()) => crate::permit::queries::check_work_permit_create(txn, &permit).await,
            Err(err) => Err(err),
        };
        match checked {
            Ok(checked) => {
                drafts.push(crate::permit::queries::insert_work_permit(txn, &permit, checked, actor).await?);
            }
            Err(err) if draft_refused(&err) => {
                tracing::info!(wo_id, permit_type = %code, error = %err, "dépannage: no permit draft");
            }
            Err(err) => return Err(err),
        }
    }
    Ok(drafts)
}

/// The names of permit types as the plant lists them (the code when a type is not found).
async fn permit_type_names(conn: &impl ConnectionTrait, codes: &[String]) -> AppResult<Vec<String>> {
    let mut names = Vec::with_capacity(codes.len());
    for code in codes {
        let name = conn
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT name FROM permit_types WHERE code = ?",
                [code.clone().into()],
            ))
            .await?
            .and_then(|r| r.try_get::<Option<String>>("", "name").ok().flatten())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| code.clone());
        names.push(name);
    }
    Ok(names)
}

/// The permit types as a notice shows them; `None` when no particular type is required.
fn joined(names: &[String]) -> Option<String> {
    (!names.is_empty()).then(|| names.join(", "))
}

/// "OT dépannage en attente de permis" to the planners of the site and the responsibles of the node.
async fn notify_waiting(conn: &impl ConnectionTrait, wo: &WorkOrder, type_names: &[String]) {
    let types = joined(type_names);
    let input = NotificationEventInput {
        source_module: "work_orders".into(),
        source_record_id: Some(wo.id.to_string()),
        event_code: WAITING_MESSAGE_KEY.into(),
        category_code: NOTIFICATION_CATEGORY.into(),
        severity: "warning".into(),
        dedupe_key: Some(format!("{WAITING_MESSAGE_KEY}-{}", wo.id)),
        payload_json: None,
        title: format!("OT dépannage en attente de permis — {}", wo.code),
        body: Some(match &types {
            Some(t) => format!("{} · permis : {t}", wo.title),
            None => wo.title.clone(),
        }),
        action_url: Some(format!("/work-orders?openWo={}", wo.id)),
    };
    let message = NotificationMessage::new(
        WAITING_MESSAGE_KEY,
        serde_json::json!({
            "code": wo.code,
            "title": wo.title,
            "asset": wo.asset_code,
            "breakdown_at": wo.breakdown_at,
            "machine_stopped": wo.machine_stopped,
            "permit_types": types,
        }),
    )
    .with_urgency(wo.urgency_code.clone());
    if let Err(err) = emit_event_with(conn, input, Some(&message)).await {
        tracing::warn!(error = %err, wo_id = wo.id, "dépannage waiting notification emit failed");
    }
}

/// "Permis à établir — dépannage" to the permit issuers of the work order's site, at once
/// ([`PERMIT_NEEDED_CATEGORY`]): the link opens the permit draft written for it, or "Créer le
/// permis" for the work order when there is none (`/permits?createForWo=`).
async fn notify_permit_needed(
    conn: &impl ConnectionTrait,
    wo: &WorkOrder,
    type_names: &[String],
    drafts: &[WorkPermit],
) {
    let types = joined(type_names);
    let draft_codes = drafts.iter().map(|d| d.code.clone()).collect::<Vec<_>>();
    let action_url = match drafts.first() {
        Some(draft) => format!("/permits?permit={}", draft.id),
        None => format!("/permits?createForWo={}", wo.id),
    };
    let input = NotificationEventInput {
        source_module: "work_orders".into(),
        source_record_id: Some(wo.id.to_string()),
        event_code: PERMIT_NEEDED_CATEGORY.into(),
        category_code: PERMIT_NEEDED_CATEGORY.into(),
        severity: "critical".into(),
        dedupe_key: Some(format!("{PERMIT_NEEDED_CATEGORY}-{}", wo.id)),
        payload_json: None,
        title: format!("Permis à établir — dépannage {}", wo.code),
        body: Some(match &types {
            Some(t) => format!("{} · permis : {t}", wo.title),
            None => wo.title.clone(),
        }),
        action_url: Some(action_url),
    };
    let message = NotificationMessage::new(
        PERMIT_NEEDED_CATEGORY,
        serde_json::json!({
            "code": wo.code,
            "title": wo.title,
            "asset": wo.asset_code,
            "breakdown_at": wo.breakdown_at,
            "machine_stopped": wo.machine_stopped,
            "permit_types": types,
            "drafts": joined(&draft_codes),
        }),
    )
    .with_urgency(wo.urgency_code.clone());
    if let Err(err) = emit_event_with(conn, input, Some(&message)).await {
        tracing::warn!(error = %err, wo_id = wo.id, "dépannage permit-needed notification emit failed");
    }
}

/// "OT dépannage démarré" to the planners of the site and the responsibles of the node
/// (fire-and-log, as every notification).
async fn notify_started(conn: &impl ConnectionTrait, wo: &WorkOrder) {
    let input = NotificationEventInput {
        source_module: "work_orders".into(),
        source_record_id: Some(wo.id.to_string()),
        event_code: NOTIFICATION_CATEGORY.into(),
        category_code: NOTIFICATION_CATEGORY.into(),
        severity: "warning".into(),
        dedupe_key: Some(format!("{NOTIFICATION_CATEGORY}-{}", wo.id)),
        payload_json: None,
        title: format!("OT dépannage démarré — {}", wo.code),
        body: Some(wo.title.clone()),
        action_url: Some(format!("/work-orders?openWo={}", wo.id)),
    };
    let message = NotificationMessage::new(
        NOTIFICATION_CATEGORY,
        serde_json::json!({
            "code": wo.code,
            "title": wo.title,
            "asset": wo.asset_code,
            "breakdown_at": wo.breakdown_at,
            "machine_stopped": wo.machine_stopped,
        }),
    )
    .with_urgency(wo.urgency_code.clone());
    if let Err(err) = emit_event_with(conn, input, Some(&message)).await {
        tracing::warn!(error = %err, wo_id = wo.id, "dépannage notification emit failed");
    }
}

/// Changes the "Panne constatée à" of a dépannage, until it is closed or cancelled. The command
/// records the change in the work order's audit trail (field `breakdown_at`, old → new).
pub async fn set_breakdown_at(db: &DatabaseConnection, input: WoBreakdownAtInput) -> AppResult<WoDepannageResult> {
    let (breakdown_at, breakdown_old_warning) = checked_breakdown_at(input.breakdown_at.as_deref(), now_utc_second())
        .map_err(|e| AppError::ValidationFailed(vec![e]))?;
    let txn = db.begin().await?;
    let (_, status, _) = load_wo_status(&txn, input.wo_id).await?;
    let current = get_work_order(&txn, input.wo_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.wo_id.to_string(),
        })?;
    if !current.is_depannage {
        return Err(AppError::ValidationFailed(vec![refused(
            CODE_NOT_A_DEPANNAGE,
            "Cet OT n'est pas un dépannage.",
        )]));
    }
    if status.is_terminal() {
        return Err(AppError::ValidationFailed(vec![format!(
            "{CODE_CLOSED}:status={}: La date de constat de la panne ne se modifie plus une fois l'OT clôturé ou annulé.",
            crate::wo::workflow::state_machine::status_i18n_key(status.as_str())
        )]));
    }
    let now = crate::wo::time::now_utc_z();
    let result = txn
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE work_orders SET breakdown_at = ?, row_version = row_version + 1, updated_at = ? \
              WHERE id = ? AND row_version = ?",
            [
                breakdown_at.into(),
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
    Ok(WoDepannageResult {
        wo,
        breakdown_old_warning,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Synchronisation
// ═══════════════════════════════════════════════════════════════════════════════

/// Re-check of the dépannage facts of a received work order: a dépannage carries its "Panne
/// constatée à" (readable, not later than the moment its row was written, within the tolerated
/// clock difference) and its "Machine arrêtée"; a dépannage is never turned back into an ordinary
/// work order. A failed rule is a conflict for a person to decide. The gates of the dépannage
/// itself are judged by the work order's own checks (see the module documentation).
pub(crate) async fn inbound_depannage_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    received: &crate::permit::sync_check::Payload,
) -> AppResult<Option<InboundVerdict>> {
    use crate::permit::sync_check::{conflict, field, local_wire, overlay};
    let flag = |p: &crate::permit::sync_check::Payload, key: &str| match p.get(key) {
        Some(serde_json::Value::Bool(b)) => Some(*b),
        Some(serde_json::Value::Number(n)) => n.as_i64().map(|v| v != 0),
        _ => None,
    };
    let local = local_wire(conn, "work_orders", entity_sync_id).await?;
    let wo = overlay(local.as_ref(), received);
    let code = field(&wo, "code").unwrap_or(entity_sync_id).to_string();
    let was_depannage = local.as_ref().and_then(|l| flag(l, "is_depannage")) == Some(true);
    let is_depannage = flag(&wo, "is_depannage") == Some(true);
    if was_depannage && !is_depannage {
        return conflict(
            CODE_SYNC_FACTS_INVALID,
            format!("work order {code} is a dépannage here and was received as an ordinary work order"),
        );
    }
    if !is_depannage {
        return Ok(None);
    }
    if flag(&wo, "machine_stopped").is_none() {
        return conflict(
            CODE_SYNC_FACTS_INVALID,
            format!("dépannage {code} was received without its \"machine arrêtée\""),
        );
    }
    let Some(found) = field(&wo, "breakdown_at").and_then(crate::settings::site::instant) else {
        return conflict(
            CODE_SYNC_FACTS_INVALID,
            format!("dépannage {code} was received without a readable \"panne constatée à\""),
        );
    };
    let skew = Duration::minutes(MAX_CLOCK_SKEW_MINUTES);
    let written = field(&wo, "updated_at").and_then(crate::settings::site::instant);
    if found > crate::clock::now() + skew || written.is_some_and(|w| found > w + skew) {
        return conflict(
            CODE_SYNC_FACTS_INVALID,
            format!("dépannage {code}: its breakdown ({found}) is later than the moment its row was written"),
        );
    }
    Ok(None)
}
