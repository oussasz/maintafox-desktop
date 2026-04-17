//! Personnel IPC (PRD §6.6).
//!
//! Permission gates: `per.view` (read), `per.manage` (writes). `deactivate_personnel` also
//! requires step-up verification.

use tauri::State;

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{emit_event as emit_notification_event, NotificationEventInput};
use crate::personnel::{availability, import, reports, skills, teams};
use crate::personnel::domain::{
    CompanyListFilter, ExternalCompany, ExternalCompanyContact, Personnel, PersonnelAuthorization,
    DeclareOwnSkillInput, PersonnelAvailabilityBlock, PersonnelCreateInput, PersonnelDetailPayload,
    PersonnelListFilter, PersonnelListPage, PersonnelRateCard, PersonnelSkillReferenceValue,
    PersonnelTeamAssignment, PersonnelUpdateInput, PersonnelWorkHistoryEntry, PersonnelWorkloadSummary, Position,
    ScheduleClassWithDetails, SuccessionRiskRow,
};
use crate::personnel::queries;
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

async fn linked_personnel_id(state: &State<'_, AppState>, user_id: i32) -> AppResult<Option<i64>> {
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT personnel_id FROM user_accounts WHERE id = ? AND deleted_at IS NULL",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| r.try_get("", "personnel_id").ok()).flatten())
}

async fn can_read_personnel_record(
    state: &State<'_, AppState>,
    user: &crate::auth::session_manager::AuthenticatedUser,
    personnel_id: i64,
) -> AppResult<bool> {
    let has_global = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        "per.view",
        &PermissionScope::Global,
    )
    .await?;
    if has_global {
        return Ok(true);
    }
    Ok(linked_personnel_id(state, user.user_id).await? == Some(personnel_id))
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_personnel — per.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_personnel(
    filter: PersonnelListFilter,
    state: State<'_, AppState>,
) -> AppResult<PersonnelListPage> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    queries::list_personnel(&state.db, filter).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// B) get_personnel — per.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn get_personnel(id: i64, state: State<'_, AppState>) -> AppResult<PersonnelDetailPayload> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }

    queries::get_personnel_detail(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: id.to_string(),
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) create_personnel — per.manage
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn create_personnel(
    input: PersonnelCreateInput,
    state: State<'_, AppState>,
) -> AppResult<Personnel> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);

    let trimmed = input.full_name.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le nom complet est obligatoire.".into(),
        ]));
    }
    if trimmed.chars().count() > 200 {
        return Err(AppError::ValidationFailed(vec![
            "Le nom complet ne peut pas dépasser 200 caractères.".into(),
        ]));
    }

    let mut payload = input;
    payload.full_name = trimmed;

    if let Some(pid) = payload.position_id {
        queries::assert_position_exists(&state.db, pid).await?;
    }
    if let Some(eid) = payload.primary_entity_id {
        queries::assert_org_node_exists(&state.db, eid).await?;
    }
    if let Some(tid) = payload.primary_team_id {
        queries::assert_org_node_exists(&state.db, tid).await?;
    }

    queries::create_personnel(&state.db, payload, i64::from(user.user_id)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) update_personnel — per.manage
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_personnel(
    input: PersonnelUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<Personnel> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);

    if let Some(pid) = input.position_id {
        queries::assert_position_exists(&state.db, pid).await?;
    }
    if let Some(eid) = input.primary_entity_id {
        queries::assert_org_node_exists(&state.db, eid).await?;
    }
    if let Some(tid) = input.primary_team_id {
        queries::assert_org_node_exists(&state.db, tid).await?;
    }

    queries::update_personnel(&state.db, input, i64::from(user.user_id)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// E) deactivate_personnel — per.manage + step-up
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn deactivate_personnel(
    id: i64,
    expected_row_version: i64,
    state: State<'_, AppState>,
) -> AppResult<Personnel> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    require_step_up!(state);

    queries::deactivate_personnel(&state.db, id, expected_row_version, i64::from(user.user_id)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// F/G) positions
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_positions(state: State<'_, AppState>) -> AppResult<Vec<Position>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    queries::list_positions(&state.db).await
}

#[tauri::command]
pub async fn create_position(
    code: String,
    name: String,
    category: String,
    state: State<'_, AppState>,
) -> AppResult<Position> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    queries::create_position(&state.db, code, name, category).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// H) schedule classes
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_schedule_classes(
    state: State<'_, AppState>,
) -> AppResult<Vec<ScheduleClassWithDetails>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    queries::list_schedule_classes(&state.db).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// I/J) rate cards
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_rate_cards(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelRateCard>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    queries::list_rate_cards(&state.db, personnel_id).await
}

#[tauri::command]
pub async fn create_rate_card(
    personnel_id: i64,
    labor_rate: f64,
    overtime_rate: f64,
    cost_center_id: Option<i64>,
    source_type: String,
    state: State<'_, AppState>,
) -> AppResult<PersonnelRateCard> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    queries::create_rate_card(
        &state.db,
        personnel_id,
        labor_rate,
        overtime_rate,
        cost_center_id,
        source_type,
        i64::from(user.user_id),
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// K/L) authorizations
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_authorizations(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelAuthorization>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    queries::list_authorizations(&state.db, personnel_id).await
}

#[tauri::command]
pub async fn create_authorization(
    personnel_id: i64,
    authorization_type: String,
    valid_from: String,
    valid_to: Option<String>,
    source_certification_type_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<PersonnelAuthorization> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    queries::create_authorization(
        &state.db,
        personnel_id,
        authorization_type,
        valid_from,
        valid_to,
        source_certification_type_id,
        i64::from(user.user_id),
    )
    .await
}

// ═══════════════════════════════════════════════════════════════════════════════
// M/N/O) external companies & contacts
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_external_companies(
    filter: CompanyListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<ExternalCompany>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    queries::list_external_companies(&state.db, filter).await
}

#[tauri::command]
pub async fn create_external_company(
    name: String,
    service_domain: Option<String>,
    contract_start: Option<String>,
    contract_end: Option<String>,
    notes: Option<String>,
    state: State<'_, AppState>,
) -> AppResult<ExternalCompany> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    if name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le nom de l'entreprise est obligatoire.".into(),
        ]));
    }
    queries::create_external_company(
        &state.db,
        name,
        service_domain,
        contract_start,
        contract_end,
        notes,
    )
    .await
}

