//! Production object-storage layout, backup catalog orchestration, and restore runbooks (PRD §16).
//!
//! VPS-side: map these types to S3-compatible APIs, lifecycle rules, and Postgres catalog tables.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::vps::domain::{VpsContractFamily, VpsTypedError};

// ─── S1: Taxonomy, integrity, retention ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentEnvironment {
    Dev,
    Staging,
    Pilot,
    Production,
}

impl DeploymentEnvironment {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Staging => "staging",
            Self::Pilot => "pilot",
            Self::Production => "prod",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "dev" => Some(Self::Dev),
            "staging" => Some(Self::Staging),
            "pilot" => Some(Self::Pilot),
            "prod" | "production" => Some(Self::Production),
            _ => None,
        }
    }
}

/// Top-level object categories under the org bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectCategory {
    /// Signed updater artifacts and manifests
    UpdaterArtifacts,
    /// PostgreSQL dumps / base backups (control-plane or tenant)
    BackupSnapshots,
    /// Tenant-scoped restore bundles (exports)
    TenantRestoreBundles,
    /// Support exports (bundles, logs metadata)
    SupportEvidence,
}

impl ObjectCategory {
    pub fn prefix_segment(self) -> &'static str {
        match self {
            Self::UpdaterArtifacts => "updates",
            Self::BackupSnapshots => "backups",
            Self::TenantRestoreBundles => "restore-bundles",
            Self::SupportEvidence => "support",
        }
    }
}

/// Retention / lifecycle class drives TTL and storage tier policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageDataClass {
    /// Short-lived rollout payloads; aggressive expiry OK
    RolloutEphemeral,
    /// Medium retention for operational backups
    BackupOperational,
    /// Long retention for compliance / audit
    ComplianceArchive,
    /// Support bundles; bounded retention, PII-aware
    SupportExport,
}

impl StorageDataClass {
    pub fn retention_days(self) -> u32 {
        match self {
            Self::RolloutEphemeral => 30,
            Self::BackupOperational => 90,
            Self::ComplianceArchive => 2555, // ~7 years placeholder; tune per legal
            Self::SupportExport => 14,
        }
    }

    pub fn for_category(category: ObjectCategory) -> Self {
        match category {
            ObjectCategory::UpdaterArtifacts => Self::RolloutEphemeral,
            ObjectCategory::BackupSnapshots => Self::BackupOperational,
            ObjectCategory::TenantRestoreBundles => Self::BackupOperational,
            ObjectCategory::SupportEvidence => Self::SupportExport,
        }
    }
}

/// Reference to a secret (never the raw key) — KMS ARN, vault path, or env ref name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectStorageSecretRef {
    pub ref_kind: String,
    pub ref_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectStorageObjectKey {
    pub full_key: String,
    pub environment: DeploymentEnvironment,
    pub category: ObjectCategory,
    pub data_class: StorageDataClass,
}

fn typed(code: &str, msg: &str) -> VpsTypedError {
    VpsTypedError {
        family: VpsContractFamily::Updates,
        code: code.to_string(),
        message: msg.to_string(),
        http_status: 400,
        retryable: false,
    }
}

