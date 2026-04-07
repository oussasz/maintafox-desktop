//! Org module IPC commands.
//!
//! Permission gates (File 04 hardened):
//!   org.view   — read structure models, node types, rules, tree, responsibilities, bindings
//!   org.manage — create/update nodes and responsibility/binding operations
//!   org.admin  — create structure models, node types, rules; archive models
//!   org.admin + require_step_up! — publish model with remap, move node, deactivate node
//!
//! Audit requirements:
//!   Successful create/update/move/deactivate/responsibility/binding/publish operations
//!   call `record_org_change`. Blocked publish validation also writes an audit row with
//!   `apply_result = 'blocked'`. Audit rows are never updated or deleted.

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::AppResult;
use crate::org::{
    audit::{self, OrgAuditEventInput, OrgChangeEvent},
    entity_bindings::{self, UpsertOrgEntityBindingPayload},
    impact_preview::{self, OrgImpactPreview, PreviewOrgChangePayload},
    node_types::{self, CreateNodeTypePayload},
    nodes::{self, CreateOrgNodePayload, MoveOrgNodePayload, UpdateOrgNodeMetadataPayload},
    relationship_rules::{self, CreateRelationshipRulePayload},
    responsibilities::{self, AssignResponsibilityPayload},
    structure_model::{self, CreateStructureModelPayload},
    tree_queries::{self, OrgDesignerNodeRow, OrgDesignerSnapshot},
    validation::{self, OrgPublishValidationResult},
    OrgEntityBinding, OrgNode, OrgNodeResponsibility, OrgNodeType, OrgRelationshipRule,
    OrgStructureModel, OrgTreeRow,
};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};

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
    require_step_up!(state);

    let result = structure_model::publish_model(&state.db, model_id, user.user_id).await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "structure_model".to_string(),
            entity_id: Some(model_id as i64),
            change_type: "publish_model_simple".to_string(),
            before_json: None,
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: true,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
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

// ─── Org node read commands (org.view) ────────────────────────────────────────

#[tauri::command]
pub async fn list_org_tree(state: State<'_, AppState>) -> AppResult<Vec<OrgTreeRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    nodes::list_active_org_tree(&state.db).await
}

#[tauri::command]
pub async fn get_org_node(node_id: i64, state: State<'_, AppState>) -> AppResult<OrgNode> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    nodes::get_org_node_by_id(&state.db, node_id).await
}

#[tauri::command]
pub async fn list_org_node_responsibilities(
    node_id: i64,
    include_inactive: bool,
    state: State<'_, AppState>,
) -> AppResult<Vec<OrgNodeResponsibility>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    responsibilities::list_node_responsibilities(&state.db, node_id, include_inactive).await
}

#[tauri::command]
pub async fn list_org_entity_bindings(
    node_id: i64,
    include_inactive: bool,
    state: State<'_, AppState>,
) -> AppResult<Vec<OrgEntityBinding>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    entity_bindings::list_entity_bindings(&state.db, node_id, include_inactive).await
}

// ─── Org node manage commands (org.manage) ────────────────────────────────────

