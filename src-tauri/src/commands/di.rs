//! DI IPC commands.
//!
//! Phase 2 – lifecycle redesign (migration 139).
//!
//! Permission gates:
//!   di.view         — list, get, review events, SLA status, list attachments
//!   di.create       — create any DI
//!   di.create.own   — create DI as self, upload attachment on own DI, cancel own DI,
//!                     answer and resubmit own DI returned for information
//!   di.screen       — triage submitted DIs (submitted → in_review)
//!   di.review       — update draft, return, close from in_review, upload attachment on any DI
//!   di.approve      — approve, defer, reactivate, close from awaiting_approval/approved
//!   di.convert      — convert DI to WO (confirmed in a dialog; no step-up)
//!   di.admin        — SLA rule management, delete attachment records
//!
//! Site scope (step 6, `docs/SITE_SCOPED_ROLES.md`): every request is checked at its org node
//! (`di::access`). Lists, queue counts, statistics and the audit log are limited in SQL to where
//! the user holds the right; the list says how many requests it leaves out and why. A plant-wide
//! role sees and acts everywhere, as before. SLA rules stay plant-wide objects.

use tauri::{Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::auth::scope::Anchor;
use crate::di::access;
use crate::di::attachments;
use crate::di::audit;
use crate::di::conversion;
use crate::di::queries;
use crate::di::review;
use crate::di::sla;
use crate::di::stats;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_permission_at, require_session};

use sea_orm::{ConnectionTrait, DbBackend, Statement};

// ═══════════════════════════════════════════════════════════════════════════════
// Site-scope checks shared by the commands (`di::access`)
// ═══════════════════════════════════════════════════════════════════════════════

/// `di.screen` or `di.review` at the request's org node (triage and screening).
async fn require_screen_or_review(state: &AppState, user_id: i32, di_id: i64, denied_message: &str) -> AppResult<()> {
    access::require_any_on_di(
        &state.db,
        &state.permission_cache,
        user_id,
        &[crate::rbac::permissions::DI_SCREEN, crate::rbac::permissions::DI_REVIEW],
        di_id,
        denied_message,
    )
    .await
}

/// The requester with `di.create.own`, or anyone with `di.review`, at the request's org node
/// (edit the draft, add an attachment).
async fn require_own_or_review(
    state: &AppState,
    user_id: i32,
    di: &crate::di::domain::InterventionRequest,
) -> AppResult<()> {
    access::require_own_or_review(&state.db, &state.permission_cache, user_id, di).await
}

