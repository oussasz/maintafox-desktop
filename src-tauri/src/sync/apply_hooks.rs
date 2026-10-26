//! Local consequences of applying an inbound row (Phase 2.4).
//!
//! The generic materializer writes the replicated columns. Some tables also feed data that each
//! device derives for itself; those derivations run here, in the same transaction as the apply:
//! - `inventory_transactions`: a received movement changes `stock_balances` of its location
//!   (`inventory::ledger`), never refused; a location left below zero gets its one review item
//!   "négatif — à rapprocher", closed again by a count or a later entry (`inventory::negative_stock`);
//!   the same count posted on two PCs by this version (one seeded id) keeps on every PC the version
//!   the server received first, for the movement and its audit link, without moving the stock again
//!   (`adopt_existing_append_only`, `inventory::count_sync`); one posted twice by older versions
//!   (two ids) is applied like any movement and listed "comptage passé deux fois — à vérifier";
//! - `org_nodes`: `ancestor_path` / `depth` (paths of local ids) are recomputed for the node and
//!   its subtree;
//! - `user_dismissals`: `scope_key` (`<issue_code>:<local equipment id>`) is rebuilt;
//! - `report_schedules`: the next run of this device is computed from the cron expression;
//! - `work_order_delay_segments`: before the apply, `hold_kind` must be `pause` or `prerequisite`
//!   (anything else is refused as a conflict) and present (a row from an older app version without
//!   it is deferred, never written with a guessed kind: it decides the PRD lifecycle state);
//! - `work_order_parts`: a line received as cancelled is refused as a conflict when it carries a
//!   used or issued quantity, or when this device's copy of it has recorded use (used / issued
//!   quantity, or stock issued from the store against its reservation): only a line with no
//!   recorded use is cancelled (`wo::parts::inbound_part_line_check`, `before_upsert_checked`);
//! - `app_settings`: a tenant setting whose change is locked here (fiscal start month, currency
//!   once postings exist) is refused as a conflict (`before_upsert_checked`); a received row that is
//!   not a tenant setting, or that would change or delete a setting of this device, is refused as a
//!   `SETTING_DEVICE_SCOPE` conflict (`inbound_device_setting_check`);
//! - `reliability_study_decisions`: an approval by the decision's own proposer is refused as a
//!   conflict unless the decision's site opted out of four-eyes; a decision still proposed or
//!   rejected that names a work order or PM version created from it is refused (`INVALID_VALUE`,
//!   audit finding AU-69); a study, equipment or site not applied yet makes it wait
//!   (`reliability::study::decisions::inbound_self_approval_check`); an approved or applied one is
//!   re-checked (status never backwards, content and approval seal unchanged, designated approver,
//!   the approver's and applier's rights here; a decision approved by a PC that does not seal yet
//!   waits, shown as waiting for that PC's update); a decision refused on another PC is taken as
//!   history, or its marks only over the one held here, which keeps its status and links; a work
//!   order created here from a decision refused since is kept and told of
//!   (`reliability::study::decision_sync`, AU-76b);
//! - `site_settings` / `site_setting_events`: a per-site rule relaxing a site is applied only with
//!   its audited change (step-up, reason, settings right), waiting for that event while it is in
//!   the inbox, a conflict otherwise; unknown rules and unreadable values are conflicts; a rule
//!   held on here moved by an organisation publish to the node replacing its deleted node is
//!   applied, moved anywhere else it is a conflict
//!   (`settings::site::inbound_site_setting_check`, `inbound_site_setting_event_check`);
//! - `weekly_schedules`: a received week that would go backwards in its lifecycle here (frozen
//!   back to draft, a later baseline back to an earlier one) is refused as a conflict
//!   (`planning::weekly_schedule::inbound_lifecycle_conflict`, `before_upsert_checked`); a week
//!   reopened (frozen back to approved or draft, same baseline) comes with its unfreeze decision,
//!   decided by another person than the requester holding plan.confirm at the site here
//!   (`planning::sync_check::inbound_unfreeze_check`);
//! - `schedule_commitments` / `schedule_break_ins`: work entering a week frozen here comes with its
//!   break-in (here, or waiting in the inbox and applied with it in one transaction), else it is a
//!   `PLANNING_WEEK_FROZEN` conflict at once, closed when a valid break-in arrives later; a received
//!   break-in that is urgent or forced a blocking check is signed by a second planner holding
//!   plan.confirm at the crew's site here (`planning::sync_check`);
//! - `work_permits` / `permit_isolations` / `work_orders`: the permit and LOTO rules (activation,
//!   close, hand-back, suspension, two-person verification, the work-order start gate) are checked
//!   again on the received state; a failed rule is a conflict, a permit type or status not applied
//!   yet makes the row wait (`permit::sync_check`);
//!   A work order whose permit flag set by a plant rule is cleared names who cleared it (holding the
//!   permit-issue right here), when (before execution) and why (`inbound_permit_rule_check`); so
//!   does one whose requirement came from a request flagged "sécurité" (`permit_from_safety_di`,
//!   migration 255: the right at its site), and that origin is never turned off by a received row;
//!   a received work order that names another rule than the one held here is refused once started,
//!   and applied before, both PCs keeping the same rule (`requirement_rules::converge_received_rule`);
//!   one that carries no rule is applied (see the plant rules below), and over a rule held here it
//!   is judged with that rule and keeps it, with its permit type, requirement and clearing held here
//!   (`before_write`, `permit::sync_check::keep_held_rule_trace`, audit finding AU-04 R2-B1);
//! - `work_orders` / `reference_values` / `reference_sets`: after the apply, the plant's permit rules
//!   held here are applied to a received work order not started yet that has no rule, and to every
//!   such work order when the rule list is received (`permit::requirement_rules::stamp_matching_rule`,
//!   owner decision D2 (a), audit finding AU-04: each PC derives the same rule from the same work order
//!   and list); a received work order already started without the rule that matches it here, or
//!   without the rule held here, is applied and recorded once as a safety finding, the permit-issue
//!   right holders of its site told (`requirement_rules::finding_for_started_without_rule`; not for
//!   plant history received by a snapshot);
//! - `work_orders` / `intervention_requests`: after the apply, a request flagged "sécurité" here
//!   protects its work orders not started yet (origin, permit required, a clearing withdrawn); a
//!   work order new here received without the column and already started takes that origin; a
//!   protected work order required again but still carrying its clearing ends protected
//!   (`after_received_work_order`, recorded in its history and audit trail);
//! - `work_orders` changing status (or new here): applied with its waiting reservations, tasks,
//!   failure coding, labour, parts, downtime, tools, verifications and status changes; entering
//!   `completed` or `closed`, then judged by the local completion and close gates on the written
//!   state; `work_order_tasks` / `work_order_failure_details` / `work_order_interveners` /
//!   `work_order_tools` / `work_order_verifications` (changes and deletes): refused after the
//!   completion or close here, a task result outside execution and a mandatory task removed past
//!   planning refused, and a verification keeps the self-verification rules
//!   (`wo::closeout_sync`); snapshot history is applied, not judged;
//! - `permit_types`: a change of the three-person lock-removal setting, or a switch-off of the
//!   energy-isolation rule, names who changed it (holding the settings and permit-issue rights
//!   here), when and why; the shipped LOTO type's isolation rule is never switched off
//!   (`inbound_permit_type_check`);
//! - `site_settings` (the four-eyes opt-out) / `permit_types` (the three-person setting): a change
//!   accepted by those checks is announced to the settings administrators in the same transaction
//!   (`settings::received_notice`);
//! - `work_permits`: a permit received closed, handed back, cancelled or suspended while its work
//!   order runs here, or dated before the work order's last start here, is applied and raised for
//!   review (`PERMIT_ENDED_DURING_WORK` / `PERMIT_ENDED_BEFORE_START`), its work order's responsible
//!   and the site's permit close-right holders told (`permit::ended_under_work`);
//! - `permit_handover_logs`: a hand-over received by the person who signed it is refused as a
//!   conflict (`permit::sync_check::inbound_handover_check`); one without a signer is accepted;
//! - `permit_handover_logs`: after the apply, the log's "from" / "to" role that is not in the
//!   plant's list (free text from a PC that has not upgraded) is added to it flagged "à revoir"
//!   (`permit::handover_roles::adopt_received_roles`; only that log's two roles);
//! - `inventory_count_lines` / `inventory_transactions`: a count line approved by its counter above
//!   the four-eyes tolerance where four-eyes applies here is a conflict, and the count's stock
//!   movements wait with it (neither applies) until a second approval or the count's reversal
//!   arrives (`inventory::count_four_eyes`, CMMS Advisor Q8);
//! - `qualification_overrides`: reason, subject, context, no self-override and the approver's
//!   certification right (`qualification::sync_check`);
//! - `media_attachments`: the file is queued for download from object storage
//!   (`sync_media_downloads`, worked by the sync worker); a deleted row drops a pending download.
//! - `inspection_anomalies`: an anomaly is decided here only by routing or deferring it; a received
//!   row that closes it without a decision, reopens a decided one, changes its link or clears its
//!   "revue permis requise" is refused as a conflict (`inspection::sync_check`, audit finding AU-72);
//! - `user_accounts`: the built-in "admin" account received with its password never changed is
//!   disabled on arrival when this PC has another administrator or did not have the account
//!   (`auth::builtin_admin::after_inbound_user_account`).
//! - `user_accounts`: an account received with a username already used here by another account
//!   (trimmed, case-insensitive) is refused as a `USERNAME_CLASH` conflict, never renamed or merged
//!   (`sync::username_clash::inbound_username_clash`).
//! - `user_accounts` / `user_scope_assignments` / `roles`: a received change or delete that would
//!   leave this PC without an active administrator is refused as a conflict
//!   (`auth::last_admin::inbound_change_check`, `before_upsert_checked`, `before_delete_checked`).
//! - `user_scope_assignments` / `role_permissions` / `user_accounts`: a received grant (new, or
//!   widened) whose author does not hold the right to make it here, now, is refused as a
//!   `GRANT_BY_UNAUTHORISED_ADMIN` conflict, waiting while its author is not applied yet; after the
//!   apply, a change that took that right away from someone lists the grants they made since in one
//!   review notice to the administrators (`sync::grant_check`, from `row_codec::apply_row`).
//! - `admin_recovery_notices` / `admin_recovery_acknowledgements`: a notice or acknowledgement
//!   already here is never changed, and an acknowledgement made by anyone but the notice's
//!   recipient is refused as a conflict (`auth::recovery_code::inbound_notice_check`,
//!   `inbound_acknowledgement_check`); both are append-only, so a received delete is a conflict.
//! - `work_orders` / `intervention_requests` / `planning_windows`: a site given to a record without
//!   one (CMMS Advisor decision Q1) is accepted only when it names who gave it (holding the
//!   organisation administration right plant-wide here), when and why, and the record still has no
//!   site here; the trace is never rewritten; a work order or request given a site without that
//!   trace is a conflict (`org::site_assignment::inbound_site_assignment_check`).

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::errors::{AppError, AppResult};
use crate::sync::row_codec::{located_row_id, RowLocator};
use crate::sync::row_layout::EntityLayout;

