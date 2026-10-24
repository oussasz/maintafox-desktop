//! Stock balances below zero after sync, to reconcile (`inventory::negative_stock`).
//!
//! Permission gates (as the stock screen and stock adjustments, plant-wide: stores have no site):
//! - `inv.view`: the list and the sync-centre counts;
//! - `inv.manage`: the count that resolves an item, and the approval of such a count waiting for a
//!   second person (four-eyes above the tolerance, D11; the right of a count line's approval; also
//!   checked by the resolution and the approval themselves).

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::inventory::negative_stock::{
    self, ApprovePendingCountInput, NegativeStockResolution, NegativeStockReviewList, NegativeStockSummary,
    ResolveNegativeStockInput,
};
use crate::state::AppState;
use crate::{require_permission, require_session};

#[tauri::command]
pub async fn list_negative_stock_reviews(
    include_closed: bool,
    state: State<'_, AppState>,
) -> AppResult<NegativeStockReviewList> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    negative_stock::list_reviews(&state.db, include_closed).await
}

#[tauri::command]
pub async fn get_negative_stock_summary(state: State<'_, AppState>) -> AppResult<NegativeStockSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_VIEW,
        PermissionScope::Global
    );
    negative_stock::summary(&state.db).await
}

#[tauri::command]
pub async fn resolve_negative_stock_by_count(
    input: ResolveNegativeStockInput,
    state: State<'_, AppState>,
) -> AppResult<NegativeStockResolution> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    // Who counted comes from the session, never from the UI payload.
    negative_stock::resolve_by_count(&state.db, user.user_id, input).await
}

#[tauri::command]
pub async fn approve_negative_stock_count(
    input: ApprovePendingCountInput,
    state: State<'_, AppState>,
) -> AppResult<NegativeStockResolution> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::INV_MANAGE,
        PermissionScope::Global
    );
    // The approver is the signed-in user, never a value sent by the UI (four-eyes).
    negative_stock::approve_pending_count(&state.db, user.user_id, input).await
}
