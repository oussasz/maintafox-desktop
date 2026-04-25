//! Vendor console: sync health, rollout governance, platform telemetry contracts (PRD §8 / §11).
//!
//! UI validation hints — every mutation must be enforced server-side with RBAC and audit.

use serde::{Deserialize, Serialize};

/// Operational severity for sync health and cross-domain alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncHealthSeverityV1 {
    Info,
    Warn,
    Critical,
}

impl SyncHealthSeverityV1 {
    pub fn rank(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Warn => 1,
            Self::Critical => 2,
        }
    }
}

/// Per-tenant sync posture (aggregate metrics for operator triage).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantSyncHealthRowV1 {
    pub tenant_id: String,
    pub lag_seconds: u64,
    pub checkpoint_age_seconds: u64,
    /// Rejection rate in basis points (100 = 1%).
    pub rejection_rate_bps: u32,
    /// Retry pressure 0–100 (worker + queue composite).
    pub retry_pressure: u8,
    pub dead_letter_count: u32,
    pub severity: SyncHealthSeverityV1,
}

/// Drill-down row for failed / stuck mirror work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFailureDrillDownRowV1 {
    pub batch_id: String,
    pub entity_type: String,
    pub failure_reason_code: String,
    pub idempotency_key: String,
    pub last_attempt_rfc3339: String,
    pub attempt_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairQueueActionV1 {
    Replay,
    Requeue,
    Acknowledge,
    Escalate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairQueueItemV1 {
    pub item_id: String,
    pub tenant_id: String,
    pub queue_kind: String,
    pub severity: SyncHealthSeverityV1,
    pub summary: String,
    pub recommended_action: RepairQueueActionV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatPolicyAnomalyV1 {
    pub tenant_id: String,
    pub machine_id: String,
    pub anomaly_code: String,
    pub detected_at_rfc3339: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutGovernanceStateV1 {
    Active,
    Paused,
    Recalled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CohortRolloutStageV1 {
    pub channel: String,
    pub cohort_label: String,
    pub tenant_count: u32,
    pub machine_count: u32,
    pub governance: RolloutGovernanceStateV1,
    pub paused_at_rfc3339: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolloutImpactPreviewV1 {
    pub release_id: String,
    pub affected_tenants: u32,
    pub affected_machines: u32,
    pub entitlement_channel_ok: bool,
    pub known_blockers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutFailureCategoryV1 {
    Download,
    SignatureVerification,
    Migration,
    PostDeployHeartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolloutDiagnosticsBucketV1 {
    pub category: RolloutFailureCategoryV1,
    pub count_24h: u32,
    pub last_event_rfc3339: Option<String>,
    pub sample_correlation_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformServiceKindV1 {
    Api,
    Workers,
    Postgresql,
    Redis,
    ObjectStorage,
    AdminUi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformServiceStatusV1 {
    pub service: PlatformServiceKindV1,
    pub severity: SyncHealthSeverityV1,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructurePressureV1 {
    pub metric_code: String,
    pub value: f64,
    pub unit: String,
    pub threshold_hint: String,
    pub trend: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpsAlertStateV1 {
    Open,
    Acknowledged,
    Resolved,
}

/// Tenant-safe incident routing: operational IDs only (no PII / free-text customer fields).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentDrillThroughRefsV1 {
    pub tenant_id_hint: Option<String>,
    pub sync_batch_id: Option<String>,
    pub rollout_release_id: Option<String>,
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpsAlertV1 {
    pub alert_id: String,
    pub title: String,
    pub severity: SyncHealthSeverityV1,
    pub state: OpsAlertStateV1,
    pub owner_actor_id: Option<String>,
    pub acknowledged_at_rfc3339: Option<String>,
    pub notes: Vec<String>,
    pub drill_refs: IncidentDrillThroughRefsV1,
}

pub fn repair_action_allowed(item: &RepairQueueItemV1, action: RepairQueueActionV1) -> bool {
    match action {
        RepairQueueActionV1::Escalate => {
            item.severity == SyncHealthSeverityV1::Critical
                || item.severity == SyncHealthSeverityV1::Warn
        }
        RepairQueueActionV1::Replay | RepairQueueActionV1::Requeue => true,
        RepairQueueActionV1::Acknowledge => {
            item.severity != SyncHealthSeverityV1::Critical || item.dead_letter_implied()
        }
    }
}

impl RepairQueueItemV1 {
    fn dead_letter_implied(&self) -> bool {
        self.queue_kind.contains("dead") || self.queue_kind == "dead_letter"
    }
}

/// Recall / emergency containment should require step-up + approval on server.
pub fn rollout_recall_requires_step_up() -> bool {
    true
}

pub fn tenant_safe_drill_through(refs: &IncidentDrillThroughRefsV1) -> bool {
    !forbidden_drill_field_present(refs.tenant_id_hint.as_deref())
        && !forbidden_drill_field_present(refs.sync_batch_id.as_deref())
        && !forbidden_drill_field_present(refs.rollout_release_id.as_deref())
        && !forbidden_drill_field_present(refs.correlation_id.as_deref())
}

fn forbidden_drill_field_present(s: Option<&str>) -> bool {
    let Some(s) = s else {
        return false;
    };
    let lower = s.to_ascii_lowercase();
    lower.contains('@')
        || lower.contains("email")
        || lower.contains("phone")
        || lower.contains("http://")
        || lower.contains("https://")
}

pub fn worst_severity(a: SyncHealthSeverityV1, b: SyncHealthSeverityV1) -> SyncHealthSeverityV1 {
    if a.rank() >= b.rank() {
        a
    } else {
        b
    }
}