/// Checks an inbound row before it is written; `Some` stops the apply with that outcome.
pub(crate) fn before_upsert(
    layout: &EntityLayout,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> Option<crate::sync::materialize::ApplyOutcome> {
    use crate::sync::materialize::{ApplyOutcome, DEFER_MISSING_FIELD};
    match layout.def.entity_type {
        "work_order_delay_segments" => match payload.get("hold_kind") {
            None | Some(serde_json::Value::Null) => Some(ApplyOutcome::Deferred {
                code: DEFER_MISSING_FIELD.into(),
                message: "hold_kind missing: sent by an older app version; waiting for an update of that device".into(),
                waits_for: None,
            }),
            Some(serde_json::Value::String(k)) if crate::wo::lifecycle::HoldKind::try_from_str(k).is_some() => None,
            Some(other) => Some(ApplyOutcome::Conflict {
                code: "INVALID_VALUE".into(),
                message: format!("hold_kind must be 'pause' or 'prerequisite', got {other}"),
            }),
        },
        _ => None,
    }
}

/// Conflict code of an audit chain stub refused on arrival (`audit::chain::inbound_stub_check`).
pub(crate) const AUDIT_STUB_REFUSED: &str = "AUDIT_STUB_REFUSED";

/// Checks of an inbound row that need the local database; `Some` stops the apply with that
/// outcome (recorded like every other sync conflict, for a person to decide).
/// - `app_settings`: the fiscal-year start month and the tenant currency cannot change once this
///   device stores postings that depend on them (`finance::fiscal`, `inventory::fx`). A change
///   made on a device where they were still free is refused here, as it is in Settings. A setting
///   of one device is never received (`inbound_device_setting_check`).
/// - `weekly_schedules`: a week never moves backwards in its lifecycle by synchronisation.
/// - `audit_chain_stubs`: never this PC's own chain, never malformed (`audit::chain`).
pub(crate) async fn before_upsert_checked(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<crate::sync::materialize::ApplyOutcome>> {
    use crate::sync::materialize::ApplyOutcome;
    if layout.def.entity_type == "audit_chain_stubs" {
        return Ok(crate::audit::chain::inbound_stub_check(conn, payload)
            .await?
            .map(|message| ApplyOutcome::Conflict {
                code: AUDIT_STUB_REFUSED.into(),
                message,
            }));
    }
    // `user_accounts`: a username already used here by another account is a coded conflict, not a
    // failed insert (`sync::username_clash::inbound_username_clash`).
    if layout.def.entity_type == "user_accounts" {
        if let Some(outcome) =
            crate::sync::username_clash::inbound_username_clash(conn, entity_sync_id, payload).await?
        {
            return Ok(Some(outcome));
        }
    }
    // `user_scope_assignments` / `role_permissions` / `user_accounts`: a grant whose author does not
    // hold the right to make it here, now, is a conflict (`sync::grant_check`).
    if let Some(outcome) = crate::sync::grant_check::inbound_grant_check(conn, layout, entity_sync_id, payload).await? {
        return Ok(Some(outcome));
    }
    // `user_accounts` / `user_scope_assignments` / `roles`: a change that would leave this PC
    // without an active administrator is a conflict (`auth::last_admin`).
    if let Some(outcome) = crate::auth::last_admin::inbound_change_check(
        conn,
        layout,
        entity_sync_id,
        crate::auth::last_admin::InboundChange::Upsert(&serde_json::Value::Object(payload.clone()).to_string()),
    )
    .await?
    {
        return Ok(Some(outcome));
    }
    // Refusal marks, four-eyes, then the approval re-check: status, content and seal, designated
    // approver, the approver's and applier's rights here (`reliability::study::decision_sync`).
    if layout.def.entity_type == "reliability_study_decisions" {
        return crate::reliability::study::decision_sync::inbound_decision_check(conn, entity_sync_id, payload).await;
    }
    // A suspension made and lifted on another PC around a start here: raised for review and told
    // (`permit::ended_under_work::after_suspension_check`, CMMS Advisor answer to S3). Rolled back
    // with the row when it is then refused or left waiting.
    if layout.def.entity_type == "permit_suspensions" {
        crate::permit::ended_under_work::after_suspension_check(conn, entity_sync_id, payload).await?;
    }
    let safety_check = match layout.def.entity_type {
        "site_settings" => {
            Some(crate::settings::site::inbound_site_setting_check(conn, entity_sync_id, payload).await?)
        }
        // The notice of a safety rule changed on another PC cannot be muted or re-routed by sync.
        "notification_rules"
        | "notification_categories"
        | "notification_events"
        | "notifications"
        | "notification_acknowledgements"
        | "notification_preferences" => Some(
            crate::settings::received_notice::inbound_local_only_check(
                conn,
                layout.def.entity_type,
                entity_sync_id,
                Some(payload),
            )
            .await?,
        ),
        "site_setting_events" => Some(crate::settings::site::inbound_site_setting_event_check(payload).await?),
        "permit_types" => {
            Some(crate::permit::sync_check::inbound_permit_type_check(conn, entity_sync_id, payload).await?)
        }
        "work_permits" => Some(crate::permit::sync_check::inbound_permit_check(conn, entity_sync_id, payload).await?),
        // Four-eyes on a count variance above the tolerance (D11, `inventory::count_four_eyes`).
        "inventory_count_lines" => {
            Some(crate::inventory::count_four_eyes::inbound_count_line_check(conn, entity_sync_id, payload).await?)
        }
        // A count's stock movement is held with its line while that line is refused or waits here
        // (CMMS Advisor Q8, `inventory::count_four_eyes::inbound_count_movement_hold`).
        "inventory_transactions" => {
            Some(crate::inventory::count_four_eyes::inbound_count_movement_hold(conn, entity_sync_id, payload).await?)
        }
        // A downtime end received while a lock is fitted here (owner decision D4, condition 2).
        "work_order_downtime_segments" => {
            Some(crate::permit::ended_hold::inbound_downtime_end_check(conn, entity_sync_id, payload).await?)
        }
        "permit_isolations" => {
            Some(crate::permit::sync_check::inbound_isolation_check(conn, entity_sync_id, payload).await?)
        }
        "permit_handover_logs" => {
            Some(crate::permit::sync_check::inbound_handover_check(conn, entity_sync_id, payload).await?)
        }
        "work_orders" => Some(
            match crate::permit::sync_check::inbound_permit_rule_check(conn, entity_sync_id, payload).await? {
                Some(verdict) => Some(verdict),
                None => {
                    let permit_gate =
                        crate::permit::sync_check::inbound_work_order_check(conn, entity_sync_id, payload).await?;
                    let closeout = match permit_gate {
                        Some(verdict) => Some(verdict),
                        None => crate::wo::closeout_sync::inbound_closeout_check(conn, entity_sync_id, payload).await?,
                    };
                    match closeout {
                        Some(verdict) => Some(verdict),
                        None => {
                            crate::org::site_assignment::inbound_site_assignment_check(
                                conn,
                                crate::org::site_assignment::NoSiteKind::WorkOrder,
                                entity_sync_id,
                                payload,
                            )
                            .await?
                        }
                    }
                }
            },
        ),
        "intervention_requests" => Some(
            crate::org::site_assignment::inbound_site_assignment_check(
                conn,
                crate::org::site_assignment::NoSiteKind::Request,
                entity_sync_id,
                payload,
            )
            .await?,
        ),
        "planning_windows" => Some(
            crate::org::site_assignment::inbound_site_assignment_check(
                conn,
                crate::org::site_assignment::NoSiteKind::PlanningWindow,
                entity_sync_id,
                payload,
            )
            .await?,
        ),
        "qualification_overrides" => {
            Some(crate::qualification::sync_check::inbound_override_check(conn, entity_sync_id, payload).await?)
        }
        // Work entering a frozen week, and break-ins: four-eyes signature at the crew's site
        // (`planning::sync_check`, audit findings AU-06, AU-102).
        "schedule_commitments" => {
            Some(crate::planning::sync_check::inbound_commitment_check(conn, entity_sync_id, payload).await?)
        }
        "schedule_break_ins" => {
            Some(crate::planning::sync_check::inbound_break_in_check(conn, entity_sync_id, payload).await?)
        }
        "work_order_parts" => Some(crate::wo::parts::inbound_part_line_check(conn, entity_sync_id, payload).await?),
        // An anomaly closed without a decision, reopened, its link changed or its "revue permis
        // requise" cleared (`inspection::sync_check`, audit finding AU-72).
        "inspection_anomalies" => {
            Some(crate::inspection::sync_check::inbound_anomaly_check(conn, entity_sync_id, payload).await?)
        }
        // A task, failure coding, labour line, tool or verification changed after its work order's
        // completion or close here (`wo::closeout_sync`, R5-R7).
        "work_order_tasks"
        | "work_order_failure_details"
        | "work_order_interveners"
        | "work_order_tools"
        | "work_order_verifications" => Some(
            crate::wo::closeout_sync::inbound_closeout_child_check(
                conn,
                layout.def.entity_type,
                entity_sync_id,
                payload,
            )
            .await?,
        ),
        crate::auth::recovery_code::NOTICE_ENTITY => {
            Some(crate::auth::recovery_code::inbound_notice_check(conn, entity_sync_id, payload).await?)
        }
        crate::auth::recovery_code::ACK_ENTITY => {
            Some(crate::auth::recovery_code::inbound_acknowledgement_check(conn, entity_sync_id, payload).await?)
        }
        _ => None,
    };
    if let Some(verdict) = safety_check {
        use crate::settings::site::InboundVerdict;
        // A site's four-eyes opt-out or a permit type's three-person setting changed on another PC
        // and accepted here: the settings administrators are told, in this apply's transaction (a
        // row then refused or left waiting rolls it back; `settings::received_notice`).
        if verdict.is_none() {
            crate::settings::received_notice::notify_received_rule_change(
                conn,
                layout.def.entity_type,
                entity_sync_id,
                payload,
            )
            .await?;
            // A permit ended on another PC while its work ran here, or before it started here:
            // applied, raised for review and told (`permit::ended_under_work`, AU-101, D9).
            if layout.def.entity_type == "work_permits" {
                crate::permit::ended_under_work::after_permit_check(conn, entity_sync_id, payload).await?;
            }
        }
        return Ok(verdict.map(|v| match v {
            InboundVerdict::Defer(message) => ApplyOutcome::Deferred {
                code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                message,
                waits_for: None,
            },
            InboundVerdict::Conflict { code, message } => ApplyOutcome::Conflict {
                code: code.into(),
                message,
            },
        }));
    }
    if layout.def.entity_type == "weekly_schedules" {
        if let Some(message) =
            crate::planning::weekly_schedule::inbound_lifecycle_conflict(conn, entity_sync_id, payload).await?
        {
            return Ok(Some(ApplyOutcome::Conflict {
                code: "WEEK_LIFECYCLE_BACKWARDS".into(),
                message,
            }));
        }
        // A reopening needs its four-eyes unfreeze decision (S4, `planning::sync_check`).
        use crate::settings::site::InboundVerdict;
        return Ok(
            crate::planning::sync_check::inbound_unfreeze_check(conn, entity_sync_id, payload)
                .await?
                .map(|v| match v {
                    InboundVerdict::Defer(message) => ApplyOutcome::Deferred {
                        code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                        message,
                        waits_for: None,
                    },
                    InboundVerdict::Conflict { code, message } => ApplyOutcome::Conflict {
                        code: code.into(),
                        message,
                    },
                }),
        );
    }
    if layout.def.entity_type != "app_settings" {
        return Ok(None);
    }
    if let Some(outcome) = inbound_device_setting_check(conn, entity_sync_id, Some(payload)).await? {
        return Ok(Some(outcome));
    }
    let text = |k: &str| payload.get(k).and_then(serde_json::Value::as_str);
    let (Some(key), Some(value)) = (text("setting_key"), text("setting_value_json")) else {
        return Ok(None);
    };
    let guard = async {
        crate::finance::fiscal::guard_fiscal_start_write(conn, key, value).await?;
        crate::inventory::fx::guard_currency_write(conn, key, value).await
    };
    match guard.await {
        Ok(()) => Ok(None),
        Err(AppError::ValidationFailed(messages)) => Ok(Some(ApplyOutcome::Conflict {
            code: "SETTING_LOCKED".into(),
            message: format!("{key}: {}", messages.join("; ")),
        })),
        Err(other) => Err(other),
    }
}

/// Conflict code of an `app_settings` row received for a setting of one device (`setting_scope`
/// other than `tenant`).
pub(crate) const SETTING_DEVICE_SCOPE: &str = "SETTING_DEVICE_SCOPE";

/// `app_settings` mixes tenant settings (synchronised) and settings of this device (licence
/// onboarding, bootstrap state, updater, diagnostics), which never leave the PC: the outbound row
/// filter of `sync::row_spec` sends tenant rows only. A received row that is not a tenant setting
/// (`payload`, or no scope at all), or that would change or delete a device setting held here
/// under its sync id, comes from an abnormal sender: it is refused as a conflict for a person to
/// see, never written.
async fn inbound_device_setting_check(
    conn: &impl ConnectionTrait,
    entity_sync_id: &str,
    payload: Option<&serde_json::Map<String, serde_json::Value>>,
) -> AppResult<Option<crate::sync::materialize::ApplyOutcome>> {
    use crate::sync::materialize::ApplyOutcome;
    let refused = |message: String| {
        Ok(Some(ApplyOutcome::Conflict {
            code: SETTING_DEVICE_SCOPE.into(),
            message,
        }))
    };
    if let Some(payload) = payload {
        let scope = payload.get("setting_scope").and_then(serde_json::Value::as_str);
        if scope != Some("tenant") {
            return refused(format!(
                "app_settings: a setting of scope {scope:?} belongs to one device and is never received by synchronisation"
            ));
        }
    }
    let local_scope = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT setting_scope FROM app_settings WHERE entity_sync_id = ? LIMIT 1",
            [entity_sync_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<Option<String>>("", "setting_scope"))
        .transpose()
        .map_err(|e| hook_err("app_settings", e))?
        .flatten();
    match local_scope {
        Some(scope) if scope != "tenant" => refused(format!(
            "app_settings: the row held here under this sync id is a setting of this device (scope {scope:?}): \
             it is never changed or deleted by synchronisation"
        )),
        _ => Ok(None),
    }
}

/// An append-only row received while this PC holds a row with the same sync id: `true` when the
/// received content replaces it instead of the replay being skipped. Only the same stock count
/// posted on two PCs (seeded id): the version the server received first is kept on every PC, this
/// PC's posting is audited (`inventory::count_sync::adopt_first_posted`).
pub(crate) async fn adopt_existing_append_only(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<bool> {
    match layout.def.entity_type {
        "inventory_transactions" | "inventory_mutation_audit_links" => {
            crate::inventory::count_sync::adopt_first_posted(conn, layout.def.entity_type, entity_sync_id, payload)
                .await
        }
        // A break-in refused on the PC that received it comes back with its refusal marks only
        // (CMMS Advisor decision 2026-10-25); any other rewrite of a break-in is skipped.
        "schedule_break_ins" => crate::planning::sync_check::refusal_marks_only(conn, entity_sync_id, payload).await,
        _ => Ok(false),
    }
}

/// Rows waiting in the inbox that a received permit or work-order start was judged on
/// (`permit::sync_check`): applied right after it, in its transaction (`after_upsert_group`).
pub(crate) async fn upsert_group(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<crate::permit::sync_check::Group>> {
    match layout.def.entity_type {
        "work_permits" => crate::permit::sync_check::permit_group(conn, entity_sync_id, payload).await,
        // A held count adjustment released with its reversal: one transaction (CMMS Advisor Q8).
        "inventory_transactions" => {
            crate::inventory::count_four_eyes::count_movement_group(conn, entity_sync_id, payload).await
        }
        // A start (or resume, reopen) brings its permits, waits and status changes; every status
        // change (and a work order new here) brings its close-out children too: one group.
        "work_orders" => {
            let start = crate::permit::sync_check::work_order_group(conn, entity_sync_id, payload).await?;
            let closeout = crate::wo::closeout_sync::closeout_group(conn, entity_sync_id, payload).await?;
            Ok(match (start, closeout) {
                (Some(start), Some(closeout)) => Some(start.followed_by(closeout)),
                (start, closeout) => start.or(closeout),
            })
        }
        // A commitment entering a frozen week brings its break-in, judged with it.
        "schedule_commitments" => crate::planning::sync_check::commitment_group(conn, entity_sync_id, payload).await,
        _ => Ok(None),
    }
}

/// Applies a group (`upsert_group`) after its row was written, then, for a received work-order
/// completion or close, judges it on the written state (`wo::closeout_sync::judge_closeout`);
/// `Some` (a conflict, or a child that waits) makes the caller roll the whole transaction back:
/// nothing of the group is kept.
pub(crate) async fn after_upsert_group(
    conn: &impl ConnectionTrait,
    group: crate::permit::sync_check::Group,
) -> AppResult<Option<crate::sync::materialize::ApplyOutcome>> {
    let closeout = group.closeout.clone();
    if let Some(outcome) = crate::permit::sync_check::apply_group(conn, group).await? {
        return Ok(Some(outcome));
    }
    match closeout {
        Some(entry) => crate::wo::closeout_sync::judge_closeout(conn, &entry).await,
        None => Ok(None),
    }
}

/// Checks of an inbound delete before it is applied; `Some` stops it with that outcome. A permit
/// past draft with its isolations and suspensions, a permit type and a qualification requirement
/// are never deleted on this device: such a delete received by sync is a conflict
/// (`permit::sync_check::inbound_delete_check`).
pub(crate) async fn before_delete_checked(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
) -> AppResult<Option<crate::sync::materialize::ApplyOutcome>> {
    use crate::sync::materialize::ApplyOutcome;
    // A delete that would leave this PC without an active administrator (`auth::last_admin`).
    if let Some(outcome) = crate::auth::last_admin::inbound_change_check(
        conn,
        layout,
        entity_sync_id,
        crate::auth::last_admin::InboundChange::Delete,
    )
    .await?
    {
        return Ok(Some(outcome));
    }
    // A setting of this device is never deleted by sync (`inbound_device_setting_check`).
    if layout.def.entity_type == "app_settings" {
        if let Some(outcome) = inbound_device_setting_check(conn, entity_sync_id, None).await? {
            return Ok(Some(outcome));
        }
    }
    let verdict = match layout.def.entity_type {
        "work_permits" | "permit_isolations" | "permit_suspensions" | "permit_types" | "qualification_requirements" => {
            crate::permit::sync_check::inbound_delete_check(conn, layout.def.entity_type, entity_sync_id).await?
        }
        // Rows of the safety rule notice are local to each PC: never deleted by sync.
        "notification_rules"
        | "notification_categories"
        | "notification_events"
        | "notifications"
        | "notification_acknowledgements"
        | "notification_preferences" => {
            crate::settings::received_notice::inbound_local_only_check(
                conn,
                layout.def.entity_type,
                entity_sync_id,
                None,
            )
            .await?
        }
        // A task, failure coding, labour line, tool or verification of a work order completed or
        // closed here is kept (`wo::closeout_sync`, R5 / R7).
        "work_order_tasks"
        | "work_order_failure_details"
        | "work_order_interveners"
        | "work_order_tools"
        | "work_order_verifications" => {
            crate::wo::closeout_sync::inbound_closeout_child_delete_check(conn, layout.def.entity_type, entity_sync_id)
                .await?
        }
        _ => None,
    };
    Ok(verdict.map(|v| match v {
        crate::settings::site::InboundVerdict::Defer(message) => ApplyOutcome::Deferred {
            code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
            message,
            waits_for: None,
        },
        crate::settings::site::InboundVerdict::Conflict { code, message } => ApplyOutcome::Conflict {
            code: code.into(),
            message,
        },
    }))
}

fn hook_err(table: &str, e: impl std::fmt::Display) -> AppError {
    AppError::SyncError(format!("{table} apply hook: {e}"))
}

/// What [`after_upsert`] must know of the row held here, read before a received row is written.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HeldBeforeWrite {
    /// A count line: the "above the tolerance" flag held here when the received version is one
    /// this PC already judged (`inventory::count_four_eyes::held_judgement`).
    pub count_line_judged: Option<bool>,
    /// A reliability decision held here without a refusal mark (`Some(true)`), before the write.
    pub decision_unrefused: Option<bool>,
}

/// Reads [`HeldBeforeWrite`] for a received row about to be written.
pub(crate) async fn held_before_write(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<HeldBeforeWrite> {
    Ok(match layout.def.entity_type {
        "inventory_count_lines" => HeldBeforeWrite {
            count_line_judged: crate::inventory::count_four_eyes::held_judgement(conn, entity_sync_id, payload).await?,
            ..HeldBeforeWrite::default()
        },
        "reliability_study_decisions" => HeldBeforeWrite {
            decision_unrefused: crate::reliability::study::decision_sync::held_unrefused(conn, entity_sync_id).await?,
            ..HeldBeforeWrite::default()
        },
        _ => HeldBeforeWrite::default(),
    })
}

/// After an inbound row passed its checks, before it is written: columns of the row that are kept
/// as held here instead of the received values, in the apply's transaction (capture suppressed: the
/// other PCs do the same). `work_orders`: a row that carries no plant rule over the rule held here
/// keeps the rule, its permit type and, without a clearing in the row, the requirement and the
/// clearing held here; a row that names another rule is noted (`permit::sync_check::
/// keep_held_rule_trace`, audit finding AU-04 R2-B1 / R2-S1). The note goes to [`after_upsert`].
pub(crate) async fn before_write(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &mut serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<crate::permit::sync_check::HeldRule>> {
    match layout.def.entity_type {
        "work_orders" => crate::permit::sync_check::keep_held_rule_trace(conn, entity_sync_id, payload).await,
        // A refused decision received over the one held here takes the marks only; a refusal held
        // here keeps its marks (`reliability::study::decision_sync::keep_held_progress`).
        "reliability_study_decisions" => {
            crate::reliability::study::decision_sync::keep_held_progress(conn, entity_sync_id, payload).await?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// After an inbound row was inserted (`inserted`) or updated; `payload` is the row as written
/// (received, with the columns [`before_write`] kept as held here, `held_rule` its note); `held` is
/// what [`held_before_write`] read of the row held here before the write.
pub(crate) async fn after_upsert(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    row: &RowLocator,
    inserted: bool,
    payload: &serde_json::Map<String, serde_json::Value>,
    held: HeldBeforeWrite,
    held_rule: Option<&crate::permit::sync_check::HeldRule>,
) -> AppResult<()> {
    match layout.def.entity_type {
        "inventory_transactions" if inserted => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "movement not found after insert"))?;
            apply_movement_to_balance(conn, id).await
        }
        "org_nodes" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "node not found after apply"))?;
            recompute_org_paths(conn, id).await
        }
        // A received count line's "above the tolerance" flag is this PC's judgement (CMMS Advisor
        // Q14, `inventory::count_four_eyes::store_judged_critique`): the derived auto-approbation
        // and the "critique" badge read it. A version already judged here keeps the flag held.
        "inventory_count_lines" => match row {
            RowLocator::Identity(sync_id) => {
                crate::inventory::count_four_eyes::store_judged_critique(conn, sync_id, held.count_line_judged).await
            }
            RowLocator::Natural(_) => Err(hook_err(layout.table(), "count line located without its sync id")),
        },
        // A free-text hand-over role from a PC that has not upgraded joins the plant's list now,
        // flagged "à revoir" (`permit::handover_roles`), not only at the next start of this PC:
        // this log's two roles only.
        // A received decision: sealed here when accepted unsealed over the proposal held here; refused
        // (new here, or newly here), the work order or PM version it gave is kept and told of
        // (CMMS Advisor Q6, S4; `reliability::study::decision_sync::after_decision_written`).
        "reliability_study_decisions" => match row {
            RowLocator::Identity(sync_id) => {
                crate::reliability::study::decision_sync::after_decision_written(
                    conn,
                    sync_id,
                    inserted,
                    held.decision_unrefused,
                )
                .await
            }
            RowLocator::Natural(_) => Ok(()),
        },
        "permit_handover_logs" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "hand-over log not found after apply"))?;
            crate::permit::handover_roles::adopt_received_roles(conn, id).await
        }
        // Two concurrent versions of a PM plan, or an occurrence due twice: the PM planners are told
        // (`pm::version_clash`).
        "pm_plan_versions" if inserted => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "version not found after insert"))?;
            crate::pm::version_clash::after_received_version(conn, id).await
        }
        "pm_occurrences" if inserted => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "occurrence not found after insert"))?;
            crate::pm::version_clash::after_received_occurrence(conn, id).await
        }
        // The plant's permit rule list received: its rules apply to the work orders not started yet
        // here (`permit::requirement_rules::stamp_open_work_orders`).
        "reference_values" | "reference_sets" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "reference row not found after apply"))?;
            let set_id = if layout.def.entity_type == "reference_sets" {
                id
            } else {
                conn.query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT set_id FROM reference_values WHERE id = ?",
                    [id.into()],
                ))
                .await?
                .ok_or_else(|| hook_err(layout.table(), "reference value vanished"))?
                .try_get::<i64>("", "set_id")
                .map_err(|e| hook_err(layout.table(), e))?
            };
            if crate::permit::requirement_rules::is_rules_set(conn, set_id).await? {
                crate::permit::requirement_rules::stamp_open_work_orders(
                    conn,
                    None,
                    crate::permit::requirement_rules::stamp_causes::RULES_RECEIVED,
                )
                .await?;
            }
            Ok(())
        }
        // The permit protection a request flagged "sécurité" gives its work orders, derived here from
        // the request here, and the plant's permit rules (`after_received_work_order`).
        "work_orders" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "work order not found after apply"))?;
            let sent_without_origin = inserted && !payload.contains_key(crate::permit::sync_check::SAFETY_ORIGIN);
            after_received_work_order(conn, id, sent_without_origin, held_rule).await
        }
        // A request received flagged "sécurité" protects its work orders not started yet, as the PC
        // that flagged it did (`permit_requirement::raise_work_orders_of_safety_request`).
        "intervention_requests" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "request not found after apply"))?;
            crate::wo::workflow::actions::permit_requirement::raise_work_orders_of_safety_request(conn, id, None)
                .await?;
            Ok(())
        }
        "user_dismissals" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "dismissal not found after apply"))?;
            conn.execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE user_dismissals SET scope_key = issue_code || ':' || CAST(equipment_id AS TEXT) WHERE id = ?",
                [id.into()],
            ))
            .await?;
            Ok(())
        }
        "report_schedules" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "schedule not found after apply"))?;
            schedule_next_run(conn, id).await
        }
        "media_attachments" if inserted => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "media row not found after insert"))?;
            queue_received_media(conn, id).await
        }
        "user_accounts" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "account not found after apply"))?;
            crate::auth::builtin_admin::after_inbound_user_account(
                conn,
                id,
                inserted,
                crate::auth::builtin_admin::BuildProfile::current(),
            )
            .await
        }
        _ => Ok(()),
    }
}

