//! Secure VPS deployment baseline, observability contracts, and DR / failure-injection readiness (PRD section 16).
//!
//! Maps to: Docker Compose topology, edge TLS, internal networks, secret stores, metrics pipelines, and operator runbooks.
//! SSH and cloud credentials are operational secrets — never committed; rotation after exposure is mandatory.

use serde::{Deserialize, Serialize};

use crate::vps::domain::{AuthBoundary, VpsContractFamily, VpsTypedError};
use crate::vps::object_storage::PostRestoreValidationChecklist;

// ─── S1: Topology, network, secrets, sizing, deploy / rollback ─────────────

/// Logical services in the production stack (names align with typical compose services).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComposeServiceRole {
    NginxEdge,
    Api,
    Worker,
    Postgres,
    Redis,
    AdminUi,
    ObjectStorageSidecar, // integration client / gateway, not the cloud itself
    ObservabilityAgent,   // e.g. vector / otel collector
}

impl ComposeServiceRole {
    pub fn compose_service_name(self) -> &'static str {
        match self {
            Self::NginxEdge => "nginx",
            Self::Api => "api",
            Self::Worker => "worker",
            Self::Postgres => "postgres",
            Self::Redis => "redis",
            Self::AdminUi => "admin-ui",
            Self::ObjectStorageSidecar => "object-storage-init",
            Self::ObservabilityAgent => "observability",
        }
    }
}

/// Ordered baseline topology for operator documentation (not an executable compose file).
pub fn production_compose_topology_order() -> Vec<ComposeServiceRole> {
    vec![
        ComposeServiceRole::NginxEdge,
        ComposeServiceRole::Api,
        ComposeServiceRole::Worker,
        ComposeServiceRole::Postgres,
        ComposeServiceRole::Redis,
        ComposeServiceRole::AdminUi,
        ComposeServiceRole::ObjectStorageSidecar,
        ComposeServiceRole::ObservabilityAgent,
    ]
}

/// Split hostnames per `01-service-boundaries` — tenant runtime vs vendor admin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionDnsBoundaries {
    /// Example only; real values come from infra DNS (e.g. `api.example.com` for tenant/runtime).
    pub tenant_runtime_api_hostname_example: String,
    pub vendor_admin_console_hostname_example: String,
}

impl Default for ProductionDnsBoundaries {
    fn default() -> Self {
        Self {
            tenant_runtime_api_hostname_example: "api.maintafox.systems".to_string(),
            vendor_admin_console_hostname_example: "console.maintafox.systems".to_string(),
        }
    }
}

impl ProductionDnsBoundaries {
    pub fn validate_split(&self) -> Result<(), VpsTypedError> {
        if self.tenant_runtime_api_hostname_example.trim().is_empty()
            || self.vendor_admin_console_hostname_example.trim().is_empty()
        {
            return Err(ops_typed(
                "dns_hostname_required",
                "Tenant runtime and vendor admin hostnames must both be set.",
            ));
        }
        if self.tenant_runtime_api_hostname_example == self.vendor_admin_console_hostname_example {
            return Err(ops_typed(
                "dns_split_required",
                "Tenant runtime API and vendor admin must use different hostnames for auth boundary isolation.",
            ));
        }
        Ok(())
    }

