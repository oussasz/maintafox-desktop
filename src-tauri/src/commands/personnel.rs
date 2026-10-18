//! Personnel IPC (PRD §6.6).
//!
//! Permission gates: `per.view` (read), `per.manage` (writes). `deactivate_personnel` also
//! requires step-up verification.

use tauri::{AppHandle, Manager, State};

use crate::auth::rbac::PermissionScope;
use crate::errors::{AppError, AppResult};
use crate::notifications::emitter::{
    emit_event_with as emit_notification_event_with, NotificationEventInput, NotificationMessage,
};
use crate::personnel::domain::{
    CompanyListFilter, DeclareOwnSkillInput, ExternalCompany, ExternalCompanyContact, Personnel,
    PersonnelAssignmentHistoryEntry, PersonnelAuthorization, PersonnelAvailabilityBlock, PersonnelAvailabilityState,
    PersonnelCreateInput, PersonnelDetailPayload, PersonnelListFilter, PersonnelListPage, PersonnelRateCard,
    PersonnelSkillReferenceValue, PersonnelTeamAssignment, PersonnelUpdateInput, PersonnelWorkHistoryEntry,
    PersonnelWorkloadSummary, Position, PositionDetailPayload, PositionListFilter, PositionRequirementSeed,
    PositionUpsertInput, ScheduleClassWithDetails, SuccessionRiskRow,
};
use crate::personnel::queries;
use crate::personnel::{
    assignment_history, availability, calendar, capacity, import, photos, reports, skills, teams, timetable,
};
use crate::state::AppState;
use crate::{require_permission, require_session, require_step_up};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

async fn linked_personnel_id(state: &State<'_, AppState>, user_id: i32) -> AppResult<Option<i64>> {
    linked_personnel_id_in(&state.db, user_id).await
}

async fn linked_personnel_id_in(db: &sea_orm::DatabaseConnection, user_id: i32) -> AppResult<Option<i64>> {
    let row = db
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
    can_read_personnel_record_in(&state.db, &state.permission_cache, user.user_id, personnel_id).await
}

/// The one personnel self-access rule: holders of per.view read every record, anyone else only the
/// record linked to their own account.
async fn can_read_personnel_record_in(
    db: &sea_orm::DatabaseConnection,
    cache: &tokio::sync::RwLock<crate::rbac::cache::PermissionCache>,
    user_id: i32,
    personnel_id: i64,
) -> AppResult<bool> {
    let has_global = crate::auth::rbac::check_permission_cached(
        db,
        cache,
        user_id,
        crate::rbac::permissions::PER_VIEW,
        &PermissionScope::Global,
    )
    .await?;
    if has_global {
        return Ok(true);
    }
    Ok(linked_personnel_id_in(db, user_id).await? == Some(personnel_id))
}

/// A skills-matrix request for one person only (no site or team list) that the self-access rule
/// allows; any other request needs per.view.
async fn skills_matrix_single_record_allowed(
    db: &sea_orm::DatabaseConnection,
    cache: &tokio::sync::RwLock<crate::rbac::cache::PermissionCache>,
    user_id: i32,
    filter: &skills::SkillsMatrixFilter,
) -> AppResult<bool> {
    match filter.personnel_id {
        Some(pid) if filter.entity_id.is_none() && filter.team_id.is_none() => {
            can_read_personnel_record_in(db, cache, user_id, pid).await
        }
        _ => Ok(false),
    }
}

/// Who may see `per.sensitiveview` fields (contact, contract/HR data, pay rates, absence reasons):
/// holders of the permission see every record; everyone else sees only their own record.
struct SensitiveAccess {
    has_permission: bool,
    own_personnel_id: Option<i64>,
}

impl SensitiveAccess {
    async fn load(state: &State<'_, AppState>, user_id: i32) -> AppResult<Self> {
        let has_permission = crate::auth::rbac::check_permission_cached(
            &state.db,
            &state.permission_cache,
            user_id,
            crate::rbac::permissions::PER_SENSITIVEVIEW,
            &PermissionScope::Global,
        )
        .await?;
        let own_personnel_id = if has_permission {
            None
        } else {
            linked_personnel_id(state, user_id).await?
        };
        Ok(Self {
            has_permission,
            own_personnel_id,
        })
    }

