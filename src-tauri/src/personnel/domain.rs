//! Personnel domain types and code generation (PRD §6.6).

use crate::errors::{AppError, AppResult};
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
    pub created_at: String,
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
    pub position_id: Option<i64>,
    pub primary_entity_id: Option<i64>,
    pub primary_team_id: Option<i64>,
    pub supervisor_id: Option<i64>,
    pub home_schedule_id: Option<i64>,
    pub availability_status: String,
    pub hire_date: Option<String>,
    pub termination_date: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub photo_path: Option<String>,
    pub hr_external_id: Option<String>,
    pub external_company_id: Option<i64>,
    pub notes: Option<String>,
    pub row_version: i64,
    pub created_at: String,
    pub updated_at: String,
    // Display (joins)
    pub position_name: Option<String>,
    pub position_category: Option<String>,
    pub entity_name: Option<String>,
    pub team_name: Option<String>,
    pub supervisor_name: Option<String>,
    pub schedule_name: Option<String>,
    pub company_name: Option<String>,
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
    pub schedule_class_id: i64,
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
    pub employee_code: Option<String>,
    pub employment_type: String,
    pub position_id: Option<i64>,
    pub primary_entity_id: Option<i64>,
    pub primary_team_id: Option<i64>,
    pub supervisor_id: Option<i64>,
    pub home_schedule_id: Option<i64>,
    pub hire_date: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub external_company_id: Option<i64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelUpdateInput {
    pub id: i64,
    pub expected_row_version: i64,
    pub full_name: Option<String>,
    pub employment_type: Option<String>,
    pub position_id: Option<i64>,
    pub primary_entity_id: Option<i64>,
    pub primary_team_id: Option<i64>,
    pub supervisor_id: Option<i64>,
    pub home_schedule_id: Option<i64>,
    pub availability_status: Option<String>,
    pub hire_date: Option<String>,
    pub termination_date: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub external_company_id: Option<i64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonnelListFilter {
    pub employment_type: Option<Vec<String>>,
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

/// Next sequential code for rows with `employee_code` like `PER-%`.
///
/// Uses numeric suffix after the `PER-` prefix (substring from column 5). This is a **read
/// only**; pairing it with a separate `INSERT` in two steps can race under concurrent writers.
/// For allocation + insert inside a transaction, pass a [`sea_orm::Transaction`] as `db`.
pub async fn generate_personnel_code(db: &impl ConnectionTrait) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(CAST(SUBSTR(employee_code, 5) AS INTEGER)), 0) + 1 AS next_seq \
             FROM personnel WHERE employee_code LIKE 'PER-%'"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "Personnel code sequence query returned no rows"
            ))
        })?;

    let next_seq: i64 = row
        .try_get::<i64>("", "next_seq")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Personnel code decode error: {e}")))?;

    Ok(format!("PER-{next_seq:04}"))
}

/// Inserts a personnel row with an auto-generated `PER-NNNN` code.
///
/// Uses a single `INSERT … SELECT … RETURNING` statement so allocation is atomic under
/// concurrent writers (unlike calling [`generate_personnel_code`] and inserting in two steps).
pub async fn insert_personnel_with_auto_code(db: &DatabaseConnection, full_name: &str) -> AppResult<String> {
    let name_esc = full_name.replace('\'', "''");
    let sql = format!(
        "INSERT INTO personnel (employee_code, full_name, employment_type, availability_status, row_version) \
         SELECT \
           'PER-' || printf('%04d', COALESCE((SELECT MAX(CAST(SUBSTR(employee_code, 5) AS INTEGER)) \
             FROM personnel WHERE employee_code LIKE 'PER-%'), 0) + 1), \
           '{name_esc}', 'employee', 'available', 1 \
         RETURNING employee_code"
    );
    let row = db
        .query_one(Statement::from_string(DbBackend::Sqlite, sql))
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!(
                "INSERT personnel with auto code: RETURNING returned no row"
            ))
        })?;
    let code: String = row
        .try_get::<String>("", "employee_code")
        .map_err(|e| AppError::Internal(anyhow::anyhow!("employee_code decode: {e}")))?;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DbBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    async fn setup_migrated_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite should connect");

        db.execute(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_keys = ON;".to_string(),
        ))
        .await
        .expect("PRAGMA foreign_keys");

        crate::migrations::Migrator::up(&db, None)
            .await
            .expect("migrations should apply");

        crate::db::seeder::seed_system_data(&db)
            .await
            .expect("seeder should run");

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
        let code = generate_personnel_code(&db).await.expect("code gen");
        assert_eq!(code, "PER-0001");
        let code2 = generate_personnel_code(&db).await.expect("code gen 2");
        assert_eq!(code2, "PER-0001", "no PER rows yet; max still 1");
    }

    #[tokio::test]
    async fn generate_personnel_code_increments_after_insert() {
        let db = setup_migrated_db().await;

        db.execute_unprepared(
            "INSERT INTO personnel (employee_code, full_name, employment_type, availability_status, row_version) \
             VALUES ('PER-0001', 'Test User', 'employee', 'available', 1)",
        )
        .await
        .expect("insert personnel");

        let next = generate_personnel_code(&db).await.expect("next code");
        assert_eq!(next, "PER-0002");
    }
}