#[tauri::command]
pub async fn create_org_node(
    payload: CreateOrgNodePayload,
    state: State<'_, AppState>,
) -> AppResult<OrgNode> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.manage", PermissionScope::Global);

    let result = nodes::create_org_node(&state.db, payload, user.user_id).await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_node".to_string(),
            entity_id: Some(result.id),
            change_type: "create_node".to_string(),
            before_json: None,
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn update_org_node_metadata(
    payload: UpdateOrgNodeMetadataPayload,
    state: State<'_, AppState>,
) -> AppResult<OrgNode> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.manage", PermissionScope::Global);

    let before = nodes::get_org_node_by_id(&state.db, payload.node_id).await.ok();
    let result = nodes::update_org_node_metadata(&state.db, payload).await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_node".to_string(),
            entity_id: Some(result.id),
            change_type: "update_metadata".to_string(),
            before_json: before.and_then(|b| serde_json::to_string(&b).ok()),
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn assign_org_node_responsibility(
    payload: AssignResponsibilityPayload,
    state: State<'_, AppState>,
) -> AppResult<OrgNodeResponsibility> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.manage", PermissionScope::Global);

    let result = responsibilities::assign_responsibility(&state.db, payload, user.user_id).await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_node_responsibility".to_string(),
            entity_id: Some(result.id),
            change_type: "assign_responsibility".to_string(),
            before_json: None,
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn end_org_node_responsibility(
    assignment_id: i64,
    valid_to: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<OrgNodeResponsibility> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.manage", PermissionScope::Global);

    let result = responsibilities::end_responsibility_assignment(
        &state.db,
        assignment_id,
        valid_to,
        user.user_id,
    )
    .await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_node_responsibility".to_string(),
            entity_id: Some(result.id),
            change_type: "end_responsibility".to_string(),
            before_json: None,
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn upsert_org_entity_binding(
    payload: UpsertOrgEntityBindingPayload,
    state: State<'_, AppState>,
) -> AppResult<OrgEntityBinding> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.manage", PermissionScope::Global);

    let result = entity_bindings::upsert_entity_binding(&state.db, payload, user.user_id).await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_entity_binding".to_string(),
            entity_id: Some(result.id),
            change_type: "upsert_binding".to_string(),
            before_json: None,
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn expire_org_entity_binding(
    binding_id: i64,
    valid_to: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<OrgEntityBinding> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.manage", PermissionScope::Global);

    let result =
        entity_bindings::expire_entity_binding(&state.db, binding_id, valid_to, user.user_id)
            .await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_entity_binding".to_string(),
            entity_id: Some(result.id),
            change_type: "expire_binding".to_string(),
            before_json: None,
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: false,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

// ─── Dangerous structural commands (org.admin + step-up + audit) ──────────────

#[tauri::command]
pub async fn move_org_node(
    payload: MoveOrgNodePayload,
    state: State<'_, AppState>,
) -> AppResult<OrgNode> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    require_step_up!(state);

    let before = nodes::get_org_node_by_id(&state.db, payload.node_id).await.ok();
    let result = nodes::move_org_node(&state.db, payload, user.user_id).await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_node".to_string(),
            entity_id: Some(result.id),
            change_type: "move_node".to_string(),
            before_json: before.and_then(|b| serde_json::to_string(&b).ok()),
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: true,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

#[tauri::command]
pub async fn deactivate_org_node(
    node_id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<OrgNode> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    require_step_up!(state);

    let before = nodes::get_org_node_by_id(&state.db, node_id).await.ok();
    let result =
        nodes::deactivate_org_node(&state.db, node_id, expected_row_version, user.user_id).await?;

    audit::record_org_change(
        &state.db,
        OrgAuditEventInput {
            entity_kind: "org_node".to_string(),
            entity_id: Some(result.id),
            change_type: "deactivate_node".to_string(),
            before_json: before.and_then(|b| serde_json::to_string(&b).ok()),
            after_json: Some(serde_json::to_string(&result).unwrap_or_default()),
            preview_summary_json: None,
            changed_by_id: Some(user.user_id as i64),
            requires_step_up: true,
            apply_result: "applied".to_string(),
        },
    )
    .await?;

    Ok(result)
}

// ── Designer read commands (SP01-F03) ─────────────────────────────────────────

#[tauri::command]
pub async fn get_org_designer_snapshot(
    state: State<'_, AppState>,
) -> AppResult<OrgDesignerSnapshot> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    tree_queries::get_org_designer_snapshot(&state.db).await
}

#[tauri::command]
pub async fn search_org_designer_nodes(
    query: String,
    status_filter: Option<String>,
    type_filter: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<Vec<OrgDesignerNodeRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    tree_queries::search_nodes(
        &state.db,
        &query,
        status_filter.as_deref(),
        type_filter.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn preview_org_change(
    payload: PreviewOrgChangePayload,
    state: State<'_, AppState>,
) -> AppResult<OrgImpactPreview> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    impact_preview::dispatch_preview(&state.db, payload).await
}

// ─── Validation and publish with remap (SP01-F04) ────────────────────────────

#[tauri::command]
pub async fn validate_org_model_for_publish(
    model_id: i64,
    state: State<'_, AppState>,
) -> AppResult<OrgPublishValidationResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    validation::validate_draft_model_for_publish(&state.db, model_id).await
}

#[tauri::command]
pub async fn publish_org_model(
    model_id: i64,
    state: State<'_, AppState>,
) -> AppResult<OrgPublishValidationResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.admin", PermissionScope::Global);
    require_step_up!(state);

    let result = validation::publish_model_with_remap(&state.db, model_id, user.user_id).await;

    match &result {
        Ok(validation_result) => {
            // Successful publish → audit with 'applied'
            audit::record_org_change(
                &state.db,
                OrgAuditEventInput {
                    entity_kind: "structure_model".to_string(),
                    entity_id: Some(model_id),
                    change_type: "publish_model".to_string(),
                    before_json: None,
                    after_json: Some(
                        serde_json::to_string(validation_result).unwrap_or_default(),
                    ),
                    preview_summary_json: None,
                    changed_by_id: Some(user.user_id as i64),
                    requires_step_up: true,
                    apply_result: "applied".to_string(),
                },
            )
            .await?;
        }
        Err(_) => {
            // Blocked publish → audit with 'blocked' and the validation summary
            let blocked_validation =
                validation::validate_draft_model_for_publish(&state.db, model_id)
                    .await
                    .ok();

            audit::record_org_change(
                &state.db,
                OrgAuditEventInput {
                    entity_kind: "structure_model".to_string(),
                    entity_id: Some(model_id),
                    change_type: "publish_model".to_string(),
                    before_json: None,
                    after_json: None,
                    preview_summary_json: blocked_validation
                        .and_then(|v| serde_json::to_string(&v).ok()),
                    changed_by_id: Some(user.user_id as i64),
                    requires_step_up: true,
                    apply_result: "blocked".to_string(),
                },
            )
            .await?;
        }
    }

    result
}

// ─── Org audit timeline (SP01-F04) ───────────────────────────────────────────

#[tauri::command]
pub async fn list_org_change_events(
    limit: Option<i64>,
    entity_kind: Option<String>,
    entity_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<OrgChangeEvent>> {
    let user = require_session!(state);
    require_permission!(state, &user, "org.view", PermissionScope::Global);
    audit::list_org_change_events(
        &state.db,
        limit,
        entity_kind.as_deref(),
        entity_id,
    )
    .await
}
