//! WO IPC commands.
//!
//! Phase 2 – Sub-phase 05 – File 01 Sprint S2 + File 02 Sprint S2 + File 03 Sprint S2.
//!
//! Permission gates:
//!   ot.view    — list, get, list_labor, list_tasks, list_wo_parts, list_delay_segments,
//!                list_downtime_segments, get_cost_summary, list_wo_attachments,
//!                get_cost_posting_hook, get_wo_analytics_snapshot, get_wo_stats
//!   ot.create  — create work order
//!   ot.edit    — update draft, cancel, plan, assign, start, pause, resume, hold,
//!                complete_mechanically, add_labor, close_labor, add_part,
//!                record_part_usage, confirm_no_parts, add_task, complete_task,
//!                open_downtime, close_downtime, save_failure_detail, save_verification,
//!                close_wo, upload_wo_attachment, update_service_cost
//!   ot.admin   — reopen_wo, delete_wo_attachment
//!
//! Site scope (step 6, `docs/SITE_SCOPED_ROLES.md`): every work order is checked at its org node
//! `work_orders.entity_id` (`wo::access`), with the same permission as before; what belongs to a
//! work order (labour, parts, tasks, tools, downtime, attachments) at its work order's node. The
//! list, the statistics, the backlog heatmap and the audit log are limited in SQL to where the
//! user holds the right; the list says how many work orders it leaves out and why. A plant-wide
//! role sees and acts everywhere, as before. The actor of an action is always the signed-in
//! user. Reference catalogs (types, statuses, urgency) and the people pickers stay plant-wide.

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::auth::scope::{AccessScope, Anchor};
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::wo::types;
use crate::wo::{
    access, analytics, attachments, audit, closeout, costs, delay, execution, labor, parts, queries, stats, statuses,
    tasks,
};
use crate::{require_permission, require_session};

use sea_orm::{ConnectionTrait, DbBackend, Statement};
use tauri::Manager;

// ═══════════════════════════════════════════════════════════════════════════════
// Site-scope checks shared by the commands (`wo::access`)
// ═══════════════════════════════════════════════════════════════════════════════

/// Where the user holds `permission` (held nowhere: refused as before); filters lists and figures.
async fn require_anywhere(state: &AppState, user_id: i32, permission: &str) -> AppResult<AccessScope> {
    access::require_anywhere(&state.db, &state.permission_cache, user_id, permission).await
}

/// `permission` on the work order `wo_id` (held nowhere: refused as before; no such work order:
/// not found; else checked at its org node).
async fn require_on_wo(state: &AppState, user_id: i32, permission: &str, wo_id: i64) -> AppResult<Anchor> {
    access::require_on_wo(&state.db, &state.permission_cache, user_id, permission, wo_id).await
}

/// `permission` for the reads of what belongs to the work order `wo_id` (a work order that no
/// longer exists has no site: plant-wide users only, who read what is left of it as before).
async fn require_on_wo_records(state: &AppState, user_id: i32, permission: &str, wo_id: i64) -> AppResult<()> {
    access::require_on_wo_records(&state.db, &state.permission_cache, user_id, permission, wo_id).await
}

/// `permission` on the work order a labour entry, part line, task, tool, downtime segment or
/// attachment belongs to (no such record: not found).
async fn require_on_child(
    state: &AppState,
    user_id: i32,
    permission: &str,
    child: access::WoChild,
    id: i64,
) -> AppResult<()> {
    access::require_on_child(&state.db, &state.permission_cache, user_id, permission, child, id).await
}

/// `permission` on the work order, and at the new equipment's node when the equipment changes.
async fn require_equipment_change(
    state: &AppState,
    user_id: i32,
    permission: &str,
    wo_id: i64,
    new_equipment_id: Option<i64>,
) -> AppResult<()> {
    access::require_equipment_change(
        &state.db,
        &state.permission_cache,
        user_id,
        permission,
        wo_id,
        new_equipment_id,
    )
    .await
}