    fn can_view(&self, personnel_id: i64) -> bool {
        self.has_permission || self.own_personnel_id == Some(personnel_id)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// A) list_personnel — per.view
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_personnel(filter: PersonnelListFilter, state: State<'_, AppState>) -> AppResult<PersonnelListPage> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    let mut page = queries::list_personnel(&state.db, filter).await?;
    let access = SensitiveAccess::load(&state, user.user_id).await?;
    for person in &mut page.items {
        if !access.can_view(person.id) {
            person.mask_sensitive();
        }
    }
    Ok(page)
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

    let mut detail = queries::get_personnel_detail(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Personnel".into(),
            id: id.to_string(),
        })?;
    if !SensitiveAccess::load(&state, user.user_id).await?.can_view(id) {
        // `sensitive_masked` on the record also tells the UI that pay rates were withheld.
        detail.personnel.mask_sensitive();
        detail.rate_cards.clear();
    }
    Ok(detail)
}

fn sensitive_denied() -> AppError {
    AppError::PermissionDenied("Permission requise : per.sensitiveview".to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// C) create_personnel — per.manage
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn create_personnel(input: PersonnelCreateInput, state: State<'_, AppState>) -> AppResult<Personnel> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );

    let trimmed = input.full_name.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le nom complet est obligatoire.".into()
        ]));
    }
    if trimmed.chars().count() > 200 {
        return Err(AppError::ValidationFailed(vec![
            "Le nom complet ne peut pas dépasser 200 caractères.".into(),
        ]));
    }

    let mut payload = input;
    payload.full_name = trimmed;

    queries::assert_position_exists(&state.db, payload.position_id).await?;
    queries::assert_org_node_exists(&state.db, payload.primary_entity_id).await?;
    crate::repository::team_repository::assert_active_crew_exists(&state.db, payload.primary_team_id).await?;

    queries::create_personnel(&state.db, payload, i64::from(user.user_id)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// D) update_personnel — per.manage
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn update_personnel(input: PersonnelUpdateInput, state: State<'_, AppState>) -> AppResult<Personnel> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );

    // employee_code change requires step-up
    if input.employee_code.is_some() {
        require_step_up!(state);
    }

    if let Some(pid) = input.position_id {
        queries::assert_position_exists(&state.db, pid).await?;
    }
    if let Some(eid) = input.primary_entity_id {
        queries::assert_org_node_exists(&state.db, eid).await?;
    }
    if let Some(tid) = input.primary_team_id {
        crate::repository::team_repository::assert_active_crew_exists(&state.db, tid).await?;
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    require_step_up!(state);

    queries::deactivate_personnel(&state.db, id, expected_row_version, i64::from(user.user_id)).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// F/G) positions
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_positions(
    filter: Option<PositionListFilter>,
    state: State<'_, AppState>,
) -> AppResult<Vec<Position>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_POSITION_VIEW,
        PermissionScope::Global
    );
    queries::list_positions_filtered(&state.db, filter.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_position(id: i64, state: State<'_, AppState>) -> AppResult<PositionDetailPayload> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_POSITION_VIEW,
        PermissionScope::Global
    );
    queries::get_position(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Position".into(),
            id: id.to_string(),
        })
}

#[tauri::command]
pub async fn upsert_position(input: PositionUpsertInput, state: State<'_, AppState>) -> AppResult<Position> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_POSITION_MANAGE,
        PermissionScope::Global
    );
    queries::upsert_position(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn archive_position(id: i64, state: State<'_, AppState>) -> AppResult<Position> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_POSITION_MANAGE,
        PermissionScope::Global
    );
    queries::archive_position(&state.db, id, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn create_position(
    code: String,
    name: String,
    category: String,
    state: State<'_, AppState>,
) -> AppResult<Position> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_POSITION_MANAGE,
        PermissionScope::Global
    );
    queries::create_position(&state.db, code, name, category).await
}

#[tauri::command]
pub async fn get_position_requirement_seed(
    position_id: i64,
    state: State<'_, AppState>,
) -> AppResult<PositionRequirementSeed> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_POSITION_VIEW,
        PermissionScope::Global
    );
    queries::get_position_requirement_seed(&state.db, position_id).await
}

