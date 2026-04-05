//! Org module IPC commands.
//!
//! Permission gates:
//!   org.view   — read structure models, node types, rules
//!   org.manage — create/update nodes and responsibility bindings (F02)
//!   org.admin  — create/publish structure models, node types, rules

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::org::{
    node_types::{self, CreateNodeTypePayload},
    relationship_rules::{self, CreateRelationshipRulePayload},
    structure_model::{self, CreateStructureModelPayload},
    OrgNodeType, OrgRelationshipRule, OrgStructureModel,
};
use crate::state::AppState;
use crate::{require_permission, require_session};

// ─── Structure model commands ─────────────────────────────────────────────────

#[tauri::command]
pub async fn list_org_structure_models(state: State<'_, AppState>) -> AppResult<Vec<OrgStructureModel>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    structure_model::list_models(&state.db).await
}

#[tauri::command]
pub async fn get_active_org_structure_model(state: State<'_, AppState>) -> AppResult<Option<OrgStructureModel>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    structure_model::get_active_model(&state.db).await
}

#[tauri::command]
pub async fn create_org_structure_model(
    payload: CreateStructureModelPayload,
    state: State<'_, AppState>,
) -> AppResult<OrgStructureModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    structure_model::create_model(&state.db, payload, user.user_id).await
}

#[tauri::command]
pub async fn publish_org_structure_model(model_id: i32, state: State<'_, AppState>) -> AppResult<OrgStructureModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    structure_model::publish_model(&state.db, model_id, user.user_id).await
}

#[tauri::command]
pub async fn archive_org_structure_model(model_id: i32, state: State<'_, AppState>) -> AppResult<OrgStructureModel> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    structure_model::archive_model(&state.db, model_id).await
}

// ─── Node type commands ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_org_node_types(structure_model_id: i32, state: State<'_, AppState>) -> AppResult<Vec<OrgNodeType>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    node_types::list_node_types(&state.db, structure_model_id).await
}

#[tauri::command]
pub async fn create_org_node_type(
    payload: CreateNodeTypePayload,
    state: State<'_, AppState>,
) -> AppResult<OrgNodeType> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    node_types::create_node_type(&state.db, payload).await
}

#[tauri::command]
pub async fn deactivate_org_node_type(node_type_id: i32, state: State<'_, AppState>) -> AppResult<OrgNodeType> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    node_types::deactivate_node_type(&state.db, node_type_id).await
}

// ─── Relationship rule commands ───────────────────────────────────────────────

#[tauri::command]
pub async fn list_org_relationship_rules(
    structure_model_id: i32,
    state: State<'_, AppState>,
) -> AppResult<Vec<OrgRelationshipRule>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    relationship_rules::list_rules(&state.db, structure_model_id).await
}

#[tauri::command]
pub async fn create_org_relationship_rule(
    payload: CreateRelationshipRulePayload,
    state: State<'_, AppState>,
) -> AppResult<OrgRelationshipRule> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    relationship_rules::create_rule(&state.db, payload).await
}

#[tauri::command]
pub async fn delete_org_relationship_rule(rule_id: i32, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    relationship_rules::delete_rule(&state.db, rule_id).await
}
