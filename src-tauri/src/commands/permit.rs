//! Work permit IPC (PRD §6.23).
//!
//! Site scope (step 6, `docs/SITE_SCOPED_ROLES.md`): every permit is checked at its org node
//! `work_permits.entity_id` (`permit::access`), with the same permission as before; what belongs to
//! a permit (isolations, suspensions, hand-over logs, LOTO cards) at its permit's node. The list,
//! the open-permits report and the compliance figure are limited in SQL to where the user holds
//! `ptw.view`; the lists say how many permits they leave out and why. A plant-wide role sees and
//! acts everywhere, as before. The actor (signer, verifier, applier) is always the signed-in user.
//! Plant-wide reference data stays plant-wide: permit types, isolation options, hand-over roles.

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::auth::session_manager::AuthenticatedUser;
use crate::errors::{AppError, AppResult};
use crate::permit::access::{self, PermitChild};
use crate::permit::domain::{
    LotoCardPrintInput, LotoCardPrintJob, LotoCardView, PermitComplianceKpi30d, PermitHandoverLog,
    PermitHandoverLogInput, PermitIsolation, PermitIsolationActionInput, PermitIsolationUpsertInput,
    PermitSuspendInput, PermitSuspension, PermitType, PermitTypeUpsertInput, WorkPermit, WorkPermitCreateInput,
    WorkPermitListFilter, WorkPermitListPage, WorkPermitStatusInput, WorkPermitUpdateInput,
};
use crate::permit::queries;
use crate::rbac::permissions::{PTW_CLOSE, PTW_ISSUE, PTW_VIEW};
use crate::state::AppState;
use crate::{require_permission, require_session};

// ═══════════════════════════════════════════════════════════════════════════════
// Site-scope checks shared by the commands (`permit::access`)
// ═══════════════════════════════════════════════════════════════════════════════

/// Where the user holds `permission` (held nowhere: refused as before); filters lists and figures.
async fn require_anywhere(
    state: &AppState,
    user_id: i32,
    permission: &str,
) -> AppResult<crate::auth::scope::AccessScope> {
    access::require_anywhere(&state.db, &state.permission_cache, user_id, permission).await
}

/// `permission` on the permit `permit_id` (held nowhere: refused as before; no such permit: not
/// found; else checked at its org node).
async fn require_on_permit(state: &AppState, user_id: i32, permission: &str, permit_id: i64) -> AppResult<()> {
    access::require_on_permit(&state.db, &state.permission_cache, user_id, permission, permit_id)
        .await
        .map(|_| ())
}

/// `permission` for the reads of what belongs to the permit `permit_id` (a permit that no longer
/// exists has no site: plant-wide users only, who read what is left of it as before).
async fn require_on_permit_records(state: &AppState, user_id: i32, permission: &str, permit_id: i64) -> AppResult<()> {
    access::require_on_permit_records(&state.db, &state.permission_cache, user_id, permission, permit_id).await
}

/// `permission` on the permit an isolation belongs to, addressed by the isolation's own id.
async fn require_on_isolation(state: &AppState, user_id: i32, permission: &str, isolation_id: i64) -> AppResult<()> {
    access::require_on_child(
        &state.db,
        &state.permission_cache,
        user_id,
        permission,
        PermitChild::Isolation,
        isolation_id,
    )
    .await
}

#[tauri::command]
pub async fn list_permit_types(state: State<'_, AppState>) -> AppResult<Vec<PermitType>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    queries::list_permit_types(&state.db).await
}

#[tauri::command]
pub async fn upsert_permit_type(input: PermitTypeUpsertInput, state: State<'_, AppState>) -> AppResult<PermitType> {
    let user = require_session!(state);
    upsert_permit_type_impl(&state, &user, input).await
}