#[tauri::command]
pub async fn list_personnel_assignment_history(
    personnel_id: i64,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<PersonnelAssignmentHistoryEntry>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    assignment_history::list_personnel_assignment_history(&state.db, personnel_id, limit.unwrap_or(50)).await
}

#[tauri::command]
pub async fn upload_personnel_photo(
    app: AppHandle,
    personnel_id: i64,
    source_path: String,
    state: State<'_, AppState>,
) -> AppResult<String> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::upload_personnel_photo(&state.db, &app_data_dir, personnel_id, &source_path).await
}

#[tauri::command]
pub async fn read_personnel_photo_preview(
    app: AppHandle,
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<photos::PersonnelPhotoPreview> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("app_data_dir: {e}")))?;
    photos::read_personnel_photo_preview(&state.db, &app_data_dir, personnel_id).await
}

#[tauri::command]
pub async fn get_personnel_availability_state(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<PersonnelAvailabilityState> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    availability::compute_personnel_availability_state(&state.db, personnel_id).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// H) schedule classes
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_schedule_classes(state: State<'_, AppState>) -> AppResult<Vec<ScheduleClassWithDetails>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    queries::list_schedule_classes(&state.db).await
}

// ═══════════════════════════════════════════════════════════════════════════════
// I/J) rate cards
// ═══════════════════════════════════════════════════════════════════════════════

#[tauri::command]
pub async fn list_rate_cards(personnel_id: i64, state: State<'_, AppState>) -> AppResult<Vec<PersonnelRateCard>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    if !SensitiveAccess::load(&state, user.user_id)
        .await?
        .can_view(personnel_id)
    {
        return Err(sensitive_denied());
    }
    queries::list_rate_cards(&state.db, personnel_id).await
}

#[tauri::command]
pub async fn get_active_rate_card(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Option<PersonnelRateCard>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    if !SensitiveAccess::load(&state, user.user_id)
        .await?
        .can_view(personnel_id)
    {
        return Err(sensitive_denied());
    }
    queries::get_active_rate_card(&state.db, personnel_id).await
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    if name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Le nom de l'entreprise est obligatoire.".into(),
        ]));
    }
    queries::create_external_company(&state.db, name, service_domain, contract_start, contract_end, notes).await
}

/// Contractor company record: contract dates, onboarding and insurance status.
#[tauri::command]
pub async fn update_external_company(
    input: queries::ExternalCompanyUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<ExternalCompany> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    queries::update_external_company(&state.db, input).await
}

#[tauri::command]
pub async fn list_company_contacts(
    company_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<ExternalCompanyContact>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    queries::list_company_contacts(&state.db, company_id).await
}

#[tauri::command]
pub async fn list_skills_matrix(
    filter: skills::SkillsMatrixFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<skills::SkillMatrixRow>> {
    let user = require_session!(state);
    // One person's skills: the same rule as their work history and workload (per.view, or the
    // person's own linked record, read from their profile). Any wider list needs per.view.
    let single_record =
        skills_matrix_single_record_allowed(&state.db, &state.permission_cache, user.user_id, &filter).await?;
    if !single_record {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::PER_VIEW,
            PermissionScope::Global
        );
    }
    skills::list_skills_matrix(&state.db, filter).await
}

#[tauri::command]
pub async fn list_availability_calendar(
    filter: availability::AvailabilityCalendarFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<availability::AvailabilityCalendarEntry>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    availability::list_availability_calendar(&state.db, filter).await
}

#[tauri::command]
pub async fn list_team_capacity_summary(
    filter: teams::TeamCapacityFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<teams::TeamCapacitySummaryRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    teams::list_team_capacity_summary(&state.db, filter).await
}

#[tauri::command]
pub async fn list_active_teams(
    filter: crate::repository::team_repository::TeamListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::repository::team_repository::TeamSummary>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    crate::repository::team_repository::list_active_teams(&state.db, filter).await
}

#[tauri::command]
pub async fn get_team(
    team_id: i64,
    state: State<'_, AppState>,
) -> AppResult<crate::repository::team_repository::TeamSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    crate::repository::team_repository::get_team_by_id(&state.db, team_id).await
}

