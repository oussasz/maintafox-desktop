use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::vps::domain::{VpsContractFamily, VpsTypedError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MirrorQueueKind {
    PushIngestion,
    PullMaterialization,
    RestorePreparation,
    ReplayRepair,
    ConflictReview,
    DeadLetter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MirrorRecordClass {
    AppendOnlyEvent,
    GovernedSnapshot,
    MutableOperational,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantProvisioningPlan {
    pub tenant_id: String,
    pub schema_name: String,
    pub baseline_migrations: Vec<String>,
    pub required_invariants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorSchemaEvolutionStep {
    pub migration_id: String,
    pub additive_only: bool,
    pub backward_reader_compatible: bool,
    pub rollout_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorQueueJob {
    pub job_id: String,
    pub tenant_id: String,
    pub target_schema: String,
    pub queue_kind: MirrorQueueKind,
    pub idempotency_key: String,
    pub checkpoint_token: Option<String>,
    pub record_class: MirrorRecordClass,
    pub merge_key: String,
    pub local_row_version: i64,
    pub incoming_row_version: i64,
    pub attempt: i32,
    pub injected_failure_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorWorkerConfig {
    pub max_batch_size: usize,
    pub max_retry_attempts: i32,
    pub tenant_fairness_quantum: usize,
    pub tenant_lag_alert_threshold: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MirrorWorkerMetrics {
    pub queue_depth_by_kind: BTreeMap<String, i64>,
    pub retry_count: i64,
    pub dead_letter_count: i64,
    pub conflict_queue_count: i64,
    pub per_tenant_processed: BTreeMap<String, i64>,
    pub per_tenant_lag: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MirrorProcessingReport {
    pub processed_count: i64,
    pub deduped_count: i64,
    pub conflict_routed_count: i64,
    pub retried_count: i64,
    pub dead_lettered_count: i64,
    pub stale_checkpoint_blocked_count: i64,
    pub tenant_isolation_violations: i64,
}

#[derive(Debug, Default)]
pub struct MirrorWorkerState {
    pub pending: VecDeque<MirrorQueueJob>,
    pub conflict_queue: VecDeque<MirrorQueueJob>,
    pub dead_letter_queue: VecDeque<MirrorQueueJob>,
    seen_idempotency: HashSet<(String, String)>,
    tenant_checkpoint: HashMap<String, String>,
    pub metrics: MirrorWorkerMetrics,
}

fn typed_error(code: &str, message: &str) -> VpsTypedError {
    VpsTypedError {
        family: VpsContractFamily::Sync,
        code: code.to_string(),
        message: message.to_string(),
        http_status: 400,
        retryable: false,
    }
}

fn parse_checkpoint_sequence(token: &str) -> Option<i64> {
    token.rsplit('-').next()?.parse::<i64>().ok()
}

fn is_stale_checkpoint(current: &str, incoming: &str) -> bool {
    match (parse_checkpoint_sequence(current), parse_checkpoint_sequence(incoming)) {
        (Some(cur), Some(inc)) => inc < cur,
        _ => false,
    }
}

fn queue_kind_name(kind: &MirrorQueueKind) -> &'static str {
    match kind {
        MirrorQueueKind::PushIngestion => "push_ingestion",
        MirrorQueueKind::PullMaterialization => "pull_materialization",
        MirrorQueueKind::RestorePreparation => "restore_preparation",
        MirrorQueueKind::ReplayRepair => "replay_repair",
        MirrorQueueKind::ConflictReview => "conflict_review",
        MirrorQueueKind::DeadLetter => "dead_letter",
    }
}

pub fn control_plane_tables() -> Vec<&'static str> {
    vec![
        "cp_tenants",
        "cp_tenant_lifecycle",
        "cp_sync_checkpoint_state",
        "cp_sync_idempotency_keys",
        "cp_sync_replay_runs",
        "cp_mirror_audit_traces",
        "cp_worker_queue_metrics",
        "cp_dead_letter_records",
    ]
}

pub fn mirror_table_blueprint() -> Vec<&'static str> {
    vec![
        "mirror_records(sync_id, row_version, origin_machine_id, checkpoint_token, payload_hash)",
        "mirror_change_log(sync_id, operation, row_version, applied_at, idempotency_key)",
        "mirror_conflicts(conflict_key, local_payload_hash, remote_payload_hash, authority_side, checkpoint_token)",
    ]
}

pub fn queue_topology() -> Vec<MirrorQueueKind> {
    vec![
        MirrorQueueKind::PushIngestion,
        MirrorQueueKind::PullMaterialization,
        MirrorQueueKind::RestorePreparation,
        MirrorQueueKind::ReplayRepair,
        MirrorQueueKind::ConflictReview,
        MirrorQueueKind::DeadLetter,
    ]
}

pub fn mirror_schema_evolution_path() -> Vec<MirrorSchemaEvolutionStep> {
    vec![
        MirrorSchemaEvolutionStep {
            migration_id: "vps_0001_baseline".to_string(),
            additive_only: true,
            backward_reader_compatible: true,
            rollout_note: "Create control-plane tables and baseline mirror record table.".to_string(),
        },
        MirrorSchemaEvolutionStep {
            migration_id: "vps_0002_add_conflict_indexes".to_string(),
            additive_only: true,
            backward_reader_compatible: true,
            rollout_note: "Add indexes and conflict diagnostics without dropping columns.".to_string(),
        },
        MirrorSchemaEvolutionStep {
            migration_id: "vps_0003_expand_policy_metadata".to_string(),
            additive_only: true,
            backward_reader_compatible: true,
            rollout_note: "Add nullable policy metadata columns and preserve previous readers.".to_string(),
        },
    ]
}

pub fn tenant_schema_name(tenant_id: &str) -> Result<String, VpsTypedError> {
    let normalized = tenant_id.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(typed_error("tenant_id_required", "tenant_id is required."));
    }
    let is_valid = normalized
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !is_valid {
        return Err(typed_error(
            "tenant_id_invalid",
            "tenant_id must contain only ascii alphanumeric, '-' or '_'.",
        ));
    }
    Ok(format!("tenant_{}", normalized.replace('-', "_")))
}

pub fn build_tenant_provisioning_plan(tenant_id: &str) -> Result<TenantProvisioningPlan, VpsTypedError> {
    let schema_name = tenant_schema_name(tenant_id)?;
    Ok(TenantProvisioningPlan {
        tenant_id: tenant_id.to_string(),
        schema_name,
        baseline_migrations: mirror_schema_evolution_path()
            .into_iter()
            .map(|step| step.migration_id)
            .collect(),
        required_invariants: vec![
            "search_path_scoped_to_tenant_schema".to_string(),
            "control_plane_tables_exist".to_string(),
            "mirror_baseline_tables_exist".to_string(),
            "idempotency_unique_key_enabled".to_string(),
        ],
    })
}

pub fn validate_tenant_provisioning(
    plan: &TenantProvisioningPlan,
    applied_migrations: &[String],
    observed_invariants: &[String],
) -> Result<(), VpsTypedError> {
    let applied_set: HashSet<&str> = applied_migrations.iter().map(String::as_str).collect();
    for migration in &plan.baseline_migrations {
        if !applied_set.contains(migration.as_str()) {
            return Err(typed_error(
                "tenant_provisioning_incomplete",
                "Tenant schema is missing baseline migration.",
            ));
        }
    }
    let invariant_set: HashSet<&str> = observed_invariants.iter().map(String::as_str).collect();
    for invariant in &plan.required_invariants {
        if !invariant_set.contains(invariant.as_str()) {
            return Err(typed_error(
                "tenant_invariant_failed",
                "Tenant lifecycle invariant failed.",
            ));
        }
    }
    Ok(())
}

pub fn enqueue_worker_job(
    state: &mut MirrorWorkerState,
    job: MirrorQueueJob,
    cfg: &MirrorWorkerConfig,
) -> Result<(), VpsTypedError> {
    if job.idempotency_key.trim().is_empty() {
        return Err(typed_error(
            "idempotency_key_required",
            "Worker queue requires idempotency key.",
        ));
    }
    if job.attempt < 0 || job.attempt > cfg.max_retry_attempts {
        return Err(typed_error(
            "attempt_out_of_bounds",
            "Worker attempt exceeds configured bounds.",
        ));
    }
    state.pending.push_back(job);
    Ok(())
}

fn update_metrics_from_state(state: &mut MirrorWorkerState) {
    let mut depth = BTreeMap::new();
    for kind in queue_topology() {
        let name = queue_kind_name(&kind).to_string();
        let pending_count = state
            .pending
            .iter()
            .filter(|job| queue_kind_name(&job.queue_kind) == name)
            .count() as i64;
        depth.insert(name, pending_count);
    }
    state.metrics.queue_depth_by_kind = depth;
    state.metrics.dead_letter_count = i64::try_from(state.dead_letter_queue.len()).unwrap_or(i64::MAX);
    state.metrics.conflict_queue_count = i64::try_from(state.conflict_queue.len()).unwrap_or(i64::MAX);
}

pub fn process_worker_round(
    state: &mut MirrorWorkerState,
    cfg: &MirrorWorkerConfig,
) -> Result<MirrorProcessingReport, VpsTypedError> {
    if cfg.max_batch_size == 0 || cfg.tenant_fairness_quantum == 0 {
        return Err(typed_error(
            "invalid_worker_config",
            "max_batch_size and tenant_fairness_quantum must be > 0.",
        ));
    }
    let mut report = MirrorProcessingReport::default();
    let mut fairness_used: HashMap<String, usize> = HashMap::new();
    let mut deferred = VecDeque::new();

    while report.processed_count < i64::try_from(cfg.max_batch_size).unwrap_or(i64::MAX) {
        let Some(mut job) = state.pending.pop_front() else {
            break;
        };
        let tenant_budget = fairness_used.entry(job.tenant_id.clone()).or_insert(0);
        if *tenant_budget >= cfg.tenant_fairness_quantum {
            deferred.push_back(job);
            continue;
        }
        *tenant_budget += 1;

        let expected_schema = tenant_schema_name(&job.tenant_id)?;
        if job.target_schema != expected_schema {
            report.tenant_isolation_violations += 1;
            report.dead_lettered_count += 1;
            job.queue_kind = MirrorQueueKind::DeadLetter;
            state.dead_letter_queue.push_back(job);
            continue;
        }

        if let Some(current_checkpoint) = state.tenant_checkpoint.get(&job.tenant_id) {
            if let Some(incoming_checkpoint) = &job.checkpoint_token {
                if is_stale_checkpoint(current_checkpoint, incoming_checkpoint) {
                    report.stale_checkpoint_blocked_count += 1;
                    report.conflict_routed_count += 1;
                    job.queue_kind = MirrorQueueKind::ConflictReview;
                    state.conflict_queue.push_back(job);
                    continue;
                }
            }
        }

        let idempotency_fingerprint = (job.tenant_id.clone(), job.idempotency_key.clone());
        if !state.seen_idempotency.insert(idempotency_fingerprint) {
            report.deduped_count += 1;
            continue;
        }

        if let Some(_failure_code) = &job.injected_failure_code {
            if job.attempt + 1 >= cfg.max_retry_attempts {
                report.dead_lettered_count += 1;
                job.queue_kind = MirrorQueueKind::DeadLetter;
                state.dead_letter_queue.push_back(job);
            } else {
                report.retried_count += 1;
                state.metrics.retry_count += 1;
                job.attempt += 1;
                job.injected_failure_code = None;
                deferred.push_back(job);
            }
            continue;
        }

        let conflict = match job.record_class {
            MirrorRecordClass::AppendOnlyEvent => false,
            MirrorRecordClass::GovernedSnapshot => job.incoming_row_version <= job.local_row_version,
            MirrorRecordClass::MutableOperational => job.incoming_row_version < job.local_row_version,
        };
        if conflict {
            report.conflict_routed_count += 1;
            job.queue_kind = MirrorQueueKind::ConflictReview;
            state.conflict_queue.push_back(job);
            continue;
        }

        if let Some(checkpoint_token) = &job.checkpoint_token {
            state
                .tenant_checkpoint
                .insert(job.tenant_id.clone(), checkpoint_token.clone());
        }
        *state
            .metrics
            .per_tenant_processed
            .entry(job.tenant_id.clone())
            .or_insert(0) += 1;
        report.processed_count += 1;
    }

    while let Some(job) = deferred.pop_front() {
        state.pending.push_back(job);
    }
    let max_processed = state.metrics.per_tenant_processed.values().copied().max().unwrap_or(0);
    for (tenant, processed) in &state.metrics.per_tenant_processed {
        state.metrics.per_tenant_lag.insert(tenant.clone(), max_processed - processed);
    }
    update_metrics_from_state(state);
    Ok(report)
}
