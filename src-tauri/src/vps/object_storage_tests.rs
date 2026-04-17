use crate::vps::object_storage::{
    aggregate_parts_payload_sha256, backup_allowed_for_queue_health, build_object_key, canonical_manifest_json,
    emergency_corrupted_snapshot_runbook_steps, emergency_expired_credentials_runbook_steps,
    emergency_rollout_deletion_runbook_steps, manifest_integrity_hash, platform_restore_runbook, tenant_restore_runbook,
    verify_manifest_against_parts, BackupManifestPart, BackupManifestV1, BackupScope, DeploymentEnvironment,
    IntegrityVerificationJobResult, ObjectCategory, PostRestoreValidationChecklist, QueueHealthSnapshot, StorageDataClass,
};

#[test]
fn object_key_enforces_env_and_tenant_boundary() {
    let k = build_object_key(
        DeploymentEnvironment::Production,
        ObjectCategory::BackupSnapshots,
        Some("acme-corp"),
        &["pg", "2026", "snap-001.tar.zst"],
    )
    .expect("key");
    assert!(k.full_key.starts_with("prod/backups/acme-corp/"));
    assert_eq!(k.data_class, StorageDataClass::BackupOperational);
}

#[test]
fn platform_scope_uses_reserved_segment() {
    let k = build_object_key(
        DeploymentEnvironment::Pilot,
        ObjectCategory::UpdaterArtifacts,
        None,
        &["channel-stable", "v1.2.3", "manifest.json"],
    )
    .expect("key");
    assert!(k.full_key.contains("/_platform/"));
}

#[test]
fn invalid_tenant_segment_rejected() {
    let err = build_object_key(
        DeploymentEnvironment::Production,
        ObjectCategory::TenantRestoreBundles,
        Some("bad/id"),
        &["bundle.zip"],
    )
    .expect_err("invalid tenant");
    assert_eq!(err.code, "tenant_key_invalid");
}

#[test]
fn manifest_integrity_round_trip() {
    let parts = vec![BackupManifestPart {
        object_key: "prod/backups/_platform/pg/a.bin".to_string(),
        sha256: "2c26b46b68ffc68ff99b453c1d3041340812a63792599d5da4e0e7db9339a023".to_string(),
        byte_length: 3,
    }];
    let payload_sha256 = aggregate_parts_payload_sha256(&parts);
    let manifest = BackupManifestV1 {
        manifest_version: 1,
        snapshot_id: "snap-xyz".to_string(),
        created_at_rfc3339: "2026-04-16T12:00:00Z".to_string(),
        environment: "prod".to_string(),
        payload_sha256,
        parts,
    };
    verify_manifest_against_parts(&manifest).expect("verify");
    let h = manifest_integrity_hash(&manifest);
    assert_eq!(h.len(), 64);
    let _ = canonical_manifest_json(&manifest);
}

#[test]
fn queue_health_gates_backup_window() {
    let ok = backup_allowed_for_queue_health(&QueueHealthSnapshot {
        max_sync_queue_depth_threshold: 100,
        current_max_depth: 10,
    });
    assert!(ok);
    let blocked = backup_allowed_for_queue_health(&QueueHealthSnapshot {
        max_sync_queue_depth_threshold: 5,
        current_max_depth: 100,
    });
    assert!(!blocked);
}

#[test]
fn restore_runbooks_define_rpo_rto() {
    let p = platform_restore_runbook();
    assert!(p.rto_hours > 0);
    let t = tenant_restore_runbook("tenant-a");
    assert!(t.steps.len() >= 3);
}

#[test]
fn post_restore_checklist_all_green() {
    let c = PostRestoreValidationChecklist {
        entitlement_heartbeat_ok: true,
        sync_checkpoint_continuous: true,
        admin_audit_read_ok: true,
        update_manifest_integrity_ok: true,
    };
    assert!(c.all_ok());
}

#[test]
fn integrity_job_records_isolation_target() {
    let job = IntegrityVerificationJobResult {
        snapshot_id: "snap-1".to_string(),
        isolated_target_schema: "restore_verify_snap_1".to_string(),
        schema_compatible: true,
        manifest_verified: true,
    };
    assert!(job.manifest_verified && job.schema_compatible);
}

#[test]
fn backup_scope_tenant_is_distinct_from_control_plane() {
    let cp = BackupScope::ControlPlane;
    let tn = BackupScope::TenantMirror {
        tenant_id: "t1".to_string(),
    };
    assert!(!matches!(cp, BackupScope::TenantMirror { .. }));
    assert!(matches!(tn, BackupScope::TenantMirror { .. }));
}

#[test]
fn emergency_runbooks_non_empty() {
    assert!(!emergency_rollout_deletion_runbook_steps().is_empty());
    assert!(!emergency_corrupted_snapshot_runbook_steps().is_empty());
    assert!(!emergency_expired_credentials_runbook_steps().is_empty());
}