#[tauri::command]
pub async fn list_team_members(
    team_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::personnel::domain::TeamMemberRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    queries::list_team_members(&state.db, team_id).await
}

#[tauri::command]
pub async fn create_team(
    input: crate::repository::team_repository::TeamCreateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::repository::team_repository::TeamSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    crate::repository::team_repository::create_team(&state.db, input).await
}

#[tauri::command]
pub async fn update_team(
    input: crate::repository::team_repository::TeamUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<crate::repository::team_repository::TeamSummary> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    crate::repository::team_repository::update_team(&state.db, input).await
}

#[tauri::command]
pub async fn set_team_lead(
    input: crate::personnel::domain::SetTeamLeadInput,
    state: State<'_, AppState>,
) -> AppResult<crate::personnel::domain::TeamMemberRow> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    queries::set_team_lead(&state.db, input).await
}

#[tauri::command]
pub async fn create_availability_block(
    input: availability::AvailabilityBlockCreateInput,
    state: State<'_, AppState>,
) -> AppResult<availability::PersonnelAvailabilityBlock> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );

    let block = availability::create_availability_block(&state.db, input, i64::from(user.user_id)).await?;
    if block.is_critical {
        let person = state
            .db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT full_name FROM personnel WHERE id = ?",
                [block.personnel_id.into()],
            ))
            .await
            .ok()
            .flatten()
            .and_then(|r| r.try_get::<String>("", "full_name").ok())
            .unwrap_or_default();
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
            // The block type (medical, restriction…) is personal data: recipients only learn
            // that the person is unavailable, not why.
            body: Some(format!(
                "{} is unavailable (critical block) from {} to {}.",
                person,
                block.start_at.chars().take(16).collect::<String>().replace('T', " "),
                block.end_at.chars().take(16).collect::<String>().replace('T', " ")
            )),
            action_url: Some("/personnel".to_string()),
        };
        // Same rule for the localised text: who and when, never the block type.
        let message = NotificationMessage::new(
            "personnel_critical_block",
            serde_json::json!({ "person": person, "start_at": block.start_at, "end_at": block.end_at }),
        );
        if let Err(err) = emit_notification_event_with(&state.db, event_input, Some(&message)).await {
            tracing::warn!(error = %err, "failed to emit personnel critical block notification");
        }
    }
    Ok(block)
}

#[tauri::command]
pub async fn update_availability_block(
    input: availability::AvailabilityBlockUpdateInput,
    state: State<'_, AppState>,
) -> AppResult<availability::PersonnelAvailabilityBlock> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    availability::update_availability_block(&state.db, input).await
}

#[tauri::command]
pub async fn delete_availability_block(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    availability::delete_availability_block(&state.db, id).await
}

#[tauri::command]
pub async fn end_availability_block(
    id: i64,
    state: State<'_, AppState>,
) -> AppResult<availability::PersonnelAvailabilityBlock> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    availability::end_availability_block(&state.db, id).await
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
    let mut blocks = queries::list_personnel_availability_blocks(&state.db, personnel_id, limit.unwrap_or(50)).await?;
    if !SensitiveAccess::load(&state, user.user_id)
        .await?
        .can_view(personnel_id)
    {
        for block in &mut blocks {
            block.mask_sensitive();
        }
    }
    Ok(blocks)
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    queries::scan_succession_risk(&state.db, entity_id, team_id).await
}

#[tauri::command]
pub async fn declare_own_skill(payload: DeclareOwnSkillInput, state: State<'_, AppState>) -> AppResult<()> {
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
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    import::create_import_batch(&state.db, input, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn get_personnel_import_preview(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<import::PersonnelImportPreview> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    import::get_import_preview(&state.db, batch_id).await
}

#[tauri::command]
pub async fn apply_personnel_import_batch(
    batch_id: i64,
    state: State<'_, AppState>,
) -> AppResult<import::PersonnelImportApplyResult> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    require_step_up!(state);
    import::apply_import_batch(&state.db, batch_id, Some(i64::from(user.user_id))).await
}

#[tauri::command]
pub async fn get_workforce_summary_report(state: State<'_, AppState>) -> AppResult<reports::WorkforceSummaryReport> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_REPORT,
        PermissionScope::Global
    );
    reports::workforce_summary(&state.db).await
}

