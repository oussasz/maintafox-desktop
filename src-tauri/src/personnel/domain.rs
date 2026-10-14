//! Personnel domain types and code generation (PRD §6.6).

use crate::errors::AppResult;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};

/// Paginated list result for personnel grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelListPage {
    pub items: Vec<Personnel>,
    pub total: i64,
}

/// Schedule class with its weekday/shift rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleClassWithDetails {
    pub class: ScheduleClass,
    pub details: Vec<ScheduleDetail>,
}

/// Filter for external company pickers / lists.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompanyListFilter {
    pub onboarding_status: Option<String>,
    pub search: Option<String>,
}

/// Detail bundle for `get_personnel` IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelDetailPayload {
    pub personnel: Personnel,
    pub rate_cards: Vec<PersonnelRateCard>,
    pub authorizations: Vec<PersonnelAuthorization>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelTeamAssignment {
    pub id: i64,
    pub personnel_id: i64,
    pub team_id: i64,
    pub team_code: Option<String>,
    pub team_name: Option<String>,
    pub role_code: String,
    pub allocation_percent: f64,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub is_lead: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Open assignment row for a crew roster (team-scoped).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMemberRow {
    pub assignment_id: i64,
    pub personnel_id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub position_name: Option<String>,
    pub role_code: String,
    pub allocation_percent: f64,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub is_lead: i64,
    pub availability_status: String,
    /// Linked login account when `user_accounts.personnel_id` matches (WO assignment bridge).
    pub user_account_id: Option<i64>,
}

/// Set or clear crew lead on an existing open assignment (does not touch membership sync).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetTeamLeadInput {
    pub team_id: i64,
    pub personnel_id: i64,
    pub is_lead: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelAvailabilityBlock {
    pub id: i64,
    pub personnel_id: i64,
    pub block_type: String,
    pub start_at: String,
    pub end_at: String,
    pub reason_note: Option<String>,
    pub is_critical: bool,
    pub created_by_id: Option<i64>,
    pub created_by_label: Option<String>,
    pub created_at: String,
    /// True when `reason_note` was withheld (caller lacks `per.sensitiveview`, not own record).
    #[serde(default)]
    pub reason_masked: bool,
}

impl PersonnelAvailabilityBlock {
    pub fn mask_sensitive(&mut self) {
        self.reason_note = None;
        self.reason_masked = true;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelWorkHistoryEntry {
    pub source_module: String,
    pub record_id: i64,
    pub record_code: Option<String>,
    pub role_code: String,
    pub status_code: Option<String>,
    pub title: String,
    pub happened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelWorkloadSummary {
    pub open_work_orders: i64,
    pub in_progress_work_orders: i64,
    pub pending_interventions: i64,
    pub interventions_last_30d: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessionRiskRow {
    pub personnel_id: i64,
    pub full_name: String,
    pub employee_code: String,
    pub position_name: Option<String>,
    pub team_name: Option<String>,
    pub coverage_count: i64,
    pub risk_level: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclareOwnSkillInput {
    pub reference_value_id: i64,
    pub proficiency_level: i64,
    pub valid_to: Option<String>,
    pub note: Option<String>,
    pub is_primary: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelSkillReferenceValue {
    pub id: i64,
    pub code: String,
    pub label: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// EmploymentType — PRD §6.6 `personnel.employment_type`
// ═══════════════════════════════════════════════════════════════════════════════

/// `employee` / `contractor` / `temp` / `vendor`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmploymentType {
    Employee,
    Contractor,
    Temp,
    Vendor,
}

impl EmploymentType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Employee => "employee",
            Self::Contractor => "contractor",
            Self::Temp => "temp",
            Self::Vendor => "vendor",
        }
    }
}

impl TryFrom<&str> for EmploymentType {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "employee" => Ok(Self::Employee),
            "contractor" => Ok(Self::Contractor),
            "temp" => Ok(Self::Temp),
            "vendor" => Ok(Self::Vendor),
            other => Err(format!("Unknown employment type: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EmploymentOrigin — internal | external (orthogonal to employment_type)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmploymentOrigin {
    Internal,
    External,
}

impl EmploymentOrigin {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::External => "external",
        }
    }
}

impl TryFrom<&str> for EmploymentOrigin {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "internal" => Ok(Self::Internal),
            "external" => Ok(Self::External),
            other => Err(format!("Unknown employment origin: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EmploymentStatus — active | inactive | suspended | terminated
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmploymentStatus {
    Active,
    Inactive,
    Suspended,
    Terminated,
}

impl EmploymentStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Suspended => "suspended",
            Self::Terminated => "terminated",
        }
    }
}

impl TryFrom<&str> for EmploymentStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "active" => Ok(Self::Active),
            "inactive" => Ok(Self::Inactive),
            "suspended" => Ok(Self::Suspended),
            "terminated" => Ok(Self::Terminated),
            other => Err(format!("Unknown employment status: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ComputedAvailabilityStatus — display / gates (not free-form on create)
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComputedAvailabilityStatus {
    Available,
    Assigned,
    InTraining,
    OnLeave,
    Blocked,
}

impl ComputedAvailabilityStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Assigned => "assigned",
            Self::InTraining => "in_training",
            Self::OnLeave => "on_leave",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelAvailabilityState {
    pub personnel_id: i64,
    pub status: String,
    pub blocked_override: bool,
    pub reasons: Vec<String>,
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelAssignmentHistoryEntry {
    pub id: i64,
    pub personnel_id: i64,
    pub entity_id: Option<i64>,
    pub team_id: Option<i64>,
    pub position_id: Option<i64>,
    pub manager_id: Option<i64>,
    pub schedule_reference_value_id: Option<i64>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub reason: Option<String>,
    pub changed_by_id: Option<i64>,
    pub created_at: String,
    // Display
    pub entity_name: Option<String>,
    pub team_name: Option<String>,
    pub position_code: Option<String>,
    pub position_name: Option<String>,
    pub manager_name: Option<String>,
    pub schedule_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionChangeEvent {
    pub id: i64,
    pub position_id: i64,
    pub event_type: String,
    pub summary: String,
    pub detail_json: Option<String>,
    pub changed_by_id: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelSkillDraft {
    pub reference_value_id: i64,
    pub proficiency_level: i64,
    pub source_type: String,
    pub valid_to: Option<String>,
    pub is_primary: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelCertDraft {
    pub certification_type_id: i64,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub issuing_body: Option<String>,
    pub certificate_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionRequirementProfileInput {
    pub profile_name: String,
    pub skill_reference_value_ids: Vec<i64>,
    pub certification_type_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpsertInput {
    pub id: Option<i64>,
    pub code: String,
    pub name: String,
    pub category: String,
    pub is_active: Option<bool>,
    pub requirement_profile: Option<PositionRequirementProfileInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionListFilter {
    pub search: Option<String>,
    pub include_inactive: Option<bool>,
    #[serde(default = "default_list_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

impl Default for PositionListFilter {
    fn default() -> Self {
        Self {
            search: None,
            include_inactive: Some(false),
            limit: default_list_limit(),
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionDetailPayload {
    pub position: Position,
    pub skill_reference_value_ids: Vec<i64>,
    pub certification_type_ids: Vec<i64>,
    pub change_events: Vec<PositionChangeEvent>,
}

/// Skills + certification types suggested by a position's requirement profile
/// (personnel create dialog seed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionRequirementSeed {
    pub skill_reference_value_ids: Vec<i64>,
    pub certification_type_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadPersonnelPhotoInput {
    pub personnel_id: i64,
    pub source_path: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// AvailabilityStatus — PRD §6.6 `personnel.availability_status`
// ═══════════════════════════════════════════════════════════════════════════════

/// `available` / `assigned` / `in_training` / `on_leave` / `blocked` / `inactive`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AvailabilityStatus {
    Available,
    Assigned,
    InTraining,
    OnLeave,
    Blocked,
    Inactive,
}

impl AvailabilityStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Assigned => "assigned",
            Self::InTraining => "in_training",
            Self::OnLeave => "on_leave",
            Self::Blocked => "blocked",
            Self::Inactive => "inactive",
        }
    }
}

impl TryFrom<&str> for AvailabilityStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "available" => Ok(Self::Available),
            "assigned" => Ok(Self::Assigned),
            "in_training" => Ok(Self::InTraining),
            "on_leave" => Ok(Self::OnLeave),
            "blocked" => Ok(Self::Blocked),
            "inactive" => Ok(Self::Inactive),
            other => Err(format!("Unknown availability status: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PositionCategory — PRD §6.6 `positions.category`
// ═══════════════════════════════════════════════════════════════════════════════

/// technician / supervisor / engineer / operator / contractor / planner / storekeeper / hse
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionCategory {
    Technician,
    Supervisor,
    Engineer,
    Operator,
    Contractor,
    Planner,
    Storekeeper,
    Hse,
}

impl PositionCategory {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Technician => "technician",
            Self::Supervisor => "supervisor",
            Self::Engineer => "engineer",
            Self::Operator => "operator",
            Self::Contractor => "contractor",
            Self::Planner => "planner",
            Self::Storekeeper => "storekeeper",
            Self::Hse => "hse",
        }
    }
}

impl TryFrom<&str> for PositionCategory {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "technician" => Ok(Self::Technician),
            "supervisor" => Ok(Self::Supervisor),
            "engineer" => Ok(Self::Engineer),
            "operator" => Ok(Self::Operator),
            "contractor" => Ok(Self::Contractor),
            "planner" => Ok(Self::Planner),
            "storekeeper" => Ok(Self::Storekeeper),
            "hse" => Ok(Self::Hse),
            other => Err(format!("Unknown position category: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// AuthorizationType — PRD §6.6 `personnel_authorizations.authorization_type`
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthorizationType {
    PermitIssuer,
    IsolationAuthority,
    Inspector,
    WarehouseSignoff,
}

impl AuthorizationType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PermitIssuer => "permit_issuer",
            Self::IsolationAuthority => "isolation_authority",
            Self::Inspector => "inspector",
            Self::WarehouseSignoff => "warehouse_signoff",
        }
    }
}

impl TryFrom<&str> for AuthorizationType {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "permit_issuer" => Ok(Self::PermitIssuer),
            "isolation_authority" => Ok(Self::IsolationAuthority),
            "inspector" => Ok(Self::Inspector),
            "warehouse_signoff" => Ok(Self::WarehouseSignoff),
            other => Err(format!("Unknown authorization type: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// OnboardingStatus — PRD §6.6 `external_companies.onboarding_status`
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OnboardingStatus {
    Pending,
    Active,
    Suspended,
    Expired,
}

impl OnboardingStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Expired => "expired",
        }
    }
}

impl TryFrom<&str> for OnboardingStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "expired" => Ok(Self::Expired),
            other => Err(format!("Unknown onboarding status: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// InsuranceStatus — PRD §6.6 `external_companies.insurance_status`
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InsuranceStatus {
    Unknown,
    Valid,
    Expired,
    NotRequired,
}

impl InsuranceStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Valid => "valid",
            Self::Expired => "expired",
            Self::NotRequired => "not_required",
        }
    }
}

impl TryFrom<&str> for InsuranceStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "unknown" => Ok(Self::Unknown),
            "valid" => Ok(Self::Valid),
            "expired" => Ok(Self::Expired),
            "not_required" => Ok(Self::NotRequired),
            other => Err(format!("Unknown insurance status: '{other}'")),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Row structs — DDL + join display fields
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Personnel {
    pub id: i64,
    pub employee_code: String,
    pub full_name: String,
    pub employment_type: String,
    pub employment_origin: String,
    pub employment_status: String,
    pub blocked_override: bool,
    pub position_id: Option<i64>,
    pub primary_entity_id: Option<i64>,
    pub primary_team_id: Option<i64>,
    pub supervisor_id: Option<i64>,
    pub home_schedule_reference_value_id: Option<i64>,
    pub availability_status: String,
    pub hire_date: Option<String>,
    pub termination_date: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub photo_path: Option<String>,
    pub hr_external_id: Option<String>,
    pub external_company_id: Option<i64>,
    pub contract_number: Option<String>,
    pub contract_start_date: Option<String>,
    pub contract_end_date: Option<String>,
    pub notes: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
    // Display (joins)
    pub position_name: Option<String>,
    pub position_code: Option<String>,
    pub position_category: Option<String>,
    pub entity_name: Option<String>,
    pub team_name: Option<String>,
    pub supervisor_name: Option<String>,
    pub schedule_name: Option<String>,
    pub company_name: Option<String>,
    /// True when contact, contract and HR fields were withheld (caller lacks `per.sensitiveview`
    /// and this is not their own record). Withheld fields are `None`, not empty values.
    #[serde(default)]
    pub sensitive_masked: bool,
}

impl Personnel {
    /// Withholds the fields covered by `per.sensitiveview`.
    pub fn mask_sensitive(&mut self) {
        self.email = None;
        self.phone = None;
        self.hr_external_id = None;
        self.contract_number = None;
        self.contract_start_date = None;
        self.contract_end_date = None;
        self.notes = None;
        self.sensitive_masked = true;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub category: String,
    pub requirement_profile_id: Option<i64>,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleClass {
    pub id: i64,
    pub name: String,
    pub shift_pattern_code: String,
    pub is_continuous: i64,
    pub nominal_hours_per_day: f64,
    pub is_active: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleDetail {
    pub id: i64,
    pub reference_value_id: i64,
    pub day_of_week: i64,
    pub shift_start: String,
    pub shift_end: String,
    pub is_rest_day: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelRateCard {
    pub id: i64,
    pub personnel_id: i64,
    pub effective_from: String,
    pub labor_rate: f64,
    pub overtime_rate: f64,
    pub cost_center_id: Option<i64>,
    pub source_type: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelAuthorization {
    pub id: i64,
    pub personnel_id: i64,
    pub authorization_type: String,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub source_certification_type_id: Option<i64>,
    pub is_active: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalCompany {
    pub id: i64,
    pub name: String,
    pub service_domain: Option<String>,
    pub contract_start: Option<String>,
    pub contract_end: Option<String>,
    pub onboarding_status: String,
    pub insurance_status: String,
    pub notes: Option<String>,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalCompanyContact {
    pub id: i64,
    pub company_id: i64,
    pub contact_name: String,
    pub contact_role: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub is_primary: i64,
    pub created_at: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Input DTOs
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelCreateInput {
    pub full_name: String,
    pub employee_code: String,
    pub employment_type: String,
    pub employment_origin: String,
    pub employment_status: Option<String>,
    pub position_id: i64,
    pub primary_entity_id: i64,
    pub primary_team_id: i64,
    pub supervisor_id: Option<i64>,
    pub home_schedule_reference_value_id: i64,
    pub hire_date: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub external_company_id: Option<i64>,
    pub contract_number: Option<String>,
    pub contract_start_date: Option<String>,
    pub contract_end_date: Option<String>,
    pub notes: Option<String>,
    pub blocked_override: Option<bool>,
    pub assignment_reason: Option<String>,
    #[serde(default)]
    pub skills: Vec<PersonnelSkillDraft>,
    #[serde(default)]
    pub certifications: Vec<PersonnelCertDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelUpdateInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub full_name: Option<String>,
    pub employment_type: Option<String>,
    pub employment_origin: Option<String>,
    pub employment_status: Option<String>,
    pub position_id: Option<i64>,
    pub primary_entity_id: Option<i64>,
    pub primary_team_id: Option<i64>,
    pub supervisor_id: Option<i64>,
    pub home_schedule_reference_value_id: Option<i64>,
    pub availability_status: Option<String>,
    pub blocked_override: Option<bool>,
    pub hire_date: Option<String>,
    pub termination_date: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub external_company_id: Option<i64>,
    pub contract_number: Option<String>,
    pub contract_start_date: Option<String>,
    pub contract_end_date: Option<String>,
    pub notes: Option<String>,
    pub assignment_reason: Option<String>,
    /// Privileged path only — omitted for normal updates.
    pub employee_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelListFilter {
    pub employment_type: Option<Vec<String>>,
    pub employment_origin: Option<Vec<String>>,
    pub employment_status: Option<Vec<String>>,
    pub availability_status: Option<Vec<String>>,
    pub position_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub team_id: Option<i64>,
    pub company_id: Option<i64>,
    pub search: Option<String>,
    #[serde(default = "default_list_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_list_limit() -> i64 {
    50
}

impl Default for PersonnelListFilter {
    fn default() -> Self {
        Self {
            employment_type: None,
            employment_origin: None,
            employment_status: None,
            availability_status: None,
            position_id: None,
            entity_id: None,
            team_id: None,
            company_id: None,
            search: None,
            limit: default_list_limit(),
            offset: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Personnel code generator — `PER-NNNN`
// ═══════════════════════════════════════════════════════════════════════════════

/// Next code `PER-<device>-NNNN` (`sync::device_code`): the sequence counts only this device's
/// codes, so two devices never hand out the same one. Codes made before device codes existed
/// (`PER-NNNN`) are kept as they are.
///
/// This is a **read only**; pairing it with a separate `INSERT` in two steps can race under
/// concurrent writers. For allocation + insert, pass the inserting [`sea_orm::Transaction`].
pub async fn generate_personnel_code(db: &impl ConnectionTrait) -> AppResult<String> {
    crate::sync::code_clash::PERSONNEL.next_number(db).await
}

/// Inserts a personnel row with an auto-generated `PER-<device>-NNNN` code.
///
/// Uses a single `INSERT … SELECT … RETURNING` statement so allocation is atomic under
/// concurrent writers (unlike calling [`generate_personnel_code`] and inserting in two steps).
pub async fn insert_personnel_with_auto_code(db: &DatabaseConnection, full_name: &str) -> AppResult<String> {
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    let stem = crate::sync::device_code::device_stem(db, &crate::sync::code_clash::PERSONNEL.prefix()).await?;
    let (next, next_values) = crate::sync::device_code::next_number_sql("personnel", "employee_code", &stem, 4);
    let mut values: Vec<sea_orm::Value> = vec![entity_sync_id.into()];
    values.extend(next_values);
    values.push(full_name.into());
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "INSERT INTO personnel (entity_sync_id, employee_code, full_name, employment_type, availability_status, row_version)                  SELECT ?, {next}, ?, 'employee', 'available', 1                  RETURNING employee_code"
            ),
            values,
        ))
        .await?
        .ok_or_else(|| {
            crate::errors::AppError::Internal(anyhow::anyhow!(
                "INSERT personnel with auto code: RETURNING returned no row"
            ))
        })?;
    row.try_get::<String>("", "employee_code")
        .map_err(|e| crate::errors::AppError::Internal(anyhow::anyhow!("employee_code decode: {e}")))
}

#[cfg(test)]
mod masking_tests {
    use super::*;

    #[test]
    fn mask_sensitive_withholds_contact_contract_and_hr_fields_only() {
        let mut p: Personnel = serde_json::from_value(serde_json::json!({
            "id": 7, "employee_code": "PER-0007", "full_name": "A. Tech",
            "employment_type": "employee", "employment_origin": "internal", "employment_status": "active",
            "blocked_override": false, "position_id": null, "primary_entity_id": null,
            "primary_team_id": null, "supervisor_id": null, "home_schedule_reference_value_id": null,
            "availability_status": "available", "hire_date": "2020-01-01", "termination_date": null,
            "email": "a@example.com", "phone": "+213 555", "photo_path": null, "hr_external_id": "HR-1",
            "external_company_id": null, "contract_number": "C-1", "contract_start_date": "2020-01-01",
            "contract_end_date": "2030-01-01", "notes": "private", "row_version": 1,
            "created_at": "2020-01-01", "updated_at": "2020-01-01", "position_name": null,
            "position_code": null, "position_category": null, "entity_name": null, "team_name": "Crew A",
            "supervisor_name": null, "schedule_name": null, "company_name": null
        }))
        .expect("fixture");
        assert!(!p.sensitive_masked, "defaults to unmasked when absent");

        p.mask_sensitive();
        assert!(p.sensitive_masked);
        for field in [&p.email, &p.phone, &p.hr_external_id, &p.contract_number, &p.notes] {
            assert!(field.is_none());
        }
        assert!(p.contract_start_date.is_none() && p.contract_end_date.is_none());
        assert_eq!(p.full_name, "A. Tech", "operational identity stays visible");
        assert_eq!(p.team_name.as_deref(), Some("Crew A"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ConnectionTrait;

    async fn setup_migrated_db() -> DatabaseConnection {
        let db = crate::db::test_db::seeded().await;

        db
    }

    #[test]
    fn employment_type_round_trip() {
        for v in [
            EmploymentType::Employee,
            EmploymentType::Contractor,
            EmploymentType::Temp,
            EmploymentType::Vendor,
        ] {
            let s = v.as_str();
            assert_eq!(EmploymentType::try_from(s).unwrap(), v);
        }
        assert!(EmploymentType::try_from("invalid").is_err());
    }

    #[test]
    fn availability_status_round_trip() {
        for v in [
            AvailabilityStatus::Available,
            AvailabilityStatus::Assigned,
            AvailabilityStatus::InTraining,
            AvailabilityStatus::OnLeave,
            AvailabilityStatus::Blocked,
            AvailabilityStatus::Inactive,
        ] {
            let s = v.as_str();
            assert_eq!(AvailabilityStatus::try_from(s).unwrap(), v);
        }
        assert!(AvailabilityStatus::try_from("vacation").is_err());
    }

    #[test]
    fn position_category_round_trip() {
        for v in [
            PositionCategory::Technician,
            PositionCategory::Supervisor,
            PositionCategory::Engineer,
            PositionCategory::Operator,
            PositionCategory::Contractor,
            PositionCategory::Planner,
            PositionCategory::Storekeeper,
            PositionCategory::Hse,
        ] {
            let s = v.as_str();
            assert_eq!(PositionCategory::try_from(s).unwrap(), v);
        }
    }

    #[test]
    fn authorization_type_round_trip() {
        for v in [
            AuthorizationType::PermitIssuer,
            AuthorizationType::IsolationAuthority,
            AuthorizationType::Inspector,
            AuthorizationType::WarehouseSignoff,
        ] {
            let s = v.as_str();
            assert_eq!(AuthorizationType::try_from(s).unwrap(), v);
        }
    }

    #[test]
    fn onboarding_and_insurance_round_trip() {
        for v in [
            OnboardingStatus::Pending,
            OnboardingStatus::Active,
            OnboardingStatus::Suspended,
            OnboardingStatus::Expired,
        ] {
            assert_eq!(OnboardingStatus::try_from(v.as_str()).unwrap(), v);
        }
        for v in [
            InsuranceStatus::Unknown,
            InsuranceStatus::Valid,
            InsuranceStatus::Expired,
            InsuranceStatus::NotRequired,
        ] {
            assert_eq!(InsuranceStatus::try_from(v.as_str()).unwrap(), v);
        }
    }

    #[tokio::test]
    async fn generate_personnel_code_first_is_per_0001() {
        let db = setup_migrated_db().await;
        let dev = crate::sync::device_code::this_device_code(&db)
            .await
            .expect("device code");
        let code = generate_personnel_code(&db).await.expect("code gen");
        assert_eq!(code, format!("PER-{dev}-0001"));
        let code2 = generate_personnel_code(&db).await.expect("code gen 2");
        assert_eq!(code2, format!("PER-{dev}-0001"), "no PER rows yet; max still 1");
    }

    #[tokio::test]
    async fn generate_personnel_code_increments_after_insert() {
        let db = setup_migrated_db().await;

        let dev = crate::sync::device_code::this_device_code(&db)
            .await
            .expect("device code");
        db.execute_unprepared(&format!(
            "INSERT INTO personnel (entity_sync_id, employee_code, full_name, employment_type, availability_status, row_version) \
             VALUES ('test-personnel-per-0001', 'PER-{dev}-0001', 'Test User', 'employee', 'available', 1)"
        ))
        .await
        .expect("insert personnel");

        let next = generate_personnel_code(&db).await.expect("next code");
        assert_eq!(next, format!("PER-{dev}-0002"));
    }
}
