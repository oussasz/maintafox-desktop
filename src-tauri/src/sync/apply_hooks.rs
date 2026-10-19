//! Local consequences of applying an inbound row (Phase 2.4).
//!
//! The generic materializer writes the replicated columns. Some tables also feed data that each
//! device derives for itself; those derivations run here, in the same transaction as the apply:
//! - `inventory_transactions`: a received movement changes `stock_balances` of its location
//!   (`inventory::ledger`);
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
//!   once postings exist) is refused as a conflict (`before_upsert_checked`);
//! - `reliability_study_decisions`: an approval by the decision's own proposer is refused as a
//!   conflict unless the decision's site opted out of four-eyes; a study, equipment or site not
//!   applied yet makes it wait (`reliability::study::decisions::inbound_self_approval_check`);
//! - `site_settings` / `site_setting_events`: a per-site rule relaxing a site is applied only with
//!   its audited change (step-up, reason, settings right), waiting for that event while it is in
//!   the inbox, a conflict otherwise; unknown rules and unreadable values are conflicts
//!   (`settings::site::inbound_site_setting_check`, `inbound_site_setting_event_check`);
//! - `weekly_schedules`: a received week that would go backwards in its lifecycle here (frozen
//!   back to draft, a later baseline back to an earlier one) is refused as a conflict
//!   (`planning::weekly_schedule::inbound_lifecycle_conflict`, `before_upsert_checked`);
//! - `work_permits` / `permit_isolations` / `work_orders`: the permit and LOTO rules (activation,
//!   close, hand-back, suspension, two-person verification, the work-order start gate) are checked
//!   again on the received state; a failed rule is a conflict, a permit type or status not applied
//!   yet makes the row wait (`permit::sync_check`);
//!   A work order whose permit flag set by a plant rule is cleared names who cleared it (holding the
//!   permit-issue right here), when (before execution) and why (`inbound_permit_rule_check`);
//! - `permit_types`: a change of the three-person lock-removal setting, or a switch-off of the
//!   energy-isolation rule, names who changed it (holding the settings and permit-issue rights
//!   here), when and why; the shipped LOTO type's isolation rule is never switched off
//!   (`inbound_permit_type_check`);
//! - `permit_handover_logs`: a hand-over received by the person who signed it is refused as a
//!   conflict (`permit::sync_check::inbound_handover_check`); one without a signer is accepted;
//! - `permit_handover_logs`: after the apply, the log's "from" / "to" role that is not in the
//!   plant's list (free text from a PC that has not upgraded) is added to it flagged "à revoir"
//!   (`permit::handover_roles::adopt_received_roles`; only that log's two roles);
//! - `qualification_overrides`: reason, subject, context, no self-override and the approver's
//!   certification right (`qualification::sync_check`);
//! - `media_attachments`: the file is queued for download from object storage
//!   (`sync_media_downloads`, worked by the sync worker); a deleted row drops a pending download.

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