/// Build a canonical object key: `{env}/{category}/{tenant|_platform}/...segments`
/// Tenant-scoped keys MUST include normalized tenant id; platform scope uses `_platform`.
pub fn build_object_key(
    env: DeploymentEnvironment,
    category: ObjectCategory,
    tenant_scope: Option<&str>,
    segments: &[&str],
) -> Result<ObjectStorageObjectKey, VpsTypedError> {
    let scope = match tenant_scope {
        None | Some("") => "_platform",
        Some(t) => {
            let n = t.trim();
            if n.is_empty() {
                "_platform"
            } else if !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(typed(
                    "tenant_key_invalid",
                    "Tenant scope for object key must be ascii alphanumeric, '-' or '_'.",
                ));
            } else {
                n
            }
        }
    };

    let mut parts: Vec<String> = vec![
        env.as_str().to_string(),
        category.prefix_segment().to_string(),
        scope.to_string(),
    ];
    for s in segments {
        let seg = s.trim();
        if seg.is_empty() || seg.contains("..") || seg.starts_with('/') {
            return Err(typed("object_segment_invalid", "Invalid path segment."));
        }
        parts.push(seg.to_string());
    }

    let full_key = parts.join("/");
    let data_class = StorageDataClass::for_category(category);
    Ok(ObjectStorageObjectKey {
        full_key,
        environment: env,
        category,
        data_class,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImmutableArtifactMetadata {
    pub content_sha256: String,
    pub manifest_sha256: String,
    pub signed_at_rfc3339: Option<String>,
    pub signer_key_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifestV1 {
    pub manifest_version: u32,
    pub snapshot_id: String,
    pub created_at_rfc3339: String,
    pub environment: String,
    pub payload_sha256: String,
    pub parts: Vec<BackupManifestPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifestPart {
    pub object_key: String,
    pub sha256: String,
    pub byte_length: u64,
}

pub fn canonical_manifest_json(manifest: &BackupManifestV1) -> String {
    serde_json::to_string(manifest).unwrap_or_else(|_| "{}".to_string())
}

pub fn manifest_integrity_hash(manifest: &BackupManifestV1) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_manifest_json(manifest).as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn aggregate_parts_payload_sha256(parts: &[BackupManifestPart]) -> String {
    let parts_json = serde_json::to_string(parts).unwrap_or_else(|_| "[]".to_string());
    let mut hasher = Sha256::new();
    hasher.update(parts_json.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn verify_manifest_against_parts(manifest: &BackupManifestV1) -> Result<(), VpsTypedError> {
    if manifest.manifest_version != 1 {
        return Err(typed("manifest_version_unsupported", "Only manifest v1 is supported."));
    }
    if manifest.snapshot_id.trim().is_empty() {
        return Err(typed("snapshot_id_required", "snapshot_id is required."));
    }
    if manifest.parts.is_empty() {
        return Err(typed("manifest_parts_required", "Manifest must list at least one part."));
    }
    for p in &manifest.parts {
        if p.sha256.len() != 64 || !p.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(typed("part_checksum_invalid", "Each part must have a hex sha256."));
        }
    }
    let expected = aggregate_parts_payload_sha256(&manifest.parts);
    if expected != manifest.payload_sha256 {
        return Err(typed(
            "manifest_payload_mismatch",
            "payload_sha256 does not match canonical aggregate of parts.",
        ));
    }
    Ok(())
}

// ─── S2: Backup orchestration catalog & isolation ──────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupScope {
    ControlPlane,
    TenantMirror { tenant_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupVerifyStatus {
    Pending,
    Verified,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupCatalogRecord {
    pub snapshot_id: String,
    pub scope: BackupScope,
    pub started_at_rfc3339: String,
    pub completed_at_rfc3339: Option<String>,
    pub sha256_manifest: String,
    pub encryption_context: String,
    pub retention_class: StorageDataClass,
    pub verify_status: BackupVerifyStatus,
    pub pitr_wal_archive_ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitRecoveryPolicy {
    pub base_snapshot_required: bool,
    pub wal_archive_required_for_pitr: bool,
    pub max_wal_lag_minutes: u32,
}

impl Default for PitRecoveryPolicy {
    fn default() -> Self {
        Self {
            base_snapshot_required: true,
            wal_archive_required_for_pitr: true,
            max_wal_lag_minutes: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityVerificationJobResult {
    pub snapshot_id: String,
    pub isolated_target_schema: String,
    pub schema_compatible: bool,
    pub manifest_verified: bool,
}

/// Backup windows must not starve sync workers: only run heavy backup outside peak or when queues are healthy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueHealthSnapshot {
    pub max_sync_queue_depth_threshold: i64,
    pub current_max_depth: i64,
}

pub fn backup_allowed_for_queue_health(snap: &QueueHealthSnapshot) -> bool {
    snap.current_max_depth <= snap.max_sync_queue_depth_threshold
}

// ─── S3: Runbooks, drills, post-restore validation ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRunbook {
    pub title: String,
    pub scope: String,
    pub prerequisites: Vec<String>,
    pub rpo_hours: u32,
    pub rto_hours: u32,
    pub steps: Vec<String>,
}

pub fn platform_restore_runbook() -> RestoreRunbook {
    RestoreRunbook {
        title: "Platform (control-plane) restore".to_string(),
        scope: "control_plane_metadata".to_string(),
        prerequisites: vec![
            "Maintenance window announced".to_string(),
            "Latest verified snapshot_id from backup catalog".to_string(),
            "Object storage credentials rotated if expired".to_string(),
        ],
        rpo_hours: 24,
        rto_hours: 4,
        steps: vec![
            "Stop API and workers; drain queues".to_string(),
            "Restore PostgreSQL control-plane schema from snapshot + WAL if PITR".to_string(),
            "Verify migration version compatibility".to_string(),
            "Run post-restore validation checklist".to_string(),
            "Bring API/workers online; smoke-test heartbeat and admin audit read".to_string(),
        ],
    }
}

pub fn tenant_restore_runbook(tenant_id: &str) -> RestoreRunbook {
    RestoreRunbook {
        title: format!("Tenant mirror restore ({tenant_id})"),
        scope: format!("tenant_mirror:{tenant_id}"),
        prerequisites: vec![
            "Tenant id confirmed and access approved".to_string(),
            "Snapshot or restore bundle key scoped to tenant prefix".to_string(),
            "No conflicting sync batches in progress for tenant".to_string(),
        ],
        rpo_hours: 24,
        rto_hours: 8,
        steps: vec![
            "Isolate restore to empty schema tenant_* on staging host".to_string(),
            "Apply snapshot; verify row counts vs manifest".to_string(),
            "Promote or swap only after verification job passes".to_string(),
            "Reconcile sync checkpoint from control plane".to_string(),
        ],
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostRestoreValidationChecklist {
    pub entitlement_heartbeat_ok: bool,
    pub sync_checkpoint_continuous: bool,
    pub admin_audit_read_ok: bool,
    pub update_manifest_integrity_ok: bool,
}

impl PostRestoreValidationChecklist {
    pub fn all_ok(&self) -> bool {
        self.entitlement_heartbeat_ok
            && self.sync_checkpoint_continuous
            && self.admin_audit_read_ok
            && self.update_manifest_integrity_ok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreDrillEvidence {
    pub drill_id: String,
    pub drill_type: String,
    pub started_at_rfc3339: String,
    pub completed_at_rfc3339: String,
    pub seconds_to_restore: i64,
    pub checklist: PostRestoreValidationChecklist,
    pub residual_issues: Vec<String>,
}

pub fn emergency_rollout_deletion_runbook_steps() -> Vec<String> {
    vec![
        "Freeze new rollout publishes".to_string(),
        "Restore manifest index from compliance-archive copy".to_string(),
        "Re-verify signatures against trusted keys".to_string(),
        "Invalidate CDN/edge cache for update paths".to_string(),
    ]
}

pub fn emergency_corrupted_snapshot_runbook_steps() -> Vec<String> {
    vec![
        "Mark snapshot verify_status failed in catalog".to_string(),
        "Select prior verified snapshot_id".to_string(),
        "Run integrity job on isolated target before any promotion".to_string(),
    ]
}

pub fn emergency_expired_credentials_runbook_steps() -> Vec<String> {
    vec![
        "Rotate object storage IAM / API keys via secret ref".to_string(),
        "Update runtime secret injection; restart workers".to_string(),
        "Verify list/get on canary prefix".to_string(),
    ]
}