/// A work order applied from a received row, in the apply's transaction (the same rules on every PC,
/// from the request held here; `permit::sync_check::inbound_permit_rule_check` judged the row with
/// them):
/// - not started yet (draft, planning, ready) and its request flagged "sécurité": it carries that
///   origin and requires a permit (`permit_requirement::raise_work_orders_of_safety_request`);
/// - new here, sent by a PC without migration 255 (`sent_without_origin`: no `permit_from_safety_di`
///   in the row), already started, its request flagged: it carries that origin;
/// - protected, required again but still carrying its clearing (a re-tick made by an older build):
///   its clearing is withdrawn, recorded (`withdraw_received_clearing_of_required`);
/// - not started yet and received naming another rule than the one held here (`held_rule`
///   `Differs`): the same rule kept on every PC (`requirement_rules::converge_received_rule`);
/// - not started yet with no plant rule: the rule of the list here that matches its equipment and work
///   type is recorded on it (`requirement_rules::stamp_matching_rule`; a row that changed them is
///   judged the same way, audit finding AU-04 S3);
/// - already started with no plant rule while one here matches it, or received without the rule
///   held here (`held_rule` `Kept`, the rule kept): recorded once as a safety finding
///   (`requirement_rules::finding_for_started_without_rule`), not for plant history received by a
///   snapshot.
async fn after_received_work_order(
    conn: &impl ConnectionTrait,
    wo_id: i64,
    sent_without_origin: bool,
    held_rule: Option<&crate::permit::sync_check::HeldRule>,
) -> AppResult<()> {
    use crate::permit::sync_check::HeldRule;
    let source_di: Option<i64> = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT source_di_id FROM work_orders WHERE id = ?",
            [wo_id.into()],
        ))
        .await?
        .and_then(|r| r.try_get::<Option<i64>>("", "source_di_id").ok().flatten());
    if let Some(di) = source_di {
        crate::wo::workflow::actions::permit_requirement::raise_work_orders_of_safety_request(conn, di, None).await?;
    }
    if sent_without_origin {
        conn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE work_orders SET permit_from_safety_di = 1 \
              WHERE id = ? AND permit_from_safety_di = 0 AND EXISTS ( \
                SELECT 1 FROM intervention_requests ir \
                 WHERE ir.id = work_orders.source_di_id AND ir.safety_flag = 1)",
            [wo_id.into()],
        ))
        .await?;
    }
    // A row that named a rule other than the one held here, or a rule while none is held here, on a
    // work order not started: each PC keeps the rule its own list gives (R2-S1, R3-B1, R4-B1).
    match held_rule {
        Some(HeldRule::Differs { held }) => {
            crate::permit::requirement_rules::converge_received_rule(
                conn,
                wo_id,
                Some(held),
                crate::permit::requirement_rules::stamp_causes::RULES_CONVERGED,
            )
            .await?;
        }
        Some(HeldRule::Received) => {
            crate::permit::requirement_rules::converge_received_rule(
                conn,
                wo_id,
                None,
                crate::permit::requirement_rules::stamp_causes::RULES_CONVERGED,
            )
            .await?;
        }
        // The same rule as here: one that came from another PC and was not derived here is judged
        // again by the list here (a work order received back in a status not started, R5-S1).
        _ => {
            crate::permit::requirement_rules::recheck_received_rule(
                conn,
                wo_id,
                crate::permit::requirement_rules::stamp_causes::WORK_ORDER_RECEIVED,
            )
            .await?;
        }
    }
    crate::wo::workflow::actions::permit_requirement::withdraw_received_clearing_of_required(conn, wo_id).await?;
    crate::permit::requirement_rules::stamp_matching_rule(
        conn,
        wo_id,
        None,
        crate::permit::requirement_rules::stamp_causes::WORK_ORDER_RECEIVED,
    )
    .await?;
    if !crate::sync::materialize::applying_snapshot_row() {
        let received_without_held_rule = match held_rule {
            Some(HeldRule::Kept { received_requires }) => Some(*received_requires),
            _ => None,
        };
        crate::permit::requirement_rules::finding_for_started_without_rule(
            conn,
            wo_id,
            received_without_held_rule,
            matches!(held_rule, Some(HeldRule::Received)),
        )
        .await?;
    }
    Ok(())
}