    pub fn boundary_for_hostname(&self, host: &str) -> Option<AuthBoundary> {
        let h = host.trim().to_ascii_lowercase();
        if h == self.tenant_runtime_api_hostname_example.to_ascii_lowercase() {
            Some(AuthBoundary::TenantRuntime)
        } else if h == self.vendor_admin_console_hostname_example.to_ascii_lowercase() {
            Some(AuthBoundary::VendorAdmin)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkExposureTier {
    /// Internet-facing HTTPS only (reverse proxy).
    PublicHttpsEdge,
    /// Internal bridge / overlay — api, worker, db, redis.
    InternalServices,
    /// Bastion or break-glass only; no app traffic.
    OpsRestrictedSsh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSegmentRule {
    pub from_tier: NetworkExposureTier,
    pub to_tier: NetworkExposureTier,
    pub allowed: bool,
    pub note: String,
}

/// Hardened baseline: only edge is public; DB/Redis never exposed publicly.
pub fn hardened_network_segment_rules() -> Vec<NetworkSegmentRule> {
    vec![
        NetworkSegmentRule {
            from_tier: NetworkExposureTier::PublicHttpsEdge,
            to_tier: NetworkExposureTier::InternalServices,
            allowed: true,
            note: "TLS-terminated traffic to api/admin upstreams only.".to_string(),
        },
        NetworkSegmentRule {
            from_tier: NetworkExposureTier::InternalServices,
            to_tier: NetworkExposureTier::PublicHttpsEdge,
            allowed: true,
            note: "Outbound webhooks only via allow-listed egress if required.".to_string(),
        },
        NetworkSegmentRule {
            from_tier: NetworkExposureTier::OpsRestrictedSsh,
            to_tier: NetworkExposureTier::InternalServices,
            allowed: true,
            note: "SSH via bastion or provider serial console; MFA enforced.".to_string(),
        },
        NetworkSegmentRule {
            from_tier: NetworkExposureTier::PublicHttpsEdge,
            to_tier: NetworkExposureTier::OpsRestrictedSsh,
            allowed: false,
            note: "No direct SSH from the internet.".to_string(),
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecretInjectionStrategy {
    /// Named env var holds a *reference* (path/ARN), not the secret value in compose.
    EnvironmentReference,
    RuntimeSecretStore,
    KmsEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretHandlingContract {
    pub strategy: SecretInjectionStrategy,
    /// e.g. `VAULT_PATH`, `AWS_SECRETS_ARN` — never inline PEM/key material in repo.
    pub reference_var_names: Vec<String>,
    pub forbid_plaintext_in_compose: bool,
    pub rotate_after_exposure: bool,
}

impl Default for SecretHandlingContract {
    fn default() -> Self {
        Self {
            strategy: SecretInjectionStrategy::RuntimeSecretStore,
            reference_var_names: vec![
                "DATABASE_URL_REF".to_string(),
                "REDIS_URL_REF".to_string(),
                "OBJECT_STORAGE_SECRET_REF".to_string(),
                "SIGNING_KEY_REF".to_string(),
            ],
            forbid_plaintext_in_compose: true,
            rotate_after_exposure: true,
        }
    }
}

/// PRD §16.1-aligned sizing hints (operators tune per load tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentSizingProfile {
    Pilot,
    SharedProduction,
    Growth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSizingHints {
    pub api_replicas: u32,
    pub worker_replicas: u32,
    pub postgres_cpu_units: u32,
    pub postgres_memory_gb: u32,
    pub redis_memory_gb: u32,
    pub recommended_connection_pool_api: u32,
}

pub fn sizing_hints(profile: DeploymentSizingProfile) -> ResourceSizingHints {
    match profile {
        DeploymentSizingProfile::Pilot => ResourceSizingHints {
            api_replicas: 1,
            worker_replicas: 1,
            postgres_cpu_units: 2,
            postgres_memory_gb: 4,
            redis_memory_gb: 1,
            recommended_connection_pool_api: 10,
        },
        DeploymentSizingProfile::SharedProduction => ResourceSizingHints {
            api_replicas: 2,
            worker_replicas: 2,
            postgres_cpu_units: 4,
            postgres_memory_gb: 16,
            redis_memory_gb: 4,
            recommended_connection_pool_api: 40,
        },
        DeploymentSizingProfile::Growth => ResourceSizingHints {
            api_replicas: 4,
            worker_replicas: 6,
            postgres_cpu_units: 8,
            postgres_memory_gb: 32,
            redis_memory_gb: 8,
            recommended_connection_pool_api: 100,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeployPreflightKind {
    DbMigrationReadiness,
    QueueDrainPolicy,
    ArtifactIntegrityVerified,
    SecretRefsResolvable,
    SloBaselineHealthy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployPreflightItem {
    pub kind: DeployPreflightKind,
    pub description: String,
    pub blocking: bool,
}

pub fn deploy_preflight_checklist() -> Vec<DeployPreflightItem> {
    vec![
        DeployPreflightItem {
            kind: DeployPreflightKind::DbMigrationReadiness,
            description: "Forward migrations applied or backward-compatible; rollback migration plan documented."
                .to_string(),
            blocking: true,
        },
        DeployPreflightItem {
            kind: DeployPreflightKind::QueueDrainPolicy,
            description: "Workers finish in-flight idempotent jobs or checkpoint; surge policy avoids unbounded backlog."
                .to_string(),
            blocking: true,
        },
        DeployPreflightItem {
            kind: DeployPreflightKind::ArtifactIntegrityVerified,
            description: "Container images and update manifests match signed digests / manifest hashes."
                .to_string(),
            blocking: true,
        },
        DeployPreflightItem {
            kind: DeployPreflightKind::SecretRefsResolvable,
            description: "Runtime can resolve all secret refs; no plaintext keys in compose files on disk."
                .to_string(),
            blocking: true,
        },
        DeployPreflightItem {
            kind: DeployPreflightKind::SloBaselineHealthy,
            description: "Control-plane SLO signals green or explicitly waived for maintenance."
                .to_string(),
            blocking: false,
        },
    ]
}

pub fn safe_deploy_workflow_steps() -> Vec<String> {
    vec![
        "Run preflight checklist; abort on any blocking failure.".to_string(),
        "Deploy canary API (single replica) behind edge; verify health checks.".to_string(),
        "Roll remaining API replicas; observe error budget.".to_string(),
        "Deploy workers with compatible schema; verify queue depth and DLQ.".to_string(),
        "Enable admin-ui static deploy if changed; verify vendor boundary cookies.".to_string(),
    ]
}

pub fn rollback_workflow_steps() -> Vec<String> {
    vec![
        "Stop new job intake at edge (503 maintenance or feature flag).".to_string(),
        "Revert API/worker images to last known-good digests.".to_string(),
        "Run backward-compatible DB downgrade only if planned; else restore from snapshot.".to_string(),
        "Re-verify heartbeat and sync smoke tests before traffic restore.".to_string(),
    ]
}

// ─── S2: Structured logs, SLOs, tenant health, alerting ─────────────────────

/// Required fields for cross-service traceability (API, worker, admin).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredLogContractV1 {
    pub schema_version: u32,
    pub required_fields: Vec<String>,
}

pub fn structured_log_contract_v1() -> StructuredLogContractV1 {
    StructuredLogContractV1 {
        schema_version: 1,
        required_fields: vec![
            "timestamp_rfc3339".to_string(),
            "correlation_id".to_string(),
            "service".to_string(),
            "environment".to_string(),
            "contract_family".to_string(),
            "auth_boundary".to_string(),
            "tenant_id".to_string(), // use sentinel "platform" when N/A
            "severity".to_string(),
            "message_code".to_string(),
        ],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlPlaneSlo {
    pub slo_id: String,
    pub description: String,
    /// Target availability or success ratio in (0,1], e.g. 0.999
    pub target_ratio: f64,
    pub window_days: u32,
    pub metric_keys: Vec<String>,
}

pub fn default_control_plane_slos() -> Vec<ControlPlaneSlo> {
    vec![
        ControlPlaneSlo {
            slo_id: "license_heartbeat_availability".to_string(),
            description: "License heartbeat endpoint success (non-5xx) for authenticated clients."
                .to_string(),
            target_ratio: 0.999,
            window_days: 30,
            metric_keys: vec![
                "vps_heartbeat_requests_total".to_string(),
                "vps_heartbeat_success_total".to_string(),
            ],
        },
        ControlPlaneSlo {
            slo_id: "sync_throughput".to_string(),
            description: "Sync push acknowledged within lag budget under nominal load.".to_string(),
            target_ratio: 0.99,
            window_days: 7,
            metric_keys: vec![
                "vps_sync_queue_lag_ms_p95".to_string(),
                "vps_sync_push_latency_ms_p95".to_string(),
            ],
        },
        ControlPlaneSlo {
            slo_id: "rollout_service_health".to_string(),
            description: "Update manifest fetch and signature verification success.".to_string(),
            target_ratio: 0.995,
            window_days: 30,
            metric_keys: vec![
                "vps_update_manifest_fetch_success".to_string(),
                "vps_update_download_failures_total".to_string(),
            ],
        },
        ControlPlaneSlo {
            slo_id: "storage_pressure".to_string(),
            description: "Postgres and object-storage free capacity above critical thresholds.".to_string(),
            target_ratio: 1.0,
            window_days: 1,
            metric_keys: vec![
                "postgres_disk_used_ratio".to_string(),
                "object_store_bucket_free_ratio".to_string(),
            ],
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SloAlertThreshold {
    pub slo_id: String,
    /// Burn rate or breach multiplier vs error budget (operator-defined scale).
    pub alert_window_minutes: u32,
    pub threshold_description: String,
}

pub fn default_slo_alert_thresholds() -> Vec<SloAlertThreshold> {
    default_control_plane_slos()
        .into_iter()
        .map(|s| SloAlertThreshold {
            slo_id: s.slo_id.clone(),
            alert_window_minutes: 60,
            threshold_description: format!(
                "Page when {} error budget burn exceeds 2x expected in {}m window.",
                s.slo_id, 60
            ),
        })
        .collect()
}

/// Tenant-scoped rollup for support — aggregates only; no cross-tenant payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantHealthIndicators {
    pub tenant_id: String,
    pub heartbeat_success_ratio_24h: f64,
    pub sync_queue_lag_ms_p95: u64,
    pub rollout_download_failure_count_24h: u32,
    pub worker_retry_count_24h: u32,
    pub worker_dead_letter_count_24h: u32,
    pub degraded: bool,
}

impl TenantHealthIndicators {
    pub fn evaluate_degraded(&mut self) {
        self.degraded = self.heartbeat_success_ratio_24h < 0.95
            || self.sync_queue_lag_ms_p95 > 300_000
            || self.rollout_download_failure_count_24h > 10
            || self.worker_dead_letter_count_24h > 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentSeverity {
    /// Full platform or many tenants
    Sev1,
    /// Single critical tenant or partial outage
    Sev2,
    /// Degraded SLO, workaround exists
    Sev3,
    /// Low impact, informational
    Sev4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnCallRoutingContract {
    pub severity_map: Vec<(IncidentSeverity, String)>,
    pub ack_required_within_minutes: u32,
    pub escalation_note: String,
}

pub fn default_on_call_routing_contract() -> OnCallRoutingContract {
    OnCallRoutingContract {
        severity_map: vec![
            (
                IncidentSeverity::Sev1,
                "Page primary + secondary; exec bridge within 15m.".to_string(),
            ),
            (
                IncidentSeverity::Sev2,
                "Page primary; customer comms template if tenant-specific.".to_string(),
            ),
            (
                IncidentSeverity::Sev3,
                "Ticket + Slack; next business day review if unresolved.".to_string(),
            ),
            (
                IncidentSeverity::Sev4,
                "Log and batch into weekly review.".to_string(),
            ),
        ],
        ack_required_within_minutes: 30,
        escalation_note: "Unacked Sev1/Sev2 pages escalate per vendor policy.".to_string(),
    }
}

// ─── S3: DR drills, failure injection, keys/certs runbooks, readiness ────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryDrillCategory {
    ControlPlaneMetadataRestore,
    TenantMirrorRestore,
    UpdateArtifactRecovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryValidationEvidence {
    pub drill_id: String,
    pub category: RecoveryDrillCategory,
    pub completed_at_rfc3339: String,
    pub post_restore: PostRestoreValidationChecklist,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureInjectionScenario {
    pub scenario_id: String,
    pub title: String,
    pub operator_response_steps: Vec<String>,
}

pub fn failure_injection_scenarios() -> Vec<FailureInjectionScenario> {
    vec![
        FailureInjectionScenario {
            scenario_id: "worker_outage".to_string(),
            title: "All worker processes stopped or crash-looping".to_string(),
            operator_response_steps: vec![
                "Confirm API still accepts heartbeats; surface maintenance banner if needed.".to_string(),
                "Inspect queue depth and DLQ; scale workers or roll back bad release.".to_string(),
                "Replay safe idempotent jobs after root cause fix.".to_string(),
            ],
        },
        FailureInjectionScenario {
            scenario_id: "redis_pressure".to_string(),
            title: "Redis memory pressure or latency spike".to_string(),
            operator_response_steps: vec![
                "Check eviction policy and maxmemory; add shard or vertical scale.".to_string(),
                "Throttle non-critical consumers; preserve heartbeat/session paths.".to_string(),
            ],
        },
        FailureInjectionScenario {
            scenario_id: "postgres_failover".to_string(),
            title: "PostgreSQL primary failover or read replica lag".to_string(),
            operator_response_steps: vec![
                "Verify connection string targets new primary; drain long transactions.".to_string(),
                "Run migration compatibility check; validate RTO/RPO vs catalog.".to_string(),
            ],
        },
        FailureInjectionScenario {
            scenario_id: "object_store_unavailable".to_string(),
            title: "Object storage list/get errors or credential expiry".to_string(),
            operator_response_steps: vec![
                "Follow expired-credentials emergency steps; verify IAM and bucket policy.".to_string(),
                "Serve cached manifests at edge only if integrity verified; disable new publishes if unsafe.".to_string(),
            ],
        },
    ]
}

pub fn tls_edge_runbook_steps() -> Vec<String> {
    vec![
        "Terminate TLS at nginx; obtain certs via ACME (e.g. Let's Encrypt) or managed CA.".to_string(),
        "Enable HTTP→HTTPS redirect; set HSTS max-age consistent with rollback plan.".to_string(),
        "Publish CAA DNS records restricting issuance to approved CAs.".to_string(),
        "Document renewal job (cron or provider); alert 14 days before expiry.".to_string(),
    ]
}

pub fn certificate_renewal_runbook_steps() -> Vec<String> {
    vec![
        "Verify ACME client health and challenge path from internet.".to_string(),
        "Renew staging cert first; reload nginx with zero-downtime reload.".to_string(),
        "Validate chain in browser and openssl s_client; rollback nginx config if handshake fails.".to_string(),
    ]
}

pub fn signing_key_rotation_runbook_steps() -> Vec<String> {
    vec![
        "Generate new key in HSM or vault; keep old key active for dual-sign period.".to_string(),
        "Publish manifests signed with both keys; monitor client verification metrics.".to_string(),
        "Retire old key only after error rate stable and minimum TTL elapsed.".to_string(),
    ]
}

pub fn emergency_access_hardening_runbook_steps() -> Vec<String> {
    vec![
        "Revoke break-glass credentials after incident; rotate SSH host keys if compromised.".to_string(),
        "Audit admin and cloud audit logs for session replay; force re-auth for vendor admins.".to_string(),
        "Patch edge and internal packages; verify network segment rules still enforced.".to_string(),
    ]
}

pub fn incident_response_runbook_outline() -> Vec<String> {
    vec![
        "Declare severity using customer impact matrix; open incident channel.".to_string(),
        "Preserve correlation IDs and logs; freeze destructive changes until triage.".to_string(),
        "Coordinate with DR runbooks if data loss suspected; notify legal/comms per policy.".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentReadinessChecklist {
    pub dr_drills_evidence_present: bool,
    pub failure_scenarios_documented: bool,
    pub post_restore_entitlement_sync_validated: bool,
    pub cert_and_key_runbooks_acknowledged: bool,
    pub all_items_ok: bool,
}

impl DeploymentReadinessChecklist {
    pub fn recompute(&mut self) {
        self.all_items_ok = self.dr_drills_evidence_present
            && self.failure_scenarios_documented
            && self.post_restore_entitlement_sync_validated
            && self.cert_and_key_runbooks_acknowledged;
    }
}

fn ops_typed(code: &str, msg: &str) -> VpsTypedError {
    VpsTypedError {
        family: VpsContractFamily::Admin,
        code: code.to_string(),
        message: msg.to_string(),
        http_status: 400,
        retryable: false,
    }
}