#[tauri::command]
pub async fn get_workforce_skills_gap_report(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<reports::WorkforceSkillsGapRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_REPORT,
        PermissionScope::Global
    );
    reports::workforce_skills_gap(&state.db, limit).await
}

#[tauri::command]
pub async fn get_workforce_kpi_report(state: State<'_, AppState>) -> AppResult<reports::WorkforceKpiReport> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_REPORT,
        PermissionScope::Global
    );
    reports::workforce_kpis(&state.db).await
}

#[tauri::command]
pub async fn export_workforce_report_csv(report_kind: String, state: State<'_, AppState>) -> AppResult<String> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_REPORT,
        PermissionScope::Global
    );
    match report_kind.trim().to_lowercase().as_str() {
        "summary" => reports::export_summary_csv(&state.db).await,
        "skills_gap" => reports::export_skills_gap_csv(&state.db).await,
        "kpi" => reports::export_kpis_csv(&state.db).await,
        _ => Err(AppError::ValidationFailed(vec![
            "report_kind must be one of: summary, skills_gap, kpi.".to_string(),
        ])),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Capacity, holiday calendar, skills recorded by managers
// ═══════════════════════════════════════════════════════════════════════════════

fn parse_capacity_day(field: &str, value: &str) -> AppResult<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| {
        crate::errors::org_validation_failed(
            "CAPACITY_RANGE_INVALID",
            "Enter dates as YYYY-MM-DD.",
            std::collections::HashMap::from([("field".to_string(), field.to_string())]),
        )
    })
}

/// Team capacity per day from the members' shift calendars, absences and holidays.
#[tauri::command]
pub async fn get_team_capacity(
    team_id: i64,
    date_from: String,
    date_to: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<capacity::TeamDayCapacity>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    capacity::team_capacity_range(
        &state.db,
        team_id,
        parse_capacity_day("date_from", &date_from)?,
        parse_capacity_day("date_to", &date_to)?,
    )
    .await
}

/// One person's capacity per day. Absence types are personal data (`per.sensitiveview`).
#[tauri::command]
pub async fn get_person_capacity(
    personnel_id: i64,
    date_from: String,
    date_to: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<capacity::PersonDayCapacity>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    let mut days = capacity::person_capacity_range(
        &state.db,
        personnel_id,
        parse_capacity_day("date_from", &date_from)?,
        parse_capacity_day("date_to", &date_to)?,
    )
    .await?;
    if !SensitiveAccess::load(&state, user.user_id)
        .await?
        .can_view(personnel_id)
    {
        for day in &mut days {
            day.absence_types.clear();
        }
    }
    Ok(days)
}

#[tauri::command]
pub async fn list_work_calendar_holidays(
    filter: calendar::WorkCalendarHolidayFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<calendar::WorkCalendarHoliday>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    calendar::list_holidays(&state.db, filter).await
}

