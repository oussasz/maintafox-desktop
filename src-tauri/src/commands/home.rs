//! Home page (start centre) IPC: one aggregated call per panel (`home`).
//!
//! Each section is checked against the module's own view permission with the same gate as the
//! module's commands (`require_permission!`: role grant, entitlement capability, license matrix),
//! as a yes/no: a module the person may not open gives no section, so the page shows no panel and
//! no count for it.

use tauri::State;

use crate::auth::rbac::{check_permission_cached, PermissionScope};
use crate::errors::AppResult;
use crate::home::checked_period;
use crate::home::my_work::{build_my_work, HomeMyWork, MyWorkAccess};
use crate::home::planning::{build_planning, HomePlanning};
use crate::home::supervision::{build_supervision, HomeSupervision, SupervisionAccess};
use crate::rbac::permissions as p;
use crate::state::AppState;
use crate::{require_permission, require_session};

/// Whether the person may open the module behind `permission` (the `require_permission!` gate).
async fn may_view(state: &AppState, user_id: i32, permission: &str) -> AppResult<bool> {
    if !check_permission_cached(
        &state.db,
        &state.permission_cache,
        user_id,
        permission,
        &PermissionScope::Global,
    )
    .await?
    {
        return Ok(false);
    }
    if !crate::entitlements::queries::capability_allowed_for_permission(&state.db, permission).await? {
        return Ok(false);
    }
    Ok(
        crate::license::queries::evaluate_permission_matrix(&state.db, user_id, permission)
            .await?
            .allowed,
    )
}

/// "My work": the signed-in person's work orders, requests, rounds, PM and notifications.
#[tauri::command]
pub async fn get_home_my_work(state: State<'_, AppState>) -> AppResult<HomeMyWork> {
    let user = require_session!(state);
    let access = MyWorkAccess {
        work_orders: may_view(&state, user.user_id, p::OT_VIEW).await?,
        requests: may_view(&state, user.user_id, p::DI_VIEW).await?,
        rounds: may_view(&state, user.user_id, p::INS_VIEW).await?,
        pm: may_view(&state, user.user_id, p::PM_VIEW).await?,
    };
    build_my_work(&state.db, user.user_id, access, chrono::Utc::now()).await
}

/// Planner panel: backlog by readiness, this week's load vs capacity, schedule compliance over
/// `period_days` (7 or 30).
#[tauri::command]
pub async fn get_home_planning(period_days: i64, state: State<'_, AppState>) -> AppResult<HomePlanning> {
    let user = require_session!(state);
    require_permission!(state, &user, p::PLAN_VIEW, PermissionScope::Global);
    let period = checked_period(period_days)?;
    build_planning(&state.db, period, chrono::Utc::now(), chrono::Local::now().date_naive()).await
}

/// Supervisor / manager panel; compliance figures over `period_days` (7 or 30).
#[tauri::command]
pub async fn get_home_supervision(period_days: i64, state: State<'_, AppState>) -> AppResult<HomeSupervision> {
    let user = require_session!(state);
    let period = checked_period(period_days)?;
    let access = SupervisionAccess {
        requests: may_view(&state, user.user_id, p::DI_VIEW).await?,
        work_orders: may_view(&state, user.user_id, p::OT_VIEW).await?,
        pm: may_view(&state, user.user_id, p::PM_VIEW).await?,
        stock: may_view(&state, user.user_id, p::INV_VIEW).await?,
        safety: may_view(&state, user.user_id, p::PTW_VIEW).await?,
        assets: may_view(&state, user.user_id, p::EQ_VIEW).await?,
    };
    build_supervision(&state.db, user.user_id, access, period, chrono::Utc::now()).await
}
