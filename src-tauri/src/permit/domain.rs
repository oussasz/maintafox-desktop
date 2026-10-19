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
    /// A permit of this type needs at least one verified energy isolation before it starts
    /// (LOTO; migration 225, formerly a rule on the code `loto`).
    #[serde(default)]
    pub requires_energy_isolation: bool,
    /// Three-person LOTO chain (migration 236, off by default): the person who verifies a lock
    /// removal must differ from the isolation's applier (always) and from its verifier.
    #[serde(default)]
    pub requires_three_person_removal: bool,
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
    /// Display labels resolved by JOIN (never render the FKs above in the UI).
    #[serde(default)]
    pub permit_type_code: Option<String>,
    #[serde(default)]
    pub permit_type_name: Option<String>,
    #[serde(default)]
    pub asset_code: Option<String>,
    #[serde(default)]
    pub asset_label: Option<String>,
    #[serde(default)]
    pub linked_work_order_code: Option<String>,
    /// The type's `requires_energy_isolation` (the start gate needs an isolation).
    #[serde(default)]
    pub permit_type_requires_energy_isolation: bool,
    /// The type's `requires_three_person_removal` (lock removal verified by a third person).
    #[serde(default)]
    pub permit_type_requires_three_person_removal: bool,
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
    #[serde(default)]
    pub suspended_by_name: Option<String>,
    #[serde(default)]
    pub reinstated_by_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitHandoverLog {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub handed_from_role: String,
    pub handed_to_role: String,
    /// Labels of the two roles in the plant's list (`PERMIT.HANDOVER_ROLES`), looked up over every
    /// published version of the list, roles since deactivated included; `None` when the stored
    /// code is in no version of the list.
    #[serde(default)]
    pub handed_from_label: Option<String>,
    #[serde(default)]
    pub handed_to_label: Option<String>,
    pub confirmation_note: String,
    pub signed_at: String,
    /// Who recorded the hand-over (the session user; NULL on hand-overs recorded before 235).
    #[serde(default)]
    pub signed_by_id: Option<i64>,
    #[serde(default)]
    pub signed_by_name: Option<String>,
    /// The receiving person, when named.
    #[serde(default)]
    pub received_by_id: Option<i64>,
    #[serde(default)]
    pub received_by_name: Option<String>,
    pub row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitIsolation {
    pub id: i64,
    pub entity_sync_id: String,
    pub permit_id: i64,
    pub isolation_point: String,
    /// Text typed before migration 225, kept as read-only history ("Saisie d'origine"); empty
    /// for isolations recorded on the lists.
    pub energy_type: String,
    /// Text typed before migration 225 (history), empty otherwise.
    pub isolation_method: String,
    /// `PERMIT.ENERGY_TYPES` value (SYSTEM list).
    #[serde(default)]
    pub energy_type_id: Option<i64>,
    #[serde(default)]
    pub energy_type_code: Option<String>,
    #[serde(default)]
    pub energy_type_label: Option<String>,
    /// `PERMIT.ISOLATION_METHODS` value (COMPANY list).
    #[serde(default)]
    pub isolation_method_id: Option<i64>,
    #[serde(default)]
    pub isolation_method_label: Option<String>,
    /// A migrated text could not be mapped onto the lists: a person must choose.
    #[serde(default)]
    pub mapping_review_required: bool,
    pub lock_number: Option<String>,
    pub applied_by_id: Option<i64>,
    pub verified_by_id: Option<i64>,
    pub applied_at: Option<String>,
    pub verified_at: Option<String>,
    pub removal_verified_at: Option<String>,
    pub removal_verified_by_id: Option<i64>,
    pub row_version: i64,
    #[serde(default)]
    pub applied_by_name: Option<String>,
    #[serde(default)]
    pub verified_by_name: Option<String>,
    #[serde(default)]
    pub removal_verified_by_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkPermitListFilter {
    pub status: Option<String>,
    pub permit_type_id: Option<i64>,
    pub asset_id: Option<i64>,
    #[serde(default)]
    pub linked_work_order_id: Option<i64>,
    /// Code, type, equipment or OT text (server-side search over every permit, not one page).
    #[serde(default)]
    pub search: Option<String>,
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
    /// Omitted on update: unchanged. Omitted on create: false.
    #[serde(default)]
    pub requires_energy_isolation: Option<bool>,
    /// Required (with `adm.settings` + step-up) when switching the energy-isolation rule off.
    #[serde(default)]
    pub disable_isolation_reason: Option<String>,
    /// Three-person lock-removal chain. Omitted on update: unchanged. Omitted on create: false.
    #[serde(default)]
    pub requires_three_person_removal: Option<bool>,
    /// Required (with `adm.settings` + step-up) whenever the three-person setting changes,
    /// including creating a type with it on.
    #[serde(default)]
    pub three_person_change_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkPermitCreateInput {
    pub permit_type_id: i64,
    /// Required unless `linked_work_order_id` is set (then the WO equipment is used).
    #[serde(default)]
    pub asset_id: Option<i64>,
    /// Defaults to the linked WO entity, then to the asset's installation node.
    #[serde(default)]
    pub entity_id: Option<i64>,
    pub linked_work_order_id: Option<i64>,
    /// RFC 3339; must be in the future.
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
/// Lifecycle timestamps are stamped by the server; the actor (e.g. the person
/// reinstating a suspended permit) is always the session user.
pub struct WorkPermitStatusInput {
    pub id: i64,
    pub status: String,
    pub expected_row_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitSuspendInput {
    pub permit_id: i64,
    pub expected_row_version: i64,
    pub reason: String,
    pub reactivation_conditions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitHandoverLogInput {
    pub permit_id: i64,
    /// Codes of `PERMIT.HANDOVER_ROLES` (Données de référence).
    pub handed_from_role: String,
    pub handed_to_role: String,
    /// At least `MIN_HANDOVER_NOTE_CHARS` characters.
    pub confirmation_note: String,
    /// The receiving person (a user account), optional. The signer is never sent: the server takes
    /// the session user.
    #[serde(default)]
    pub received_by_id: Option<i64>,
}

/// Shortest confirmation note a hand-over accepts.
pub const MIN_HANDOVER_NOTE_CHARS: usize = 10;

/// Defines (or edits, before application) an isolation point. Application,
/// verification and removal verification go through dedicated actions so the
/// actor is always the session user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitIsolationUpsertInput {
    pub id: Option<i64>,
    pub permit_id: i64,
    pub isolation_point: String,
    /// Active `PERMIT.ENERGY_TYPES` value (required).
    #[serde(default)]
    pub energy_type_id: Option<i64>,
    /// Active `PERMIT.ISOLATION_METHODS` value (required).
    #[serde(default)]
    pub isolation_method_id: Option<i64>,
    #[serde(default)]
    pub lock_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermitIsolationActionInput {
    pub isolation_id: i64,
    pub expected_row_version: i64,
    /// Only for `apply`: lock number fitted at application time.
    #[serde(default)]
    pub lock_number: Option<String>,
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
}