#[tauri::command]
pub async fn upsert_work_calendar_holiday(
    input: calendar::WorkCalendarHolidayUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<calendar::WorkCalendarHoliday> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    calendar::upsert_holiday(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn list_personnel_skills(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<skills::PersonnelSkillRow>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    skills::list_personnel_skills(&state.db, personnel_id).await
}

/// Records a person's skill and level. Validating it (it then counts for qualification
/// requirements) is a certification act: `trn.certify`.
#[tauri::command]
pub async fn upsert_personnel_skill(
    input: skills::PersonnelSkillUpsertInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<skills::PersonnelSkillRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    if input.validate {
        require_permission!(
            state,
            &user,
            crate::rbac::permissions::TRN_CERTIFY,
            PermissionScope::Global
        );
    }
    let validator = linked_personnel_id(&state, user.user_id).await?;
    skills::upsert_personnel_skill(&state.db, input, validator).await
}

#[tauri::command]
pub async fn remove_personnel_skill(
    personnel_id: i64,
    reference_value_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<skills::PersonnelSkillRow>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    skills::remove_personnel_skill(&state.db, personnel_id, reference_value_id).await
}

// ── Timetables (Classes horaires), team links, person exceptions, site week ────

/// Versions of a Classe horaire (or of all of them), oldest first.
#[tauri::command]
pub async fn list_schedule_class_versions(
    reference_value_id: Option<i64>,
    state: State<'_, AppState>,
) -> AppResult<Vec<timetable::ScheduleClassVersion>> {
    let user = require_session!(state);
    // Reference data readers and people managers (team and person timetables).
    crate::commands::qualification::require_any_permission(
        &state,
        &user,
        &[crate::rbac::permissions::REF_VIEW, crate::rbac::permissions::PER_VIEW],
    )
    .await?;
    timetable::list_class_versions(&state.db, reference_value_id).await
}

/// Open work orders planned on a version, shown before editing it (they keep their window).
#[tauri::command]
pub async fn get_schedule_class_version_impact(
    version_id: i64,
    state: State<'_, AppState>,
) -> AppResult<timetable::VersionWorkOrderImpact> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    timetable::version_work_order_impact(&state.db, version_id).await
}

/// Adds a version of a Classe horaire from a date, or edits one not yet in force.
#[tauri::command]
pub async fn upsert_schedule_class_version(
    input: timetable::ScheduleClassVersionInput,
    state: State<'_, AppState>,
) -> AppResult<timetable::ScheduleClassVersion> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    timetable::upsert_class_version(&state.db, input, i64::from(user.user_id)).await
}

/// Confirms a Classe horaire flagged for review by the upgrade.
#[tauri::command]
pub async fn confirm_schedule_class_review(
    reference_value_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<timetable::ScheduleClassVersion>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::REF_MANAGE,
        PermissionScope::Global
    );
    timetable::confirm_class_review(&state.db, reference_value_id).await
}

/// A team's Classe horaire history.
#[tauri::command]
pub async fn list_team_schedule(
    team_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<timetable::TeamScheduleAssignment>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    timetable::list_team_schedule(&state.db, team_id).await
}

/// Links a team to a Classe horaire from a date.
#[tauri::command]
pub async fn set_team_schedule(
    input: timetable::TeamScheduleAssignmentInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<timetable::TeamScheduleAssignment>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    timetable::set_team_schedule(&state.db, input, i64::from(user.user_id)).await
}

/// Confirms a team's Classe horaire link flagged for review by the upgrade.
#[tauri::command]
pub async fn confirm_team_schedule_review(
    team_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<timetable::TeamScheduleAssignment>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    timetable::confirm_team_schedule_review(&state.db, team_id).await
}

/// A person's timetable exceptions. The reason is personal data (`per.sensitiveview`).
#[tauri::command]
pub async fn list_personnel_schedule_exceptions(
    personnel_id: i64,
    state: State<'_, AppState>,
) -> AppResult<Vec<timetable::PersonnelScheduleException>> {
    let user = require_session!(state);
    if !can_read_personnel_record(&state, &user, personnel_id).await? {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    let mut rows = timetable::list_personnel_schedule_exceptions(&state.db, personnel_id).await?;
    if !SensitiveAccess::load(&state, user.user_id)
        .await?
        .can_view(personnel_id)
    {
        for row in &mut rows {
            row.reason = None;
        }
    }
    Ok(rows)
}

#[tauri::command]
pub async fn upsert_personnel_schedule_exception(
    input: timetable::PersonnelScheduleExceptionInput,
    state: State<'_, AppState>,
) -> AppResult<timetable::PersonnelScheduleException> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    timetable::upsert_personnel_schedule_exception(&state.db, input, i64::from(user.user_id)).await
}

#[tauri::command]
pub async fn delete_personnel_schedule_exception(id: i64, state: State<'_, AppState>) -> AppResult<()> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    timetable::delete_personnel_schedule_exception(&state.db, id).await
}

/// Weekly rest days of every site and of the sites that set their own.
#[tauri::command]
pub async fn list_site_work_weeks(state: State<'_, AppState>) -> AppResult<Vec<calendar::SiteWorkWeek>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_VIEW,
        PermissionScope::Global
    );
    calendar::list_site_work_weeks(&state.db).await
}