/// Checks of an inbound row that need the local database; `Some` stops the apply with that
/// outcome (recorded like every other sync conflict, for a person to decide).
/// - `app_settings`: the fiscal-year start month and the tenant currency cannot change once this
///   device stores postings that depend on them (`finance::fiscal`, `inventory::fx`). A change
///   made on a device where they were still free is refused here, as it is in Settings.
/// - `weekly_schedules`: a week never moves backwards in its lifecycle by synchronisation.
pub(crate) async fn before_upsert_checked(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    entity_sync_id: &str,
    payload: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<crate::sync::materialize::ApplyOutcome>> {
    use crate::sync::materialize::ApplyOutcome;
    if layout.def.entity_type == "reliability_study_decisions" {
        use crate::reliability::study::decisions::{inbound_self_approval_check, InboundDecisionVerdict};
        return Ok(inbound_self_approval_check(conn, entity_sync_id, payload)
            .await?
            .map(|verdict| match verdict {
                InboundDecisionVerdict::Defer(message) => ApplyOutcome::Deferred {
                    code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                    message,
                },
                InboundDecisionVerdict::Conflict(message) => ApplyOutcome::Conflict {
                    code: "DECISION_SELF_APPROVAL".into(),
                    message,
                },
            }));
    }
    let safety_check = match layout.def.entity_type {
        "site_settings" => Some(crate::settings::site::inbound_site_setting_check(conn, payload).await?),
        "site_setting_events" => Some(crate::settings::site::inbound_site_setting_event_check(payload).await?),
        "permit_types" => {
            Some(crate::permit::sync_check::inbound_permit_type_check(conn, entity_sync_id, payload).await?)
        }
        "work_permits" => Some(crate::permit::sync_check::inbound_permit_check(conn, entity_sync_id, payload).await?),
        "permit_isolations" => {
            Some(crate::permit::sync_check::inbound_isolation_check(conn, entity_sync_id, payload).await?)
        }
        "permit_handover_logs" => {
            Some(crate::permit::sync_check::inbound_handover_check(conn, entity_sync_id, payload).await?)
        }
        "work_orders" => Some(
            match crate::permit::sync_check::inbound_permit_rule_check(conn, entity_sync_id, payload).await? {
                Some(verdict) => Some(verdict),
                None => crate::permit::sync_check::inbound_work_order_check(conn, entity_sync_id, payload).await?,
            },
        ),
        "qualification_overrides" => {
            Some(crate::qualification::sync_check::inbound_override_check(conn, entity_sync_id, payload).await?)
        }
        "work_order_parts" => Some(crate::wo::parts::inbound_part_line_check(conn, entity_sync_id, payload).await?),
        _ => None,
    };
    if let Some(verdict) = safety_check {
        use crate::settings::site::InboundVerdict;
        return Ok(verdict.map(|v| match v {
            InboundVerdict::Defer(message) => ApplyOutcome::Deferred {
                code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
                message,
            },
            InboundVerdict::Conflict { code, message } => ApplyOutcome::Conflict {
                code: code.into(),
                message,
            },
        }));
    }
    if layout.def.entity_type == "weekly_schedules" {
        return Ok(
            crate::planning::weekly_schedule::inbound_lifecycle_conflict(conn, entity_sync_id, payload)
                .await?
                .map(|message| ApplyOutcome::Conflict {
                    code: "WEEK_LIFECYCLE_BACKWARDS".into(),
                    message,
                }),
        );
    }
    if layout.def.entity_type != "app_settings" {
        return Ok(None);
    }
    let text = |k: &str| payload.get(k).and_then(serde_json::Value::as_str);
    let (Some(key), Some(value)) = (text("setting_key"), text("setting_value_json")) else {
        return Ok(None);
    };
    if text("setting_scope") != Some("tenant") {
        return Ok(None);
    }
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
        "work_orders" => crate::permit::sync_check::work_order_group(conn, entity_sync_id, payload).await,
        _ => Ok(None),
    }
}

/// Applies a group (`upsert_group`) after its row was written; `Some` (a conflict) makes the caller
/// roll the whole transaction back: nothing of the group is kept.
pub(crate) async fn after_upsert_group(
    conn: &impl ConnectionTrait,
    group: crate::permit::sync_check::Group,
) -> AppResult<Option<crate::sync::materialize::ApplyOutcome>> {
    crate::permit::sync_check::apply_group(conn, group).await
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
    let verdict = match layout.def.entity_type {
        "work_permits" | "permit_isolations" | "permit_suspensions" | "permit_types" | "qualification_requirements" => {
            crate::permit::sync_check::inbound_delete_check(conn, layout.def.entity_type, entity_sync_id).await?
        }
        _ => None,
    };
    Ok(verdict.map(|v| match v {
        crate::settings::site::InboundVerdict::Defer(message) => ApplyOutcome::Deferred {
            code: crate::sync::materialize::DEFER_MISSING_PARENT.into(),
            message,
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

/// After an inbound row was inserted (`inserted`) or updated.
pub(crate) async fn after_upsert(
    conn: &impl ConnectionTrait,
    layout: &EntityLayout,
    row: &RowLocator,
    inserted: bool,
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
        // A free-text hand-over role from a PC that has not upgraded joins the plant's list now,
        // flagged "à revoir" (`permit::handover_roles`), not only at the next start of this PC:
        // this log's two roles only.
        "permit_handover_logs" => {
            let id = located_row_id(conn, layout, row)
                .await?
                .ok_or_else(|| hook_err(layout.table(), "hand-over log not found after apply"))?;
            crate::permit::handover_roles::adopt_received_roles(conn, id).await
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
        _ => Ok(()),
    }
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
    crate::inventory::ledger::apply_received_movement(
        conn,
        get_i64("article_id")?,
        get_i64("warehouse_id")?,
        get_i64("location_id")?,
        &movement_type,
        quantity,
    )
    .await
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