/// The reference catalogs of the work-order screens (types, statuses) are plant-wide objects any
/// work-order user reads: `ref.view` plant-wide, or `ot.view` / `ot.create` held anywhere (a
/// site-only user reads them too). Refused as before otherwise.
async fn require_wo_catalog_read(state: &AppState, user_id: i32) -> AppResult<()> {
    let has_ref_view = crate::auth::rbac::check_permission(
        &state.db,
        user_id,
        crate::rbac::permissions::REF_VIEW,
        &PermissionScope::Global,
    )
    .await?;
    if has_ref_view {
        return Ok(());
    }
    if access::holds_anywhere(
        &state.db,
        &state.permission_cache,
        user_id,
        crate::rbac::permissions::OT_VIEW,
    )
    .await?
    {
        return Ok(());
    }
    require_anywhere(state, user_id, crate::rbac::permissions::OT_CREATE)
        .await
        .map(|_| ())
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_wo — requires ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo(filter: queries::WoListFilter, state: State<'_, AppState>) -> AppResult<queries::WoListPage> {
    let user = require_session!(state);
    let scope = require_anywhere(&state, user.user_id, crate::rbac::permissions::OT_VIEW).await?;
    // Where the user holds ot.view, in SQL before paging; the page counts what it leaves out.
    queries::list_work_orders_in_scope(&state.db, filter, &scope).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_wo — requires ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_wo(id: i64, state: State<'_, AppState>) -> AppResult<queries::WoGetResponse> {
    let user = require_session!(state);
    let scope = require_anywhere(&state, user.user_id, crate::rbac::permissions::OT_VIEW).await?;

    let wo = queries::get_work_order(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: id.to_string(),
        })?;
    crate::auth::scope::check_anchor(
        &state.db,
        &scope,
        crate::rbac::permissions::OT_VIEW,
        Anchor::of(wo.entity_id),
    )
    .await?;

    let transitions = queries::get_wo_transition_log(&state.db, id).await?;

    Ok(queries::WoGetResponse { wo, transitions })
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) create_wo — requires ot.create
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn create_wo(
    mut input: crate::wo::domain::WoCreateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    // The creator (requester of record) is the signed-in user, never an id sent by the screen.
    input.creator_id = i64::from(user.user_id);
    // Rule §2.6: ot.create at the new work order's org node (the node given, else its equipment's
    // installed node — as the create derives it) and at the equipment's node when that is
    // another one. No derivable node: plant-wide users only (the create then refuses it for
    // everyone, as before).
    access::require_create_at(
        &state.db,
        &state.permission_cache,
        user.user_id,
        input.entity_id,
        input.equipment_id,
    )
    .await?;

    // ── Validate required fields ─────────────────────────────────────────
    let mut errors: Vec<String> = Vec::new();

    if input.title.trim().is_empty() {
        errors.push("Le titre est obligatoire.".into());
    }

    // Validate type_code resolves
    match types::resolve_work_order_type_id_by_code(&state.db, &input.type_code).await {
        Ok(_) => {}
        Err(AppError::ValidationFailed(mut errs)) => errors.append(&mut errs),
        Err(e) => return Err(e),
    }

    // Validate source_di_id resolves if provided; linking a request needs the request view right
    // at the request's site (no work order tied to another site's request).
    if let Some(di_id) = input.source_di_id {
        let di_row = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT org_node_id FROM intervention_requests WHERE id = ?",
                [di_id.into()],
            ))
            .await?;
        match di_row {
            None => errors.push(format!("DI introuvable (source_di_id={di_id}).")),
            Some(row) => {
                let di_node: Option<i64> = row.try_get("", "org_node_id")?;
                crate::auth::scope::require_at(
                    &state.db,
                    &state.permission_cache,
                    user.user_id,
                    crate::rbac::permissions::DI_VIEW,
                    crate::auth::scope::Anchor::of(di_node),
                )
                .await?;
            }
        }
    }
    // Other sources: a cause diagram belongs to its equipment's site; an inspection anomaly to
    // the inspections module, not site-scoped yet (stage 3), so linking one stays plant-wide.
    if let Some(did) = input.source_ram_ishikawa_diagram_id {
        let node = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT e.installed_at_node_id AS node FROM ram_ishikawa_diagrams d \
                   LEFT JOIN equipment e ON e.id = d.equipment_id WHERE d.id = ?",
                [did.into()],
            ))
            .await?
            .map(|row| row.try_get::<Option<i64>>("", "node"))
            .transpose()?
            .flatten();
        crate::auth::scope::require_at(
            &state.db,
            &state.permission_cache,
            user.user_id,
            crate::rbac::permissions::OT_CREATE,
            crate::auth::scope::Anchor::of(node),
        )
        .await?;
    }
    if input.source_inspection_anomaly_id.is_some() {
        crate::auth::scope::require_at(
            &state.db,
            &state.permission_cache,
            user.user_id,
            crate::rbac::permissions::OT_CREATE,
            crate::auth::scope::Anchor::Plant,
        )
        .await?;
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    let wo = queries::create_work_order(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "created".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order created".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

#[tauri::command]
pub async fn list_work_order_types(state: State<'_, AppState>) -> AppResult<Vec<types::WorkOrderTypeOption>> {
    let user = require_session!(state);
    require_wo_catalog_read(&state, user.user_id).await?;
    types::list_work_order_types(&state.db).await
}

/// OT types without a maintenance nature, with how many work orders use each (they count in no
/// work mix). Same reading rights as the OT type list.
#[tauri::command]
pub async fn list_work_order_types_to_review(
    state: State<'_, AppState>,
) -> AppResult<Vec<types::WorkOrderTypeToReview>> {
    let user = require_session!(state);
    let has_ref_view = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        crate::rbac::permissions::REF_VIEW,
        &PermissionScope::Global,
    )
    .await?;
    if !has_ref_view {
        require_permission!(state, &user, crate::rbac::permissions::OT_VIEW, PermissionScope::Global);
    }
    types::list_types_without_nature(&state.db).await
}

/// Give a plant's own OT type without a nature its maintenance nature (reference-data manage
/// right, reason, audited).
#[tauri::command]
pub async fn set_work_order_type_nature(
    input: types::SetWorkOrderTypeNatureInput,
    state: State<'_, AppState>,
) -> AppResult<types::WorkOrderTypeOption> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    types::set_work_order_type_nature(&state.db, input, i64::from(user.user_id)).await
}