#[tauri::command]
pub async fn list_company_contacts(
    company_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ExternalCompanyContact>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    queries::list_company_contacts(&state.db, company_id).await
}

#[tauri::command]
pub async fn list_skills_matrix(
    filter: skills::SkillsMatrixFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<skills::SkillMatrixRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    skills::list_skills_matrix(&state.db, filter).await
}

#[tauri::command]
pub async fn list_availability_calendar(
    filter: availability::AvailabilityCalendarFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<availability::AvailabilityCalendarEntry>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    availability::list_availability_calendar(&state.db, filter).await
}

#[tauri::command]
pub async fn list_team_capacity_summary(
    filter: teams::TeamCapacityFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<teams::TeamCapacitySummaryRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    teams::list_team_capacity_summary(&state.db, filter).await
}

#[tauri::command]
pub async fn create_availability_block(
    input: availability::AvailabilityBlockCreateInput,
    state: State<'_, AppState>,
) -> AppResult<availability::PersonnelAvailabilityBlock> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);

    let block = availability::create_availability_block(&state.db, input, i64::from(user.user_id)).await?;
    if block.is_critical {
        let event_input = NotificationEventInput {
            source_module: "personnel".to_string(),
            source_record_id: Some(block.personnel_id.to_string()),
            event_code: "personnel_critical_block".to_string(),
            category_code: "personnel_critical_block".to_string(),
            severity: "critical".to_string(),
            dedupe_key: Some(format!(
                "personnel-critical-block-{}-{}-{}",
                block.personnel_id, block.start_at, block.end_at
            )),
            payload_json: None,
            title: "Critical personnel availability block".to_string(),
            body: Some(format!(
                "A critical block ({}) was recorded for personnel #{}.",
                block.block_type, block.personnel_id
            )),
            action_url: Some("/personnel".to_string()),
        };
        if let Err(err) = emit_notification_event(&state.db, event_input).await {
            tracing::warn!(error = %err, "failed to emit personnel critical block notification");
        }
    }
    Ok(block)
}

