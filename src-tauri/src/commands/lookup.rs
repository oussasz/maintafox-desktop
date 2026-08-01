use crate::errors::AppResult;
use crate::repository::lookup_repository::{
    LookupDomainFilter, LookupDomainSummary, LookupValueOption, LookupValueRecord,
};
use crate::repository::PageRequest;
use crate::services::lookup_service;
use crate::state::AppState;
use crate::require_session;
use tauri::State;

/// Returns a paginated list of all lookup domains.
/// Called by the Lookup Manager admin page and any filter panel that needs to
/// enumerate available governed vocabularies.
#[tauri::command]
pub async fn list_lookup_domains(
    state: State<'_, AppState>,
    filter: Option<LookupDomainFilter>,
    page: Option<PageRequest>,
) -> AppResult<crate::repository::Page<LookupDomainSummary>> {
    let _user = require_session!(state);
    let db = &state.db;
    lookup_service::list_domains(db, filter.unwrap_or_default(), page.unwrap_or_default()).await
}

/// Returns all active values for a given domain key.
///
/// This is the primary call for populating dropdowns, filter chips, and badge resolvers.
/// Pass `domainKey` as the stable programmatic key (e.g. "equipment.criticality").
#[tauri::command]
pub async fn get_lookup_values(state: State<'_, AppState>, domain_key: String) -> AppResult<Vec<LookupValueOption>> {
    let _user = require_session!(state);
    let db = &state.db;
    lookup_service::get_domain_values(db, &domain_key).await
}

/// Resolves a single lookup value by its integer id.
/// Called when rendering a stored FK as a labeled badge or detail field.
/// Pass `valueId` in the invoke payload.
#[tauri::command]
pub async fn get_lookup_value_by_id(state: State<'_, AppState>, value_id: i32) -> AppResult<LookupValueRecord> {
    let _user = require_session!(state);
    let db = &state.db;
    lookup_service::get_value_by_id(db, value_id).await
}