/// The shift windows a work order can be planned on: its assigned team's — or `team_id`'s, the
/// team being assigned — Classes horaires at `planned_start` (local `YYYY-MM-DDTHH:MM` as typed,
/// or a stored instant; default: its planned start). Empty, with the reason, when there is no
/// team or no date, or nobody of the team works then.
#[tauri::command]
pub async fn list_wo_shift_options(
    wo_id: i64,
    planned_start: Option<String>,
    team_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::shifts::WoShiftOptions> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    crate::wo::shifts::list_wo_shift_options(&state.db, wo_id, planned_start.as_deref(), team_id).await
}

/// The plant's OT sub-types (`WORK.ORDER_SUBTYPES`), each with the maintenance nature it belongs to.
#[tauri::command]
pub async fn list_work_order_subtypes(state: State<'_, AppState>) -> AppResult<Vec<types::WorkOrderSubtypeOption>> {
    let _user = require_session!(state);
    types::list_work_order_subtypes(&state.db).await
}

/// The shared DI / OT urgency scale (`WORK.URGENCY`), most urgent first, with the plant's
/// labels and target times. Read by the request and work-order forms, lists and SLA screens.
#[tauri::command]
pub async fn list_urgency_levels(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::reference::urgency_scale::UrgencyLevel>> {
    let _user = require_session!(state);
    crate::reference::urgency_scale::list_urgency_levels(&state.db).await
}

/// Saves the company attributes of one urgency level (labels FR/EN and target times). The
/// levels themselves are fixed by Maintafox.
#[tauri::command]
pub async fn update_urgency_level(
    code: String,
    input: crate::reference::urgency_scale::UpdateUrgencyLevelInput,
    state: State<'_, AppState>,
) -> AppResult<crate::reference::urgency_scale::UrgencyLevel> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    crate::reference::audience::assert_module_catalog_editable(
        crate::reference::audience::MODULE_CATALOG_URGENCY_SCALE,
    )?;
    crate::reference::urgency_scale::update_urgency_level(&state.db, &code, input).await
}

#[tauri::command]
pub async fn list_work_order_statuses(state: State<'_, AppState>) -> AppResult<Vec<statuses::WorkOrderStatusOption>> {
    let user = require_session!(state);
    require_wo_catalog_read(&state, user.user_id).await?;
    statuses::list_work_order_statuses(&state.db).await
}