async fn queue_received_media(conn: &impl ConnectionTrait, media_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT entity_sync_id, object_key, content_sha256, byte_size, relative_path
             FROM media_attachments WHERE id = ?",
            [media_id.into()],
        ))
        .await?
        .ok_or_else(|| hook_err("media_attachments", "media row vanished"))?;
    let text = |c: &str| {
        row.try_get::<String>("", c)
            .map_err(|e| hook_err("media_attachments", e))
    };
    let relative_path: Option<String> = row
        .try_get("", "relative_path")
        .map_err(|e| hook_err("media_attachments", e))?;
    let byte_size: i64 = row
        .try_get("", "byte_size")
        .map_err(|e| hook_err("media_attachments", e))?;
    crate::sync::media::queue_media_download(
        conn,
        &text("entity_sync_id")?,
        &text("object_key")?,
        &text("content_sha256")?,
        byte_size,
        relative_path.as_deref(),
    )
    .await
}

async fn schedule_next_run(conn: &impl ConnectionTrait, schedule_id: i64) -> AppResult<()> {
    use std::str::FromStr;
    let cron_expr: String = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT cron_expr FROM report_schedules WHERE id = ?",
            [schedule_id.into()],
        ))
        .await?
        .ok_or_else(|| hook_err("report_schedules", "schedule vanished"))?
        .try_get("", "cron_expr")
        .map_err(|e| hook_err("report_schedules", e))?;
    let schedule = cron::Schedule::from_str(cron_expr.trim())
        .map_err(|e| hook_err("report_schedules", format!("cron expression '{cron_expr}': {e}")))?;
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE report_schedules SET next_run_at = ? WHERE id = ?",
        [
            crate::reports::scheduler::next_run_rfc3339(&schedule).into(),
            schedule_id.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// After an inbound delete removed a row.
pub(crate) async fn after_delete(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
) -> AppResult<()> {
    match layout.def.entity_type {
        "media_attachments" => crate::sync::media::forget_media_download(conn, entity_sync_id).await,
        _ => Ok(()),
    }
}

async fn apply_movement_to_balance(conn: &impl ConnectionTrait, movement_id: i64) -> AppResult<()> {
    let row = conn
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT article_id, warehouse_id, location_id, movement_type, quantity
             FROM inventory_transactions WHERE id = ?",
            [movement_id.into()],
        ))
        .await?
        .ok_or_else(|| hook_err("inventory_transactions", "movement vanished"))?;
    let get_i64 = |c: &str| {
        row.try_get::<i64>("", c)
            .map_err(|e| hook_err("inventory_transactions", e))
    };
    let movement_type: String = row
        .try_get("", "movement_type")
        .map_err(|e| hook_err("inventory_transactions", e))?;
    let quantity: f64 = row
        .try_get("", "quantity")
        .map_err(|e| hook_err("inventory_transactions", e))?;
    let (article_id, location_id) = (get_i64("article_id")?, get_i64("location_id")?);
    crate::inventory::ledger::apply_received_movement(
        conn,
        article_id,
        get_i64("warehouse_id")?,
        location_id,
        &movement_type,
        quantity,
    )
    .await?;
    // Applied as received (a physical fact); a location left below zero is to reconcile, one
    // item per article and location (`inventory::negative_stock`).
    crate::inventory::negative_stock::reconcile_pair_in(conn, article_id, location_id).await
}

/// `ancestor_path` (root `/{id}/`, child `{parent path}{id}/`) and `depth` for a node and every
/// node below it, as the org commands write them.
async fn recompute_org_paths(conn: &impl ConnectionTrait, node_id: i64) -> AppResult<()> {
    conn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "WITH RECURSIVE tree(id, path, depth) AS (
             SELECT n.id,
                    COALESCE(p.ancestor_path, '/') || n.id || '/',
                    COALESCE(p.depth + 1, 0)
             FROM org_nodes n LEFT JOIN org_nodes p ON p.id = n.parent_id
             WHERE n.id = ?
             UNION ALL
             SELECT c.id, tree.path || c.id || '/', tree.depth + 1
             FROM org_nodes c JOIN tree ON c.parent_id = tree.id
             WHERE tree.depth < 64
         )
         UPDATE org_nodes
         SET ancestor_path = (SELECT path FROM tree WHERE tree.id = org_nodes.id),
             depth = (SELECT depth FROM tree WHERE tree.id = org_nodes.id)
         WHERE id IN (SELECT id FROM tree)",
        [node_id.into()],
    ))
    .await?;
    Ok(())
}
