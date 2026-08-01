use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitType {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub name: String,
    pub description: String,
    pub requires_hse_approval: bool,
    pub requires_operations_approval: bool,
    pub requires_atmospheric_test: bool,
    pub max_duration_hours: Option<f64>,
    pub mandatory_ppe_ids_json: String,
    pub mandatory_control_rules_json: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPermit {
    pub id: i64,
    pub entity_sync_id: String,
    pub code: String,
    pub linked_work_order_id: Option<i64>,
    pub permit_type_id: i64,
    pub asset_id: i64,
    pub entity_id: i64,
    pub status: String,
    pub requested_at: Option<String>,
    pub issued_at: Option<String>,
    pub activated_at: Option<String>,
    pub expires_at: Option<String>,
    pub closed_at: Option<String>,
    pub handed_back_at: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitSuspension {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub reason: String,
    pub suspended_by_id: i64,
    pub suspended_at: String,
    pub reinstated_by_id: Option<i64>,
    pub reinstated_at: Option<String>,
    pub reactivation_conditions: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitHandoverLog {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub handed_from_role: String,
    pub handed_to_role: String,
    pub confirmation_note: String,
    pub signed_at: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitIsolation {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub isolation_point: String,
    pub energy_type: String,
    pub isolation_method: String,
    pub lock_number: Option<String>,
    pub applied_by_id: Option<i64>,
    pub verified_by_id: Option<i64>,
    pub applied_at: Option<String>,
    pub verified_at: Option<String>,
    pub removal_verified_at: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkPermitListFilter {
    pub status: Option<String>,
    pub permit_type_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitTypeUpsertInput {
    pub id: Option<i64>,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub requires_hse_approval: bool,
    pub requires_operations_approval: bool,
    pub requires_atmospheric_test: bool,
    pub max_duration_hours: Option<f64>,
    pub mandatory_ppe_ids_json: Option<String>,
    pub mandatory_control_rules_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPermitCreateInput {
    pub permit_type_id: i64,
    pub asset_id: i64,
    pub entity_id: i64,
    pub linked_work_order_id: Option<i64>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPermitUpdateInput {
    pub id: i64,
    pub linked_work_order_id: Option<i64>,
    pub asset_id: Option<i64>,
    pub entity_id: Option<i64>,
    pub expires_at: Option<String>,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPermitStatusInput {
    pub id: i64,
    pub status: String,
    pub expected_row_version: i64,
    pub issued_at: Option<String>,
    pub activated_at: Option<String>,
    pub closed_at: Option<String>,
    pub handed_back_at: Option<String>,
    /// When leaving `revalidation_required` or `suspended` to `active`, closes the open suspension.
    pub reinstated_by_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitSuspendInput {
    pub permit_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
    pub suspended_by_id: i64,
    pub reactivation_conditions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitHandoverLogInput {
    pub permit_id: i64,
    pub handed_from_role: String,
    pub handed_to_role: String,
    pub confirmation_note: String,
    pub signed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitIsolationUpsertInput {
    pub id: Option<i64>,
    pub permit_id: i64,
    pub isolation_point: String,
    pub energy_type: String,
    pub isolation_method: String,
    #[serde(default)]
    pub lock_number: Option<String>,
    pub applied_by_id: Option<i64>,
    pub verified_by_id: Option<i64>,
    pub applied_at: Option<String>,
    pub verified_at: Option<String>,
    pub removal_verified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotoCardPrintJob {
    pub id: i64,
    pub permit_id: i64,
    pub isolation_id: i64,
    pub printed_at: String,
    pub printed_by_id: i64,
    pub entity_sync_id: String,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotoCardView {
    pub permit_code: String,
    pub equipment_label: String,
    pub energy_type: String,
    pub isolation_id: i64,
    pub isolation_point: String,
    pub lock_number: Option<String>,
    pub verifier_signature: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitComplianceKpi30d {
    pub activated_count: i64,
    pub handed_back_on_time_count: i64,
    pub rate: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LotoCardPrintInput {
    pub permit_id: i64,
    pub isolation_id: i64,
    pub printed_by_id: i64,
}