#[tauri::command]
pub async fn update_work_order_status(
    id: i64,
    input: statuses::UpdateWorkOrderStatusInput,
    state: State<'_, AppState>,
) -> AppResult<statuses::WorkOrderStatusOption> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    crate::reference::audience::assert_module_catalog_editable(
        crate::reference::audience::MODULE_CATALOG_WORK_ORDER_STATUSES,
    )?;
    statuses::update_work_order_status(&state.db, id, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) update_wo_draft — requires ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_wo_draft(
    input: crate::wo::domain::WoDraftUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    // At the work order's site, and at the new equipment's site when the equipment changes
    // (rule §2.7; the order's own node is not changed by a draft edit).
    require_equipment_change(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        input.id,
        input.equipment_id,
    )
    .await?;
    let before = queries::get_work_order(&state.db, input.id).await?;
    let wo = queries::update_wo_draft_fields(&state.db, input).await?;
    // A draft that moved with its equipment to another site: who, when, from where to where.
    if let Some(before) = before.filter(|b| b.entity_id != wo.entity_id) {
        audit::record_wo_change_event(
            &state.db,
            audit::WoAuditInput {
                wo_id: Some(wo.id),
                action: "site_moved".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Draft moved to the site of its new equipment".into()),
                details_json: Some(
                    serde_json::json!({
                        "old_entity_id": before.entity_id,
                        "new_entity_id": wo.entity_id,
                        "old_equipment_id": before.equipment_id,
                        "new_equipment_id": wo.equipment_id,
                    })
                    .to_string(),
                ),
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;
    }
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) cancel_wo — requires ot.edit + step-up if in_progress or later
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn cancel_wo(
    mut input: crate::wo::domain::WoCancelInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);

    // Check if the WO is in_progress or later — require step-up
    let current = queries::get_work_order(&state.db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkOrder".into(),
            id: input.id.to_string(),
        })?;

    let status_code = current.status_code.as_deref().unwrap_or("unknown");
    let status = crate::wo::domain::WoStatus::try_from_str(status_code)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Stored WO has invalid status: {e}")))?;

    // Step-up required for executing or completed states
    if status.is_executing() || matches!(status, crate::wo::domain::WoStatus::Completed) {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_wo_change_event(
                &state.db,
                audit::WoAuditInput {
                    wo_id: Some(input.id),
                    action: "cancelled".into(),
                    actor_id: Some(i64::from(user.user_id)),
                    summary: Some("Cancellation blocked: step-up verification failed".into()),
                    details_json: None,
                    requires_step_up: true,
                    apply_result: "blocked".into(),
                },
            )
            .await;
            return Err(AppError::StepUpRequired);
        }
    }

    let wo = queries::cancel_work_order(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "cancelled".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order cancelled".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) plan_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn plan_wo(
    mut input: execution::WoPlanInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = execution::plan_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "planned".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order planned".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F1) set_wo_permit_requirement — ot.edit (draft / planning / ready only)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn set_wo_permit_requirement(
    input: crate::wo::workflow::actions::WoPermitRequirementInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    let actor_id = i64::from(user.user_id);
    let requires_permit = input.requires_permit;
    let wo = crate::wo::workflow::actions::set_permit_requirement(&state.db, input, actor_id).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "permit_requirement_changed".into(),
            actor_id: Some(actor_id),
            summary: Some(
                if requires_permit {
                    "Permis / consignation exigé"
                } else {
                    "Permis / consignation non exigé"
                }
                .into(),
            ),
            details_json: Some(serde_json::json!({ "requires_permit": requires_permit }).to_string()),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

/// Clear the permit flag a plant rule set when the work order was converted from a request.
/// Needs the permit-issue right (`ptw.issue`) — editing the work order is not enough — and a
/// reason of 10 characters or more; recorded on the work order and in its audit trail.
#[tauri::command]
pub async fn clear_wo_permit_rule_flag(
    input: crate::wo::workflow::actions::permit_requirement::WoPermitRuleClearInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    // A safety gate: clearing a permit requirement set by a plant rule stays plant-wide (the
    // action itself re-checks the plant-wide right in its transaction).
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    let actor_id = i64::from(user.user_id);
    let reason = input.reason.trim().to_string();
    let wo =
        crate::wo::workflow::actions::permit_requirement::clear_permit_rule_flag(&state.db, input, actor_id).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "permit_rule_flag_cleared".into(),
            actor_id: Some(actor_id),
            summary: Some("Exigence de permis (règle) levée".into()),
            details_json: Some(
                serde_json::json!({
                    "requires_permit": false,
                    "rule": wo.permit_rule_code,
                    "reason": reason,
                })
                .to_string(),
            ),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F2) submit / mark ready / readiness / return / approve — ot.edit / ot.plan
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn submit_wo(
    mut input: crate::wo::workflow::actions::WoSubmitInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = crate::wo::workflow::actions::submit_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "submitted".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order submitted to planning".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

#[tauri::command]
pub async fn evaluate_wo_readiness(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::workflow::readiness::ReadinessReport> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    crate::wo::workflow::readiness::evaluate_wo_readiness(&state.db, wo_id).await
}

#[tauri::command]
pub async fn mark_wo_ready(
    mut input: crate::wo::workflow::actions::WoMarkReadyInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = crate::wo::workflow::actions::mark_wo_ready(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "marked_ready".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order marked ready".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

#[tauri::command]
pub async fn return_to_planning(
    mut input: crate::wo::workflow::actions::WoReturnToPlanningInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = crate::wo::workflow::actions::return_to_planning(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "returned_to_planning".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order returned to planning".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

#[tauri::command]
pub async fn approve_planning(
    mut input: crate::wo::workflow::actions::WoApprovePlanningInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_APPROVE, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = crate::wo::workflow::actions::approve_planning(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "planning_approved".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order planning approved".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) assign_wo — ot.edit (requires override_reason; prefer emergency_reassign_wo)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn assign_wo(
    mut input: execution::WoAssignInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EMERGENCY_ASSIGN,
        input.wo_id,
    )
    .await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let reason = input.override_reason.clone();
    let wo = execution::assign_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "emergency_reassigned".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order emergency reassigned".into()),
            details_json: Some(serde_json::json!({ "override_reason": reason }).to_string()),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G2) emergency_reassign_wo — ot.emergency_assign
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn emergency_reassign_wo(
    mut input: execution::WoEmergencyReassignInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EMERGENCY_ASSIGN,
        input.wo_id,
    )
    .await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let reason = input.override_reason.clone();
    let wo = execution::emergency_reassign_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "emergency_reassigned".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order emergency reassigned".into()),
            details_json: Some(serde_json::json!({ "override_reason": reason }).to_string()),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// H) start_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn start_wo(
    mut input: execution::WoStartInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = execution::start_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "started".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order started".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// I) pause_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn pause_wo(
    mut input: execution::WoPauseInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = execution::pause_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "paused".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order paused".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// J) resume_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn resume_wo(
    mut input: execution::WoResumeInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = execution::resume_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "resumed".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order resumed".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// K) hold_wo — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn hold_wo(
    mut input: execution::WoHoldInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = execution::set_waiting_for_prerequisite(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "held".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order put on hold".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// L) complete_wo_mechanically — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn complete_wo_mechanically(
    mut input: execution::WoMechCompleteInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);
    let wo = execution::complete_wo_mechanically(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "mechanically_completed".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order mechanically completed".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

#[tauri::command]
pub async fn evaluate_wo_completion_gates(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::wo::execution::WoCompletionGate>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    crate::wo::execution::evaluate_completion_gates(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// M) add_labor — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn add_labor(input: labor::AddLaborInput, state: State<'_, AppState>) -> AppResult<labor::WoIntervener> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    labor::add_labor_entry(&state.db, input).await
}

/// Labour for a person of the register, login or not (contractor technicians). Refused with
/// the reason when they lack a qualification the work order requires, unless an override was
/// recorded (`record_qualification_override`).
#[tauri::command]
pub async fn add_personnel_labor(
    input: labor::AddPersonnelLaborInput,
    state: State<'_, AppState>,
) -> AppResult<labor::WoIntervener> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    labor::add_personnel_labor_entry(&state.db, input).await
}

/// Everyone who can be booked for labour (active personnel, with or without a login).
#[tauri::command]
pub async fn list_wo_labor_people(state: State<'_, AppState>) -> AppResult<Vec<labor::WoLaborPerson>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::OT_VIEW, PermissionScope::Global);
    crate::commands::qualification::require_any_permission(&state, &user, crate::commands::qualification::PEOPLE_VIEW)
        .await?;
    labor::list_labor_people(&state.db).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// N) close_labor — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_labor(
    intervener_id: i64,
    ended_at: String,
    actor_id: i64,
    state: State<'_, AppState>,
) -> AppResult<labor::WoIntervener> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        access::WoChild::Labor,
        intervener_id,
    )
    .await?;
    // The actor is the signed-in user; the id the screen still sends is not used.
    let _ = actor_id;
    labor::close_labor_entry(&state.db, intervener_id, ended_at, i64::from(user.user_id)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// O) list_labor — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_labor(wo_id: i64, state: State<'_, AppState>) -> AppResult<Vec<labor::WoIntervener>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    labor::list_labor_entries(&state.db, wo_id).await
}

/// People who can be booked on a WO labour entry (personnel register ∩ active logins).
#[tauri::command]
pub async fn list_wo_labor_candidates(state: State<'_, AppState>) -> AppResult<Vec<labor::WoLaborCandidate>> {
    let user = require_session!(state);
    require_permission!(state, &user, crate::rbac::permissions::OT_VIEW, PermissionScope::Global);
    labor::list_labor_candidates(&state.db).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// P) add_part — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn add_part(input: parts::AddPartInput, state: State<'_, AppState>) -> AppResult<parts::WoPart> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    parts::add_planned_part(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Q) record_part_usage — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn record_part_usage(
    wo_part_id: i64,
    quantity_used: f64,
    unit_cost: Option<f64>,
    state: State<'_, AppState>,
) -> AppResult<parts::WoPart> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        access::WoChild::Part,
        wo_part_id,
    )
    .await?;
    parts::record_actual_usage(&state.db, wo_part_id, quantity_used, unit_cost).await
}

#[tauri::command]
pub async fn mark_part_not_used(
    input: parts::MarkPartNotUsedInput,
    state: State<'_, AppState>,
) -> AppResult<parts::WoPart> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        access::WoChild::Part,
        input.wo_part_id,
    )
    .await?;
    // The actor is the signed-in user, never an id sent by the screen.
    let mut input = input;
    input.actor_id = Some(user.user_id.into());
    parts::mark_part_not_used(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// R) confirm_no_parts — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn confirm_no_parts(wo_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, wo_id).await?;
    parts::confirm_no_parts_used(&state.db, wo_id, user.user_id.into()).await
}

#[tauri::command]
pub async fn unconfirm_no_parts(wo_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, wo_id).await?;
    parts::unconfirm_no_parts_used(&state.db, wo_id, user.user_id.into()).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// R2) list_wo_parts — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo_parts(wo_id: i64, state: State<'_, AppState>) -> AppResult<Vec<parts::WoPart>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    parts::list_wo_parts(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// S) add_task — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn add_task(input: tasks::AddTaskInput, state: State<'_, AppState>) -> AppResult<tasks::WoTask> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    tasks::add_task(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// T) complete_task — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn complete_task(
    task_id: i64,
    actor_id: i64,
    result_code: String,
    notes: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<tasks::WoTask> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        access::WoChild::Task,
        task_id,
    )
    .await?;
    // The actor is the signed-in user; the id the screen still sends is not used.
    let _ = actor_id;
    tasks::complete_task(&state.db, task_id, i64::from(user.user_id), result_code, notes).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// U) list_tasks — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_tasks(wo_id: i64, state: State<'_, AppState>) -> AppResult<Vec<tasks::WoTask>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    tasks::list_tasks(&state.db, wo_id).await
}

#[tauri::command]
pub async fn get_wo_plan_adherence(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::plan_adherence::WoPlanAdherence> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    crate::wo::plan_adherence::get_plan_adherence(&state.db, wo_id).await
}

#[tauri::command]
pub async fn list_wo_execution_events(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::wo::execution_log::WoExecutionEvent>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    crate::wo::execution_log::list_execution_events(&state.db, wo_id).await
}

#[tauri::command]
pub async fn list_wo_tools(wo_id: i64, state: State<'_, AppState>) -> AppResult<Vec<crate::wo::tools::WoTool>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    crate::wo::tools::list_tools(&state.db, wo_id).await
}

#[tauri::command]
pub async fn add_wo_tool(
    input: crate::wo::tools::AddToolInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::tools::WoTool> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    crate::wo::tools::add_tool(&state.db, input).await
}

#[tauri::command]
pub async fn mark_wo_tool_used(tool_id: i64, state: State<'_, AppState>) -> AppResult<crate::wo::tools::WoTool> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        access::WoChild::Tool,
        tool_id,
    )
    .await?;
    crate::wo::tools::mark_tool_used(&state.db, tool_id).await
}

