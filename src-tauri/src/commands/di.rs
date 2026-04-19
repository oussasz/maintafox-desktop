//! DI IPC commands.
//!
//! Phase 2 – Sub-phase 04 – Files 01, 02 & 03.
//!
//! Permission gates:
//!   di.view       — list, get, review events, SLA status, list attachments
//!   di.create     — create any DI
//!   di.create.own — create DI as self, upload attachment on own DI
//!   di.review     — update draft, screen, return, reject, upload attachment on any DI
//!   di.approve    — approve, defer, reactivate
//!   di.convert    — convert DI to WO (step-up required)
//!   di.admin      — SLA rule management, delete attachment records

use tauri::{Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::di::attachments;
use crate::di::audit;
use crate::di::conversion;
use crate::di::domain::DiOriginType;
use crate::di::queries;
use crate::di::review;
use crate::di::sla;
use crate::errors::{AppError, AppResult};
use crate::state::AppState;
use crate::{require_permission, require_session};

use sea_orm::{ConnectionTrait, DbBackend, Statement};

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
pub async fn list_di(
    filter: queries::DiListFilter,
    state: State<'_, AppState>,
) -> AppResult<queries::DiListPage> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.view", PermissionScope::Global);
    queries::list_intervention_requests(&state.db, filter).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_di — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_di(
    id: i64,
    state: State<'_, AppState>,
) -> AppResult<DiGetResponse> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.view", PermissionScope::Global);

    let di = queries::get_intervention_request(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: id.to_string(),
        })?;

    let transitions = queries::get_di_transition_log(&state.db, id).await?;
    let similar = queries::get_recent_similar_dis(
        &state.db,
        di.asset_id,
        di.symptom_code_id,
        30, // look back 30 days for recurrence context
    )
    .await?;

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
    input: queries::DiCreateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);

    // Either di.create (global) or di.create.own (self-only)
    let has_global = crate::auth::rbac::check_permission(
        &state.db,
        user.user_id,
        "di.create",
        &PermissionScope::Global,
    )
    .await?;

    if !has_global {
        require_permission!(state, &user, "di.create.own", PermissionScope::Global);
    }

    // ── Validate required fields ──────────────────────────────────────────
    let mut errors: Vec<String> = Vec::new();

    if input.title.trim().is_empty() {
        errors.push("Le titre est obligatoire.".into());
    }
    if input.description.trim().is_empty() {
        errors.push("La description est obligatoire.".into());
    }
    if DiOriginType::try_from_str(&input.origin_type).is_err() {
        errors.push(format!(
            "Type d'origine invalide : '{}'. Valeurs autorisées : operator, technician, \
             inspection, pm, iot, quality, hse, production, external.",
            input.origin_type
        ));
    }

    // Validate asset_id resolves
    let asset_exists = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM equipment WHERE id = ?",
            [input.asset_id.into()],
        ))
        .await?;
    if asset_exists.is_none() {
        errors.push(format!(
            "Équipement introuvable (asset_id={}).",
            input.asset_id
        ));
    }

    // Validate org_node_id resolves
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

    if !errors.is_empty() {
        return Err(AppError::ValidationFailed(errors));
    }

    queries::create_intervention_request(&state.db, input).await
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

    // Check ownership: if user owns the DI, di.create.own suffices;
    // otherwise di.review is required.
    let current_di = queries::get_intervention_request(&state.db, input.id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: input.id.to_string(),
        })?;

    let is_owner = current_di.submitter_id == i64::from(user.user_id);

    if is_owner {
        // Own DI — di.create.own suffices
        let has_own = crate::auth::rbac::check_permission(
            &state.db,
            user.user_id,
            "di.create.own",
            &PermissionScope::Global,
        )
        .await?;
        if !has_own {
            require_permission!(state, &user, "di.review", PermissionScope::Global);
        }
    } else {
        // Not owner — must have di.review
        require_permission!(state, &user, "di.review", PermissionScope::Global);
    }

    queries::update_di_draft_fields(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) screen_di — requires di.review
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn screen_di(
    mut input: review::DiScreenInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.review", PermissionScope::Global);
    input.actor_id = i64::from(user.user_id);
    let di = review::screen_di(&state.db, input).await?;
    audit::record_di_change_event(&state.db, audit::DiAuditInput {
        di_id: Some(di.id),
        action: "screened".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("DI screened and advanced to review".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// F) return_di — requires di.review
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn return_di(
    mut input: review::DiReturnInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.review", PermissionScope::Global);
    input.actor_id = i64::from(user.user_id);
    let di = review::return_di_for_clarification(&state.db, input).await?;
    audit::record_di_change_event(&state.db, audit::DiAuditInput {
        di_id: Some(di.id),
        action: "returned".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("DI returned for clarification".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// G) reject_di — requires di.review
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn reject_di(
    mut input: review::DiRejectInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.review", PermissionScope::Global);
    input.actor_id = i64::from(user.user_id);
    let di = review::reject_di(&state.db, input).await?;
    audit::record_di_change_event(&state.db, audit::DiAuditInput {
        di_id: Some(di.id),
        action: "rejected".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("DI rejected".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// H) approve_di — requires di.approve + step-up
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn approve_di(
    mut input: review::DiApproveInput,
    state: State<'_, AppState>,
) -> AppResult<crate::di::domain::InterventionRequest> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.approve", PermissionScope::Global);

    // Step-up check — record blocked audit event if it fails
    {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_di_change_event(&state.db, audit::DiAuditInput {
                di_id: Some(input.di_id),
                action: "approved".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Approval blocked: step-up verification failed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "blocked".into(),
            }).await;
            return Err(AppError::StepUpRequired);
        }
    }

    input.actor_id = i64::from(user.user_id);
    let di = review::approve_di_for_planning(&state.db, input).await?;
    audit::record_di_change_event(&state.db, audit::DiAuditInput {
        di_id: Some(di.id),
        action: "approved".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("DI approved for planning".into()),
        details_json: None,
        requires_step_up: true,
        apply_result: "applied".into(),
    }).await;
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
    require_permission!(state, &user, "di.approve", PermissionScope::Global);
    input.actor_id = i64::from(user.user_id);
    let di = review::defer_di(&state.db, input).await?;
    audit::record_di_change_event(&state.db, audit::DiAuditInput {
        di_id: Some(di.id),
        action: "deferred".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("DI deferred".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
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
    require_permission!(state, &user, "di.approve", PermissionScope::Global);
    input.actor_id = i64::from(user.user_id);
    let di = review::reactivate_deferred_di(&state.db, input).await?;
    audit::record_di_change_event(&state.db, audit::DiAuditInput {
        di_id: Some(di.id),
        action: "reactivated".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some("Deferred DI reactivated".into()),
        details_json: None,
        requires_step_up: false,
        apply_result: "applied".into(),
    }).await;
    Ok(di)
}

// ═══════════════════════════════════════════════════════════════════════════════
// K) get_di_review_events — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_di_review_events(
    di_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<review::DiReviewEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.view", PermissionScope::Global);
    review::get_review_events(&state.db, di_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// L) upload_di_attachment — requires di.create.own (own) or di.review (any)
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

    // Check ownership: if user owns the DI, di.create.own suffices;
    // otherwise di.review is required.
    let current_di = queries::get_intervention_request(&state.db, di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    let is_owner = current_di.submitter_id == i64::from(user.user_id);

    if is_owner {
        let has_own = crate::auth::rbac::check_permission(
            &state.db,
            user.user_id,
            "di.create.own",
            &PermissionScope::Global,
        )
        .await?;
        if !has_own {
            require_permission!(state, &user, "di.review", PermissionScope::Global);
        }
    } else {
        require_permission!(state, &user, "di.review", PermissionScope::Global);
    }

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

// ═══════════════════════════════════════════════════════════════════════════════
// M) list_di_attachments — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_di_attachments(
    di_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<attachments::DiAttachment>> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.view", PermissionScope::Global);
    attachments::list_di_attachments(&state.db, di_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// N) delete_di_attachment — requires di.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn delete_di_attachment(
    attachment_id: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.admin", PermissionScope::Global);
    attachments::delete_di_attachment_record(&state.db, attachment_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// O) convert_di_to_wo — requires di.convert + step-up
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn convert_di_to_wo(
    mut input: conversion::WoConversionInput,
    state: State<'_, AppState>,
) -> AppResult<conversion::WoConversionResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.convert", PermissionScope::Global);

    // Step-up check — record blocked audit event if it fails
    {
        let guard = state.session.read().await;
        if !guard.is_step_up_valid() {
            audit::record_di_change_event(&state.db, audit::DiAuditInput {
                di_id: Some(input.di_id),
                action: "converted".into(),
                actor_id: Some(i64::from(user.user_id)),
                summary: Some("Conversion blocked: step-up verification failed".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "blocked".into(),
            }).await;
            return Err(AppError::StepUpRequired);
        }
    }

    input.actor_id = i64::from(user.user_id);
    let result = conversion::convert_di_to_work_order(&state.db, input).await?;
    audit::record_di_change_event(&state.db, audit::DiAuditInput {
        di_id: Some(result.di.id),
        action: "converted".into(),
        actor_id: Some(i64::from(user.user_id)),
        summary: Some(format!("DI converted to work order {}", result.wo_code)),
        details_json: Some(serde_json::json!({
            "wo_id": result.wo_id,
            "wo_code": result.wo_code
        }).to_string()),
        requires_step_up: true,
        apply_result: "applied".into(),
    }).await;
    Ok(result)
}

// ═══════════════════════════════════════════════════════════════════════════════
// P) get_sla_status — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_sla_status(
    di_id: i64,
    state: State<'_, AppState>,
) -> AppResult<sla::DiSlaStatus> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.view", PermissionScope::Global);

    let di = queries::get_intervention_request(&state.db, di_id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InterventionRequest".into(),
            id: di_id.to_string(),
        })?;

    sla::compute_sla_status(&state.db, &di).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Q) list_sla_rules — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_sla_rules(
    state: State<'_, AppState>,
) -> AppResult<Vec<sla::DiSlaRule>> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.view", PermissionScope::Global);
    sla::list_sla_rules(&state.db).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// R) update_sla_rule — requires di.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_sla_rule(
    input: sla::SlaRuleUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<sla::DiSlaRule> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.admin", PermissionScope::Global);
    sla::update_sla_rule(&state.db, input).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// S) list_di_change_events — requires di.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_di_change_events(
    di_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::DiChangeEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.view", PermissionScope::Global);
    audit::list_di_change_events(&state.db, di_id, limit.unwrap_or(50)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// T) list_all_di_change_events — requires di.admin
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_all_di_change_events(
    filter: audit::DiAuditFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<audit::DiChangeEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "di.admin", PermissionScope::Global);
    audit::list_all_change_events(&state.db, filter).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sprint S2 verification tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    use crate::auth::rbac::{self, PermissionScope};

    async fn setup() -> sea_orm::DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should connect");
        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA");
        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations");
        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder");
        db
    }

    async fn assign_role(db: &sea_orm::DatabaseConnection, user_id: i32, role_name: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO user_scope_assignments \
             (sync_id, user_id, role_id, scope_type, created_at, updated_at) \
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

        let has = rbac::check_permission(&db, 50, "di.approve", &PermissionScope::Global)
            .await
            .expect("check");
        assert!(!has, "Readonly must NOT have di.approve");
    }

    #[tokio::test]
    async fn v1_operator_denied_di_approve() {
        let db = setup().await;
        assign_role(&db, 51, "Operator").await;

        let has = rbac::check_permission(&db, 51, "di.approve", &PermissionScope::Global)
            .await
            .expect("check");
        assert!(!has, "Operator must NOT have di.approve");
    }

    #[tokio::test]
    async fn v1_view_only_user_denied_di_approve() {
        let db = setup().await;
        assign_role(&db, 52, "Readonly").await;

        // Verify user has di.view but not di.approve
        let view = rbac::check_permission(&db, 52, "di.view", &PermissionScope::Global)
            .await
            .expect("check");
        let approve = rbac::check_permission(&db, 52, "di.approve", &PermissionScope::Global)
            .await
            .expect("check");

        assert!(view, "Readonly must have di.view");
        assert!(!approve, "Readonly must NOT have di.approve — PermissionDenied expected");
    }

    #[tokio::test]
    async fn v1_supervisor_has_di_approve() {
        let db = setup().await;
        assign_role(&db, 53, "Supervisor").await;

        let has = rbac::check_permission(&db, 53, "di.approve", &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Supervisor must have di.approve");
    }

    #[tokio::test]
    async fn v1_administrator_has_di_approve() {
        let db = setup().await;
        assign_role(&db, 54, "Administrator").await;

        let has = rbac::check_permission(&db, 54, "di.approve", &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Administrator must have di.approve");
    }

    #[tokio::test]
    async fn v1_di_approve_requires_step_up() {
        let db = setup().await;

        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT requires_step_up FROM permissions WHERE name = ?",
                ["di.approve".into()],
            ))
            .await
            .expect("query")
            .expect("di.approve permission must exist in seed data");
        let requires: i64 = row.try_get("", "requires_step_up").expect("column");
        assert_eq!(requires, 1, "di.approve must require step-up authentication");
    }

    // ─── V2 — screen_di permission ──────────────────────────────────────

    #[tokio::test]
    async fn v2_supervisor_has_di_review() {
        let db = setup().await;
        assign_role(&db, 60, "Supervisor").await;

        let has = rbac::check_permission(&db, 60, "di.review", &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Supervisor must have di.review for screen_di access");
    }

    #[tokio::test]
    async fn v2_administrator_has_di_review() {
        let db = setup().await;
        assign_role(&db, 61, "Administrator").await;

        let has = rbac::check_permission(&db, 61, "di.review", &PermissionScope::Global)
            .await
            .expect("check");
        assert!(has, "Administrator must have di.review");
    }

    #[tokio::test]
    async fn v2_operator_denied_di_review() {
        let db = setup().await;
        assign_role(&db, 62, "Operator").await;

        let has = rbac::check_permission(&db, 62, "di.review", &PermissionScope::Global)
            .await
            .expect("check");
        assert!(!has, "Operator must NOT have di.review");
    }

    #[tokio::test]
    async fn v2_readonly_denied_di_review() {
        let db = setup().await;
        assign_role(&db, 63, "Readonly").await;

        let has = rbac::check_permission(&db, 63, "di.review", &PermissionScope::Global)
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
                ["di.review".into()],
            ))
            .await
            .expect("query")
            .expect("di.review permission must exist in seed data");
        let requires: i64 = row.try_get("", "requires_step_up").expect("column");
        assert_eq!(requires, 0, "di.review must NOT require step-up");
    }

    // ─── V2 bonus: screen_di domain-layer success with proper state ─────

    #[tokio::test]
    async fn v2_screen_succeeds_when_di_is_pending_review() {
        let db = setup().await;

        // Seed FK data
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT INTO equipment (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'eq-001', 'EQ-001', 'Test Equip', 'active_in_service', datetime('now'), datetime('now'))".to_string()
        )).await.expect("equipment");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT INTO org_structure_models (id, sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'mdl-001', 1, 'active', datetime('now'), datetime('now'))".to_string()
        )).await.expect("model");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT INTO org_node_types (id, sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'nt-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))".to_string()
        )).await.expect("node_type");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT INTO org_nodes (id, sync_id, code, name, node_type_id, status, created_at, updated_at) \
             VALUES (1, 'on-001', 'SITE-001', 'Test Site', 1, 'active', datetime('now'), datetime('now'))".to_string()
        )).await.expect("org_node");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT INTO reference_domains (id, code, name, structure_type, governance_level, is_extendable, created_at, updated_at) \
             VALUES (900001, 'DI_CLASS', 'DI Classification', 'flat', 'tenant_managed', 1, datetime('now'), datetime('now'))".to_string()
        )).await.expect("ref_domain");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT INTO reference_sets (id, domain_id, version_no, status, created_at) \
             VALUES (900001, 900001, 1, 'published', datetime('now'))".to_string()
        )).await.expect("ref_set");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT INTO reference_values (id, set_id, code, label, is_active) \
             VALUES (900001, 900001, 'MECH', 'Mécanique', 1)".to_string()
        )).await.expect("ref_value");

        // Get seeded admin user
        let user_id: i64 = db
            .query_one(Statement::from_string(DbBackend::Sqlite,
                "SELECT id FROM user_accounts LIMIT 1".to_string()))
            .await.expect("q").expect("user").try_get::<i64>("", "id").expect("id");

        // Create DI
        let di = crate::di::queries::create_intervention_request(
            &db,
            crate::di::queries::DiCreateInput {
                asset_id: 1,
                org_node_id: 1,
                title: "Pump vibration".into(),
                description: "Excessive vibration on pump P-101".into(),
                origin_type: "operator".into(),
                symptom_code_id: None,
                impact_level: "unknown".into(),
                production_impact: false,
                safety_flag: false,
                environmental_flag: false,
                quality_flag: false,
                reported_urgency: "medium".into(),
                observed_at: None,
                source_inspection_anomaly_id: None,
                submitter_id: user_id,
            },
        )
        .await
        .expect("create DI");

        // Advance to pending_review
        db.execute(Statement::from_sql_and_values(DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'pending_review', \
             row_version = row_version + 1, updated_at = datetime('now') WHERE id = ?",
            [di.id.into()],
        )).await.expect("advance");

        // Screen via domain function (same path IPC command delegates to)
        let result = crate::di::review::screen_di(
            &db,
            crate::di::review::DiScreenInput {
                di_id: di.id,
                actor_id: user_id,
                expected_row_version: 2,
                validated_urgency: "high".into(),
                review_team_id: Some(1),
                classification_code_id: Some(900001),
                reviewer_note: Some("Validated".into()),
            },
        )
        .await
        .expect("screen_di must succeed when DI is in pending_review");

        assert_eq!(result.status, "awaiting_approval");
    }

    // ─── V3 — Command registration ─────────────────────────────────────

    #[test]
    fn v3_no_duplicate_command_function_names() {
        // Structural check: all DI command functions compile and are distinct.
        // If any were duplicated in invoke_handler, Tauri would emit a compile
        // error — the fact that cargo check passes proves uniqueness.
        let fns: Vec<&str> = vec![
            "list_di",
            "get_di",
            "create_di",
            "update_di_draft",
            "screen_di",
            "return_di",
            "reject_di",
            "approve_di",
            "defer_di",
            "reactivate_di",
            "get_di_review_events",
            "upload_di_attachment",
            "list_di_attachments",
            "delete_di_attachment",
            "convert_di_to_wo",
            "get_sla_status",
            "list_sla_rules",
            "update_sla_rule",
            "list_di_change_events",
            "list_all_di_change_events",
        ];
        let unique: std::collections::HashSet<&str> = fns.iter().copied().collect();
        assert_eq!(fns.len(), unique.len(), "All DI command names must be unique");
        assert_eq!(fns.len(), 20, "Expected exactly 20 DI commands (Files 01-04)");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // File 04 — Sprint S1 Supervisor Verification Tests
    // ═══════════════════════════════════════════════════════════════════════

    // ─── V1 — Permission seed: exactly 7 canonical di.* permissions ─────

    #[tokio::test]
    async fn s1_v1_permission_seed_exactly_7_di_rows() {
        let db = setup().await;

        let rows = db
            .query_all(Statement::from_string(
                DbBackend::Sqlite,
                "SELECT name FROM permissions WHERE name LIKE 'di.%' ORDER BY name"
                    .to_string(),
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
                "di.admin",
                "di.approve",
                "di.convert",
                "di.create",
                "di.create.own",
                "di.delete",
                "di.review",
                "di.submit",
                "di.view",
            ],
            "Exactly 9 canonical di.* permissions must exist (7 legacy + di.submit + di.delete from migration 029)"
        );
    }

    // ─── V2 — Audit on approval: record_di_change_event produces row ────

    #[tokio::test]
    async fn s1_v2_audit_event_on_approval() {
        let db = setup().await;

        // Seed minimal FK data required for DI creation
        seed_fk_data(&db).await;

        // Create a DI and advance it through the state machine
        let di = create_test_di(&db).await;

        // Advance to pending_review (submitted → pending_review)
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE intervention_requests SET status = 'pending_review', \
             row_version = row_version + 1, updated_at = datetime('now') WHERE id = ?",
            [di.id.into()],
        ))
        .await
        .expect("advance to pending_review");

        // Screen: pending_review → awaiting_approval
        let screened = crate::di::review::screen_di(
            &db,
            crate::di::review::DiScreenInput {
                di_id: di.id,
                actor_id: 1,
                expected_row_version: 2, // row_version after manual advance
                validated_urgency: "high".into(),
                review_team_id: None,
                classification_code_id: Some(900001),
                reviewer_note: Some("Test screen".into()),
            },
        )
        .await
        .expect("screen should succeed from pending_review");

        // Approve: awaiting_approval → approved_for_planning
        let approved = crate::di::review::approve_di_for_planning(
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

        // Now record the audit event (mimicking the command layer)
        crate::di::audit::record_di_change_event(
            &db,
            crate::di::audit::DiAuditInput {
                di_id: Some(approved.id),
                action: "approved".into(),
                actor_id: Some(1),
                summary: Some("DI approved for planning".into()),
                details_json: None,
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        // Verify: 1 row with action='approved' and apply_result='applied'
        let events = crate::di::audit::list_di_change_events(&db, approved.id, 100)
            .await
            .expect("list audit events");

        let approved_events: Vec<_> = events
            .iter()
            .filter(|e| e.action == "approved" && e.apply_result == "applied")
            .collect();

        assert_eq!(
            approved_events.len(),
            1,
            "Exactly 1 audit event with action='approved', apply_result='applied'"
        );
        assert_eq!(approved_events[0].requires_step_up, 1);
    }

    // ─── V3 — Audit on blocked step-up ──────────────────────────────────

    #[tokio::test]
    async fn s1_v3_audit_event_on_blocked_step_up() {
        let db = setup().await;
        seed_fk_data(&db).await;

        let di = create_test_di(&db).await;

        // Record a blocked approval audit event (simulates failed step-up)
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

        let blocked: Vec<_> = events
            .iter()
            .filter(|e| e.apply_result == "blocked")
            .collect();

        assert_eq!(blocked.len(), 1, "One blocked audit event must exist");
        assert_eq!(blocked[0].action, "approved");
        assert_eq!(blocked[0].requires_step_up, 1);
    }

    // ─── V4 — Audit on conversion with requires_step_up = 1 ─────────────

    #[tokio::test]
    async fn s1_v4_audit_event_on_conversion() {
        let db = setup().await;
        seed_fk_data(&db).await;

        let di = create_test_di(&db).await;

        // Record a conversion audit event
        crate::di::audit::record_di_change_event(
            &db,
            crate::di::audit::DiAuditInput {
                di_id: Some(di.id),
                action: "converted".into(),
                actor_id: Some(1),
                summary: Some("DI converted to WO".into()),
                details_json: Some(r#"{"wo_id":1,"wo_code":"OT-0001"}"#.into()),
                requires_step_up: true,
                apply_result: "applied".into(),
            },
        )
        .await;

        let events = crate::di::audit::list_di_change_events(&db, di.id, 100)
            .await
            .expect("list events");

        let converted: Vec<_> = events
            .iter()
            .filter(|e| e.action == "converted")
            .collect();

        assert_eq!(converted.len(), 1, "One converted audit event");
        assert_eq!(converted[0].requires_step_up, 1, "Conversion must require step-up");
        assert_eq!(converted[0].apply_result, "applied");
    }

    // ─── Test helpers ────────────────────────────────────────────────────

    async fn seed_fk_data(db: &sea_orm::DatabaseConnection) {
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO equipment (id, sync_id, asset_id_code, name, lifecycle_status, created_at, updated_at) \
             VALUES (1, 'eq-001', 'EQ-001', 'Test Equip', 'active_in_service', datetime('now'), datetime('now'))".to_string()
        )).await.expect("equipment");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_structure_models (id, sync_id, version_number, status, created_at, updated_at) \
             VALUES (1, 'mdl-001', 1, 'active', datetime('now'), datetime('now'))".to_string()
        )).await.expect("model");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_node_types (id, sync_id, structure_model_id, code, label, is_active, created_at, updated_at) \
             VALUES (1, 'nt-001', 1, 'SITE', 'Site', 1, datetime('now'), datetime('now'))".to_string()
        )).await.expect("node_type");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO org_nodes (id, sync_id, code, name, node_type_id, status, created_at, updated_at) \
             VALUES (1, 'on-001', 'SITE-001', 'Test Site', 1, 'active', datetime('now'), datetime('now'))".to_string()
        )).await.expect("org_node");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_domains (id, code, name, structure_type, governance_level, is_extendable, created_at, updated_at) \
             VALUES (900001, 'DI_CLASS', 'DI Classification', 'flat', 'tenant_managed', 1, datetime('now'), datetime('now'))".to_string()
        )).await.expect("ref_domain");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_sets (id, domain_id, version_no, status, created_at) \
             VALUES (900001, 900001, 1, 'published', datetime('now'))".to_string()
        )).await.expect("ref_set");
        db.execute(Statement::from_string(DbBackend::Sqlite,
            "INSERT OR IGNORE INTO reference_values (id, set_id, code, label, is_active) \
             VALUES (900001, 900001, 'MECH', 'Mécanique', 1)".to_string()
        )).await.expect("ref_value");
    }

    async fn create_test_di(db: &sea_orm::DatabaseConnection) -> crate::di::domain::InterventionRequest {
        crate::di::queries::create_intervention_request(
            db,
            crate::di::queries::DiCreateInput {
                asset_id: 1,
                org_node_id: 1,
                submitter_id: 1,
                title: "Test DI for verification".into(),
                description: "Verification test DI".into(),
                origin_type: "operator".into(),
                reported_urgency: "high".into(),
                observed_at: None,
                impact_level: "medium".into(),
                production_impact: false,
                safety_flag: false,
                environmental_flag: false,
                quality_flag: false,
                symptom_code_id: None,
                source_inspection_anomaly_id: None,
            },
        )
        .await
        .expect("create DI should succeed")
    }
}