/// `upsert_permit_type` once the session is known (tests call it with a session of their own).
pub(crate) async fn upsert_permit_type_impl(
    state: &AppState,
    user: &AuthenticatedUser,
    input: PermitTypeUpsertInput,
) -> AppResult<PermitType> {
    require_permission!(
        state,
        user,
        crate::rbac::permissions::PTW_ISSUE,
        PermissionScope::Global
    );
    // Also the Données de référence catalog `PERMIT.TYPES`: one write path, same rules.
    crate::reference::audience::assert_module_catalog_editable(
        crate::reference::audience::MODULE_CATALOG_PERMIT_TYPES,
    )?;
    // Switching the energy-isolation rule off weakens the start gate, and changing the
    // three-person lock-removal chain (either way, or a new type with it on) changes who may
    // confirm that locks came off: administrator + step-up (the query refuses it otherwise,
    // needs a reason and audits it; the LOTO type keeps its isolation rule).
    let current = match input.id {
        Some(id) => queries::get_permit_type(&state.db, id).await?,
        None => None,
    };
    let relaxes_isolation = current
        .as_ref()
        .is_some_and(|c| queries::relaxes_energy_isolation(c, &input));
    let changes_three_person =
        (input.id.is_none() || current.is_some()) && queries::changes_three_person_removal(current.as_ref(), &input);
    // Checked below only when needed: true means `adm.settings` and a valid step-up were checked.
    let admin_step_up = relaxes_isolation || changes_three_person;
    if admin_step_up {
        require_permission!(
            state,
            user,
            crate::rbac::permissions::ADM_SETTINGS,
            PermissionScope::Global
        );
        crate::require_step_up!(state);
    }
    queries::upsert_permit_type(
        &state.db,
        input,
        queries::PermitTypeWriter {
            actor_id: i64::from(user.user_id),
            may_relax_isolation: admin_step_up,
            may_change_three_person: admin_step_up,
        },
    )
    .await
}

#[tauri::command]
pub async fn list_work_permits(
    filter: WorkPermitListFilter,
    state: State<'_, AppState>,
) -> AppResult<WorkPermitListPage> {
    let user = require_session!(state);
    let scope = require_anywhere(&state, user.user_id, PTW_VIEW).await?;
    // Where the user holds ptw.view, in SQL before the limit; the page counts what it leaves out.
    queries::list_work_permits_in_scope(&state.db, filter, &scope).await
}

#[tauri::command]
pub async fn get_work_permit(id: i64, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    require_on_permit(&state, user.user_id, PTW_VIEW, id).await?;
    queries::get_work_permit(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "WorkPermit".into(),
            id: id.to_string(),
        })
}

#[tauri::command]
pub async fn create_work_permit(input: WorkPermitCreateInput, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    // Rule §2.6: at the new permit's node, its equipment's node and its linked work order's node.
    access::require_create_at(&state.db, &state.permission_cache, user.user_id, &input).await?;
    queries::create_work_permit(&state.db, input).await
}

#[tauri::command]
pub async fn update_work_permit(input: WorkPermitUpdateInput, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    // Rule §2.7: at the permit's node, and at every new node, equipment or work order it is given.
    access::require_update(&state.db, &state.permission_cache, user.user_id, &input).await?;
    queries::update_work_permit(&state.db, input).await
}

#[tauri::command]
pub async fn set_work_permit_status(input: WorkPermitStatusInput, state: State<'_, AppState>) -> AppResult<WorkPermit> {
    let user = require_session!(state);
    let permission = match input.status.as_str() {
        "closed" | "handed_back" | "cancelled" => PTW_CLOSE,
        _ => PTW_ISSUE,
    };
    require_on_permit(&state, user.user_id, permission, input.id).await?;
    queries::set_work_permit_status(&state.db, input, i64::from(user.user_id)).await
}

/// Choices of the isolation form (energy types, the plant's isolation methods). Read with the
/// permit view right: a technician recording a LOTO needs no reference-data permission.
#[tauri::command]
pub async fn list_permit_isolation_options(
    state: State<'_, AppState>,
) -> AppResult<crate::permit::energy_reference::IsolationOptions> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    crate::permit::energy_reference::list_isolation_options(&state.db).await
}