#[tauri::command]
pub async fn mark_wo_tool_not_used(tool_id: i64, state: State<'_, AppState>) -> AppResult<crate::wo::tools::WoTool> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        access::WoChild::Tool,
        tool_id,
    )
    .await?;
    crate::wo::tools::mark_tool_not_used(&state.db, tool_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// V) open_downtime — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn open_downtime(
    wo_id: i64,
    downtime_type: String,
    comment: Option<String>,
    actor_id: i64,
    classification_code: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<delay::WoDowntimeSegment> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, wo_id).await?;
    // The actor is the signed-in user; the id the screen still sends is not used.
    let _ = actor_id;
    delay::open_downtime_segment(
        &state.db,
        delay::OpenDowntimeInput {
            wo_id,
            downtime_type,
            comment,
            actor_id: i64::from(user.user_id),
            classification_code,
        },
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// W) close_downtime — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_downtime(
    segment_id: i64,
    ended_at: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<delay::WoDowntimeSegment> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        access::WoChild::Downtime,
        segment_id,
    )
    .await?;
    delay::close_downtime_segment(&state.db, segment_id, ended_at).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// X) list_delay_segments — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_delay_segments(wo_id: i64, state: State<'_, AppState>) -> AppResult<Vec<delay::WoDelaySegment>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    delay::list_delay_segments(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Y) list_downtime_segments — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_downtime_segments(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<delay::WoDowntimeSegment>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    delay::list_downtime_segments(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// File 03 — Close-Out, Verification, Cost, Attachments, Analytics
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Z) save_failure_detail — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn save_failure_detail(
    input: closeout::SaveFailureCodingInput,
    state: State<'_, AppState>,
) -> AppResult<closeout::WoFailureDetail> {
    let user = require_session!(state);
    require_on_wo(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_EDIT,
        input.base.wo_id,
    )
    .await?;
    let wo_id = input.base.wo_id;
    let detail = closeout::save_failure_coding(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "failure_detail_saved".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Failure detail recorded".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(detail)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AA) save_verification — ot.edit (confirmed in a dialog; no step-up)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn save_verification(
    mut input: closeout::SaveVerificationInput,
    state: State<'_, AppState>,
) -> AppResult<(closeout::WoVerification, crate::wo::domain::WorkOrder)> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    // The verifier is the signed-in user (as the screen sends), never another id.
    input.verified_by_id = i64::from(user.user_id);

    let result = closeout::save_verification(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(result.1.id),
            action: "verification_saved".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Technical verification recorded".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AB) close_wo — ot.close (confirmed in a dialog; no step-up)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_wo(
    mut input: closeout::WoCloseInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_CLOSE, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);

    match closeout::close_wo(&state.db, input).await {
        Ok(wo) => {
            audit::record_wo_change_event(
                &state.db,
                audit::WoAuditInput {
                    wo_id: Some(wo.id),
                    action: "closed".into(),
                    actor_id: Some(i64::from(user.user_id)),
                    summary: Some("Work order closed".into()),
                    details_json: None,
                    requires_step_up: false,
                    apply_result: "applied".into(),
                },
            )
            .await;
            Ok(wo)
        }
        Err(AppError::ValidationFailed(ref errs)) => {
            let details = serde_json::json!({ "quality_gate_errors": errs }).to_string();
            audit::record_wo_change_event(
                &state.db,
                audit::WoAuditInput {
                    wo_id: None,
                    action: "closed".into(),
                    actor_id: Some(i64::from(user.user_id)),
                    summary: Some("Close blocked: quality gate failed".into()),
                    details_json: Some(details),
                    requires_step_up: false,
                    apply_result: "blocked".into(),
                },
            )
            .await;
            Err(AppError::ValidationFailed(errs.clone()))
        }
        Err(other) => Err(other),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AC) reopen_wo — ot.reopen + step-up
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn reopen_wo(
    mut input: closeout::WoReopenInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_REOPEN, input.wo_id).await?;
    // The actor is the signed-in user, never an id sent by the screen.
    input.actor_id = i64::from(user.user_id);

    // Manual step-up check to record blocked event
    {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_wo_change_event(
                &state.db,
                audit::WoAuditInput {
                    wo_id: Some(input.wo_id),
                    action: "reopened".into(),
                    actor_id: Some(i64::from(user.user_id)),
                    summary: Some("Reopen blocked: step-up verification failed".into()),
                    details_json: None,
                    requires_step_up: true,
                    apply_result: "blocked".into(),
                },
            )
            .await;
            return Err(AppError::StepUpRequired);
        }
    }

    let wo = closeout::reopen_wo(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: "reopened".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Work order reopened".into()),
            details_json: None,
            requires_step_up: true,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AC-2) update_wo_rca — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_wo_rca(input: closeout::UpdateWoRcaInput, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, input.wo_id).await?;
    let wo_id = input.wo_id;
    closeout::update_wo_rca(&state.db, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "rca_updated".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Root cause analysis updated".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// AD) upload_wo_attachment — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn upload_wo_attachment(
    app: tauri::AppHandle,
    wo_id: i64,
    file_name: String,
    file_bytes: Vec<u8>,
    mime_type: String,
    notes: Option<String>,
    phase: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<attachments::WoAttachment> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, wo_id).await?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;

    let input = attachments::WoAttachmentInput {
        wo_id,
        file_name,
        file_bytes,
        mime_type,
        notes,
        uploaded_by_id: i64::from(user.user_id),
        phase,
    };

    let attachment = attachments::save_wo_attachment(&state.db, &app_data_dir, input).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "attachment_uploaded".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("Attachment uploaded".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(attachment)
}