/// `permission` on the request `di_id` (held nowhere: refused as before; no such request: not
/// found; else checked at its org node).
async fn require_on_di(state: &AppState, user_id: i32, permission: &str, di_id: i64) -> AppResult<Anchor> {
    access::require_on_di(&state.db, &state.permission_cache, user_id, permission, di_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Composite response for get_di detail view
// ═══════════════════════════════════════════════════════════════════════════════

/// Full detail payload returned by `get_di`.
/// Bundles the DI record, its transition history, and recent similar DIs.
#[derive(Debug, serde::Serialize)]
pub struct DiGetResponse {
    pub di: crate::di::domain::InterventionRequest,
    pub transitions: Vec<queries::DiTransitionRow>,
    pub similar: Vec<queries::DiSummaryRow>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_di — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_di(mut filter: queries::DiListFilter, state: State<'_, AppState>) -> AppResult<queries::DiListPage> {
    let user = require_session!(state);
    let scope = access::require_anywhere(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
    )
    .await?;
    // "My requests" is always the signed-in user's, whatever submitter the caller sent.
    if filter.queue.as_deref().map(str::trim) == Some("mine") {
        filter.submitter_id = Some(i64::from(user.user_id));
    }
    // Where the user holds di.view, in SQL before paging; the page counts what it leaves out.
    queries::list_intervention_requests_in_scope(&state.db, filter, &scope).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// A2) get_di_queue_counts — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_di_queue_counts(state: State<'_, AppState>) -> AppResult<queries::DiQueueCounts> {
    let user = require_session!(state);
    let scope = access::require_anywhere(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
    )
    .await?;
    queries::count_di_queues_in_scope(&state.db, i64::from(user.user_id), &access::ScopeSql::of(&scope)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_di — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_di(id: i64, state: State<'_, AppState>) -> AppResult<DiGetResponse> {
    let user = require_session!(state);
    let scope = access::require_anywhere(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
    )
    .await?;

    let di = queries::get_intervention_request(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: id.to_string(),
        })?;
    crate::auth::scope::check_anchor(
        &state.db,
        &scope,
        crate::rbac::permissions::DI_VIEW,
        Anchor::of(Some(di.org_node_id)),
    )
    .await?;

    let transitions = queries::get_di_transition_log(&state.db, id).await?;
    // Only the similar requests the user may open.
    let similar =
        queries::get_recent_similar_dis_in_scope(&state.db, di.equipment_id, di.symptom_code_id, 30, &scope).await?;

    Ok(DiGetResponse {
        di,
        transitions,
        similar,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) create_di — requires di.create or di.create.own
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn create_di(
    mut input: queries::DiCreateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    // The requester is the signed-in user, never a value sent by the screen (a di.create.own
    // holder could otherwise file a request in someone else's name).
    input.submitter_id = i64::from(user.user_id);

    // Decision O7 (an org node is required, for everyone), then di.create or di.create.own at the
    // new request's org node and at its equipment's node (rule §2.6).
    access::require_create_at(
        &state.db,
        &state.permission_cache,
        user.user_id,
        input.org_node_id,
        input.equipment_id,
    )
    .await?;

    let mut errors: Vec<String> = Vec::new();

    if input.title.trim().is_empty() {
        errors.push("Le titre est obligatoire.".into());
    }
    if input.description.trim().is_empty() {
        errors.push("La description est obligatoire.".into());
    }
    if input.origin_type.trim().is_empty() {
        errors.push("Le type d'origine est obligatoire.".into());
    }
    if input.request_type.trim().is_empty() {
        errors.push("Le type de demande est obligatoire.".into());
    }
    if input.symptom_code_id.is_none() {
        errors.push("Le symptôme est obligatoire.".into());
    }

    let asset_exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ?",
            [input.equipment_id.into()],
        ))
        .await?;
    if asset_exists.is_none() {
        errors.push(format!("Équipement introuvable (equipment_id={}).", input.equipment_id));
    }

    let node_exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM org_nodes WHERE id = ?",
            [input.org_node_id.into()],
        ))
        .await?;
    if node_exists.is_none() {
        errors.push(format!(
            "Nœud organisationnel introuvable (org_node_id={}).",
            input.org_node_id
        ));
    }

    if errors.is_empty() {
        match crate::di::reference_catalog::validate_di_origin_code(&state.db, &input.origin_type).await {
            Ok(_) => {}
            Err(AppError::ValidationFailed(mut catalog_errors)) => errors.append(&mut catalog_errors),
            Err(other) => return Err(other),
        }
        match crate::di::reference_catalog::validate_di_request_type(&state.db, &input.request_type).await {
            Ok(_) => {}
            Err(AppError::ValidationFailed(mut catalog_errors)) => errors.append(&mut catalog_errors),
            Err(other) => return Err(other),
        }
        if let Some(sid) = input.symptom_code_id {
            match crate::di::reference_catalog::validate_di_symptom_id(&state.db, sid).await {
                Ok(()) => {}
                Err(AppError::ValidationFailed(mut catalog_errors)) => errors.append(&mut catalog_errors),
                Err(other) => return Err(other),
            }
        }
    }

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    queries::create_intervention_request(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// C2) triage_submitted_di — requires di.screen or di.review
//     submitted|returned_for_clarification → in_review
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn triage_submitted_di(
    input: queries::DiTriageSubmittedInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    let uid = i64::from(user.user_id);

    require_screen_or_review(
        &state,
        user.user_id,
        input.di_id,
        "Tri des demandes : permission di.screen ou di.review requise.",
    )
    .await?;

    let di = queries::triage_submitted_di(&state.db, input, uid).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "triage_submitted".into(),
            actor_id: Some(uid),
            summary: Some("Tri d'entrée : DI admise en revue (soumis → en_revue)".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) update_di_draft — requires di.create.own (own) or di.review (any)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_di_draft(
    input: queries::DiDraftUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);

    let current_di = queries::get_intervention_request(&state.db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.id.to_string(),
        })?;

    // The draft fields do not include the org node or the equipment: the request stays where it
    // is, so its own anchor is the only one to check (no move, rule §2.7 does not apply).
    require_own_or_review(&state, user.user_id, &current_di).await?;

    queries::update_di_draft_fields(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) screen_di — requires di.screen or di.review
//    in_review → awaiting_approval (atomic single step)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn screen_di(
    mut input: review::DiScreenInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_screen_or_review(
        &state,
        user.user_id,
        input.di_id,
        "Examen de la demande : permission di.screen ou di.review requise.",
    )
    .await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::screen_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "screened".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI screened and advanced to awaiting_approval".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) return_di — requires di.review
//    in_review → returned_for_clarification
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn return_di(
    mut input: review::DiReturnInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_REVIEW, input.di_id).await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::return_di_for_clarification(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "returned".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI returned for clarification".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F2) resubmit_di — requires di.create.own; actor must be the requester (enforced in domain)
//     returned_for_clarification → in_review, with the requester's answer
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn resubmit_di(
    mut input: review::DiResubmitInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(
        &state,
        user.user_id,
        crate::rbac::permissions::DI_CREATE_OWN,
        input.di_id,
    )
    .await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::resubmit_returned_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "resubmitted".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI answered by the requester and resubmitted for review.".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) close_di — permission depends on current DI status
//    in_review       → requires di.review
//    awaiting_approval | approved → requires di.approve
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_di(
    mut input: review::DiCloseInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);

    // Load current status to determine which permission is required.
    let current_di = queries::get_intervention_request(&state.db, input.di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.di_id.to_string(),
        })?;

    match current_di.status.as_str() {
        "in_review" => {
            require_permission_at!(
                state,
                &user,
                crate::rbac::permissions::DI_REVIEW,
                Anchor::of(Some(current_di.org_node_id))
            );
        }
        "awaiting_approval" | "approved" => {
            require_permission_at!(
                state,
                &user,
                crate::rbac::permissions::DI_APPROVE,
                Anchor::of(Some(current_di.org_node_id))
            );
        }
        other => {
            return Err(AppError::ValidationFailed(vec![format!(
                "La fermeture n'est possible qu'au statut 'in_review', 'awaiting_approval' ou 'approved'. \
                 Statut actuel : '{}'.",
                other
            )]));
        }
    }

    input.actor_id = i64::from(user.user_id);
    let di = review::close_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "closed".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some(format!(
                "DI fermée avec disposition '{}'.",
                di.disposition_code.as_deref().unwrap_or("?")
            )),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G2) cancel_own_di — submitted|returned_for_clarification → closed (requester cancel)
//     requires di.create.own; actor must be submitter (enforced in domain)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn cancel_own_di(
    mut input: review::DiCancelOwnInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(
        &state,
        user.user_id,
        crate::rbac::permissions::DI_CREATE_OWN,
        input.di_id,
    )
    .await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::cancel_own_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "cancelled_own".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI annulée par le demandeur.".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G1b) reject_di — compat wrapper → close with rejected_invalid / duplicate
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn reject_di(
    mut input: review::DiRejectInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_REVIEW, input.di_id).await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::reject_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "rejected".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some(format!(
                "DI rejected (disposition '{}').",
                di.disposition_code.as_deref().unwrap_or("rejected_invalid")
            )),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G1c) close_di_as_non_executable — compat wrapper → close with no_work_required
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn close_di_as_non_executable(
    mut input: review::DiCloseNonExecutableInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_APPROVE, input.di_id).await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::close_di_as_non_executable(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "closed_non_executable".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI closed as non-executable (disposition no_work_required).".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// H) approve_di — requires di.approve (confirmed in a dialog; no step-up)
//    awaiting_approval → approved
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn approve_di(
    mut input: review::DiApproveInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_APPROVE, input.di_id).await?;

    input.actor_id = i64::from(user.user_id);
    let di = review::approve_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "approved".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI approuvée.".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// I) defer_di — requires di.approve
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn defer_di(
    mut input: review::DiDeferInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_APPROVE, input.di_id).await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::defer_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "deferred".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI reportée.".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// J) reactivate_di — requires di.approve
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn reactivate_di(
    mut input: review::DiReactivateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_APPROVE, input.di_id).await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::reactivate_deferred_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "reactivated".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI réactivée depuis le report.".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// K) archive_di — requires di.approve (only on closed DIs)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn archive_di(
    mut input: review::DiArchiveInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_APPROVE, input.di_id).await?;
    input.actor_id = i64::from(user.user_id);
    let di = review::archive_di(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(di.id),
            action: "archived".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some("DI archivée.".into()),
            details_json: None,
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// L) get_di_review_events — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_di_review_events(di_id: i64, state: State<'_, AppState>) -> AppResult<Vec<review::DiReviewEvent>> {
    let user = require_session!(state);
    access::require_on_di_records(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
        di_id,
    )
    .await?;
    review::get_review_events(&state.db, di_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// M) upload_di_attachment — requires di.create.own (own) or di.review (any)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn upload_di_attachment(
    app: tauri::AppHandle,
    di_id: i64,
    file_name: String,
    file_bytes: Vec<u8>,
    mime_type: String,
    attachment_type: String,
    notes: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<attachments::DiAttachment> {
    let user = require_session!(state);

    let current_di = queries::get_intervention_request(&state.db, di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    require_own_or_review(&state, user.user_id, &current_di).await?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;

    let input = attachments::DiAttachmentInput {
        di_id,
        file_name,
        file_bytes,
        mime_type,
        attachment_type,
        notes,
        uploaded_by_id: i64::from(user.user_id),
    };

    attachments::save_di_attachment(&state.db, &app_data_dir, input).await
}

#[tauri::command]
pub async fn upload_di_attachment_from_path(
    app: tauri::AppHandle,
    di_id: i64,
    source_path: String,
    attachment_type: Option<String>,
    notes: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<attachments::DiAttachment> {
    let user = require_session!(state);

    let current_di = queries::get_intervention_request(&state.db, di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    require_own_or_review(&state, user.user_id, &current_di).await?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;

    attachments::save_di_attachment_from_path(
        &state.db,
        &app_data_dir,
        di_id,
        &source_path,
        attachment_type.as_deref().unwrap_or(""),
        notes,
        i64::from(user.user_id),
    )
    .await
}

#[tauri::command]
pub async fn read_di_attachment_preview(
    app: tauri::AppHandle,
    attachment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<attachments::DiAttachmentPreview> {
    let user = require_session!(state);
    access::require_on_attachment(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
        attachment_id,
    )
    .await?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;

    attachments::read_di_attachment_preview(&state.db, &app_data_dir, attachment_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// N) list_di_attachments — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_di_attachments(di_id: i64, state: State<'_, AppState>) -> AppResult<Vec<attachments::DiAttachment>> {
    let user = require_session!(state);
    access::require_on_di_records(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
        di_id,
    )
    .await?;
    attachments::list_di_attachments(&state.db, di_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// O) delete_di_attachment — requires di.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn delete_di_attachment(attachment_id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    access::require_on_attachment(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_ADMIN,
        attachment_id,
    )
    .await?;
    attachments::delete_di_attachment_record(&state.db, attachment_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// P) convert_di_to_wo — requires di.convert (confirmed in a dialog; no step-up)
//    approved → closed (disposition=converted_to_wo)
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn convert_di_to_wo(
    mut input: conversion::WoConversionInput,
    state: State<'_, AppState>,
) -> AppResult<conversion::WoConversionResult> {
    let user = require_session!(state);
    // di.convert on the request, and the work-order create right where the new work order is
    // anchored (`conversion` copies the request's org node into `work_orders.entity_id`).
    access::require_convert(&state.db, &state.permission_cache, user.user_id, input.di_id).await?;

    input.actor_id = i64::from(user.user_id);
    let result = conversion::convert_di_to_work_order(&state.db, input).await?;
    audit::record_di_change_event(
        &state.db,
        audit::DiAuditInput {
            di_id: Some(result.di.id),
            action: "converted".into(),
            actor_id: Some(i64::from(user.user_id)),
            summary: Some(format!("DI convertie en OT {}", result.wo_code)),
            details_json: Some(
                serde_json::json!({
                    "wo_id": result.wo_id,
                    "wo_code": result.wo_code
                })
                .to_string(),
            ),
            requires_step_up: false,
            apply_result: "applied".into(),
        },
    )
    .await;
    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Q) get_sla_status — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_sla_status(di_id: i64, state: State<'_, AppState>) -> AppResult<sla::DiSlaStatus> {
    let user = require_session!(state);
    require_on_di(&state, user.user_id, crate::rbac::permissions::DI_VIEW, di_id).await?;

    let di = queries::get_intervention_request(&state.db, di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    sla::compute_sla_status(&state.db, &di).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// R) list_sla_rules — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_sla_rules(state: State<'_, AppState>) -> AppResult<Vec<sla::DiSlaRule>> {
    let user = require_session!(state);
    // SLA rules are plant-wide reference settings: plant-wide right (step 6 §2.9).
    require_permission!(state, &user, crate::rbac::permissions::DI_VIEW, PermissionScope::Global);
    sla::list_sla_rules(&state.db).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// S) update_sla_rule — requires di.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_sla_rule(input: sla::SlaRuleUpdateInput, state: State<'_, AppState>) -> AppResult<sla::DiSlaRule> {
    let user = require_session!(state);
    // SLA rules are plant-wide reference settings: plant-wide right (step 6 §2.9).
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::DI_ADMIN,
        PermissionScope::Global
    );
    sla::update_sla_rule(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// T) list_di_change_events — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_di_change_events(
    di_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::DiChangeEvent>> {
    let user = require_session!(state);
    access::require_on_di_records(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
        di_id,
    )
    .await?;
    audit::list_di_change_events(&state.db, di_id, limit.unwrap_or(50)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// U) list_all_di_change_events — requires di.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_all_di_change_events(
    filter: audit::DiAuditFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::DiChangeEvent>> {
    let user = require_session!(state);
    // The audit log of the requests where the user holds di.admin.
    let scope = access::require_anywhere(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_ADMIN,
    )
    .await?;
    audit::list_all_change_events(&state.db, filter, &access::ScopeSql::of(&scope)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// V) get_di_stats — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_di_stats(
    filter: stats::DiStatsFilter,
    state: State<'_, AppState>,
) -> AppResult<stats::DiStatsPayload> {
    let user = require_session!(state);
    let scope = access::require_anywhere(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::DI_VIEW,
    )
    .await?;
    stats::get_di_stats_in_scope(&state.db, filter, &access::ScopeSql::of(&scope)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use crate::auth::rbac::{self, PermissionScope};

    async fn setup() -> sea_orm::DatabaseConnection {
        crate::db::test_db::seeded().await
    }

    async fn assign_role(db: &sea_orm::DatabaseConnection, user_id: i32, role_name: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (entity_sync_id, user_id, role_id, scope_type, created_at, updated_at) \
             VALUES ('test-assign-' || ?, ?, \
               (SELECT id FROM roles WHERE name = ?), \
               'tenant', ?, ?)",
            [
                user_id.into(),
                user_id.into(),
                role_name.into(),
                now.clone().into(),
                now.into(),
            ],
        ))
        .await
        .expect("insert user_scope_assignment");
    }

    // ─── V1 — Permission guard on approve ───────────────────────────────

    #[tokio::test]
    async fn v1_readonly_user_denied_di_approve() {
        let db = setup().await;
        assign_role(&db, 50, "Readonly").await;

        let has = rbac::check_permission(&db, 50, crate::rbac::permissions::DI_APPROVE, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(!has, "Readonly must NOT have di.approve");
    }

    #[tokio::test]
    async fn v1_operator_denied_di_approve() {
        let db = setup().await;
        assign_role(&db, 51, "Operator").await;

        let has = rbac::check_permission(&db, 51, crate::rbac::permissions::DI_APPROVE, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(!has, "Operator must NOT have di.approve");
    }

    #[tokio::test]
    async fn v1_view_only_user_denied_di_approve() {
        let db = setup().await;
        assign_role(&db, 52, "Readonly").await;

        let view = rbac::check_permission(&db, 52, crate::rbac::permissions::DI_VIEW, &PermissionScope::Global)
            .await
            .expect("check");
        let approve = rbac::check_permission(&db, 52, crate::rbac::permissions::DI_APPROVE, &PermissionScope::Global)
            .await
            .expect("check");

        assert!(view, "Readonly must have di.view");
        assert!(!approve, "Readonly must NOT have di.approve");
    }

    #[tokio::test]
    async fn v1_supervisor_has_di_approve() {
        let db = setup().await;
        assign_role(&db, 53, "Supervisor").await;

        let has = rbac::check_permission(&db, 53, crate::rbac::permissions::DI_APPROVE, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Supervisor must have di.approve");
    }

    #[tokio::test]
    async fn v1_administrator_has_di_approve() {
        let db = setup().await;
        assign_role(&db, 54, "Administrator").await;

        let has = rbac::check_permission(&db, 54, crate::rbac::permissions::DI_APPROVE, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Administrator must have di.approve");
    }

    #[tokio::test]
    async fn v1_di_approve_does_not_require_step_up() {
        let db = setup().await;

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                [crate::rbac::permissions::DI_APPROVE.into()],
            ))
            .await
            .expect("query")
            .expect("di.approve permission must exist in seed data");
        let requires: i64 = row.try_get("", "requires_step_up").expect("column");
        // Owner decision (2026-10-14): approval is confirmed in a dialog, not by password re-entry.
        assert_eq!(requires, 0, "di.approve must not require step-up authentication");
    }

    // ─── V2 — screen_di permission ──────────────────────────────────────

    #[tokio::test]
    async fn v2_supervisor_has_di_review() {
        let db = setup().await;
        assign_role(&db, 60, "Supervisor").await;

        let has = rbac::check_permission(&db, 60, crate::rbac::permissions::DI_REVIEW, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Supervisor must have di.review for screen_di access");
    }

    #[tokio::test]
    async fn v2_administrator_has_di_review() {
        let db = setup().await;
        assign_role(&db, 61, "Administrator").await;

        let has = rbac::check_permission(&db, 61, crate::rbac::permissions::DI_REVIEW, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Administrator must have di.review");
    }

    #[tokio::test]
    async fn v2_operator_denied_di_review() {
        let db = setup().await;
        assign_role(&db, 62, "Operator").await;

        let has = rbac::check_permission(&db, 62, crate::rbac::permissions::DI_REVIEW, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(!has, "Operator must NOT have di.review");
    }

    #[tokio::test]
    async fn v2_readonly_denied_di_review() {
        let db = setup().await;
        assign_role(&db, 63, "Readonly").await;

        let has = rbac::check_permission(&db, 63, crate::rbac::permissions::DI_REVIEW, &PermissionScope::Global)
            .await
            .expect("check");
        assert!(!has, "Readonly must NOT have di.review");
    }

    #[tokio::test]
    async fn v2_di_review_does_not_require_step_up() {
        let db = setup().await;

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                [crate::rbac::permissions::DI_REVIEW.into()],
            ))
            .await
            .expect("query")
            .expect("di.review permission must exist in seed data");
        let requires: i64 = row.try_get("", "requires_step_up").expect("column");
        assert_eq!(requires, 0, "di.review must NOT require step-up");
    }

    // ─── V2 bonus: screen_di domain-layer success ───────────────────────

    #[tokio::test]
    async fn v2_screen_succeeds_when_di_is_in_review() {
        let db = setup().await;

        seed_fk_data(&db).await;
        let user_id: i64 = db
            .query_one(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT id FROM user_accounts LIMIT 1".to_string(),
            ))
            .await
            .expect("q")
            .expect("user")
            .try_get::<i64>("", "id")
            .expect("id");

        let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(&db, "vibration")
            .await
            .expect("lookup")
            .expect("seeded");
        let di = crate::di::queries::create_intervention_request(
            &db,
            crate::di::queries::DiCreateInput {
                equipment_id: 1,
                org_node_id: 1,
                title: "Pump vibration".into(),
                description: "Excessive vibration on pump P-101".into(),
                origin_type: "operator".into(),
                request_type: "repair".to_string(),
                symptom_code_id: Some(symptom_id),
                impact_level: "unknown".into(),
                production_impact: false,
                safety_flag: false,
                environmental_flag: false,
                quality_flag: false,
                reported_urgency: "normal".into(),
                observed_at: None,
                source_inspection_anomaly_id: None,
                submitter_id: user_id,
            },
        )
        .await
        .expect("create DI");

        // Advance to in_review
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'in_review', \
             row_version = row_version + 1, updated_at = datetime('now') WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("advance");

        let result = crate::di::review::screen_di(
            &db,
            crate::di::review::DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "high".into(),
                classification_code_id: Some(900001),
                reviewer_note: Some("Validated".into()),
            },
        )
        .await
        .expect("screen_di must succeed when DI is in_review");

        assert_eq!(result.status, "awaiting_approval");
    }

    // ─── V3 — Command registration ─────────────────────────────────────

    #[test]
    fn v3_no_duplicate_command_function_names() {
        let fns: Vec<&str> = vec![
            "list_di",
            "get_di",
            "create_di",
            "update_di_draft",
            "triage_submitted_di",
            "screen_di",
            "return_di",
            "close_di",
            "cancel_own_di",
            "reject_di",
            "close_di_as_non_executable",
            "approve_di",
            "defer_di",
            "reactivate_di",
            "archive_di",
            "get_di_review_events",
            "upload_di_attachment",
            "upload_di_attachment_from_path",
            "read_di_attachment_preview",
            "list_di_attachments",
            "delete_di_attachment",
            "convert_di_to_wo",
            "get_sla_status",
            "list_sla_rules",
            "update_sla_rule",
            "list_di_change_events",
            "list_all_di_change_events",
            "get_di_stats",
            "get_di_queue_counts",
            "resubmit_di",
        ];
        let unique: std::collections::HashSet<&str> = fns.iter().copied().collect();
        assert_eq!(fns.len(), unique.len(), "All DI command names must be unique");
        assert_eq!(
            fns.len(),
            30,
            "Expected exactly 30 DI commands (incl. close/reject compat paths)"
        );
    }

    // ─── Lifecycle V1 — Permission seed ─────────────────────────────────

    #[tokio::test]
    async fn s1_v1_permission_seed_exactly_7_di_rows() {
        let db = setup().await;

        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM permissions WHERE name LIKE 'di.%' ORDER BY name".to_string(),
            ))
            .await
            .expect("query permissions");

        let names: Vec<String> = rows
            .iter()
            .map(|r| r.try_get::<String>("", "name").expect("name col"))
            .collect();

        assert_eq!(
            names,
            vec![
                crate::rbac::permissions::DI_ADMIN,
                crate::rbac::permissions::DI_APPROVE,
                crate::rbac::permissions::DI_CONVERT,
                crate::rbac::permissions::DI_CREATE,
                crate::rbac::permissions::DI_CREATE_OWN,
                crate::rbac::permissions::DI_DELETE,
                crate::rbac::permissions::DI_REVIEW,
                crate::rbac::permissions::DI_SCREEN,
                crate::rbac::permissions::DI_VIEW,
            ],
            "Canonical di.* permissions"
        );
    }

    // ─── Lifecycle V2 — Audit on approval ────────────────────────────────

    #[tokio::test]
    async fn s1_v2_audit_event_on_approval() {
        let db = setup().await;
        seed_fk_data(&db).await;

        let di = create_test_di(&db).await;

        // Advance to in_review then screen to awaiting_approval
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'in_review', \
             row_version = row_version + 1, updated_at = datetime('now') WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("advance to in_review");

        let screened = crate::di::review::screen_di(
            &db,
            crate::di::review::DiScreenInput {
                di_id: di.id,
                actor_id: 1,
                expected_row_version: 2,
                validated_urgency: "high".into(),
                classification_code_id: Some(900001),
                reviewer_note: Some("Test screen".into()),
            },
        )
        .await
        .expect("screen should succeed from in_review");

        let approved = crate::di::review::approve_di(
            &db,
            crate::di::review::DiApproveInput {
                di_id: screened.id,
                actor_id: 1,
                expected_row_version: screened.row_version,
                notes: Some("Approved for test".into()),
            },
        )
        .await
        .expect("approve should succeed");

        crate::di::audit::record_di_change_event(
            &db,
            crate::di::audit::DiAuditInput {
                di_id: Some(approved.id),
                action: "approved".into(),
                actor_id: Some(1),
                summary: Some("DI approuvée.".into()),
                details_json: None,
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        let events = crate::di::audit::list_di_change_events(&db, approved.id, 100)
            .await
            .expect("list audit events");

        let approved_events: Vec<_> = events
            .iter()
            .filter(|e| e.action == "approved" && e.apply_result == "applied")
            .collect();

        assert_eq!(approved_events.len(), 1, "Exactly 1 audit event with action='approved'");
        assert_eq!(approved.status, "approved");
        assert_eq!(approved_events[0].requires_step_up, 0);
    }

    // ─── Lifecycle V3 — Audit on blocked step-up ─────────────────────────

    #[tokio::test]
    async fn s1_v3_audit_event_on_blocked_step_up() {
        let db = setup().await;
        seed_fk_data(&db).await;

        let di = create_test_di(&db).await;

        crate::di::audit::record_di_change_event(
            &db,
            crate::di::audit::DiAuditInput {
                di_id: Some(di.id),
                action: "approved".into(),
                actor_id: Some(1),
                summary: Some("Approval blocked: step-up verification failed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "blocked".into(),
            },
        )
        .await;

        let events = crate::di::audit::list_di_change_events(&db, di.id, 100)
            .await
            .expect("list events");

        let blocked: Vec<_> = events.iter().filter(|e| e.apply_result == "blocked").collect();

        assert_eq!(blocked.len(), 1, "One blocked audit event must exist");
        assert_eq!(blocked[0].action, "approved");
        assert_eq!(blocked[0].requires_step_up, 1);
    }

    // ─── Lifecycle V4 — Audit on conversion ──────────────────────────────

    #[tokio::test]
    async fn s1_v4_audit_event_on_conversion() {
        let db = setup().await;
        seed_fk_data(&db).await;

        let di = create_test_di(&db).await;

        crate::di::audit::record_di_change_event(
            &db,
            crate::di::audit::DiAuditInput {
                di_id: Some(di.id),
                action: "converted".into(),
                actor_id: Some(1),
                summary: Some("DI convertie en OT".into()),
                details_json: Some(r#"{"wo_id":1,"wo_code":"OT-0001"}"#.into()),
                requires_step_up: false,
                apply_result: "applied".into(),
            },
        )
        .await;

        let events = crate::di::audit::list_di_change_events(&db, di.id, 100)
            .await
            .expect("list events");

        let converted: Vec<_> = events.iter().filter(|e| e.action == "converted").collect();

        assert_eq!(converted.len(), 1, "One converted audit event");
        assert_eq!(
            converted[0].requires_step_up, 0,
            "Conversion is confirmed in a dialog, no step-up"
        );
        assert_eq!(converted[0].apply_result, "applied");
    }

    // ─── Test helpers ────────────────────────────────────────────────────

    async fn seed_fk_data(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO equipment (id, entity_sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'eq-001', 'EQ-001', 'Test Equip', 'active_in_service', datetime('now'), datetime('now'))".to_string()
        )).await.expect("equipment");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_structure_models (id, entity_sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'mdl-001', 1, 'active', datetime('now'), datetime('now'))"
                .to_string(),
        ))
        .await
        .expect("model");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_node_types (id, entity_sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'nt-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))".to_string()
        )).await.expect("node_type");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_nodes (id, entity_sync_id, code, name, node_type_id, structure_model_id, status, created_at, updated_at) \
             VALUES (1, 'on-001', 'SITE-001', 'Test Site', 1, 1, 'active', datetime('now'), datetime('now'))".to_string()
        )).await.expect("org_node");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_domains (id, code, name, structure_type, governance_level, is_extendable, created_at, updated_at) \
             VALUES (900001, 'DI_CLASS', 'DI Classification', 'flat', 'tenant_managed', 1, datetime('now'), datetime('now'))".to_string()
        )).await.expect("ref_domain");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_sets (id, domain_id, version_no, status, created_at) \
             VALUES (900001, 900001, 1, 'published', datetime('now'))"
                .to_string(),
        ))
        .await
        .expect("ref_set");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values (id, entity_sync_id, set_id, code, label, is_active) \
             VALUES (900001, 'test-rv-di-class-mech', 900001, 'MECH', 'Mécanique', 1)"
                .to_string(),
        ))
        .await
        .expect("ref_value");
    }

    async fn create_test_di(db: &sea_orm::DatabaseConnection) -> crate::di::domain::InterventionRequest {
        let symptom_id = crate::di::reference_catalog::resolve_di_symptom_id_by_code(db, "vibration")
            .await
            .expect("lookup")
            .expect("seeded");
        crate::di::queries::create_intervention_request(
            db,
            crate::di::queries::DiCreateInput {
                equipment_id: 1,
                org_node_id: 1,
                submitter_id: 1,
                title: "Test DI for verification".into(),
                description: "Verification test DI".into(),
                origin_type: "operator".into(),
                request_type: "repair".to_string(),
                reported_urgency: "high".into(),
                observed_at: None,
                impact_level: "medium".into(),
                production_impact: false,
                safety_flag: false,
                environmental_flag: false,
                quality_flag: false,
                symptom_code_id: Some(symptom_id),
                source_inspection_anomaly_id: None,
            },
        )
        .await
        .expect("create DI should succeed")
    }
}