#[tauri::command]
pub async fn set_site_work_week(
    input: calendar::SiteWorkWeekInput,
    state: State<'_, AppState>,
) -> AppResult<Vec<calendar::SiteWorkWeek>> {
    let user = require_session!(state);
    require_permission!(
        state,
        &user,
        crate::rbac::permissions::PER_MANAGE,
        PermissionScope::Global
    );
    calendar::set_site_work_week(&state.db, input, i64::from(user.user_id)).await
}

#[cfg(test)]
mod skills_self_access_tests {
    use super::*;
    use sea_orm::DatabaseConnection;

    async fn exec(db: &DatabaseConnection, sql: &str) {
        db.execute(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .expect("query")
            .expect("row")
            .try_get_by_index::<i64>(0)
            .expect("i64")
    }

    fn one(personnel_id: Option<i64>, team_id: Option<i64>) -> skills::SkillsMatrixFilter {
        skills::SkillsMatrixFilter {
            personnel_id,
            entity_id: None,
            team_id,
            skill_code: None,
            include_inactive: Some(true),
        }
    }

    #[tokio::test]
    async fn without_per_view_only_the_own_record_is_allowed() {
        let db = crate::db::test_db::seeded().await;
        // Two people; a user without any role, linked to the first.
        for code in ["SELF-1", "OTHER-2"] {
            exec(
                &db,
                &format!(
                    "INSERT INTO personnel (entity_sync_id, employee_code, full_name, employment_type, \
                     availability_status, created_at, updated_at, row_version) \
                     VALUES ('p-{code}', '{code}', 'Person {code}', 'employee', 'available', \
                     datetime('now'), datetime('now'), 1)"
                ),
            )
            .await;
        }
        let me = scalar(&db, "SELECT id FROM personnel WHERE employee_code = 'SELF-1'").await;
        let other = scalar(&db, "SELECT id FROM personnel WHERE employee_code = 'OTHER-2'").await;
        exec(
            &db,
            &format!(
                "INSERT INTO user_accounts (entity_sync_id, username, identity_mode, password_hash, is_active, \
                 is_admin, force_password_change, failed_login_attempts, personnel_id, created_at, updated_at, \
                 row_version) VALUES ('u-self', 'self.user', 'local', 'x', 1, 0, 0, 0, {me}, datetime('now'), \
                 datetime('now'), 1)"
            ),
        )
        .await;
        let user = i32::try_from(scalar(&db, "SELECT id FROM user_accounts WHERE username = 'self.user'").await)
            .expect("fits");
        let cache = tokio::sync::RwLock::new(crate::rbac::cache::PermissionCache::default());

        assert!(
            skills_matrix_single_record_allowed(&db, &cache, user, &one(Some(me), None))
                .await
                .expect("own")
        );
        assert!(
            !skills_matrix_single_record_allowed(&db, &cache, user, &one(Some(other), None))
                .await
                .expect("other"),
            "another person's skills need per.view"
        );
        assert!(
            !skills_matrix_single_record_allowed(&db, &cache, user, &one(None, None))
                .await
                .expect("all"),
            "the whole matrix needs per.view"
        );
        assert!(
            !skills_matrix_single_record_allowed(&db, &cache, user, &one(Some(me), Some(1)))
                .await
                .expect("team"),
            "a team list is not a single record"
        );
    }
}

/// The personnel lists of Données de référence (employment statuses and types, absence types with
/// their behaviour tag, position categories). Holders of per.view, or anyone whose account is
/// linked to a personnel record (the self-service screens read the same lists).
#[tauri::command]
pub async fn list_personnel_reference_options(
    state: State<'_, AppState>,
) -> AppResult<crate::personnel::reference_lists::PersonnelReferenceOptions> {
    let user = require_session!(state);
    let has_global = crate::auth::rbac::check_permission_cached(
        &state.db,
        &state.permission_cache,
        user.user_id,
        crate::rbac::permissions::PER_VIEW,
        &PermissionScope::Global,
    )
    .await?;
    if !has_global && linked_personnel_id_in(&state.db, user.user_id).await?.is_none() {
        return Err(AppError::PermissionDenied("Permission requise : per.view".to_string()));
    }
    crate::personnel::reference_lists::list_personnel_reference_options(&state.db).await
}