// ═══════════════════════════════════════════════════════════════════════════════
// AE) list_wo_attachments — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo_attachments(wo_id: i64, state: State<'_, AppState>) -> AppResult<Vec<attachments::WoAttachment>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    attachments::list_wo_attachments(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AF) delete_wo_attachment — ot.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn delete_wo_attachment(attachment_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_on_child(
        &state,
        user.user_id,
        crate::rbac::permissions::OT_ADMIN,
        access::WoChild::Attachment,
        attachment_id,
    )
    .await?;
    attachments::delete_wo_attachment_record(&state.db, attachment_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AG) get_cost_summary — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_cost_summary(wo_id: i64, state: State<'_, AppState>) -> AppResult<costs::WoCostSummary> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    costs::get_cost_summary(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AH) update_service_cost — ot.edit
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_service_cost(wo_id: i64, service_cost: f64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, wo_id).await?;
    costs::update_service_cost(&state.db, wo_id, service_cost, i64::from(user.user_id)).await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo_id),
            action: "service_cost_updated".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some(format!("Service cost updated to {service_cost}")),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// AI) get_cost_posting_hook — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_cost_posting_hook(wo_id: i64, state: State<'_, AppState>) -> AppResult<costs::CostPostingHook> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    costs::get_cost_posting_hook(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AJ) get_wo_analytics_snapshot — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