#[tauri::command]
pub async fn list_permit_isolations(permit_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PermitIsolation>> {
    let user = require_session!(state);
    require_on_permit_records(&state, user.user_id, PTW_VIEW, permit_id).await?;
    queries::list_permit_isolations(&state.db, permit_id).await
}

#[tauri::command]
pub async fn upsert_permit_isolation(
    input: PermitIsolationUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    // A new isolation at its permit's node; an existing one at its own permit's node and at the
    // permit named (the save refuses moving it to another permit).
    access::require_isolation_upsert(
        &state.db,
        &state.permission_cache,
        user.user_id,
        input.id,
        input.permit_id,
    )
    .await?;
    queries::upsert_permit_isolation(&state.db, input).await
}

/// Records the session user as the person who applied the isolation (lock fitted).
#[tauri::command]
pub async fn apply_permit_isolation(
    input: PermitIsolationActionInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    require_on_isolation(&state, user.user_id, PTW_ISSUE, input.isolation_id).await?;
    queries::apply_permit_isolation(&state.db, input, i64::from(user.user_id)).await
}

/// Records the session user as verifier; rejected when they also applied it (two-person rule).
#[tauri::command]
pub async fn verify_permit_isolation(
    input: PermitIsolationActionInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    require_on_isolation(&state, user.user_id, PTW_ISSUE, input.isolation_id).await?;
    queries::verify_permit_isolation(&state.db, input, i64::from(user.user_id)).await
}

/// Records verified lock removal by the session user (part of closing a permit).
#[tauri::command]
pub async fn verify_permit_isolation_removal(
    input: PermitIsolationActionInput,
    state: State<'_, AppState>,
) -> AppResult<PermitIsolation> {
    let user = require_session!(state);
    require_on_isolation(&state, user.user_id, PTW_CLOSE, input.isolation_id).await?;
    queries::verify_permit_isolation_removal(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn suspend_work_permit(
    input: PermitSuspendInput,
    state: State<'_, AppState>,
) -> AppResult<(WorkPermit, PermitSuspension)> {
    let user = require_session!(state);
    require_on_permit(&state, user.user_id, PTW_ISSUE, input.permit_id).await?;
    queries::suspend_work_permit(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn append_permit_handover_log(
    input: PermitHandoverLogInput,
    state: State<'_, AppState>,
) -> AppResult<PermitHandoverLog> {
    let user = require_session!(state);
    require_on_permit(&state, user.user_id, PTW_CLOSE, input.permit_id).await?;
    // The signer is the session user, never a value from the screen; the receiver must hold
    // ptw.close at the permit's site (checked by the save).
    queries::append_permit_handover_log(&state.db, input, i64::from(user.user_id)).await
}

/// Who may receive the hand-over of the permit `permit_id`: active accounts holding `ptw.close` at
/// the permit's site (plant-wide holders included), by name, never the person asking, paged with
/// the total. Requires `ptw.close` at the permit's site.
#[tauri::command]
pub async fn list_permit_handover_receivers(
    permit_id: i64,
    search: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
    state: State<'_, AppState>,
) -> AppResult<queries::HandoverReceiverPage> {
    let user = require_session!(state);
    require_on_permit(&state, user.user_id, PTW_CLOSE, permit_id).await?;
    queries::list_handover_receivers(&state.db, i64::from(user.user_id), permit_id, search, limit, offset).await
}

/// The plant's hand-over roles (Données de référence › `PERMIT.HANDOVER_ROLES`). Requires `ptw.view`.
#[tauri::command]
pub async fn list_permit_handover_roles(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::permit::handover_roles::HandoverRoleOption>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PTW_VIEW,
        PermissionScope::Global
    );
    crate::permit::handover_roles::list_handover_roles(&state.db).await
}

#[tauri::command]
pub async fn list_permit_suspensions(permit_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PermitSuspension>> {
    let user = require_session!(state);
    require_on_permit_records(&state, user.user_id, PTW_VIEW, permit_id).await?;
    queries::list_permit_suspensions(&state.db, permit_id).await
}

#[tauri::command]
pub async fn list_permit_handover_logs(
    permit_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PermitHandoverLog>> {
    let user = require_session!(state);
    require_on_permit_records(&state, user.user_id, PTW_VIEW, permit_id).await?;
    queries::list_permit_handover_logs(&state.db, permit_id).await
}

#[tauri::command]
pub async fn get_loto_card_view(
    permit_id: i64,
    isolation_id: i64,
    state: State<'_, AppState>,
) -> AppResult<LotoCardView> {
    let user = require_session!(state);
    // At the node of the isolation's own permit (the view also requires it to be this permit's).
    require_on_isolation(&state, user.user_id, PTW_VIEW, isolation_id).await?;
    queries::get_loto_card_view(&state.db, permit_id, isolation_id).await
}

#[tauri::command]
pub async fn record_loto_card_print(
    input: LotoCardPrintInput,
    state: State<'_, AppState>,
) -> AppResult<LotoCardPrintJob> {
    let user = require_session!(state);
    require_on_isolation(&state, user.user_id, PTW_ISSUE, input.isolation_id).await?;
    queries::record_loto_card_print(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn list_open_permits_report(state: State<'_, AppState>) -> AppResult<WorkPermitListPage> {
    let user = require_session!(state);
    // Rule §2.8: the open permits of the user's sites only, with the count of those left out.
    let scope = require_anywhere(&state, user.user_id, PTW_VIEW).await?;
    queries::list_open_permits_report_in_scope(&state.db, &scope).await
}

#[tauri::command]
pub async fn permit_compliance_kpi_30d(state: State<'_, AppState>) -> AppResult<PermitComplianceKpi30d> {
    let user = require_session!(state);
    // Rule §2.8: the figure of the user's sites only (never plant-wide for a site-only user).
    let scope = require_anywhere(&state, user.user_id, PTW_VIEW).await?;
    queries::permit_compliance_kpi_30d_in_scope(&state.db, &scope).await
}
