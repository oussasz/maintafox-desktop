//! Immutable admin audit ledger, support workflow contracts, and hardening checklist (PRD §16).
//!
//! Integrity hashing is a **contract** for append-only storage; VPS must persist and verify server-side.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::vps::sync_rollout_platform_ops::SyncHealthSeverityV1;

/// High-level taxonomy for filtering and compliance exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VendorAdminAuditActionCategoryV1 {
    AuthSession,
    Entitlement,
    Machine,
    SyncRepair,
    RolloutIntervention,
    PlatformOverride,
    SupportIntervention,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuditEntityRefsV1 {
    pub tenant_id: Option<String>,
    pub entitlement_id: Option<String>,
    pub machine_id: Option<String>,
    pub sync_batch_id: Option<String>,
    pub release_id: Option<String>,
    pub incident_id: Option<String>,
    pub support_ticket_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorAdminAuditRecordV1 {
    pub record_id: String,
    pub sequence: u64,
    pub occurred_at_rfc3339: String,
    pub actor_id: String,
    pub action_code: String,
    pub action_category: VendorAdminAuditActionCategoryV1,
    pub correlation_id: String,
    pub scope_tenant_id: Option<String>,
    pub before_snapshot_sha256: Option<String>,
    pub after_snapshot_sha256: Option<String>,
    pub payload_canonical_sha256: String,
    pub chain_prev_hash: Option<String>,
    /// HMAC-like binding: SHA-256 over `audit_record_preimage` (excludes this field).
    pub record_integrity_sha256: String,
    pub reason_code: Option<String>,
    pub approval_correlation_id: Option<String>,
    pub entity_refs: AuditEntityRefsV1,
}

fn opt_str(o: &Option<String>) -> &str {
    o.as_deref().unwrap_or("")
}

/// Canonical string for integrity (versioned). Must match desktop `auditRecordPreimage`.
fn category_str(c: VendorAdminAuditActionCategoryV1) -> &'static str {
    match c {
        VendorAdminAuditActionCategoryV1::AuthSession => "auth_session",
        VendorAdminAuditActionCategoryV1::Entitlement => "entitlement",
        VendorAdminAuditActionCategoryV1::Machine => "machine",
        VendorAdminAuditActionCategoryV1::SyncRepair => "sync_repair",
        VendorAdminAuditActionCategoryV1::RolloutIntervention => "rollout_intervention",
        VendorAdminAuditActionCategoryV1::PlatformOverride => "platform_override",
        VendorAdminAuditActionCategoryV1::SupportIntervention => "support_intervention",
    }
}

pub fn audit_record_preimage(record: &VendorAdminAuditRecordV1) -> String {
    let e = &record.entity_refs;
    format!(
        "audit_v1|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        record.record_id,
        record.sequence,
        record.occurred_at_rfc3339,
        record.actor_id,
        record.action_code,
        category_str(record.action_category),
        record.correlation_id,
        opt_str(&record.scope_tenant_id),
        opt_str(&record.before_snapshot_sha256),
        opt_str(&record.after_snapshot_sha256),
        record.payload_canonical_sha256,
        opt_str(&record.chain_prev_hash),
        opt_str(&record.reason_code),
        opt_str(&record.approval_correlation_id),
        opt_str(&e.tenant_id),
        opt_str(&e.entitlement_id),
        opt_str(&e.machine_id),
        opt_str(&e.sync_batch_id),
        opt_str(&e.release_id),
        opt_str(&e.incident_id),
        opt_str(&e.support_ticket_id),
    )
}

pub fn compute_record_integrity_sha256(record: &VendorAdminAuditRecordV1) -> String {
    let mut h = Sha256::new();
    h.update(audit_record_preimage(record).as_bytes());
    format!("{:x}", h.finalize())
}

pub fn verify_record_integrity(record: &VendorAdminAuditRecordV1) -> bool {
    compute_record_integrity_sha256(record) == record.record_integrity_sha256
}

pub fn verify_audit_chain(records: &[VendorAdminAuditRecordV1]) -> Result<(), &'static str> {
    if records.is_empty() {
        return Ok(());
    }
    for r in records {
        if !verify_record_integrity(r) {
            return Err("integrity_mismatch");
        }
    }
    for w in records.windows(2) {
        if w[1].chain_prev_hash.as_deref() != Some(w[0].record_integrity_sha256.as_str()) {
            return Err("chain_broken");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportTicketStateV1 {
    New,
    Triaged,
    WaitingForVendor,
    WaitingForCustomer,
    Resolved,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportTicketV1 {
    pub ticket_id: String,
    pub tenant_id: String,
    pub state: SupportTicketStateV1,
    pub severity: SyncHealthSeverityV1,
    pub affected_module: String,
    pub sync_status_hint: String,
    pub app_version_reported: String,
    pub linked_incident_ids: Vec<String>,
    pub linked_audit_record_ids: Vec<String>,
    pub sla_due_rfc3339: Option<String>,
    pub created_at_rfc3339: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticBundleManifestV1 {
    pub bundle_id: String,
    pub created_at_rfc3339: String,
    pub redaction_profile: String,
    pub artifacts: Vec<DiagnosticArtifactRefV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticArtifactRefV1 {
    pub logical_path: String,
    pub sha256: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflineTicketReconciliationRowV1 {
    pub desktop_queue_id: String,
    pub vendor_ticket_id: Option<String>,
    pub sync_state: String,
    pub duplicate_of: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentRunbookIdV1 {
    HeartbeatOutage,
    SyncBacklogSurge,
    FailedRollout,
    StoragePressure,
    KeyRotation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentRunbookEntryV1 {
    pub runbook_id: IncidentRunbookIdV1,
    pub title: String,
    pub summary: String,
    pub first_steps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceExportKindV1 {
    EntitlementHistory,
    MachineStateTimeline,
    RolloutActions,
    SupportResolutionChronology,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReadinessCheckItemV1 {
    pub code: String,
    pub description: String,
    pub passed: bool,
}

/// High-blast-radius actions require reason + step-up; dual approval optional per policy.
pub fn privileged_action_guard_ok(
    reason_code: Option<&str>,
    step_up_satisfied: bool,
    dual_approval_present: bool,
    requires_dual_approval: bool,
) -> bool {
    let reason_ok = reason_code.map(|s| s.len() >= 4).unwrap_or(false);
    if !reason_ok || !step_up_satisfied {
        return false;
    }
    if requires_dual_approval && !dual_approval_present {
        return false;
    }
    true
}