/// Backlog heatmap (PRD §6.5): planned load per day vs crew capacity, unscheduled, overdue and
/// blocked work by delay category.
#[tauri::command]
pub async fn get_wo_backlog_heatmap(
    input: crate::wo::backlog::WoBacklogHeatmapInput,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::backlog::WoBacklogHeatmap> {
    let user = require_session!(state);
    let scope = require_anywhere(&state, user.user_id, crate::rbac::permissions::OT_VIEW).await?;
    // The work orders and the crews of the user's sites (plant-wide: all of them, as before).
    crate::wo::backlog::get_wo_backlog_heatmap_in_scope(&state.db, input, &scope).await
}

#[tauri::command]
pub async fn get_wo_analytics_snapshot(
    wo_id: i64,
    state: State<'_, AppState>,
) -> AppResult<analytics::WoAnalyticsSnapshot> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    analytics::get_wo_analytics_snapshot(&state.db, wo_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AK) get_wo_stats — ot.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_wo_stats(state: State<'_, AppState>) -> AppResult<stats::WoStatsPayload> {
    let user = require_session!(state);
    let scope = require_anywhere(&state, user.user_id, crate::rbac::permissions::OT_VIEW).await?;
    // Figures over the work orders of the user's sites only (rule §2.8).
    stats::get_wo_stats_in_scope(&state.db, &access::ScopeSql::of(&scope)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AL) list_wo_change_events — ot.view (per-WO audit timeline)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_wo_change_events(
    wo_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::WoChangeEvent>> {
    let user = require_session!(state);
    require_on_wo_records(&state, user.user_id, crate::rbac::permissions::OT_VIEW, wo_id).await?;
    audit::list_wo_change_events(&state.db, wo_id, limit.unwrap_or(50)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// AM) list_all_wo_change_events — ot.admin (global audit log)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_all_wo_change_events(
    filter: audit::WoAuditFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::WoChangeEvent>> {
    let user = require_session!(state);
    let scope = require_anywhere(&state, user.user_id, crate::rbac::permissions::OT_ADMIN).await?;
    // The events of the work orders of the user's sites (no work order: plant-wide users only).
    audit::list_all_wo_change_events_in_scope(&state.db, filter, &access::ScopeSql::of(&scope)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// set_wo_requires_shutdown — ot.edit (a planning field, like plan_wo)
// ═══════════════════════════════════════════════════════════════════════════════

/// Flags (or unflags) a work order being prepared as needing the equipment stopped; planning then
/// checks it against shutdown windows like a PM plan.
#[tauri::command]
pub async fn set_wo_requires_shutdown(
    wo_id: i64,
    requires_shutdown: bool,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::wo::domain::WorkOrder> {
    let user = require_session!(state);
    require_on_wo(&state, user.user_id, crate::rbac::permissions::OT_EDIT, wo_id).await?;
    let wo = crate::wo::workflow::actions::schedule::save_shutdown_requirement(
        &state.db,
        wo_id,
        requires_shutdown,
        expected_row_version,
        i64::from(user.user_id),
    )
    .await?;
    audit::record_wo_change_event(
        &state.db,
        audit::WoAuditInput {
            wo_id: Some(wo.id),
            action: if requires_shutdown {
                "shutdown_required_set".into()
            } else {
                "shutdown_required_cleared".into()
            },
            actor_id: Some(i64::from(user.user_id)),
            summary: Some(if requires_shutdown {
                "Work order flagged as needing a shutdown".into()
            } else {
                "Work order no longer needs a shutdown".into()
            }),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(wo)
}