#[tauri::command]
pub async fn list_personnel_team_assignments(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelTeamAssignment>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    queries::list_personnel_team_assignments(&state.db, personnel_id).await
}

#[tauri::command]
pub async fn list_personnel_availability_blocks(
    personnel_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelAvailabilityBlock>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    queries::list_personnel_availability_blocks(&state.db, personnel_id, limit.unwrap_or(50)).await
}

#[tauri::command]
pub async fn list_personnel_work_history(
    personnel_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelWorkHistoryEntry>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    queries::list_personnel_work_history(&state.db, personnel_id, limit.unwrap_or(60)).await
}

#[tauri::command]
pub async fn get_personnel_workload_summary(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<PersonnelWorkloadSummary> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    queries::get_personnel_workload_summary(&state.db, personnel_id).await
}

#[tauri::command]
pub async fn scan_succession_risk(
    entity_id: Option<i64>,
    team_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<SuccessionRiskRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.view", PermissionScope::Global);
    queries::scan_succession_risk(&state.db, entity_id, team_id).await
}

#[tauri::command]
pub async fn declare_own_skill(
    payload: DeclareOwnSkillInput,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let user = require_session!(state);
    let Some(personnel_id) = linked_personnel_id(&state, user.user_id).await? else {
        return Err(AppError::ValidationFailed(vec![
            "No personnel record linked to current session user.".to_string(),
        ]));
    };
    skills::declare_personnel_skill(
        &state.db,
        personnel_id,
        payload.reference_value_id,
        payload.proficiency_level,
        payload.valid_to,
        payload.note,
        payload.is_primary.unwrap_or(false),
    )
    .await
}

#[tauri::command]
pub async fn list_personnel_skill_reference_values(
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelSkillReferenceValue>> {
    let _user = require_session!(state);
    queries::list_personnel_skill_reference_values(&state.db).await
}

#[tauri::command]
pub async fn create_personnel_import_batch(
    input: import::PersonnelImportCreateInput,
    state: State<'_, AppState>,
) -> AppResult<import::PersonnelImportBatchSummary> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    import::create_import_batch(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn get_personnel_import_preview(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<import::PersonnelImportPreview> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    import::get_import_preview(&state.db, batch_id).await
}

#[tauri::command]
pub async fn apply_personnel_import_batch(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<import::PersonnelImportApplyResult> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.manage", PermissionScope::Global);
    require_step_up!(state);
    import::apply_import_batch(&state.db, batch_id, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn get_workforce_summary_report(
    state: State<'_, AppState>,
) -> AppResult<reports::WorkforceSummaryReport> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.report", PermissionScope::Global);
    reports::workforce_summary(&state.db).await
}

#[tauri::command]
pub async fn get_workforce_skills_gap_report(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<reports::WorkforceSkillsGapRow>> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.report", PermissionScope::Global);
    reports::workforce_skills_gap(&state.db, limit).await
}

#[tauri::command]
pub async fn get_workforce_kpi_report(
    state: State<'_, AppState>,
) -> AppResult<reports::WorkforceKpiReport> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.report", PermissionScope::Global);
    reports::workforce_kpis(&state.db).await
}

#[tauri::command]
pub async fn export_workforce_report_csv(
    report_kind: String,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let user = require_session!(state);
    require_permission!(state, &user, "per.report", PermissionScope::Global);
    match report_kind.trim().to_lowercase().as_str() {
        "summary" => reports::export_summary_csv(&state.db).await,
        "skills_gap" => reports::export_skills_gap_csv(&state.db).await,
        "kpi" => reports::export_kpis_csv(&state.db).await,
        _ => Err(AppError::ValidationFailed(vec![
            "report_kind must be one of: summary, skills_gap, kpi.".to_string(),
        ])),
    }
}
